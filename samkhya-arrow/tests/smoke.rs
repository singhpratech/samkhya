//! Smoke test: build a small heterogeneous RecordBatch, run it through
//! the batch helpers, check the shapes line up with the schema.

use std::sync::Arc;

use arrow::array::{Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use samkhya_arrow::batch::{build_column_sketches, build_histograms};

fn small_batch() -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("score", DataType::Float64, false),
    ]));
    let ids = Int64Array::from(vec![1i64, 2, 3, 4, 5]);
    let names = StringArray::from(vec!["alice", "bob", "carol", "dave", "eve"]);
    let scores = Float64Array::from(vec![0.1f64, 0.5, 0.9, 1.5, 2.1]);
    RecordBatch::try_new(
        schema,
        vec![Arc::new(ids), Arc::new(names), Arc::new(scores)],
    )
    .expect("schema/columns line up")
}

#[test]
fn build_column_sketches_one_per_column() {
    let batch = small_batch();
    let sketches = build_column_sketches(&batch, 12).expect("hll precision in range");
    assert_eq!(sketches.len(), 3, "one HLL per column");
    // Each HLL should see 5 distinct values — the estimate may wobble
    // but at p=12 the small-cardinality (linear-counting) branch lands
    // exactly on 5 for 5 unique items.
    for hll in &sketches {
        let est = hll.estimate();
        assert!(
            (3..=7).contains(&est),
            "expected ~5 distinct values, got {est}"
        );
    }
}

#[test]
fn build_histograms_none_for_string_column() {
    let batch = small_batch();
    let hists = build_histograms(&batch, 4).expect("numeric columns build cleanly");
    assert_eq!(hists.len(), 3, "schema-aligned vector");
    assert!(hists[0].is_some(), "Int64 column produces a histogram");
    assert!(hists[1].is_none(), "Utf8 column slots in as None");
    assert!(hists[2].is_some(), "Float64 column produces a histogram");
}
