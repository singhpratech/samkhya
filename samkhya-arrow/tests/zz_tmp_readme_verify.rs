//! TEMPORARY verification of the README example. Delete after checking.
use arrow::array::{Int64Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use std::sync::Arc;

use samkhya_arrow::batch::build_column_sketches;
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};

#[test]
fn readme_example_verbatim() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "order_id",
        DataType::Int64,
        false,
    )]));
    let col = |v: Vec<i64>| {
        RecordBatch::try_new(schema.clone(), vec![Arc::new(Int64Array::from(v))]).unwrap()
    };

    let orders = col((0..10i64).collect());
    let line_items = col((0..100i64).map(|i| i % 10).collect());

    let order_hll = &build_column_sketches(&orders, 12)?[0];
    let item_hll = &build_column_sketches(&line_items, 12)?[0];

    const ORDER_ID: u32 = 0;
    let graph = JoinGraph::new(vec![
        JoinRelation::new(10).with_degree(ORDER_ID, AttributeDegree::from_hll_floor(10, order_hll)),
        JoinRelation::new(100)
            .with_degree(ORDER_ID, AttributeDegree::from_hll_floor(100, item_hll)),
    ])
    .with_edge(0, 1, ORDER_ID);

    assert_eq!(graph.ceiling(), 100);
    Ok(())
}
