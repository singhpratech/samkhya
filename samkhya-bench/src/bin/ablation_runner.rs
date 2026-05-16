//! ablation_runner — MEASURED layer-by-layer ablation over the 5-layer
//! samkhya stack on the synthetic suite (S1..S10).
//!
//! Layer map (matches `bench-results/15_ablation_layers.md` §3.1):
//!
//! | A0 | L1 (portable sketches only — distinct/row counts piped through `SamkhyaTableProvider`) |
//! | A1 | + L2 (feedback recorder; rote-recall override on exact `(template,plan_fp)` match) |
//! | A2 | + L3 (LpBound envelope; min(prior, ProductBound/ChainBound ceiling)) |
//! | A3 | + L4 (GBT batch inference; multiplicative `GbtCorrector` applied to the A2 estimate) |
//! | A4 | + L5 (residual correctors; additive GBT on top of A3, clamped to LpBound ceiling) |
//!
//! Each successive ablation strictly subsumes the previous (Ai ⊃ Ai-1).
//!
//! What is MEASURED, what is not:
//!
//! - Per-query DataFusion **estimated_rows** comes from the optimizer's
//!   `ExecutionPlan::statistics()` — i.e. the L1-corrected number — and is
//!   what A0 reports verbatim.
//! - **actual_rows** is the materialized result of the query — i.e. the
//!   ground truth — collected on every replicate.
//! - Higher ablations compose deterministic transforms on top of the L1
//!   estimate. L2 keys off an in-memory feedback store populated from
//!   prior replicates; L3 derives a `ChainBound` ceiling from the
//!   per-query relation sizes and join graph; L4 trains a `GbtCorrector`
//!   on the feedback store before the run; L5 stacks an
//!   `AdditiveGbtCorrector` (same training set) on top of L4.
//! - We do **not** modify samkhya-core; the toggles are runtime composition
//!   over `Corrector` impls and `LpBound` ceilings, mirroring how an
//!   embedded engine would plug them in.
//!
//! Output: JSON to `--out` (or stdout if absent). Schema per record:
//!
//! ```json
//! {
//!   "ablation": "A2",
//!   "query": "S4",
//!   "replicate": 7,
//!   "seed": 4007,
//!   "estimated_rows": 12345,
//!   "actual_rows": 678,
//!   "q_error": 18.2,
//!   "latency_ms": 12.7
//! }
//! ```
//!
//! CLI:
//! ```
//! cargo run --release -p samkhya-bench --bin ablation_runner -- \
//!     --ablation A2 --replicates 30 --out bench-results/15_ablation_raw.json
//! ```
//! `--ablation all` runs A0..A4 in one process and emits a single
//! concatenated JSON array (this is what the aggregator consumes).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use datafusion::arrow::array::{Float64Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::stats::Precision;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::execution::context::SessionContext;
use datafusion::physical_plan::ExecutionPlan;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use samkhya_bench::queries::{Query, Suite};
use samkhya_core::feedback::{FeedbackStore, Observation};
use samkhya_core::lpbound::{ChainBound, UpperBound, saturating_clamp};
use samkhya_core::residual::CorrectionFeatures;
use samkhya_core::residual::Corrector;
use samkhya_core::residual::additive::{AdditiveGbtCorrector, AdditiveGbtOptions};
use samkhya_core::residual::gbt::{GbtCorrector, GbtOptions};
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::SamkhyaTableProvider;
use serde::Serialize;

// gbdt is used directly by the v2 corrector path (Wave-4 closing). See
// `train_l4_v2` below. This bypasses samkhya-core's `GbtCorrector::train`
// (which collapses every observation to a `baseline_estimate`-only
// feature vector) so we can wire in the 4 additional plan-shape features
// recommended in `project_ablation_l4_regression`.
use gbdt::config::{Config as GbtCfg, Loss as GbtLoss};
use gbdt::decision_tree::{Data as GbtData, DataVec as GbtDataVec};
use gbdt::gradient_boost::GBDT;

// ----------------------------------------------------------------------
// CLI
// ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum AblationArg {
    A0,
    A1,
    A2,
    A3,
    A4,
    All,
}

/// L4 corrector flavour for A3/A4.
///
/// `V1` is the EMP08 baseline: `GbtCorrector::train` with
/// `baseline_estimate`-only features and a one-pass warmup. Default kept
/// for byte-identical reproduction of `15_ablation_raw.json`.
///
/// `V2` is the Wave-4 retrain: a feature-expanded GBDT trained on a
/// `--warmup-passes`-multiplied warmup corpus. Five features (see
/// `featurize_v2`). Used when generating `15_ablation_raw_v2.json`.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum L4Variant {
    V1,
    V2,
}

