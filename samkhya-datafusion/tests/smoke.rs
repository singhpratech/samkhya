//! End-to-end smoke test for the samkhya DataFusion adapter.
//!
//! Builds a `SessionContext` whose `SessionState` has the
//! `SamkhyaOptimizerRule` registered, then runs a trivial query and a
//! query against a small in-memory table to confirm the rule traverses
//! both `EmptyRelation` / `Projection` and `TableScan` plans without
//! breaking the optimizer pipeline.

use std::sync::Arc;

use datafusion::arrow::array::{Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::execution::context::SessionContext;
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::prelude::SessionConfig;
use samkhya_datafusion::SamkhyaOptimizerRule;

#[tokio::test(flavor = "multi_thread")]
async fn samkhya_rule_registers_and_runs_select_1() {
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new())
        .with_default_features()
        .with_optimizer_rule(Arc::new(SamkhyaOptimizerRule::new()))
        .build();

    let ctx = SessionContext::new_with_state(state);

    let df = ctx
        .sql("SELECT 1 AS one")
        .await
        .expect("SELECT 1 should plan");
    let batches = df.collect().await.expect("SELECT 1 should execute");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn samkhya_rule_walks_table_scan() {
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new())
        .with_default_features()
        .with_optimizer_rule(Arc::new(SamkhyaOptimizerRule::new()))
        .build();

    let ctx = SessionContext::new_with_state(state);

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(vec![1, 2, 3])),
            Arc::new(StringArray::from(vec!["a", "b", "c"])),
        ],
    )
    .expect("record batch");
    let table = MemTable::try_new(Arc::clone(&schema), vec![vec![batch]]).expect("mem table");
    ctx.register_table("t", Arc::new(table))
        .expect("register table");

    let df = ctx
        .sql("SELECT id, name FROM t WHERE id > 1")
        .await
        .expect("query should plan");
    let batches = df.collect().await.expect("query should execute");
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 2);
}
