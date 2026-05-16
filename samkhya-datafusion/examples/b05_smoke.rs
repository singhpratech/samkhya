//! B05 smoke test / benchmark example for samkhya-datafusion.
//!
//! Added by agent B05 (binary-acceptance wave) — NOT part of the original
//! samkhya-datafusion crate source. This file documents the integration
//! smoke test for the B05 acceptance run. It is safe to keep as a
//! permanent example; it exercises the full 3-layer integration path.
//!
//! # What this tests
//!
//! 1. Builds a `SessionContext` with `SamkhyaOptimizerRule` registered on
//!    both the logical and physical optimizer chains.
//! 2. Registers a synthetic `SamkhyaTableProvider`-wrapped `MemTable` with
//!    10 000 rows and 3 columns (id INT64, cat INT32, tag UTF8).
//! 3. Runs four queries and records actual vs estimated cardinality:
//!    - Q1: `SELECT COUNT(*)` — table-level cardinality
//!    - Q2: `SELECT COUNT(*) WHERE id < 1000` — filter selectivity
//!    - Q3: `SELECT cat, COUNT(*) GROUP BY cat` — group-by output size
//!    - Q4: 2-table JOIN with WHERE filter
//! 4. Computes q-error (max(e/a, a/e)) per query.
//! 5. Verifies `SamkhyaOptimizerRule` is present in
//!    `ctx.state().physical_optimizers()`.
//! 6. Runs the whole suite 10× and asserts determinism.
//! 7. Prints an EXPLAIN snippet and checks for `SamkhyaStatsExec` in output.
//!
//! # Run
//!
//! ```bash
//! export CARGO_TARGET_DIR=/tmp/samkhya-b05-target
//! cargo run --release -p samkhya-datafusion --example b05_smoke
//! ```

use std::sync::Arc;

use datafusion::arrow::array::{Array, Int32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::stats::Precision;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::execution::context::SessionContext;
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::prelude::SessionConfig;
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::{SamkhyaOptimizerRule, SamkhyaTableProvider};

const N_ROWS: usize = 10_000;
/// Override injected via SamkhyaTableProvider — deliberately != real count
/// to prove the corrected value propagates.
const SAMKHYA_ROW_OVERRIDE: u64 = 8_500;

// -----------------------------------------------------------------------
// Table builders
// -----------------------------------------------------------------------

fn build_main_table() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("cat", DataType::Int32, false),
        Field::new("tag", DataType::Utf8, false),
    ]));
    let ids: Vec<i64> = (0..N_ROWS as i64).collect();
    let cats: Vec<i32> = (0..N_ROWS as i32).map(|i| i % 10).collect();
    let tags: Vec<String> = (0..N_ROWS).map(|i| format!("cat_{}", i % 10)).collect();
    let tag_refs: Vec<&str> = tags.iter().map(|s| s.as_str()).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(Int32Array::from(cats)),
            Arc::new(StringArray::from(tag_refs)),
        ],
    )
    .expect("main batch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("main mem table"))
}

fn build_dim_table() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("cat_id", DataType::Int32, false),
        Field::new("label", DataType::Utf8, false),
    ]));
    let cat_ids: Vec<i32> = (0..10).collect();
    let labels: Vec<String> = (0..10).map(|i| format!("label_{i}")).collect();
    let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int32Array::from(cat_ids)),
            Arc::new(StringArray::from(label_refs)),
        ],
    )
    .expect("dim batch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("dim table"))
}

// -----------------------------------------------------------------------
// Context factory
// -----------------------------------------------------------------------

