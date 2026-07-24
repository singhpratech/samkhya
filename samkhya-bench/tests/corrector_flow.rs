// SPDX-License-Identifier: Apache-2.0
//
// samkhya-bench: the train -> freeze -> evaluate loop, and the query filter
// that makes it honest.
//
// Sole author: Prateek Singh.
//
// The WAVE4-F campaign published a "corrected" arm that contained no
// corrector, because no CLI option could attach one. These tests cover the
// pieces that made that possible: whether a corrector can be trained at all,
// whether a fitted model survives a process boundary, and whether the
// evaluation set can be held out from the training set.

use samkhya_bench::queries::Suite;
use samkhya_bench::runner::Runner;
#[cfg(feature = "gbt-flow")]
use samkhya_core::feedback::PlanObservation;
use samkhya_core::feedback::{FeedbackStore, Observation};
#[cfg(feature = "gbt-flow")]
use samkhya_core::residual::CorrectionFeatures;

fn temp_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("samkhya_bench_corrector_flow");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

/// `--only` must actually restrict the run. A filter that parses but does
/// not filter is worse than none: it silently reports a full-corpus result
/// for a partial run.
#[test]
fn only_restricts_the_executed_queries() {
    let all = Suite::Synthetic.queries().len();
    assert!(all > 2, "synthetic suite should have several queries");

    let runner =
        Runner::new(Suite::Synthetic, true).with_only(vec!["S1".to_string(), "S2".to_string()]);
    assert_eq!(runner.selected_query_names(), vec!["S1", "S2"]);
}

/// `--exclude` is applied after `--only`, so the two compose into a
/// complement without the caller having to enumerate it twice.
#[test]
fn exclude_applies_after_only() {
    let runner = Runner::new(Suite::Synthetic, true)
        .with_only(vec!["S1".into(), "S2".into(), "S3".into()])
        .with_exclude(vec!["S2".into()]);
    assert_eq!(runner.selected_query_names(), vec!["S1", "S3"]);
}

/// With no filter the full suite runs, so the default path is unchanged.
#[test]
fn no_filter_selects_the_whole_suite() {
    let runner = Runner::new(Suite::Synthetic, true);
    assert_eq!(
        runner.selected_query_names().len(),
        Suite::Synthetic.queries().len()
    );
}

/// An unknown name in `--only` selects nothing rather than silently
/// falling back to the whole suite, which would turn a typo into a
/// full-corpus run reported as a filtered one.
#[test]
fn an_unknown_only_name_selects_nothing() {
    let runner = Runner::new(Suite::Synthetic, true).with_only(vec!["NOPE".into()]);
    assert!(runner.selected_query_names().is_empty());
}

/// The training and evaluation sets a held-out measurement needs must be
/// genuinely disjoint.
#[test]
fn only_and_exclude_partition_the_suite() {
    let train = Runner::new(Suite::Synthetic, true)
        .with_only(vec!["S1".into(), "S2".into(), "S3".into()])
        .selected_query_names();
    let eval = Runner::new(Suite::Synthetic, true)
        .with_exclude(vec!["S1".into(), "S2".into(), "S3".into()])
        .selected_query_names();

    for name in &train {
        assert!(
            !eval.contains(name),
            "{name} appears in both the training and evaluation sets"
        );
    }
    assert_eq!(
        train.len() + eval.len(),
        Suite::Synthetic.queries().len(),
        "the two halves should cover the suite exactly once"
    );
}

/// End to end: record featured observations, train, freeze, reload, and
/// confirm the reloaded model is usable as a `Corrector`.
#[test]
#[cfg(feature = "gbt-flow")]
fn train_freeze_reload_round_trip() {
    use samkhya_core::residual::Corrector;
    use samkhya_core::residual::gbt::GbtOptions;

    let db = temp_path("flow.db");
    let model = temp_path("flow.json");
    std::fs::remove_file(&db).ok();
    std::fs::remove_file(&model).ok();

    let store = FeedbackStore::open(&db).expect("store opens");
    for i in 1..60u64 {
        store
            .record_plan(&PlanObservation {
                template_hash: "flow".into(),
                plan_fingerprint: format!("p{i}"),
                features: CorrectionFeatures {
                    baseline_estimate: i * 10,
                    join_depth: (i % 3) as u32,
                    predicate_count: (i % 2) as u32,
                    ..Default::default()
                },
                actual_rows: i * 30,
                latency_ms: None,
            })
            .expect("records");
    }
    drop(store);

    samkhya_bench::report::train(&db, "flow", &model, GbtOptions::default())
        .expect("training succeeds");
    assert!(model.exists(), "model file was not written");

    let loaded =
        samkhya_core::residual::gbt::GbtCorrector::load(&model, u64::MAX).expect("model loads");
    let probe = CorrectionFeatures {
        baseline_estimate: 100,
        join_depth: 1,
        ..Default::default()
    };
    let corrected = loaded.correct(&probe).expect("corrects").expect("some");
    assert!(corrected > 0, "a trained corrector should produce a value");

    std::fs::remove_file(&db).ok();
    std::fs::remove_file(&model).ok();
}

/// Training must refuse a store whose rows carry no features rather than
/// padding them with zeros — that is exactly how the model ends up blind.
#[test]
fn training_refuses_featureless_observations() {
    use samkhya_core::residual::gbt::GbtOptions;

    let db = temp_path("legacy_only.db");
    let model = temp_path("legacy_only.json");
    std::fs::remove_file(&db).ok();

    let store = FeedbackStore::open(&db).expect("store opens");
    for i in 1..20u64 {
        store
            .record(&Observation {
                template_hash: "legacy".into(),
                plan_fingerprint: format!("p{i}"),
                est_rows: i * 10,
                actual_rows: i * 30,
                latency_ms: None,
            })
            .expect("records");
    }
    drop(store);

    let err = samkhya_bench::report::train(&db, "legacy", &model, GbtOptions::default())
        .expect_err("training on featureless rows must fail");
    let message = err.to_string();
    assert!(
        message.contains("none carrying plan features"),
        "error should explain why the rows are unusable, got: {message}"
    );
    assert!(
        !model.exists(),
        "no model should be written when training is refused"
    );

    std::fs::remove_file(&db).ok();
}
