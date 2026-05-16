//! Residual correction model.
//!
//! Optional feedback-driven correction layer. Takes a baseline cardinality
//! estimate plus a feature vector (query plan + column stats) and returns
//! a corrected estimate. Trained on observations recorded by
//! [`crate::feedback`].
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
//! - `additive_gbt` — additive gradient-boosted trees ([`additive`], gated on `additive_gbt`)
//! - `tabpfn` — foundation-model interface (see the [`tabpfn`] submodule)
//!
//! # Foundation-model interface (Layer 5)
//!
//! The architecture reserves a pluggable backend slot for foundation tabular
//! models such as TabPFN-2.5 (arXiv 2511.08667). The contract is identical
//! to every other backend:
//!
//! > *feed [`CorrectionFeatures`], receive `Option<u64>` clamped to the
//! > LpBound ceiling.*
//!
//! Two deployment shapes are scaffolded:
//!
//! 1. **localhost HTTP** — a Python TabPFN inference server runs out of
//!    band; samkhya POSTs the feature vector as JSON and reads back an
//!    `{"estimate": <u64>}` response. Implemented today behind the
//!    `tabpfn_http` cargo feature (see [`tabpfn::TabPfnHttpCorrector`]).
//! 2. **subprocess** — samkhya spawns a Python child, frames JSON over
//!    stdin/stdout, and keeps the process warm across estimates. Deferred:
//!    the scaffolding is present (umbrella `tabpfn` feature), the
//!    transport itself is not implemented in this revision.
//!
//! A no-op [`TabPfnStub`] is **always** compiled in, regardless of
//! features. Its job is purely architectural: downstream code can reference
//! `TabPfnStub` to mark "TabPFN integration point, currently disabled"
//! without taking the `tabpfn_http` feature dependency. This reflects the
//! integration point in every build, so the contract is visible even when
//! the transport is not.
//!
//! Failure policy across all TabPFN backends: any transport error, parse
//! error, or timeout returns `Ok(None)` (never `Err`). The engine then
//! falls back cleanly to the native estimate. This is the safety contract;
//! a remote inference server going down must never surface as a query
//! failure.

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

/// No-op stub for the foundation-model interface (Layer 5).
///
/// Always compiled, regardless of cargo features. Returns `Ok(None)` from
/// every call, signalling the engine to fall back to its native estimate.
///
/// The point of an always-on stub is architectural: it lets downstream
/// callers reference `TabPfnStub` to mark "TabPFN integration point,
/// currently disabled" without taking the `tabpfn_http` feature
/// dependency. The integration shape is visible in every build.
///
/// To wire in a real foundation-model backend, swap this for
/// [`tabpfn::TabPfnHttpCorrector`] (gated on `tabpfn_http`) or a future
/// subprocess adapter. The trait contract is identical, so the swap is
/// a one-line change at the call site.
pub struct TabPfnStub;

impl Corrector for TabPfnStub {
    fn correct(&self, _features: &CorrectionFeatures) -> Result<Option<u64>> {
        // Deliberately `None`: the integration point is wired but
        // disabled. The engine falls back to the native estimate.
        Ok(None)
    }

    fn name(&self) -> &'static str {
        "tabpfn-stub"
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

#[cfg(feature = "additive_gbt")]
pub mod additive {
    //! Additive gradient-boosted-tree residual corrector.
    //!
    //! Sibling backend to [`super::gbt`]. The multiplicative form trains on
    //! `log(actual / baseline_estimate)` and applies the correction as
    //! `baseline * exp(predicted)`. That model is structurally trapped at
    //! zero whenever the engine hands us `baseline_estimate = 0` — the
    //! q=∞ regime where the upstream estimator has completely collapsed
    //! (a common DataFusion 46 symptom on chained joins).
    //!
    //! The additive backend sidesteps that trap by training the model to
    //! predict the **absolute** `actual_rows` from the full
    //! [`CorrectionFeatures`] vector (all 7 features, not just the
    //! baseline). The prediction is clamped to a non-negative integer and
    //! then to the configured LpBound ceiling via
    //! [`crate::lpbound::saturating_clamp`], so the envelope contract is
    //! preserved.
    //!
    //! Cargo feature: `additive_gbt`. Independent of the `gbt` feature —
    //! they can be enabled separately or together.