fn build_samkhya_ctx() -> (SessionContext, Arc<SamkhyaOptimizerRule>) {
    let rule = Arc::new(SamkhyaOptimizerRule::new());
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new())
        .with_default_features()
        .with_optimizer_rule(rule.clone())
        .with_physical_optimizer_rule(rule.clone())
        .build();
    let ctx = SessionContext::new_with_state(state);

    let wrapped_main = Arc::new(
        SamkhyaTableProvider::new(build_main_table())
            .with_column_stats(
                0,
                ColumnStats::new()
                    .with_row_count(SAMKHYA_ROW_OVERRIDE)
                    .with_distinct_count(SAMKHYA_ROW_OVERRIDE)
                    .with_null_count(0)
                    .with_upper_bound(N_ROWS as u64),
            )
            .with_column_stats(
                1,
                ColumnStats::new()
                    .with_distinct_count(10)
                    .with_null_count(0),
            ),
    );
    ctx.register_table("main_t", wrapped_main as Arc<dyn TableProvider>)
        .expect("register main_t");

    ctx.register_table("dim_t", build_dim_table() as Arc<dyn TableProvider>)
        .expect("register dim_t");

    (ctx, rule)
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn q_error(estimated: usize, actual: usize) -> f64 {
    let e = estimated.max(1) as f64;
    let a = actual.max(1) as f64;
    (e / a).max(a / e)
}

#[derive(Debug, Clone)]
struct QueryResult {
    label: &'static str,
    actual: usize,
    estimated: usize,
    q_error: f64,
}

async fn estimated_rows(ctx: &SessionContext, sql: &str) -> usize {
    let logical = ctx
        .state()
        .create_logical_plan(sql)
        .await
        .expect("logical plan");
    let physical = ctx
        .state()
        .create_physical_plan(&logical)
        .await
        .expect("physical plan");
    match physical.statistics().expect("statistics").num_rows {
        Precision::Exact(n) | Precision::Inexact(n) => n,
        Precision::Absent => 0,
    }
}

// -----------------------------------------------------------------------
// Per-run suite
// -----------------------------------------------------------------------

async fn run_smoke_once(run_idx: usize) -> Vec<QueryResult> {
    let (ctx, rule) = build_samkhya_ctx();
    let mut results = Vec::new();

    // Q1 — COUNT(*)
    {
        let sql = "SELECT COUNT(*) FROM main_t";
        let estimated = estimated_rows(&ctx, sql).await;
        let batches = ctx
            .sql(sql)
            .await
            .expect("Q1")
            .collect()
            .await
            .expect("Q1 exec");
        let actual = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("int64")
            .value(0) as usize;
        results.push(QueryResult {
            label: "Q1_COUNT_STAR",
            actual,
            estimated,
            q_error: q_error(estimated, actual),
        });
    }

    // Q2 — WHERE filter
    {
        let sql = "SELECT COUNT(*) FROM main_t WHERE id < 1000";
        let estimated = estimated_rows(&ctx, sql).await;
        let batches = ctx
            .sql(sql)
            .await
            .expect("Q2")
            .collect()
            .await
            .expect("Q2 exec");
        let actual = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("int64")
            .value(0) as usize;
        results.push(QueryResult {
            label: "Q2_WHERE_FILTER",
            actual,
            estimated,
            q_error: q_error(estimated, actual),
        });
    }

    // Q3 — GROUP BY
    {
        let sql = "SELECT cat, COUNT(*) as cnt FROM main_t GROUP BY cat";
        let estimated = estimated_rows(&ctx, sql).await;
        let batches = ctx
            .sql(sql)
            .await
            .expect("Q3")
            .collect()
            .await
            .expect("Q3 exec");
        let actual: usize = batches.iter().map(|b| b.num_rows()).sum();
        results.push(QueryResult {
            label: "Q3_GROUP_BY",
            actual,
            estimated,
            q_error: q_error(estimated, actual),
        });
    }

    // Q4 — 2-table JOIN
    {
        let sql =
            "SELECT m.id, d.label FROM main_t m JOIN dim_t d ON m.cat = d.cat_id WHERE m.id < 500";
        let estimated = estimated_rows(&ctx, sql).await;
        let batches = ctx
            .sql(sql)
            .await
            .expect("Q4")
            .collect()
            .await
            .expect("Q4 exec");
        let actual: usize = batches.iter().map(|b| b.num_rows()).sum();
        results.push(QueryResult {
            label: "Q4_JOIN",
            actual,
            estimated,
            q_error: q_error(estimated, actual),
        });
    }

    // Verify rule registration in physical optimizer chain
    let state = ctx.state();
    let phys_opts = state.physical_optimizers();
    let registered = phys_opts
        .iter()
        .any(|r| r.name() == "samkhya_cardinality_correction");
    assert!(
        registered,
        "run {run_idx}: SamkhyaOptimizerRule not found in physical_optimizers()"
    );

    // Verify physical pass saw at least one SamkhyaStatsExec
    let leaves = rule.samkhya_leaves_seen();
    assert!(
        leaves > 0,
        "run {run_idx}: samkhya_leaves_seen = 0, wiring broken"
    );

    if run_idx == 0 {
        println!("  rule_registered_in_physical_opts: {registered}");
        println!("  samkhya_leaves_seen: {leaves}");
    }

    results
}

