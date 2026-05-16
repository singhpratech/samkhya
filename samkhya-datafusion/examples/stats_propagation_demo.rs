//! Demonstrates that samkhya's `SamkhyaTableProvider` +
//! `SamkhyaOptimizerRule` actually flow corrected row counts into
//! DataFusion 46's physical plan, by comparing the row-count estimate
//! reported by `ctx.state().create_physical_plan(&plan)?.statistics()`
//! in two configurations:
//!
//! * "without rule": the bare `MemTable` is registered directly. The
//!   mainline planner builds a `DataSourceExec` whose `statistics()`
//!   defaults to whatever the `MemTable` reports.
//! * "with rule":   the same `MemTable` is wrapped in
//!   `SamkhyaTableProvider` with a row-count override of 42, and the
//!   `SamkhyaOptimizerRule` is registered against the `SessionContext`
//!   on both the logical and physical optimizer chains. The table
//!   provider's `scan()` wraps the inner exec with `SamkhyaStatsExec`,
//!   so the top-level `physical.statistics()?.num_rows` reflects the
//!   override.
//!
//! Run with:
//!   cargo run -p samkhya-datafusion --example stats_propagation_demo
//!
//! Expected output (the exact "without" value depends on the inner
//! provider; `MemTable` reports 1000 rows because it owns the batches):
//!
//!     without rule: 1000, with rule: 42
//!
//! Note that 1000 is the truthful (exact) MemTable count, and 42 is the
//! samkhya-corrected (inexact) override — the demo deliberately uses an
//! override that disagrees with the real count to prove the corrected
//! value is what propagates through the planner.

use std::sync::Arc;

use datafusion::arrow::array::Int64Array;
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::Result;
use datafusion::common::stats::Precision;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::execution::context::SessionContext;
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::prelude::SessionConfig;
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::{SamkhyaOptimizerRule, SamkhyaTableProvider};

const N_ROWS: usize = 1000;
const OVERRIDE_ROWS: u64 = 42;

fn build_mem_table() -> Arc<MemTable> {
    let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
    let values: Vec<i64> = (0..N_ROWS as i64).collect();
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![Arc::new(Int64Array::from(values))],
    )
    .expect("record batch");
    Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("mem table"))
}

async fn estimated_rows(ctx: &SessionContext) -> Result<Option<usize>> {
    let logical = ctx.state().create_logical_plan("SELECT * FROM t").await?;
    let physical = ctx.state().create_physical_plan(&logical).await?;
    let stats = physical.statistics()?;
    Ok(match stats.num_rows {
        Precision::Exact(n) | Precision::Inexact(n) => Some(n),
        Precision::Absent => None,
    })
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // ----- "without rule" configuration: plain MemTable, vanilla ctx. -----
    let baseline_ctx = SessionContext::new();
    baseline_ctx.register_table("t", build_mem_table() as Arc<dyn TableProvider>)?;
    let without = estimated_rows(&baseline_ctx).await?;

    // ----- "with rule" configuration: SamkhyaTableProvider + rule. -----
    let rule = Arc::new(SamkhyaOptimizerRule::new());
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new())
        .with_default_features()
        .with_optimizer_rule(rule.clone())
        .with_physical_optimizer_rule(rule.clone())
        .build();
    let samkhya_ctx = SessionContext::new_with_state(state);

    let wrapped = Arc::new(
        SamkhyaTableProvider::new(build_mem_table())
            .with_column_stats(0, ColumnStats::new().with_row_count(OVERRIDE_ROWS)),
    );
    samkhya_ctx.register_table("t", wrapped as Arc<dyn TableProvider>)?;

    let with = estimated_rows(&samkhya_ctx).await?;

    let fmt = |o: Option<usize>| match o {
        Some(n) => n.to_string(),
        None => "<absent>".to_string(),
    };
    println!("without rule: {}, with rule: {}", fmt(without), fmt(with));

    // Cross-check that the physical-optimizer rule actually observed the
    // SamkhyaStatsExec wrapper in the plan tree.
    println!(
        "samkhya_leaves_seen (physical pass): {}",
        rule.samkhya_leaves_seen()
    );

    // Sanity assertion: "with rule" must report the override, not the
    // MemTable's real count.
    assert_eq!(
        with,
        Some(OVERRIDE_ROWS as usize),
        "with-rule num_rows must equal the override ({OVERRIDE_ROWS}), got {with:?}"
    );

    Ok(())
}