    use gbdt::config::{Config, Loss};
    use gbdt::decision_tree::{Data, DataVec};
    use gbdt::gradient_boost::GBDT;
    use std::sync::Mutex;

    use super::{CorrectionFeatures, Corrector};
    use crate::feedback::Observation;
    use crate::lpbound::saturating_clamp;
    use crate::{Error, Result};

    /// Tunables for [`AdditiveGbtCorrector::train`]. Defaults mirror
    /// [`super::gbt::GbtOptions`] so the two backends are
    /// drop-in-comparable when benchmarking.
    #[derive(Debug, Clone)]
    pub struct AdditiveGbtOptions {
        /// Shrinkage / learning rate applied to each tree's contribution.
        pub learning_rate: f64,
        /// Max depth of each regression tree. Root is depth 0.
        pub max_depth: u32,
        /// Number of boosting iterations (one tree per iteration).
        pub num_trees: u32,
        /// Inclusive upper bound applied to every corrected estimate.
        /// Use `u64::MAX` to disable.
        pub ceiling: u64,
        /// Minimum samples per leaf — guards against overfitting tiny
        /// feedback histories.
        pub min_leaf_size: usize,
    }

    impl Default for AdditiveGbtOptions {
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

    /// Trained additive GBT corrector. Predicts absolute row counts.
    ///
    /// The model is wrapped in a [`Mutex`] because `gbdt::GBDT::predict`
    /// takes `&mut self` on some configurations; the lock is held only
    /// for the prediction call and is uncontended in the common single-
    /// threaded estimate path.
    pub struct AdditiveGbtCorrector {
        model: Mutex<GBDT>,
        ceiling: u64,
    }

    impl AdditiveGbtCorrector {
        /// Train an additive corrector from a slice of [`Observation`]s.
        ///
        /// Returns [`Error::Feedback`] if the observation slice is empty.
        /// Unlike the multiplicative backend, observations with
        /// `est_rows == 0` are **kept** — they are precisely the q=∞
        /// regime this backend exists to handle. Observations with
        /// `actual_rows == 0` are also kept (a true-zero output is a
        /// valid signal for an additive model).
        pub fn train(observations: &[Observation], options: AdditiveGbtOptions) -> Result<Self> {
            if observations.is_empty() {
                return Err(Error::Feedback(
                    "cannot train AdditiveGbtCorrector: observation slice is empty".into(),
                ));
            }

            let mut training: DataVec = Vec::with_capacity(observations.len());
            for obs in observations {
                // Reconstruct a feature vector from the observation. The
                // feedback table doesn't yet carry the full plan-shape
                // feature set, so we synthesize from `est_rows`. As
                // `Observation` gains columns, mirror the additions here.
                let features = CorrectionFeatures {
                    baseline_estimate: obs.est_rows,
                    ..Default::default()
                };
                let feature_f32: Vec<f32> =
                    features.to_vec().into_iter().map(|v| v as f32).collect();
                let target = obs.actual_rows as f32;
                training.push(Data::new_training_data(feature_f32, 1.0, target, None));
            }

            // Empty observations are caught above; the synthesized
            // training set here is always non-empty.
            debug_assert!(!training.is_empty());

            let mut cfg = Config::new();
            cfg.set_feature_size(CorrectionFeatures::FEATURE_LEN);
            cfg.set_max_depth(options.max_depth);
            cfg.set_iterations(options.num_trees as usize);
            cfg.set_shrinkage(options.learning_rate as f32);
            cfg.set_min_leaf_size(options.min_leaf_size);
            cfg.set_loss(&gbdt::config::loss2string(&Loss::SquaredError));

            let mut model = GBDT::new(&cfg);
            model.fit(&mut training);

            Ok(Self {
                model: Mutex::new(model),
                ceiling: options.ceiling,
            })
        }

        /// Predict the absolute row count for a feature vector.
        /// Exposed for diagnostics; the production path is
        /// [`Corrector::correct`].
        pub fn predict_rows(&self, features: &CorrectionFeatures) -> f64 {
            let feature_f32: Vec<f32> = features.to_vec().into_iter().map(|v| v as f32).collect();
            let probe: DataVec = vec![Data::new_test_data(feature_f32, None)];
            let model = self.model.lock().expect("AdditiveGbtCorrector model lock");
            let preds = model.predict(&probe);
            preds.first().copied().unwrap_or(0.0) as f64
        }

