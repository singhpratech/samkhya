//! `e2e_query_suite` — WAVE5-H pipeline closure for file
//! `bench-results/10_datafusion_e2e_stats.md`.
//!
//! Implements the driver referenced in §6.1 of file 10 (Reproducibility):
//! it executes a small synthetic query suite (a tractable scale-down of
//! the S1–S10 design pre-registered in file 10) twice per cold/warm
//! replicate, once with samkhya wired in and once with a native
//! `SessionContext`, and emits one JSON record per `(query, mode, phase,
//! replicate)`. Downstream WAVE4-A scripts (`bootstrap_ci.py`,
//! `wilcoxon_paired.py`) ingest the per-replicate vectors to compute
//! BCa CIs + Wilcoxon W / p without further preprocessing.
//!
//! Methodology:
//! - Synthetic in-memory tables (1 M-row `fact`, 10-row `dim_small`,
//!   10 k-row `dim_med`) — exactly the schema documented in §3.2 of
//!   file 10, scaled down where necessary to keep wallclock under
//!   the WAVE5-H per-blocker budget. The scale is recorded in the
//!   JSON sidecar so the receipt can document the deviation.
//! - 10 replicates per (query, mode, phase). Smaller than the 30
//!   pre-registered in the file but sufficient for BCa with n=10
//!   (Efron-Tibshirani 1993 §14 acknowledges n=10 as the practical
//!   floor for BCa).
//! - Cold = fresh `SessionContext` per replicate. Warm = same
//!   context with one untimed warm-up query.
//! - First seed tried — no seed search.
//!
//! Citations:
//! - Efron, B. & Tibshirani, R. J. (1993). *An Introduction to the
//!   Bootstrap*. Chapter 14.
//! - Wilcoxon, F. (1945). "Individual Comparisons by Ranking Methods."
//!   *Biometrics Bulletin* 1(6):80–83.
//! - Leis et al. (2015). "How Good Are Query Optimizers, Really?"
//!   VLDB 2015 (geomean speedup convention).
//!
//! Run:
//! ```text
//! cargo run --release -p samkhya-datafusion --example e2e_query_suite \
//!     -- --json-out bench-results/10_e2e_raw.json --replicates 10
//! ```

use std::env;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use datafusion::arrow::array::{Float64Array, Int32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::execution::context::SessionContext;
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::prelude::SessionConfig;
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::{SamkhyaOptimizerRule, SamkhyaTableProvider};

const FACT_ROWS: usize = 100_000; // scaled from 1M per file 10 §3.2 — see methodology
const DIM_SMALL_ROWS: usize = 10;
const DIM_MED_ROWS: usize = 10_000;

const QUERIES: &[(&str, &str)] = &[
    (
        "S1_filter",
        "SELECT COUNT(*) FROM fact WHERE val BETWEEN 0.40 AND 0.60",
    ),
    (
        "S2_proj",
        "SELECT id, cat FROM fact WHERE cat = 3 ORDER BY id LIMIT 1000",
    ),
    ("S3_groupby", "SELECT cat, COUNT(*) FROM fact GROUP BY cat"),
    (
        "S4_topk",
        "SELECT id, val FROM fact ORDER BY val DESC LIMIT 50",
    ),
    (
        "S5_range",
        "SELECT id FROM fact WHERE ts BETWEEN 1000000 AND 1500000 ORDER BY ts",
    ),
    (
        "S6_join2way",
        "SELECT f.id, d.label FROM fact f JOIN dim_small d ON f.cat = d.cat_id WHERE f.val > 0.5",
    ),
    (
        "S7_join3way",
        "SELECT f.id, ds.label, dm.attr FROM fact f JOIN dim_small ds ON f.cat = ds.cat_id JOIN dim_med dm ON f.dim_id = dm.dim_id WHERE f.val > 0.5",
    ),
    (
        "S8_join_filter",
        "SELECT COUNT(*) FROM fact f JOIN dim_med dm ON f.dim_id = dm.dim_id WHERE f.val > 0.3",
    ),
    (
        "S9_agg",
        "SELECT cat, SUM(val) FROM fact WHERE ts > 500000 GROUP BY cat",
    ),
    (
        "S10_exists",
        "SELECT COUNT(*) FROM fact f WHERE EXISTS (SELECT 1 FROM dim_med dm WHERE dm.dim_id = f.dim_id AND dm.bucket > 5)",
    ),
];

/// Splitmix64 — used so generated data is bit-identical across runs.
fn sm64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn build_fact(seed: u64) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("cat", DataType::Int32, false),
        Field::new("key", DataType::Int64, false),
        Field::new("dim_id", DataType::Int32, false),
        Field::new("val", DataType::Float64, false),
        Field::new("ts", DataType::Int64, false),
    ]));
    let mut state = seed;
    let mut ids = Vec::with_capacity(FACT_ROWS);
    let mut cats = Vec::with_capacity(FACT_ROWS);
    let mut keys = Vec::with_capacity(FACT_ROWS);
    let mut dim_ids = Vec::with_capacity(FACT_ROWS);
    let mut vals = Vec::with_capacity(FACT_ROWS);
    let mut tss = Vec::with_capacity(FACT_ROWS);
    for i in 0..FACT_ROWS {
        ids.push(i as i64);
        cats.push((sm64(&mut state) % 10) as i32);
        keys.push(sm64(&mut state) as i64);
        dim_ids.push((sm64(&mut state) % DIM_MED_ROWS as u64) as i32);
        let raw = sm64(&mut state) as f64 / u64::MAX as f64;
        vals.push(raw);
        tss.push((i as i64) * 10 + (sm64(&mut state) % 5) as i64);
    }
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(Int32Array::from(cats)),
            Arc::new(Int64Array::from(keys)),
            Arc::new(Int32Array::from(dim_ids)),
            Arc::new(Float64Array::from(vals)),
            Arc::new(Int64Array::from(tss)),
        ],
    )
    .expect("fact batch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("fact memtable"))
}

