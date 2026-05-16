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
//! Concrete backends (none shipped yet, all behind feature flags later):
//!
//! - `gbt` — gradient-boosted trees (e.g. lightgbm-rs ~100KB model)
//! - `tabpfn` — TabPFN-style foundation-model wrapper (network or local)

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
}