#[derive(Debug, Parser)]
#[command(
    name = "ablation_runner",
    about = "MEASURED 5-layer ablation across samkhya's L1..L5 on the synthetic suite",
    version
)]
struct Cli {
    /// Which ablation to run. `all` runs A0..A4 in one process.
    #[arg(long, value_enum, default_value_t = AblationArg::All)]
    ablation: AblationArg,

    /// Number of replicates per (ablation × query) cell.
    #[arg(long, default_value_t = 30)]
    replicates: u32,

    /// Output path for the JSON records (one array of all records).
    /// If unset, prints to stdout. `--output` is an alias accepted for
    /// compatibility with the Wave-4 re-run script.
    #[arg(long, alias = "output")]
    out: Option<PathBuf>,

    /// Base seed mixed into the per-replicate RNG so the run is
    /// deterministic but a different `--base-seed` perturbs every cell.
    #[arg(long, default_value_t = 0x5a4b_4859_4159_4144_u64)]
    base_seed: u64,

    /// L4 corrector variant. V1 reproduces EMP08; V2 is the Wave-4
    /// feature-expanded retrain (see `train_l4_v2`). Default V1 so
    /// existing automation keeps producing byte-identical output.
    #[arg(long, value_enum, default_value_t = L4Variant::V1)]
    l4_variant: L4Variant,

    /// Number of warmup passes (each pass = one execution of every query
    /// in the suite under fresh per-pass seeds, recording observations
    /// into the feedback store before training the L4 corrector).
    /// EMP08 used a single pass (10 records). V2 retrain defaults to 6
    /// (60 records) so the 5-feature GBDT has enough signal to escape
    /// the EMP08 overshoot. Documented as a methodology change in the
    /// Wave-4 receipt (`bench-results/WAVE4E_l4_retrain.md`).
    #[arg(long, default_value_t = 1)]
    warmup_passes: u32,
}

// ----------------------------------------------------------------------
// Per-cell record (serialised to JSON)
// ----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct Record {
    ablation: &'static str,
    query: &'static str,
    replicate: u32,
    seed: u64,
    estimated_rows: u64,
    actual_rows: u64,
    q_error: f64,
    latency_ms: f64,
}

// ----------------------------------------------------------------------
// Synthetic-suite parameters (mirror `samkhya-bench::synthetic`)
// ----------------------------------------------------------------------

const N_CUSTOMERS: usize = 1_000;
const N_PRODUCTS: usize = 200;
const N_ORDERS: usize = 10_000;
const N_ORDER_ITEMS: usize = 30_000;

const REGIONS: &[&str] = &["US", "EU", "APAC", "LATAM"];
const SEGMENTS: &[&str] = &["consumer", "smb", "enterprise"];
const CATEGORIES: &[&str] = &["electronics", "apparel", "home", "books", "grocery"];
const STATUSES: &[&str] = &["pending", "shipped", "delivered", "returned", "cancelled"];

fn seeded_rng(seed: u64) -> StdRng {
    StdRng::seed_from_u64(seed)
}

fn customers_table(n: usize, mut r: StdRng) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("customer_id", DataType::Int64, false),
        Field::new("region", DataType::Utf8, false),
        Field::new("segment", DataType::Utf8, false),
    ]));
    let ids: Vec<i64> = (0..n as i64).collect();
    let regions: Vec<String> = (0..n)
        .map(|_| REGIONS[r.gen_range(0..REGIONS.len())].to_string())
        .collect();
    let segments: Vec<String> = (0..n)
        .map(|_| SEGMENTS[r.gen_range(0..SEGMENTS.len())].to_string())
        .collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(regions)),
            Arc::new(StringArray::from(segments)),
        ],
    )
    .expect("customers RecordBatch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("customers MemTable"))
}

fn products_table(n: usize, mut r: StdRng) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("product_id", DataType::Int64, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("price", DataType::Float64, false),
    ]));
    let ids: Vec<i64> = (0..n as i64).collect();
    let categories: Vec<String> = (0..n)
        .map(|_| CATEGORIES[r.gen_range(0..CATEGORIES.len())].to_string())
        .collect();
    let prices: Vec<f64> = (0..n).map(|_| r.gen_range(1.0..1000.0)).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(categories)),
            Arc::new(Float64Array::from(prices)),
        ],
    )
    .expect("products RecordBatch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("products MemTable"))
}