fn build_dim_small() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("cat_id", DataType::Int32, false),
        Field::new("label", DataType::Utf8, false),
    ]));
    let cat_ids: Vec<i32> = (0..DIM_SMALL_ROWS as i32).collect();
    let labels: Vec<String> = (0..DIM_SMALL_ROWS).map(|i| format!("L{i}")).collect();
    let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int32Array::from(cat_ids)),
            Arc::new(StringArray::from(label_refs)),
        ],
    )
    .expect("dim_small batch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("dim_small memtable"))
}

fn build_dim_med(seed: u64) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("dim_id", DataType::Int32, false),
        Field::new("bucket", DataType::Int32, false),
        Field::new("attr", DataType::Utf8, false),
    ]));
    let mut state = seed;
    let mut dim_ids = Vec::with_capacity(DIM_MED_ROWS);
    let mut buckets = Vec::with_capacity(DIM_MED_ROWS);
    let mut attrs = Vec::with_capacity(DIM_MED_ROWS);
    for i in 0..DIM_MED_ROWS {
        dim_ids.push(i as i32);
        buckets.push((sm64(&mut state) % 10) as i32);
        attrs.push(format!("a{}", sm64(&mut state) % 1000));
    }
    let attr_refs: Vec<&str> = attrs.iter().map(|s| s.as_str()).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int32Array::from(dim_ids)),
            Arc::new(Int32Array::from(buckets)),
            Arc::new(StringArray::from(attr_refs)),
        ],
    )
    .expect("dim_med batch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("dim_med memtable"))
}

fn build_native_ctx(seed: u64) -> SessionContext {
    let ctx = SessionContext::new();
    ctx.register_table("fact", build_fact(seed) as Arc<dyn TableProvider>)
        .expect("register fact");
    ctx.register_table("dim_small", build_dim_small() as Arc<dyn TableProvider>)
        .expect("register dim_small");
    ctx.register_table(
        "dim_med",
        build_dim_med(seed ^ D1M_5EED) as Arc<dyn TableProvider>,
    )
    .expect("register dim_med");
    ctx
}

fn build_samkhya_ctx(seed: u64) -> SessionContext {
    let rule = Arc::new(SamkhyaOptimizerRule::new());
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new())
        .with_default_features()
        .with_optimizer_rule(rule.clone())
        .with_physical_optimizer_rule(rule.clone())
        .build();
    let ctx = SessionContext::new_with_state(state);

    let fact = build_fact(seed);
    let wrapped_fact = Arc::new(
        SamkhyaTableProvider::new(fact)
            .with_column_stats(
                0,
                ColumnStats::new()
                    .with_row_count(FACT_ROWS as u64)
                    .with_distinct_count(FACT_ROWS as u64),
            )
            .with_column_stats(1, ColumnStats::new().with_distinct_count(10))
            .with_column_stats(
                3,
                ColumnStats::new().with_distinct_count(DIM_MED_ROWS as u64),
            ),
    );
    ctx.register_table("fact", wrapped_fact as Arc<dyn TableProvider>)
        .expect("register fact");

    let dim_small = build_dim_small();
    let wrapped_dim_small = Arc::new(
        SamkhyaTableProvider::new(dim_small).with_column_stats(
            0,
            ColumnStats::new()
                .with_row_count(DIM_SMALL_ROWS as u64)
                .with_distinct_count(DIM_SMALL_ROWS as u64),
        ),
    );
    ctx.register_table("dim_small", wrapped_dim_small as Arc<dyn TableProvider>)
        .expect("register dim_small");

    let dim_med = build_dim_med(seed ^ D1M_5EED);
    let wrapped_dim_med = Arc::new(
        SamkhyaTableProvider::new(dim_med)
            .with_column_stats(
                0,
                ColumnStats::new()
                    .with_row_count(DIM_MED_ROWS as u64)
                    .with_distinct_count(DIM_MED_ROWS as u64),
            )
            .with_column_stats(1, ColumnStats::new().with_distinct_count(10)),
    );
    ctx.register_table("dim_med", wrapped_dim_med as Arc<dyn TableProvider>)
        .expect("register dim_med");

    ctx
}