// -----------------------------------------------------------------------
// main
// -----------------------------------------------------------------------

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    println!("=== B05 samkhya-datafusion smoke test ===");
    println!("N_ROWS={N_ROWS}  SAMKHYA_ROW_OVERRIDE={SAMKHYA_ROW_OVERRIDE}");

    let mut all_runs: Vec<Vec<QueryResult>> = Vec::new();

    for run_idx in 0..10 {
        let results = run_smoke_once(run_idx).await;
        if run_idx == 0 {
            println!("\n--- Run 0 results (subsequent runs suppressed) ---");
            for r in &results {
                println!(
                    "  {}: actual={} estimated={} q_error={:.3}",
                    r.label, r.actual, r.estimated, r.q_error
                );
            }
        }
        all_runs.push(results);
    }

    // Determinism check
    println!("\n=== Determinism check (10 runs) ===");
    let first = &all_runs[0];
    let mut all_det = true;
    for (run_idx, run) in all_runs.iter().enumerate().skip(1) {
        for (a, b) in first.iter().zip(run.iter()) {
            if a.actual != b.actual || a.estimated != b.estimated {
                eprintln!(
                    "FAIL run {run_idx} {}: actual {}->{}, estimated {}->{}",
                    a.label, a.actual, b.actual, a.estimated, b.estimated
                );
                all_det = false;
            }
        }
    }
    if all_det {
        println!("All 10 runs produced identical actual and estimated row counts. DETERMINISTIC.");
    } else {
        println!("WARN: non-determinism detected (see stderr).");
    }

    // Q-error table
    println!("\n=== Q-error table ===");
    println!(
        "{:<20} {:>10} {:>12} {:>10}",
        "Query", "Actual", "Estimated", "Q-error"
    );
    println!("{}", "-".repeat(56));
    for r in first {
        println!(
            "{:<20} {:>10} {:>12} {:>10.3}",
            r.label, r.actual, r.estimated, r.q_error
        );
    }

    // EXPLAIN to check node names
    println!("\n=== EXPLAIN SELECT COUNT(*) FROM main_t ===");
    let (ctx, _) = build_samkhya_ctx();
    let explain_batches = ctx
        .sql("EXPLAIN SELECT COUNT(*) FROM main_t")
        .await
        .expect("explain plan")
        .collect()
        .await
        .expect("explain exec");

    let mut found_samkhya_node = false;
    let mut explain_lines: Vec<String> = Vec::new();
    for batch in &explain_batches {
        let col = batch.column(1);
        let arr = col
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("string array");
        for i in 0..arr.len() {
            let line = arr.value(i).to_string();
            if line.contains("SamkhyaStatsExec") {
                found_samkhya_node = true;
            }
            explain_lines.push(line);
        }
    }
    for line in &explain_lines {
        println!("{line}");
    }
    println!("\nSamkhyaStatsExec present in EXPLAIN: {found_samkhya_node}");

    // Final verdict
    println!("\n=== FINAL VERDICT ===");
    if all_det && found_samkhya_node {
        println!("PASS");
    } else {
        println!("FAIL: deterministic={all_det} samkhya_node_in_explain={found_samkhya_node}");
        std::process::exit(1);
    }
}