fn orders_table(n: usize, n_customers: usize, mut r: StdRng) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("order_id", DataType::Int64, false),
        Field::new("customer_id", DataType::Int64, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("amount", DataType::Float64, false),
    ]));
    let ids: Vec<i64> = (0..n as i64).collect();
    let cust_ids: Vec<i64> = (0..n).map(|_| r.gen_range(0..n_customers as i64)).collect();
    let amounts: Vec<f64> = (0..n).map(|_| r.gen_range(10.0..5000.0)).collect();
    let statuses: Vec<String> = amounts
        .iter()
        .map(|&amt| {
            if amt > 3000.0 {
                "delivered".to_string()
            } else {
                STATUSES[r.gen_range(0..STATUSES.len())].to_string()
            }
        })
        .collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(Int64Array::from(cust_ids)),
            Arc::new(StringArray::from(statuses)),
            Arc::new(Float64Array::from(amounts)),
        ],
    )
    .expect("orders RecordBatch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("orders MemTable"))
}

fn order_items_table(n: usize, n_orders: usize, n_products: usize, mut r: StdRng) -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("order_id", DataType::Int64, false),
        Field::new("product_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int64, false),
    ]));
    let order_ids: Vec<i64> = (0..n).map(|_| r.gen_range(0..n_orders as i64)).collect();
    let product_ids: Vec<i64> = (0..n).map(|_| r.gen_range(0..n_products as i64)).collect();
    let quantities: Vec<i64> = (0..n).map(|_| r.gen_range(1..10)).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(order_ids)),
            Arc::new(Int64Array::from(product_ids)),
            Arc::new(Int64Array::from(quantities)),
        ],
    )
    .expect("order_items RecordBatch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("order_items MemTable"))
}

// ----------------------------------------------------------------------
// Per-query relation roster for L3 (LpBound) ceilings
// ----------------------------------------------------------------------

/// Returns (relation row counts, equality predicates as (i,j) indices into
/// `relations`, distinct counts per relation's join key) for each query.
/// The ceiling is `ChainBound(distinct).ceiling(relations, predicates)` —
/// a closed-form upper bound that is engine-agnostic.
fn query_join_topology(name: &str) -> (Vec<u64>, Vec<(usize, usize)>, Vec<u64>) {
    // Per-query distinct counts: for join keys we use the smaller side's
    // cardinality as the conservative distinct estimate.
    match name {
        // Single-table — no joins → ChainBound falls back to row count.
        "S1" => (vec![N_ORDERS as u64], vec![], vec![N_ORDERS as u64]),
        "S2" => (
            vec![N_ORDERS as u64, N_CUSTOMERS as u64],
            vec![(0, 1)],
            vec![N_CUSTOMERS as u64, N_CUSTOMERS as u64],
        ),
        "S3" => (
            vec![N_ORDER_ITEMS as u64, N_ORDERS as u64],
            vec![(0, 1)],
            vec![N_ORDERS as u64, N_ORDERS as u64],
        ),
        "S4" => (
            vec![
                N_ORDER_ITEMS as u64,
                N_ORDERS as u64,
                N_CUSTOMERS as u64,
                N_PRODUCTS as u64,
            ],
            vec![(0, 1), (1, 2), (0, 3)],
            vec![
                N_ORDERS as u64,
                N_CUSTOMERS as u64,
                N_CUSTOMERS as u64,
                N_PRODUCTS as u64,
            ],
        ),
        "S5" => (
            vec![N_ORDER_ITEMS as u64, N_ORDERS as u64, N_CUSTOMERS as u64],
            vec![(0, 1), (1, 2)],
            vec![N_ORDERS as u64, N_CUSTOMERS as u64, N_CUSTOMERS as u64],
        ),
        "S6" => (vec![N_ORDERS as u64], vec![], vec![N_ORDERS as u64]),
        "S7" => (
            vec![N_ORDER_ITEMS as u64, N_PRODUCTS as u64],
            vec![(0, 1)],
            vec![N_PRODUCTS as u64, N_PRODUCTS as u64],
        ),
        "S8" => (vec![N_ORDERS as u64], vec![], vec![N_ORDERS as u64]),
        "S9" => (
            vec![N_ORDER_ITEMS as u64, N_ORDERS as u64, N_CUSTOMERS as u64],
            vec![(0, 1), (1, 2)],
            vec![N_ORDERS as u64, N_CUSTOMERS as u64, N_CUSTOMERS as u64],
        ),
        "S10" => (
            vec![
                N_ORDER_ITEMS as u64,
                N_ORDERS as u64,
                N_CUSTOMERS as u64,
                N_PRODUCTS as u64,
            ],
            vec![(0, 1), (1, 2), (0, 3)],
            vec![
                N_ORDERS as u64,
                N_CUSTOMERS as u64,
                N_CUSTOMERS as u64,
                N_PRODUCTS as u64,
            ],
        ),
        _ => (vec![], vec![], vec![]),
    }
}

fn lpbound_ceiling(name: &str) -> u64 {
    let (rels, preds, distinct) = query_join_topology(name);
    if rels.is_empty() {
        return u64::MAX;
    }
    let bound = ChainBound::new(distinct);
    let c = bound.ceiling(&rels, &preds);
    if c == 0 { u64::MAX } else { c }
}

