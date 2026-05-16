//! Battle-hardening fortress for samkhya-polars (H06).
//!
//! Validates:
//! 1. Optimized vs unoptimized LazyFrame plan produces identical results when
//!    samkhya feedback wrapping is attached.
//! 2. Adversarial DataFrames (empty, all-null, 50-col wide with List / Struct /
//!    Categorical columns) feed through every Series→Sketch helper without
//!    panicking.

#![cfg(feature = "engine")]

use std::panic::AssertUnwindSafe;

use polars::frame::DataFrame;
use polars::prelude::{
    AnyValue, CategoricalOrdering, DataType, IntoLazy, IntoSeries, ListChunked, NamedFrom, Series,
    StructChunked, col, lit,
};

use samkhya_core::feedback::FeedbackStore;
use samkhya_polars::feedback_wrapper::lazy_collect_with_feedback;
use samkhya_polars::sketcher::{
    bloom_from_series, cms_from_series, histogram_from_series, hll_from_series,
};

/// (6) LazyFrame stats path: build a LazyFrame from a Vec<u64>, run a filter
/// query through the optimized planner with samkhya feedback recording, and
/// verify the result matches the unoptimized baseline.
#[test]
fn lazyframe_optimized_matches_unoptimized_baseline() {
    let values: Vec<u64> = (0..1_000u64).collect();
    let s = Series::new("v".into(), values);
    let df = s.into_frame();

    // Baseline: optimizer disabled.
    let unopt = df
        .clone()
        .lazy()
        .without_optimizations()
        .filter(col("v").gt(lit(500u64)))
        .collect()
        .expect("unoptimized collect");

    // Optimized path, traced through the samkhya feedback wrapper. The wrapper
    // must not mutate the result.
    let store = FeedbackStore::open_in_memory().expect("open store");
    let lf = df.lazy().filter(col("v").gt(lit(500u64)));
    let opt = lazy_collect_with_feedback(lf, &store, "tmpl-fortress-filter")
        .expect("optimized collect through wrapper");

    assert_eq!(opt.height(), unopt.height(), "row count mismatch");
    assert_eq!(opt.width(), unopt.width(), "column count mismatch");
    // Element-wise equality on the single u64 column.
    let opt_col = opt.column("v").unwrap();
    let unopt_col = unopt.column("v").unwrap();
    assert!(
        opt_col.equals_missing(unopt_col),
        "optimized output diverges from unoptimized baseline"
    );

    // Feedback observation got recorded.
    let history = store.history("tmpl-fortress-filter").expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].actual_rows as usize, unopt.height());
}

// ---------------------------------------------------------------------------
// (7) Adversarial DataFrames must not panic.
// ---------------------------------------------------------------------------

/// Run every sketcher helper on `s` under `catch_unwind` and assert no panic.
/// Errors (e.g. histogram refusing a non-numeric column) are expected and OK;
/// panics are not.
fn assert_sketchers_dont_panic(label: &str, s: &Series) {
    let s_owned = s.clone();
    // Series is not RefUnwindSafe because it holds Arc<dyn SeriesTrait>; we
    // wrap with AssertUnwindSafe because our sketchers do not observe any
    // logical invariant violations even if Polars internals were to panic.
    let res = std::panic::catch_unwind(AssertUnwindSafe(move || {
        let _ = hll_from_series(&s_owned, 10);
        let _ = bloom_from_series(&s_owned, 0.01);
        let _ = cms_from_series(&s_owned, 4, 256);
        let _ = histogram_from_series(&s_owned, 8);
    }));
    assert!(
        res.is_ok(),
        "sketcher helpers panicked on adversarial column [{label}]"
    );
}

#[test]
fn adversarial_empty_dataframe() {
    let s = Series::new_empty("c".into(), &DataType::Int64);
    assert_eq!(s.len(), 0);
    assert_sketchers_dont_panic("empty-i64", &s);

    // Wrapper on an empty LazyFrame.
    let df = DataFrame::new(vec![s.into()]).expect("df");
    let store = FeedbackStore::open_in_memory().expect("store");
    let df_out = lazy_collect_with_feedback(df.lazy(), &store, "tmpl-empty").expect("collect");
    assert_eq!(df_out.height(), 0);
}

#[test]
fn adversarial_all_null_column() {
    // 100 rows of Int64 nulls.
    let av_nulls: Vec<AnyValue<'_>> = (0..100).map(|_| AnyValue::Null).collect();
    let s = Series::from_any_values_and_dtype("c".into(), &av_nulls, &DataType::Int64, true)
        .expect("all-null series");
    assert_eq!(s.len(), 100);
    assert_eq!(s.null_count(), 100);
    assert_sketchers_dont_panic("all-null-i64", &s);

    // HLL on an all-null column should estimate 0 distinct values.
    let hll = hll_from_series(&s, 10).expect("hll on nulls");
    assert_eq!(hll.estimate(), 0);
}

