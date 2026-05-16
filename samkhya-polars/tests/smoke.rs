//! Smoke tests for the `engine` feature: exercise every Series→Sketch
//! builder against tiny, deterministic inputs.

#![cfg(feature = "engine")]

use polars::prelude::{IntoLazy, NamedFrom, Series};

use samkhya_core::feedback::FeedbackStore;
use samkhya_polars::feedback_wrapper::lazy_collect_with_feedback;
use samkhya_polars::sketcher::{
    bloom_from_series, cms_from_series, histogram_from_series, hll_from_series,
};

#[test]
fn hll_distinct_count_within_relative_error() {
    // 1000 u32 values with exactly 500 distinct.
    let values: Vec<u32> = (0..1000u32).map(|i| i % 500).collect();
    let s = Series::new("c".into(), values);

    let hll = hll_from_series(&s, 12).expect("hll build");
    let est = hll.estimate() as f64;
    let err = (est - 500.0).abs() / 500.0;
    assert!(err < 0.15, "HLL estimate {est} off by {err} (target 500)");
}

#[test]
fn bloom_has_no_false_negatives_on_strings() {
    let labels: Vec<String> = (0..200u32).map(|i| format!("item-{i:04}")).collect();
    let s = Series::new("c".into(), labels.clone());

    let bloom = bloom_from_series(&s, 0.01).expect("bloom build");
    for label in &labels {
        assert!(
            bloom.contains(label.as_bytes()),
            "false negative for {label}"
        );
    }
}

#[test]
fn cms_counts_inserted_items() {
    // 1000 rows, one heavy hitter repeated 500x.
    let mut values: Vec<i64> = (0..500).map(|i| i as i64).collect();
    values.extend(std::iter::repeat_n(42i64, 500));
    let s = Series::new("c".into(), values);

    let cms = cms_from_series(&s, 5, 1024).expect("cms build");
    let heavy = cms.estimate(&42i64.to_le_bytes());
    assert!(
        heavy >= 500,
        "heavy-hitter undercount: got {heavy}, expected >= 500"
    );
    assert!(
        heavy < 600,
        "heavy-hitter wildly overcounts: got {heavy} on a 1000-element series"
    );
}

#[test]
fn histogram_total_matches_series_length() {
    let values: Vec<f64> = (0..1000).map(|i| i as f64 * 0.5).collect();
    let s = Series::new("c".into(), values);

    let hist = histogram_from_series(&s, 16).expect("histogram build");
    assert_eq!(hist.total() as usize, s.len());
}

#[test]
fn histogram_rejects_non_numeric_series() {
    let s = Series::new("c".into(), vec!["a".to_string(), "b".to_string()]);
    let err = histogram_from_series(&s, 4).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not numeric"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn feedback_wrapper_records_actual_rows() {
    let values: Vec<i64> = (0..123).collect();
    let s = Series::new("c".into(), values);
    let lf = s.into_frame().lazy();

    let store = FeedbackStore::open_in_memory().expect("open store");
    let df = lazy_collect_with_feedback(lf, &store, "tmpl-polars-smoke").expect("collect");
    assert_eq!(df.height(), 123);

    let history = store.history("tmpl-polars-smoke").expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].actual_rows, 123);
    assert_eq!(history[0].est_rows, 0); // Polars exposes no plan estimate.
    assert!(history[0].latency_ms.unwrap_or_default() >= 0.0);
}