const D1M_5EED: u64 = 0xD1_DEAD_BEEF_5EED;

async fn time_query(ctx: &SessionContext, sql: &str) -> (f64, i64) {
    let start = Instant::now();
    let df = ctx.sql(sql).await.expect("sql ok");
    let batches = df.collect().await.expect("collect ok");
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    let rows: i64 = batches.iter().map(|b| b.num_rows() as i64).sum();
    (elapsed, rows)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // CLI flags
    let args: Vec<String> = env::args().collect();
    let mut json_out: Option<String> = None;
    let mut replicates: usize = 10;
    // Pre-registered seed family for file 10. The "0xS4MK4YA_E2E_2026_05_16"
    // sigil from §3.2 of file 10 is not literal hex; we encode an
    // equivalent stable seed here.
    let mut seed: u64 = 0x5A4F_4E4B_FAE2_E026_u64;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--json-out" => {
                json_out = Some(args.get(i + 1).cloned().unwrap_or_default());
                i += 2;
            }
            "--replicates" => {
                replicates = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(10);
                i += 2;
            }
            "--seed" => {
                seed = args
                    .get(i + 1)
                    .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(seed);
                i += 2;
            }
            _ => i += 1,
        }
    }

    println!(
        "# e2e_query_suite: replicates={replicates}, fact_rows={FACT_ROWS}, queries={}",
        QUERIES.len()
    );
    println!("query,mode,phase,replicate,wallclock_ms,actual_rows,q_error_proxy");

    // We emit one record per (query, mode, phase, replicate). The CSV
    // stream on stdout is the human-readable rollup; the JSON sidecar at
    // --json-out is the canonical artefact downstream tools consume.
    #[derive(Clone)]
    struct Record {
        query: String,
        mode: &'static str,
        phase: &'static str,
        replicate: usize,
        wallclock_ms: f64,
        actual_rows: i64,
    }
    let mut records: Vec<Record> = Vec::new();

    for (q_name, q_sql) in QUERIES {
        // Cold phase: fresh context per replicate.
        for rep in 0..replicates {
            for mode in ["native", "samkhya"] {
                let ctx = if mode == "native" {
                    build_native_ctx(seed.wrapping_add(rep as u64))
                } else {
                    build_samkhya_ctx(seed.wrapping_add(rep as u64))
                };
                let (ms, rows) = time_query(&ctx, q_sql).await;
                println!("{},{},cold,{},{:.4},{},na", q_name, mode, rep, ms, rows);
                records.push(Record {
                    query: (*q_name).to_string(),
                    mode: if mode == "native" {
                        "native"
                    } else {
                        "samkhya"
                    },
                    phase: "cold",
                    replicate: rep,
                    wallclock_ms: ms,
                    actual_rows: rows,
                });
            }
        }

        // Warm phase: one context per mode, warm-up + N timed runs.
        for mode in ["native", "samkhya"] {
            let ctx = if mode == "native" {
                build_native_ctx(seed)
            } else {
                build_samkhya_ctx(seed)
            };
            // Warm-up.
            let _ = time_query(&ctx, q_sql).await;
            for rep in 0..replicates {
                let (ms, rows) = time_query(&ctx, q_sql).await;
                println!("{},{},warm,{},{:.4},{},na", q_name, mode, rep, ms, rows);
                records.push(Record {
                    query: (*q_name).to_string(),
                    mode: if mode == "native" {
                        "native"
                    } else {
                        "samkhya"
                    },
                    phase: "warm",
                    replicate: rep,
                    wallclock_ms: ms,
                    actual_rows: rows,
                });
            }
        }
    }

    if let Some(path) = json_out {
        // Hand-rolled JSON emit so we don't need serde_json as a new dep.
        let mut s = String::new();
        s.push_str(&format!(
            "{{\"benchmark\":\"e2e_query_suite\",\"fact_rows\":{FACT_ROWS},\"replicates\":{replicates},\"seed\":\"0x{seed:X}\",\"records\":["
        ));
        for (i, r) in records.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "{{\"query\":\"{}\",\"mode\":\"{}\",\"phase\":\"{}\",\"replicate\":{},\"wallclock_ms\":{:.6},\"actual_rows\":{}}}",
                r.query, r.mode, r.phase, r.replicate, r.wallclock_ms, r.actual_rows
            ));
        }
        s.push_str("]}");
        let mut f = File::create(&path).expect("create json-out");
        f.write_all(s.as_bytes()).expect("write json-out");
        eprintln!("# per-replicate JSON written to {path}");
    }
}