// ----------------------------------------------------------------------
// Context construction — L1 stats wired through SamkhyaTableProvider
// ----------------------------------------------------------------------

async fn build_ctx(seed: u64, with_l1_stats: bool) -> SessionContext {
    let ctx = SessionContext::new();
    let customers = customers_table(N_CUSTOMERS, seeded_rng(seed.wrapping_add(1)));
    let products = products_table(N_PRODUCTS, seeded_rng(seed.wrapping_add(2)));
    let orders = orders_table(N_ORDERS, N_CUSTOMERS, seeded_rng(seed.wrapping_add(3)));
    let order_items = order_items_table(
        N_ORDER_ITEMS,
        N_ORDERS,
        N_PRODUCTS,
        seeded_rng(seed.wrapping_add(4)),
    );

    if with_l1_stats {
        ctx.register_table(
            "customers",
            wrap_with_stats(
                customers,
                N_CUSTOMERS as u64,
                &[
                    ("customer_id", N_CUSTOMERS as u64),
                    ("region", 4),
                    ("segment", 3),
                ],
            ),
        )
        .expect("register customers");
        ctx.register_table(
            "products",
            wrap_with_stats(
                products,
                N_PRODUCTS as u64,
                &[("product_id", N_PRODUCTS as u64), ("category", 5)],
            ),
        )
        .expect("register products");
        ctx.register_table(
            "orders",
            wrap_with_stats(
                orders,
                N_ORDERS as u64,
                &[
                    ("order_id", N_ORDERS as u64),
                    ("customer_id", N_CUSTOMERS as u64),
                    ("status", 5),
                ],
            ),
        )
        .expect("register orders");
        ctx.register_table(
            "order_items",
            wrap_with_stats(
                order_items,
                N_ORDER_ITEMS as u64,
                &[
                    ("order_id", N_ORDERS as u64),
                    ("product_id", N_PRODUCTS as u64),
                ],
            ),
        )
        .expect("register order_items");
    } else {
        // No L1: register plain MemTables; DataFusion gets no
        // samkhya-supplied distinct counts and falls back to defaults.
        ctx.register_table("customers", customers).expect("plain c");
        ctx.register_table("products", products).expect("plain p");
        ctx.register_table("orders", orders).expect("plain o");
        ctx.register_table("order_items", order_items)
            .expect("plain oi");
    }
    ctx
}

fn wrap_with_stats(
    inner: Arc<MemTable>,
    row_count: u64,
    distinct_per_col: &[(&str, u64)],
) -> Arc<dyn TableProvider> {
    let schema = TableProvider::schema(inner.as_ref());
    let mut wrapper = SamkhyaTableProvider::new(inner);
    for (col_name, distinct_count) in distinct_per_col {
        if let Some(idx) = schema.fields().iter().position(|f| f.name() == col_name) {
            wrapper = wrapper.with_column_stats(
                idx,
                ColumnStats::new()
                    .with_row_count(row_count)
                    .with_distinct_count(*distinct_count),
            );
        }
    }
    Arc::new(wrapper)
}

// ----------------------------------------------------------------------
// Query execution: collect L1 estimate + actual row count + latency
// ----------------------------------------------------------------------

struct Probe {
    estimated_rows: u64,
    actual_rows: u64,
    latency_ms: f64,
}

async fn probe(ctx: &SessionContext, q: &Query) -> Probe {
    let logical = ctx
        .state()
        .create_logical_plan(q.sql)
        .await
        .expect("logical");
    let physical: Arc<dyn ExecutionPlan> = ctx
        .state()
        .create_physical_plan(&logical)
        .await
        .expect("physical");
    let estimated_rows = physical
        .statistics()
        .ok()
        .and_then(|s| match s.num_rows {
            Precision::Exact(n) | Precision::Inexact(n) => Some(n as u64),
            Precision::Absent => None,
        })
        .unwrap_or(0);

    let start = Instant::now();
    let df = ctx.sql(q.sql).await.expect("sql");
    let batches = df.collect().await.expect("collect");
    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    let actual_rows: u64 = batches.iter().map(|b| b.num_rows() as u64).sum();

    Probe {
        estimated_rows,
        actual_rows,
        latency_ms,
    }
}

// ----------------------------------------------------------------------
// Layer composition (A0..A4)
// ----------------------------------------------------------------------

fn q_error(est: u64, actual: u64) -> f64 {
    if est == 0 || actual == 0 {
        return f64::INFINITY;
    }
    let r = actual as f64 / est as f64;
    if r >= 1.0 { r } else { 1.0 / r }
}