        /// Configured upper bound. Set at training time; the trait method
        /// [`Corrector::correct`] enforces it via `saturating_clamp`.
        pub fn ceiling(&self) -> u64 {
            self.ceiling
        }
    }

    impl Corrector for AdditiveGbtCorrector {
        fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>> {
            let raw = self.predict_rows(features).max(0.0);
            Ok(Some(saturating_clamp(raw, self.ceiling)))
        }

        fn name(&self) -> &'static str {
            "additive_gbt"
        }
    }
}

#[cfg(feature = "tabpfn_http")]
pub mod tabpfn {
    //! Foundation-model interface — HTTP transport.
    //!
    //! Posts a [`super::CorrectionFeatures`] vector as JSON to a
    //! user-configured endpoint (e.g., a Python TabPFN inference server
    //! listening on `http://localhost:8765/infer`), parses an
    //! `{"estimate": <u64>}` reply, and clamps the result to the LpBound
    //! ceiling via [`crate::lpbound::saturating_clamp`].
    //!
    //! Transport: pure-Rust `ureq` (rustls-only, no OpenSSL). Compiled in
    //! only when the `tabpfn_http` cargo feature is enabled.
    //!
    //! # Safety contract
    //!
    //! Any failure — DNS, connection refused, HTTP non-2xx, body parse
    //! error, timeout — returns `Ok(None)`. The engine falls back to the
    //! native estimate. We never propagate transport errors to the
    //! optimizer hot path; a remote inference server going down must not
    //! surface as a query failure.
    //!
    //! Note on naming: this is *the foundation-model interface*, not a
    //! "learned" or "AI" feature. The corrector is a pluggable backend
    //! behind the same `Corrector` trait as every other backend in this
    //! module.
    //!
    //! # Wire format
    //!
    //! Request body (JSON):
    //!
    //! ```json
    //! {
    //!   "features": [<f64>, <f64>, ...],
    //!   "baseline_estimate": <u64>
    //! }
    //! ```
    //!
    //! Response body (JSON):
    //!
    //! ```json
    //! { "estimate": <u64> }
    //! ```
    //!
    //! Any extra fields in the response are ignored, so server
    //! implementations are free to add diagnostics without breaking the
    //! client.
    //!
    //! # See also
    //!
    //! - [`super::TabPfnStub`] — always-on no-op for the same integration
    //!   slot, no transport dependency.

    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    use super::{CorrectionFeatures, Corrector};
    use crate::Result;
    use crate::lpbound::saturating_clamp;

    /// Configuration for [`TabPfnHttpCorrector`].
    #[derive(Debug, Clone)]
    pub struct TabPfnHttpOptions {
        /// Inference endpoint URL. The corrector POSTs here on every
        /// `correct()` call. Example: `http://localhost:8765/infer`.
        pub base_url: String,
        /// Per-request timeout. Applies independently to the connect and
        /// read phases. Bounded by the architecture's sub-ms budget for
        /// the production path, but configurable so users can dial it up
        /// for diagnostics.
        pub timeout_ms: u64,
        /// Inclusive upper bound applied to every corrected estimate via
        /// [`saturating_clamp`]. The Layer 3 safety guarantee — corrections
        /// can never exceed this regardless of what the remote backend
        /// returns. Use `u64::MAX` to disable.
        pub ceiling: u64,
    }

    impl Default for TabPfnHttpOptions {
        fn default() -> Self {
            Self {
                base_url: "http://localhost:8765/infer".into(),
                timeout_ms: 50,
                ceiling: u64::MAX,
            }
        }
    }

    /// JSON request body sent to the inference endpoint.
    #[derive(Serialize)]
    struct InferRequest<'a> {
        features: &'a [f64],
        baseline_estimate: u64,
    }

    /// JSON response body. Extra fields are ignored.
    #[derive(Deserialize)]
    struct InferResponse {
        estimate: u64,
    }

    /// HTTP-backed foundation-model corrector.
    ///
    /// Holds a tiny client config and a base URL. The `ureq` agent is
    /// constructed per-call: the per-estimate cost is dominated by network
    /// round-trip, not agent allocation, and per-call agents keep the
    /// struct cheaply `Send + Sync` without interior mutability.
    pub struct TabPfnHttpCorrector {
        options: TabPfnHttpOptions,
    }

