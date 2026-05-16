//! Residual correction model.
//!
//! Optional learning layer. Takes a baseline cardinality estimate plus a
//! feature vector (query plan + column stats) and returns a corrected
//! estimate. Trained on observations recorded by [`crate::feedback`].
//!
//! Contracts every backend honors:
//!
//! - bounded — output never exceeds the LpBound ceiling ([`crate::lpbound`]); the corrector clamps.
//! - sub-MB / sub-ms — model footprint and per-estimate latency are the architectural budget.
//! - optional — engines opt in; with no model attached, samkhya behaves as portable stats + envelope.
//!
//! Concrete backends (all behind feature flags):
//!
//! - `gbt` — gradient-boosted trees (the [`gbt`] submodule, gated on the `gbt` cargo feature)
//! - `tabpfn` — TabPFN-style foundation-model wrapper (network or local, future)

use crate::Result;

/// Feature vector handed to the corrector at estimate time.
///
/// Intentionally minimal at v0.0.1: row count + distinct count + null
/// count + a small set of operator-level features. Will grow as the
/// feedback-collection surface widens.
#[derive(Debug, Clone, Default)]
pub struct CorrectionFeatures {
    pub baseline_estimate: u64,
    pub left_input_rows: Option<u64>,
    pub right_input_rows: Option<u64>,
    pub left_distinct: Option<u64>,
    pub right_distinct: Option<u64>,
    pub predicate_count: u32,
    pub join_depth: u32,
}

impl CorrectionFeatures {
    /// Flatten the feature struct into a fixed-length numeric vector for a
    /// regression model. `Option<u64>` slots are zero-filled when absent —
    /// callers should treat zero as "unknown" rather than "literally zero
    /// rows", which is the convention the corrector is trained against.
    ///
    /// Layout (stable; new features must be appended, never reordered):
    ///
    /// 0. `baseline_estimate`
    /// 1. `left_input_rows`  (0 if `None`)
    /// 2. `right_input_rows` (0 if `None`)
    /// 3. `left_distinct`    (0 if `None`)
    /// 4. `right_distinct`   (0 if `None`)
    /// 5. `predicate_count`
    /// 6. `join_depth`
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.baseline_estimate as f64,
            self.left_input_rows.unwrap_or(0) as f64,
            self.right_input_rows.unwrap_or(0) as f64,
            self.left_distinct.unwrap_or(0) as f64,
            self.right_distinct.unwrap_or(0) as f64,
            f64::from(self.predicate_count),
            f64::from(self.join_depth),
        ]
    }

    /// Number of entries [`to_vec`](Self::to_vec) produces.
    pub const FEATURE_LEN: usize = 7;
}

/// A pluggable corrector. Engines call [`correct`] on every estimate that
/// passes through samkhya's optimizer hook.
pub trait Corrector: Send + Sync {
    /// Return a corrected estimate, or `None` to fall back to the baseline.
    fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>>;

    /// Stable identifier for logging / model-version tracking.
    fn name(&self) -> &'static str;
}

/// Default zero-cost corrector: passes the baseline through unchanged.
///
/// Used when no feedback history exists yet (cold start) or when the
/// caller opts out of learned correction entirely.
pub struct IdentityCorrector;

impl Corrector for IdentityCorrector {
    fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>> {
        Ok(Some(features.baseline_estimate))
    }

    fn name(&self) -> &'static str {
        "identity"
    }
}

#[cfg(feature = "gbt")]
pub mod gbt {
    //! Gradient-boosted-tree residual corrector.
    //!
    //! Wraps the `gbdt` crate (Baidu / mesalock-linux,
    //! <https://github.com/mesalock-linux/gbdt-rs>) — pure-Rust, no native
    //! deps, builds on stable Rust 1.94 / edition 2024. Compiled in only
    //! when the `gbt` cargo feature is enabled.
    //!
    //! Training target is `log(actual_rows / est_rows)` — the
    //! multiplicative correction ratio in log-space. At prediction time
    //! we exponentiate and multiply through the baseline, then clamp to
    //! the configured LpBound ceiling via
    //! [`crate::lpbound::saturating_clamp`] so the corrector cannot ever
    //! violate the envelope contract.
    //!
    //! Observations with `est_rows == 0` or `actual_rows == 0` are
    //! silently dropped (log of zero is undefined); we do not invent a
    //! Laplace-style smoothing constant at the corrector layer.

    use gbdt::config::{Config, Loss};
    use gbdt::decision_tree::{Data, DataVec};
    use gbdt::gradient_boost::GBDT;

