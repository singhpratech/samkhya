// SPDX-License-Identifier: Apache-2.0
//
// samkhya-core: the corrector must actually use the features it is given.
//
// Sole author: Prateek Singh.
//
// Through v1.1 `GbtCorrector::train` synthesised a feature vector with only
// `baseline_estimate` populated, because `Observation` carried nothing else.
// Six of the seven features were therefore constant across every training
// row, no tree ever split on them, and the model was one-dimensional — while
// the DataFusion adapter fed it seven live features at inference time.
//
// Nothing failed. The corrector was simply blind, which is why this survived
// two releases. These tests make the blindness visible and assert it is gone.

#![cfg(feature = "gbt")]

use samkhya_core::feedback::{FeedbackStore, Observation, PlanObservation};
use samkhya_core::residual::gbt::{GbtCorrector, GbtOptions};
use samkhya_core::residual::{CorrectionFeatures, Corrector};

/// Observations where the true row count depends *only* on `join_depth`,
/// and the baseline estimate carries no signal at all.
///
/// A corrector that reads its features can learn this. One that sees only
/// `baseline_estimate` cannot, and must predict the same value for every
/// depth.
fn depth_driven_observations() -> Vec<PlanObservation> {
    let mut out = Vec::new();
    for i in 0..200u64 {
        let depth = (i % 4) as u32;
        // Baseline is deliberately constant: all the signal is in the depth.
        let baseline = 1_000;
        let actual = baseline * u64::from(depth + 1);
        out.push(PlanObservation {
            template_hash: "depth-driven".into(),
            plan_fingerprint: format!("plan#{depth}"),
            features: CorrectionFeatures {
                baseline_estimate: baseline,
                join_depth: depth,
                ..Default::default()
            },
            actual_rows: actual,
            latency_ms: None,
        });
    }
    out
}

fn probe(depth: u32) -> CorrectionFeatures {
    CorrectionFeatures {
        baseline_estimate: 1_000,
        join_depth: depth,
        ..Default::default()
    }
}

#[test]
fn training_on_plans_uses_features_beyond_the_baseline() {
    let observations = depth_driven_observations();
    let corrector = GbtCorrector::train_on_plans(&observations, GbtOptions::default())
        .expect("training succeeds");
    assert_eq!(corrector.training_rows(), 200);

    let shallow = corrector.correct(&probe(0)).unwrap().unwrap();
    let deep = corrector.correct(&probe(3)).unwrap().unwrap();

    assert!(
        deep > shallow,
        "corrector ignored join_depth: depth 0 -> {shallow}, depth 3 -> {deep}"
    );
    // The relationship in the data is actual = baseline * (depth + 1), so a
    // corrector that reads the feature should separate the two ends widely.
    assert!(
        deep >= shallow * 2,
        "corrector barely used join_depth: depth 0 -> {shallow}, depth 3 -> {deep}"
    );
}

#[test]
fn legacy_training_is_blind_to_everything_but_the_baseline() {
    // The same data through the legacy path, which drops the features.
    let legacy: Vec<Observation> = depth_driven_observations()
        .iter()
        .map(|o| o.to_observation())
        .collect();
    let corrector = GbtCorrector::train(&legacy, GbtOptions::default()).expect("training succeeds");

    let shallow = corrector.correct(&probe(0)).unwrap().unwrap();
    let deep = corrector.correct(&probe(3)).unwrap().unwrap();

    assert_eq!(
        shallow, deep,
        "legacy training is expected to ignore join_depth entirely; if this now \
         differs the legacy path has changed and its documentation is stale"
    );
}

#[test]
fn a_saved_model_predicts_identically_after_reload() {
    let observations = depth_driven_observations();
    let trained = GbtCorrector::train_on_plans(&observations, GbtOptions::default())
        .expect("training succeeds");

    let dir = std::env::temp_dir().join("samkhya_model_roundtrip");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("gbt.json");
    trained.save(&path).expect("model saves");

    let reloaded = GbtCorrector::load(&path, u64::MAX).expect("model loads");
    for depth in 0..4 {
        assert_eq!(
            trained.correct(&probe(depth)).unwrap(),
            reloaded.correct(&probe(depth)).unwrap(),
            "prediction changed across save/load at depth {depth}"
        );
    }
    // A reloaded model does not carry its training-set size.
    assert_eq!(reloaded.training_rows(), 0);

    std::fs::remove_file(&path).ok();
}

#[test]
fn the_store_round_trips_plan_features() {
    let store = FeedbackStore::open_in_memory().expect("store opens");
    for obs in depth_driven_observations().iter().take(5) {
        store.record_plan(obs).expect("records");
    }

    let history = store.plan_history("depth-driven").expect("reads back");
    assert_eq!(history.len(), 5);
    assert_eq!(history[0].features.baseline_estimate, 1_000);
    assert_eq!(history[1].features.join_depth, 1);
    assert_eq!(history[3].features.join_depth, 3);
}

/// Legacy rows carry no features, so training on them would silently
/// reintroduce the skew. `plan_history` must not return them.
#[test]
fn plan_history_skips_featureless_rows() {
    let store = FeedbackStore::open_in_memory().expect("store opens");
    store
        .record(&Observation {
            template_hash: "mixed".into(),
            plan_fingerprint: "legacy".into(),
            est_rows: 10,
            actual_rows: 100,
            latency_ms: None,
        })
        .expect("records legacy row");
    store
        .record_plan(&PlanObservation {
            template_hash: "mixed".into(),
            plan_fingerprint: "with-features".into(),
            features: CorrectionFeatures {
                baseline_estimate: 10,
                join_depth: 2,
                ..Default::default()
            },
            actual_rows: 100,
            latency_ms: None,
        })
        .expect("records plan row");

    // Both rows land in the same table...
    assert_eq!(store.count().unwrap(), 2);
    // ...but only the one carrying features is trainable.
    let trainable = store.plan_history("mixed").expect("reads back");
    assert_eq!(trainable.len(), 1);
    assert_eq!(trainable[0].plan_fingerprint, "with-features");
    // And the legacy view still sees both.
    assert_eq!(store.history("mixed").expect("legacy read").len(), 2);
}