    impl TabPfnHttpCorrector {
        /// Build a corrector from explicit options.
        pub fn new(options: TabPfnHttpOptions) -> Self {
            Self { options }
        }

        /// Convenience constructor: default options with the supplied URL.
        pub fn with_url(base_url: impl Into<String>) -> Self {
            Self {
                options: TabPfnHttpOptions {
                    base_url: base_url.into(),
                    ..TabPfnHttpOptions::default()
                },
            }
        }

        /// Configured options (for diagnostics / logging).
        pub fn options(&self) -> &TabPfnHttpOptions {
            &self.options
        }

        /// Attempt one inference call. Returns `None` on any failure
        /// (network, parse, non-2xx). The `correct()` trait method wraps
        /// this and applies the LpBound clamp.
        fn try_infer(&self, features: &CorrectionFeatures) -> Option<u64> {
            let feature_vec = features.to_vec();
            let payload = InferRequest {
                features: &feature_vec,
                baseline_estimate: features.baseline_estimate,
            };

            let timeout = Duration::from_millis(self.options.timeout_ms);
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout_read(timeout)
                .timeout_write(timeout)
                .build();

            let response = match agent.post(&self.options.base_url).send_json(&payload) {
                Ok(r) => r,
                Err(err) => {
                    // Map every transport error to None and log at debug.
                    // The Error::Feedback diagnostic carries the URL plus
                    // the underlying message so callers tailing logs can
                    // see what failed without us aborting the query.
                    log::debug!(
                        "tabpfn_http: request to {} failed: {}",
                        self.options.base_url,
                        err
                    );
                    return None;
                }
            };

            match response.into_json::<InferResponse>() {
                Ok(body) => Some(body.estimate),
                Err(err) => {
                    log::debug!(
                        "tabpfn_http: response from {} failed to parse: {}",
                        self.options.base_url,
                        err
                    );
                    None
                }
            }
        }
    }

    impl Corrector for TabPfnHttpCorrector {
        fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>> {
            // Safety contract: every failure returns Ok(None), not Err.
            // The engine then transparently falls back to the native
            // estimate. We use Result here to honour the trait shape and
            // to keep a door open for future *non-fallback* error modes
            // (e.g. a deliberate misconfiguration check), but on the hot
            // path failures are absorbed.
            let Some(raw) = self.try_infer(features) else {
                return Ok(None);
            };
            Ok(Some(saturating_clamp(raw as f64, self.options.ceiling)))
        }