#[test]
fn adversarial_wide_dataframe_with_unusual_types() {
    // 50 columns spanning every dtype-* feature we enable, including the
    // historically-painful List / Struct / Categorical variants.
    let mut columns: Vec<polars::prelude::Column> = Vec::with_capacity(50);

    // 1: List<Int32>
    {
        let inner_a = Series::new("".into(), &[1i32, 2, 3]);
        let inner_b = Series::new("".into(), &[4i32, 5]);
        let inner_c = Series::new("".into(), Vec::<i32>::new());
        let lc = ListChunked::from_iter([Some(inner_a), Some(inner_b), None, Some(inner_c)]);
        let mut s: Series = lc.into_series();
        s.rename("list_i32".into());
        columns.push(s.into());
    }

    // 2: Struct{a: i64, b: String}
    {
        let a = Series::new("a".into(), &[10i64, 20, 30, 40]);
        let b = Series::new("b".into(), &["x", "y", "z", "w"]);
        let fields = [a, b];
        let sc = StructChunked::from_series("struct_col".into(), 4, fields.iter())
            .expect("struct chunk")
            .into_series();
        columns.push(sc.into());
    }

    // 3: Categorical
    {
        let raw = Series::new("cat".into(), &["a", "b", "a", "c"]);
        let cat = raw
            .cast(&DataType::Categorical(None, CategoricalOrdering::Physical))
            .expect("cast categorical");
        columns.push(cat.into());
    }

    // 4..=50: a mix of every primitive dtype the crate enables, all length 4.
    let primitive_builders: Vec<(String, Series)> = vec![
        (
            "i8_col".into(),
            Series::new("i8_col".into(), &[1i8, 2, 3, 4]),
        ),
        (
            "i16_col".into(),
            Series::new("i16_col".into(), &[1i16, 2, 3, 4]),
        ),
        (
            "i32_col".into(),
            Series::new("i32_col".into(), &[1i32, 2, 3, 4]),
        ),
        (
            "i64_col".into(),
            Series::new("i64_col".into(), &[1i64, 2, 3, 4]),
        ),
        (
            "u8_col".into(),
            Series::new("u8_col".into(), &[1u8, 2, 3, 4]),
        ),
        (
            "u16_col".into(),
            Series::new("u16_col".into(), &[1u16, 2, 3, 4]),
        ),
        (
            "u32_col".into(),
            Series::new("u32_col".into(), &[1u32, 2, 3, 4]),
        ),
        (
            "u64_col".into(),
            Series::new("u64_col".into(), &[1u64, 2, 3, 4]),
        ),
        (
            "f32_col".into(),
            Series::new("f32_col".into(), &[1.0f32, 2.0, 3.0, 4.0]),
        ),
        (
            "f64_col".into(),
            Series::new("f64_col".into(), &[1.0f64, 2.0, 3.0, 4.0]),
        ),
        (
            "bool_col".into(),
            Series::new("bool_col".into(), &[true, false, true, false]),
        ),
        (
            "str_col".into(),
            Series::new("str_col".into(), &["a", "b", "c", "d"]),
        ),
    ];
    for (_, s) in primitive_builders {
        columns.push(s.into());
    }

    // Pad out to 50 total columns with deterministic Int64 noise so we exercise
    // the wide-frame code path.
    let mut idx: i64 = 0;
    while columns.len() < 50 {
        let name = format!("pad_{idx}");
        let s = Series::new(name.as_str().into(), &[idx, idx + 1, idx + 2, idx + 3]);
        columns.push(s.into());
        idx += 1;
    }
    assert_eq!(columns.len(), 50);

    let df = DataFrame::new(columns).expect("wide df");
    assert_eq!(df.width(), 50);
    assert_eq!(df.height(), 4);

    // Every column must survive each sketcher without panicking.
    for col_name in df.get_column_names_owned() {
        let s = df
            .column(&col_name)
            .unwrap()
            .as_materialized_series()
            .clone();
        assert_sketchers_dont_panic(&format!("wide:{col_name}"), &s);
    }

    // The feedback wrapper must round-trip a 50-column LazyFrame too.
    let store = FeedbackStore::open_in_memory().expect("store");
    let out = lazy_collect_with_feedback(df.lazy(), &store, "tmpl-wide-50").expect("wide collect");
    assert_eq!(out.width(), 50);
    assert_eq!(out.height(), 4);
}

#[test]
fn adversarial_struct_and_list_through_hll_and_cms() {
    // List<Int64>: distinct estimate must succeed even though contents hash via
    // debug-format fallback.
    let inner_a = Series::new("".into(), &[1i64, 2, 3]);
    let inner_b = Series::new("".into(), &[4i64, 5]);
    let lc = ListChunked::from_iter([Some(inner_a.clone()), Some(inner_b), Some(inner_a)]);
    let s: Series = lc.into_series();
    let hll = hll_from_series(&s, 10).expect("hll on list");
    // We don't pin an exact estimate (debug-format is implementation-defined),
    // but the result must be in [1, 3] inclusive for these three rows.
    let est = hll.estimate();
    assert!((1..=3).contains(&est), "HLL estimate {est} out of range");

    // CMS on a Struct column also must not panic; we only assert it returns Ok.
    let a = Series::new("a".into(), &[10i64, 20, 30]);
    let b = Series::new("b".into(), &["x", "y", "x"]);
    let fields = [a, b];
    let sc = StructChunked::from_series("st".into(), 3, fields.iter())
        .expect("struct chunk")
        .into_series();
    let _ = cms_from_series(&sc, 4, 256).expect("cms on struct");
}