    use super::{CorrectionFeatures, Corrector};
    use crate::feedback::Observation;
    use crate::lpbound::saturating_clamp;
    use crate::{Error, Result};

    /// Tunables for [`GbtCorrector::train`]. Defaults are an MVP starting
    /// point: shallow trees, modest depth, square-error loss.
    #[derive(Debug, Clone)]
    pub struct GbtOptions {
        /// Shrinkage / learning rate applied to each tree's contribution.
        pub learning_rate: f64,
        /// Max depth of each regression tree. Root is depth 0.
        pub max_depth: u32,
        /// Number of boosting iterations (one tree per iteration).
        pub num_trees: u32,
        /// Inclusive upper bound applied to every corrected estimate.
        /// Use `u64::MAX` to disable (the trait signature has no ceiling
        /// slot, so we store it here at train time).
        pub ceiling: u64,
        /// Minimum samples per leaf — guards against overfitting tiny
        /// feedback histories.
        pub min_leaf_size: usize,
    }

    impl Default for GbtOptions {
        fn default() -> Self {
            Self {
                learning_rate: 0.1,
                max_depth: 4,
                num_trees: 50,
                ceiling: u64::MAX,
                min_leaf_size: 1,
            }
        }
    }

    /// Trained GBT-backed residual corrector.
    pub struct GbtCorrector {
        model: GBDT,
        ceiling: u64,
    }

    impl GbtCorrector {
        /// Train a corrector from a slice of [`Observation`]s.
        ///
        /// Returns [`Error::Feedback`] if the observation slice is empty,
        /// or if every observation is unusable (zero est_rows or zero
        /// actual_rows). Non-positive-ratio observations are silently
        /// filtered, matching the convention in [`Observation::q_error`].
        pub fn train(observations: &[Observation], options: GbtOptions) -> Result<Self> {
            if observations.is_empty() {
                return Err(Error::Feedback(
                    "cannot train GbtCorrector: observation slice is empty".into(),
                ));
            }

            let mut training: DataVec = Vec::with_capacity(observations.len());
            for obs in observations {
                if obs.est_rows == 0 || obs.actual_rows == 0 {
                    continue;
                }
                // Reconstruct a feature vector from the observation. The
                // feedback table doesn't yet carry full plan features, so
                // we synthesize the minimal `baseline_estimate`-only
                // vector. As `Observation` gains columns the mapping
                // below should grow in lockstep with `CorrectionFeatures`.
                let features = CorrectionFeatures {
                    baseline_estimate: obs.est_rows,
                    ..Default::default()
                };
                let feature_f32: Vec<f32> =
                    features.to_vec().into_iter().map(|v| v as f32).collect();
                let ratio_log = (obs.actual_rows as f64 / obs.est_rows as f64).ln() as f32;
                training.push(Data::new_training_data(feature_f32, 1.0, ratio_log, None));
            }

            if training.is_empty() {
                return Err(Error::Feedback(
                    "cannot train GbtCorrector: all observations had zero est or actual rows"
                        .into(),
                ));
            }

            let mut cfg = Config::new();
            cfg.set_feature_size(CorrectionFeatures::FEATURE_LEN);
            cfg.set_max_depth(options.max_depth);
            cfg.set_iterations(options.num_trees as usize);
            cfg.set_shrinkage(options.learning_rate as f32);
            cfg.set_min_leaf_size(options.min_leaf_size);
            cfg.set_loss(&loss_name(Loss::SquaredError));

            let mut model = GBDT::new(&cfg);
            model.fit(&mut training);

            Ok(Self {
                model,
                ceiling: options.ceiling,
            })
        }

        /// Predict the log-ratio correction for a single feature vector.
        /// Exposed for diagnostics / unit tests; the production path is
        /// [`Corrector::correct`].
        pub fn predict_log_ratio(&self, features: &CorrectionFeatures) -> f64 {
            let feature_f32: Vec<f32> = features.to_vec().into_iter().map(|v| v as f32).collect();
            let probe: DataVec = vec![Data::new_test_data(feature_f32, None)];
            let preds = self.model.predict(&probe);
            preds.first().copied().unwrap_or(0.0) as f64
        }

        /// Configured upper bound. Set at training time; the trait method
        /// [`Corrector::correct`] enforces it via `saturating_clamp`.
        pub fn ceiling(&self) -> u64 {
            self.ceiling
        }
    }

    impl Corrector for GbtCorrector {
        fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>> {
            let log_ratio = self.predict_log_ratio(features);
            let ratio = log_ratio.exp();
            let scaled = features.baseline_estimate as f64 * ratio;
            Ok(Some(saturating_clamp(scaled, self.ceiling)))
        }