        fn name(&self) -> &'static str {
            "tabpfn-http"
        }
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
    fn tabpfn_stub_always_returns_none() {
        let corrector = TabPfnStub;
        let features = CorrectionFeatures {
            baseline_estimate: 9999,
            ..Default::default()
        };
        assert_eq!(
            corrector.correct(&features).unwrap(),
            None,
            "TabPfnStub must always return Ok(None) — it documents the integration point"
        );
        assert_eq!(corrector.name(), "tabpfn-stub");

        // Also exercise an empty feature vector — the stub should still
        // return None without inspecting the input.
        let empty = CorrectionFeatures::default();
        assert_eq!(corrector.correct(&empty).unwrap(), None);
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

#[cfg(all(test, feature = "additive_gbt"))]
mod additive_tests {
    use super::additive::{AdditiveGbtCorrector, AdditiveGbtOptions};
    use super::{CorrectionFeatures, Corrector};
    use crate::feedback::Observation;

    /// Build N synthetic observations where every actual row count is
    /// the same constant `target`. An additive model trained on this
    /// should regress toward `target` regardless of the input features.
    fn synthetic_constant(n: u64, target: u64) -> Vec<Observation> {
        (1..=n)
            .map(|i| Observation {
                template_hash: "syn-add".into(),
                plan_fingerprint: "p".into(),
                est_rows: i * 10,
                actual_rows: target,
                latency_ms: None,
            })
            .collect()
    }

    #[test]
    fn predicts_near_constant_when_training_is_constant() {
        let obs = synthetic_constant(200, 1000);
        let opts = AdditiveGbtOptions {
            learning_rate: 0.3,
            max_depth: 4,
            num_trees: 50,
            ceiling: u64::MAX,
            min_leaf_size: 1,
        };
        let corrector =
            AdditiveGbtCorrector::train(&obs, opts).expect("training additive corrector");

        let features = CorrectionFeatures {
            baseline_estimate: 500,
            ..Default::default()
        };
        let corrected = corrector
            .correct(&features)
            .expect("correct")
            .expect("Some");
        assert!(
            (800..=1200).contains(&corrected),
            "expected ~1000, got {corrected}"
        );
        assert_eq!(corrector.name(), "additive_gbt");
    }

    #[test]
    fn ceiling_clamps_when_prediction_exceeds_it() {
        let obs = synthetic_constant(200, 1000);
        let opts = AdditiveGbtOptions {
            learning_rate: 0.3,
            max_depth: 4,
            num_trees: 50,
            ceiling: 100, // far below the trained constant
            min_leaf_size: 1,
        };
        let corrector = AdditiveGbtCorrector::train(&obs, opts).expect("training");

        let features = CorrectionFeatures {
            baseline_estimate: 500,
            ..Default::default()
        };
        let corrected = corrector
            .correct(&features)
            .expect("correct")
            .expect("Some");
        assert_eq!(corrected, 100, "ceiling must clamp the additive correction");
        assert_eq!(corrector.ceiling(), 100);
    }

    #[test]
    fn corrects_nonzero_even_when_baseline_estimate_is_zero() {
        // This is the q=∞ fix proof. The multiplicative GbtCorrector
        // would return 0 here (baseline * exp(predicted) = 0 * _ = 0).
        // The additive backend must escape that trap.
        let obs = synthetic_constant(200, 1000);
        let corrector =
            AdditiveGbtCorrector::train(&obs, AdditiveGbtOptions::default()).expect("training");

        let features = CorrectionFeatures {
            baseline_estimate: 0,
            ..Default::default()
        };
        let corrected = corrector
            .correct(&features)
            .expect("correct")
            .expect("Some");
        assert!(
            corrected > 0,
            "additive corrector must return non-zero even when baseline_estimate = 0; got {corrected}"
        );
    }

    #[test]
    fn empty_observations_errors() {
        match AdditiveGbtCorrector::train(&[], AdditiveGbtOptions::default()) {
            Ok(_) => panic!("expected error on empty observations"),
            Err(e) => assert!(matches!(e, crate::Error::Feedback(_))),
        }
    }
}

#[cfg(all(test, feature = "tabpfn_http"))]
mod tabpfn_http_tests {
    use super::tabpfn::{TabPfnHttpCorrector, TabPfnHttpOptions};
    use super::{CorrectionFeatures, Corrector};

    /// Pointing at port 1 on the loopback interface is the canonical
    /// "guaranteed-to-refuse-connection" target on Linux/macOS. The
    /// safety contract says: any transport failure must surface as
    /// `Ok(None)`, never `Err`, never a panic. We verify that here
    /// without standing up a real inference server.
    #[test]
    fn http_failure_returns_none_not_error() {
        let corrector = TabPfnHttpCorrector::new(TabPfnHttpOptions {
            base_url: "http://127.0.0.1:1/infer".into(),
            timeout_ms: 50,
            ceiling: u64::MAX,
        });
        let features = CorrectionFeatures {
            baseline_estimate: 1234,
            ..Default::default()
        };
        let result = corrector.correct(&features);
        assert!(
            result.is_ok(),
            "tabpfn-http transport failure must not propagate as Err; got {result:?}"
        );
        assert_eq!(
            result.unwrap(),
            None,
            "tabpfn-http transport failure must yield Ok(None) so the engine falls back cleanly"
        );
        assert_eq!(corrector.name(), "tabpfn-http");
    }

    #[test]
    fn malformed_url_returns_none() {
        // Not even a valid URL — `ureq` rejects this at request-build
        // time, which our error path must absorb the same as any other
        // transport failure.
        let corrector = TabPfnHttpCorrector::with_url("not a url at all");
        let features = CorrectionFeatures::default();
        let result = corrector.correct(&features).expect("never Err");
        assert_eq!(result, None);
    }

    #[test]
    fn options_default_is_localhost() {
        let opts = TabPfnHttpOptions::default();
        assert!(opts.base_url.starts_with("http://"));
        assert!(opts.timeout_ms > 0);
        assert_eq!(opts.ceiling, u64::MAX);
    }
}