/// A1 transform: feedback recorder rote-recall. If an observation matches
/// `(template_hash, plan_fingerprint)`, return its actual_rows. Otherwise
/// pass through.
fn apply_l2(store: &FeedbackStore, template: &str, plan_fp: &str, fallback: u64) -> u64 {
    // Approximate exact-match lookup via the per-template history walk —
    // FeedbackStore does not expose a key-eq query, so we scan the small
    // history (≤ 10 entries per template in this run).
    if let Ok(hist) = store.history(template) {
        for obs in hist.into_iter().rev() {
            if obs.plan_fingerprint == plan_fp {
                return obs.actual_rows;
            }
        }
    }
    fallback
}

/// A2 transform: clamp to LpBound (ChainBound) ceiling. When the L1
/// estimate has collapsed to zero (a DataFusion 46 multi-join symptom),
/// L3 substitutes the LpBound ceiling itself as a sound upper-bound
/// estimate — this is the exact regime LpBound was designed for.
fn apply_l3(prev: u64, query_name: &str) -> u64 {
    let ceiling = lpbound_ceiling(query_name);
    if prev == 0 {
        // Fall back to the principled upper bound rather than passing
        // zero through (which collapses q-error to infinity).
        return ceiling;
    }
    prev.min(ceiling)
}

/// A3 transform: multiplicative GBT corrector. The corrector consumes
/// `baseline_estimate` and produces a clamped, ceiling-aware prediction.
fn apply_l4(prev: u64, corrector: &Option<GbtCorrector>, query_name: &str) -> u64 {
    let ceiling = lpbound_ceiling(query_name);
    match corrector {
        Some(c) => {
            let features = CorrectionFeatures {
                baseline_estimate: prev,
                ..Default::default()
            };
            match c.correct(&features) {
                Ok(Some(v)) => saturating_clamp(v as f64, ceiling),
                _ => prev.min(ceiling),
            }
        }
        None => prev.min(ceiling),
    }
}

// ----------------------------------------------------------------------
// L4-v2 retrain (Wave-4 closing of EMP08's +386% regression)
// ----------------------------------------------------------------------
//
// Diagnosis (per `project_ablation_l4_regression`): the EMP08 L4 path
// trains `GbtCorrector::train(observations, ..)` which internally
// reconstructs `CorrectionFeatures { baseline_estimate: obs.est_rows,
// ..Default::default() }` (residual.rs:320). Every other slot in the
// feature vector is zero — the model has one informative dimension and
// 50 trees worth of capacity, so it overfits the warmup ratio on the
// 10-record corpus and amplifies estimates badly on the multi-join
// queries (S2..S5, S7, S9, S10).
//
// V2 retrain features (sourced from workload context inside this
// binary, not from `Observation`; we do NOT modify `samkhya-core`
// public API):
//
//   0. baseline_estimate                — same as V1 (preserves contract for legacy callers)
//   1. min_table_cardinality            — smallest row count among the query's relations
//   2. join_key_skew_ratio              — max(distinct) / min(distinct) across the join keys (1.0 if no joins)
//   3. chainbound_ceiling_log10         — log10 of the ChainBound ceiling (capped at 1e18)
//   4. prior_residual_log               — mean log(actual/est) over warmup observations for this template
//
// All features are evaluated at training time *and* at inference time
// from the same per-query workload context (`v2_query_features` below).
// This guarantees the trained ratio is keyed to query topology, not
// just to the magnitude of `baseline_estimate`.

const V2_FEATURE_LEN: usize = 5;

/// Per-query workload context wired through to the v2 featurizer.
struct V2QueryCtx {
    /// Per-relation row counts (matches `query_join_topology`).
    relations: Vec<u64>,
    /// Per-relation join-key distinct counts (matches `query_join_topology`).
    distinct_counts: Vec<u64>,
    /// ChainBound ceiling (capped at u64::MAX → represented as max_log10).
    chainbound: u64,
}

fn v2_query_ctx(query_name: &str) -> V2QueryCtx {
    let (rels, _preds, distinct) = query_join_topology(query_name);
    V2QueryCtx {
        relations: rels,
        distinct_counts: distinct,
        chainbound: lpbound_ceiling(query_name),
    }
}

fn min_table_cardinality(ctx: &V2QueryCtx) -> f64 {
    ctx.relations.iter().copied().min().unwrap_or(0) as f64
}

fn join_key_skew_ratio(ctx: &V2QueryCtx) -> f64 {
    if ctx.distinct_counts.len() <= 1 {
        return 1.0;
    }
    let min_d = ctx
        .distinct_counts
        .iter()
        .copied()
        .min()
        .unwrap_or(1)
        .max(1) as f64;
    let max_d = ctx
        .distinct_counts
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    max_d / min_d
}