        fn name(&self) -> &'static str {
            "gbt"
        }
    }

    /// `gbdt::config::Config::set_loss` takes a string; this is the
    /// canonical spelling for square-error in that crate.
    fn loss_name(loss: Loss) -> String {
        gbdt::config::loss2string(&loss)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_returns_baseline() {
        let corrector = IdentityCorrector;
        let features = CorrectionFeatures {
            baseline_estimate: 1234,
            ..Default::default()
        };
        assert_eq!(corrector.correct(&features).unwrap(), Some(1234));
        assert_eq!(corrector.name(), "identity");
    }

    #[test]
    fn feature_vec_layout_is_stable() {
        let f = CorrectionFeatures {
            baseline_estimate: 100,
            left_input_rows: Some(10),
            right_input_rows: None,
            left_distinct: Some(7),
            right_distinct: None,
            predicate_count: 3,
            join_depth: 2,
        };
        let v = f.to_vec();
        assert_eq!(v.len(), CorrectionFeatures::FEATURE_LEN);
        assert_eq!(v[0], 100.0);
        assert_eq!(v[1], 10.0);
        assert_eq!(v[2], 0.0); // None → 0
        assert_eq!(v[3], 7.0);
        assert_eq!(v[4], 0.0);
        assert_eq!(v[5], 3.0);
        assert_eq!(v[6], 2.0);
    }
}

#[cfg(all(test, feature = "gbt"))]
mod gbt_tests {
    use super::gbt::{GbtCorrector, GbtOptions};
    use super::{CorrectionFeatures, Corrector};
    use crate::feedback::Observation;

    /// Build N synthetic observations where `actual = est * 2` for a
    /// spread of est values. Plenty of signal for the trees to latch on.
    fn synthetic_double(n: u64) -> Vec<Observation> {
        (1..=n)
            .map(|i| Observation {
                template_hash: "syn".into(),
                plan_fingerprint: "p".into(),
                est_rows: i * 10,
                actual_rows: i * 10 * 2,
                latency_ms: None,
            })
            .collect()
    }

    #[test]
    fn predicts_roughly_double_when_training_says_double() {
        let obs = synthetic_double(200);
        let opts = GbtOptions {
            learning_rate: 0.3,
            max_depth: 4,
            num_trees: 50,
            ceiling: u64::MAX,
            min_leaf_size: 1,
        };
        let corrector = GbtCorrector::train(&obs, opts).expect("training");

        let features = CorrectionFeatures {
            baseline_estimate: 500,
            ..Default::default()
        };
        let corrected = corrector
            .correct(&features)
            .expect("correct")
            .expect("Some");
        // True target is 1000. Trees won't be exact; require within 25%.
        let ratio = corrected as f64 / 1000.0;
        assert!(
            (0.75..=1.25).contains(&ratio),
            "expected ~1000, got {} (ratio {})",
            corrected,
            ratio
        );
        assert_eq!(corrector.name(), "gbt");
    }

    #[test]
    fn ceiling_clamps_when_prediction_exceeds_it() {
        let obs = synthetic_double(200);
        let opts = GbtOptions {
            learning_rate: 0.3,
            max_depth: 4,
            num_trees: 50,
            ceiling: 100, // far below 2 × baseline
            min_leaf_size: 1,
        };
        let corrector = GbtCorrector::train(&obs, opts).expect("training");

        let features = CorrectionFeatures {
            baseline_estimate: 500,
            ..Default::default()
        };
        let corrected = corrector
            .correct(&features)
            .expect("correct")
            .expect("Some");
        assert_eq!(corrected, 100, "ceiling must clamp the corrected estimate");
        assert_eq!(corrector.ceiling(), 100);
    }

    #[test]
    fn empty_observations_errors() {
        match GbtCorrector::train(&[], GbtOptions::default()) {
            Ok(_) => panic!("expected error on empty observations"),
            Err(e) => assert!(matches!(e, crate::Error::Feedback(_))),
        }
    }

    #[test]
    fn all_zero_observations_errors() {
        let obs = vec![
            Observation {
                template_hash: "z".into(),
                plan_fingerprint: "p".into(),
                est_rows: 0,
                actual_rows: 5,
                latency_ms: None,
            },
            Observation {
                template_hash: "z".into(),
                plan_fingerprint: "p".into(),
                est_rows: 5,
                actual_rows: 0,
                latency_ms: None,
            },
        ];
        match GbtCorrector::train(&obs, GbtOptions::default()) {
            Ok(_) => panic!("expected error when all observations are zero"),
            Err(e) => assert!(matches!(e, crate::Error::Feedback(_))),
        }
    }
}