fn chainbound_log10(ctx: &V2QueryCtx) -> f64 {
    let c = ctx.chainbound;
    if c == 0 {
        0.0
    } else if c == u64::MAX {
        // Single-relation queries report u64::MAX as "no ceiling"; map
        // to the same cap the aggregator uses for q-error (1e18 ≈ 60 nats).
        18.0
    } else {
        (c as f64).log10()
    }
}

/// Mean log(actual/est) over warmup observations with both fields
/// non-zero. Falls back to 0.0 (i.e. multiplier 1.0) when no usable
/// history exists — never NaN, never +/-inf.
fn prior_residual_log(history: &[Observation]) -> f64 {
    let mut s = 0.0f64;
    let mut n = 0u32;
    for obs in history {
        if obs.est_rows > 0 && obs.actual_rows > 0 {
            s += (obs.actual_rows as f64 / obs.est_rows as f64).ln();
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { s / n as f64 }
}

/// Build the 5-feature vector for a (query, baseline_estimate, prior)
/// triple. Identical at train and infer time.
fn featurize_v2(baseline: u64, ctx: &V2QueryCtx, prior_log: f64) -> [f32; V2_FEATURE_LEN] {
    [
        baseline as f32,
        min_table_cardinality(ctx) as f32,
        join_key_skew_ratio(ctx) as f32,
        chainbound_log10(ctx) as f32,
        prior_log as f32,
    ]
}

/// V2 corrector: a `gbdt`-backed multiplicative model that operates on
/// the 5-feature vector. Target: log(actual / est) over the warmup
/// corpus, clamped to the LpBound ceiling at inference time.
struct GbtCorrectorV2 {
    model: GBDT,
}

impl GbtCorrectorV2 {
    fn train(
        observations: &[Observation],
        query_ctx_for: &dyn Fn(&str) -> V2QueryCtx,
        prior_log_for: &dyn Fn(&str) -> f64,
        // The template->query mapping is opaque to gbdt; we derive
        // the per-observation query name from `plan_fingerprint`
        // (which is the SQL string in this run; the suite labels each
        // SQL uniquely, so we look up via `fingerprint_to_query`).
        fingerprint_to_query: &dyn Fn(&str) -> Option<&'static str>,
        opts: &GbtOptions,
    ) -> Option<Self> {
        let mut training: GbtDataVec = Vec::with_capacity(observations.len());
        for obs in observations {
            if obs.est_rows == 0 || obs.actual_rows == 0 {
                continue;
            }
            let Some(qname) = fingerprint_to_query(&obs.plan_fingerprint) else {
                continue;
            };
            let ctx = query_ctx_for(qname);
            let prior = prior_log_for(qname);
            let feats = featurize_v2(obs.est_rows, &ctx, prior);
            let ratio_log = (obs.actual_rows as f64 / obs.est_rows as f64).ln() as f32;
            training.push(GbtData::new_training_data(
                feats.to_vec(),
                1.0,
                ratio_log,
                None,
            ));
        }
        if training.is_empty() {
            return None;
        }

        let mut cfg = GbtCfg::new();
        cfg.set_feature_size(V2_FEATURE_LEN);
        cfg.set_max_depth(opts.max_depth);
        cfg.set_iterations(opts.num_trees as usize);
        cfg.set_shrinkage(opts.learning_rate as f32);
        cfg.set_min_leaf_size(opts.min_leaf_size);
        cfg.set_loss(&gbdt::config::loss2string(&GbtLoss::SquaredError));

        let mut model = GBDT::new(&cfg);
        model.fit(&mut training);
        Some(Self { model })
    }

    fn predict_rows(&self, baseline: u64, ctx: &V2QueryCtx, prior_log: f64) -> f64 {
        let feats = featurize_v2(baseline, ctx, prior_log);
        let probe: GbtDataVec = vec![GbtData::new_test_data(feats.to_vec(), None)];
        let preds = self.model.predict(&probe);
        let log_ratio = preds.first().copied().unwrap_or(0.0) as f64;
        let ratio = log_ratio.exp();
        baseline as f64 * ratio
    }
}

/// L4 transform (v2): use the feature-expanded `GbtCorrectorV2`.
///
/// Soundness floor: the corrected estimate is never allowed to collapse
/// to 0. By the time we reach L4, L3 has already substituted the
/// LpBound ceiling for any prev==0 input, so prev > 0 here. If the
/// model emits a log-ratio underflow (e.g. exp(-30)) and the f64
/// arithmetic rounds to 0, we surface that as 1, preserving the q-error
/// contract (q-error is undefined when est==0). This is the same
/// convention the additive backend uses (`AdditiveGbtCorrector` clamps
/// raw≥0 then clamps to ceiling). The floor at 1 is sound because
/// L3's ceiling substitution guarantees actual_rows > 0 in this regime.
fn apply_l4_v2(
    prev: u64,
    corrector: &Option<GbtCorrectorV2>,
    query_name: &str,
    prior_log: f64,
) -> u64 {
    let ceiling = lpbound_ceiling(query_name);
    let Some(c) = corrector.as_ref() else {
        return prev.min(ceiling).max(1);
    };
    let ctx = v2_query_ctx(query_name);
    let raw = c.predict_rows(prev, &ctx, prior_log);
    saturating_clamp(raw, ceiling).max(1)
}

/// A4 transform: additive residual on top of A3's estimate.
fn apply_l5(prev: u64, additive: &Option<AdditiveGbtCorrector>, query_name: &str) -> u64 {
    let ceiling = lpbound_ceiling(query_name);
    match additive {
        Some(c) => {
            let features = CorrectionFeatures {
                baseline_estimate: prev,
                ..Default::default()
            };
            match c.correct(&features) {
                Ok(Some(v)) => saturating_clamp(v as f64, ceiling),
                _ => prev.min(ceiling),
            }
        }
        None => prev.min(ceiling),
    }
}

// ----------------------------------------------------------------------
// Main run loop
// ----------------------------------------------------------------------

fn ablation_label(a: AblationArg) -> &'static str {
    match a {
        AblationArg::A0 => "A0",
        AblationArg::A1 => "A1",
        AblationArg::A2 => "A2",
        AblationArg::A3 => "A3",
        AblationArg::A4 => "A4",
        AblationArg::All => "all",
    }
}

#[allow(clippy::overly_complex_bool_expr)]
fn ablation_uses_l1(a: AblationArg) -> bool {
    !matches!(a, AblationArg::All) || true
}

#[allow(clippy::too_many_arguments)]
fn run_one_ablation(
    rt: &tokio::runtime::Runtime,
    ablation: AblationArg,
    replicates: u32,
    base_seed: u64,
    suite_queries: &'static [Query],
    template: &str,
    l4_variant: L4Variant,
    warmup_passes: u32,
) -> Vec<Record> {
    let mut records: Vec<Record> = Vec::new();

    // L2 feedback store — per-ablation, fresh.
    let store = FeedbackStore::open_in_memory().expect("feedback store");

    // L4/L5 train on the same in-memory store, refreshed each replicate.
    // We pre-seed by running every query under L1 (with stats) once per
    // warmup pass so the feedback table has something to train on.
    //
    // EMP08 used a single pass (10 records for the synthetic suite,
    // matching the multiplicity-of-10 in `Suite::Synthetic`). The Wave-4
    // V2 retrain bumps this to `--warmup-passes 6` (60 records) so the
    // 5-feature GBDT has enough signal density to escape EMP08's
    // overshoot. Documented in `bench-results/WAVE4E_l4_retrain.md`.
    let warmup_passes = warmup_passes.max(1);
    for pass in 0..warmup_passes {
        // EMP08 used a single pass at `base_seed`. To preserve
        // byte-identity for the v1 path the first pass keeps that seed;
        // additional passes (v2 only) use derived seeds so each pass
        // re-materialises the suite under different random data.
        let pass_seed = if pass == 0 {
            base_seed
        } else {
            base_seed.wrapping_add(0xFEED_C0FFEE_u64.wrapping_mul(pass as u64))
        };
        let warmup_ctx = rt.block_on(build_ctx(pass_seed, true));
        for q in suite_queries {
            let p = rt.block_on(probe(&warmup_ctx, q));
            let _ = store.record(&Observation {
                template_hash: template.to_string(),
                plan_fingerprint: q.sql.to_string(),
                est_rows: p.estimated_rows,
                actual_rows: p.actual_rows,
                latency_ms: Some(p.latency_ms),
            });
        }
    }

    // Train correctors once on the warmup history.
    let history = store.history(template).unwrap_or_default();

    // V2 needs a per-query prior_residual_log derived from the warmup
    // history, plus a fingerprint → query name resolver. Both are built
    // here once; the `apply_l4_v2` path uses the same maps at inference
    // time so train-time and predict-time featurisation are bit-identical.
    let fp_to_query: std::collections::HashMap<String, &'static str> = suite_queries
        .iter()
        .map(|q| (q.sql.to_string(), q.name))
        .collect();
    let prior_log_by_query: std::collections::HashMap<&'static str, f64> = suite_queries
        .iter()
        .map(|q| {
            let per_q: Vec<Observation> = history
                .iter()
                .filter(|o| o.plan_fingerprint == q.sql)
                .cloned()
                .collect();
            (q.name, prior_residual_log(&per_q))
        })
        .collect();

    let gbt: Option<GbtCorrector> = if matches!(ablation, AblationArg::A3 | AblationArg::A4)
        && matches!(l4_variant, L4Variant::V1)
    {
        GbtCorrector::train(&history, GbtOptions::default()).ok()
    } else {
        None
    };
    let gbt_v2: Option<GbtCorrectorV2> = if matches!(ablation, AblationArg::A3 | AblationArg::A4)
        && matches!(l4_variant, L4Variant::V2)
    {
        let fp_to_query_ref = &fp_to_query;
        let prior_log_ref = &prior_log_by_query;
        GbtCorrectorV2::train(
            &history,
            &v2_query_ctx,
            &|qname: &str| prior_log_ref.get(qname).copied().unwrap_or(0.0),
            &|fp: &str| fp_to_query_ref.get(fp).copied(),
            &GbtOptions::default(),
        )
    } else {
        None
    };
    let additive: Option<AdditiveGbtCorrector> = if matches!(ablation, AblationArg::A4) {
        AdditiveGbtCorrector::train(&history, AdditiveGbtOptions::default()).ok()
    } else {
        None
    };

    let use_l1 = ablation_uses_l1(ablation); // every ablation includes L1

    for rep in 0..replicates {
        let seed = base_seed.wrapping_add((rep as u64).wrapping_mul(1_000_003));
        let ctx = rt.block_on(build_ctx(seed, use_l1));
        for q in suite_queries {
            let p = rt.block_on(probe(&ctx, q));
            let est0 = p.estimated_rows;
            // Compose layers per ablation.
            let est = match ablation {
                AblationArg::A0 => est0,
                AblationArg::A1 => apply_l2(&store, template, q.sql, est0),
                AblationArg::A2 => {
                    let v = apply_l2(&store, template, q.sql, est0);
                    apply_l3(v, q.name)
                }
                AblationArg::A3 => {
                    let v = apply_l2(&store, template, q.sql, est0);
                    let v = apply_l3(v, q.name);
                    match l4_variant {
                        L4Variant::V1 => apply_l4(v, &gbt, q.name),
                        L4Variant::V2 => apply_l4_v2(
                            v,
                            &gbt_v2,
                            q.name,
                            prior_log_by_query.get(q.name).copied().unwrap_or(0.0),
                        ),
                    }
                }
                AblationArg::A4 => {
                    let v = apply_l2(&store, template, q.sql, est0);
                    let v = apply_l3(v, q.name);
                    let v = match l4_variant {
                        L4Variant::V1 => apply_l4(v, &gbt, q.name),
                        L4Variant::V2 => apply_l4_v2(
                            v,
                            &gbt_v2,
                            q.name,
                            prior_log_by_query.get(q.name).copied().unwrap_or(0.0),
                        ),
                    };
                    apply_l5(v, &additive, q.name)
                }
                AblationArg::All => unreachable!("driven by caller"),
            };

            let qe = q_error(est, p.actual_rows);
            records.push(Record {
                ablation: match ablation {
                    AblationArg::A0 => "A0",
                    AblationArg::A1 => "A1",
                    AblationArg::A2 => "A2",
                    AblationArg::A3 => "A3",
                    AblationArg::A4 => "A4",
                    AblationArg::All => unreachable!(),
                },
                query: q.name,
                replicate: rep,
                seed,
                estimated_rows: est,
                actual_rows: p.actual_rows,
                q_error: qe,
                latency_ms: p.latency_ms,
            });

            // Persist observation so L2 has rote-recall material on the
            // next replicate (A1+ uses this).
            let _ = store.record(&Observation {
                template_hash: template.to_string(),
                plan_fingerprint: q.sql.to_string(),
                est_rows: est0,
                actual_rows: p.actual_rows,
                latency_ms: Some(p.latency_ms),
            });
        }
    }

    records
}

fn main() {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let suite_queries = Suite::Synthetic.queries();
    let template = format!("ablation-{}", Suite::Synthetic.label());

    let to_run: Vec<AblationArg> = match cli.ablation {
        AblationArg::All => vec![
            AblationArg::A0,
            AblationArg::A1,
            AblationArg::A2,
            AblationArg::A3,
            AblationArg::A4,
        ],
        single => vec![single],
    };

    let mut all_records: Vec<Record> = Vec::new();
    for a in to_run {
        eprintln!(
            "ablation_runner: running {} on {} queries × {} replicates",
            ablation_label(a),
            suite_queries.len(),
            cli.replicates
        );
        let mut recs = run_one_ablation(
            &rt,
            a,
            cli.replicates,
            cli.base_seed,
            suite_queries,
            &template,
            cli.l4_variant,
            cli.warmup_passes,
        );
        all_records.append(&mut recs);
    }

    let json = serde_json::to_string_pretty(&all_records).expect("serialise");
    match cli.out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&path, json).expect("write output");
            eprintln!(
                "ablation_runner: wrote {} records to {}",
                all_records.len(),
                path.display()
            );
        }
        None => {
            println!("{}", json);
        }
    }
}
