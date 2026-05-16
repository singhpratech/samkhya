//! Pessimistic upper-bound envelope for cardinality estimates.
//!
//! Inspired by **LpBound** \[Zhang et al., SIGMOD 2025 Best Paper\]. The
//! envelope provides a *provable ceiling* on the cardinality of a join:
//! no correction may exceed it, so cold-start plans are bounded by the
//! native estimate or this ceiling — whichever is tighter — and never
//! degrade below baseline.
//!
//! The full LpBound algorithm solves an LP over ℓp-norms of column
//! degree sequences. The baseline shipped here is the trivial Cartesian
//! product bound plus a coarse AGM-style approximation; the LP-based
//! bound is a drop-in replacement behind the [`UpperBound`] trait.

use crate::{Error, Result};

/// Trait every upper-bound provider implements.
pub trait UpperBound {
    /// Compute the inclusive ceiling for a join.
    ///
    /// * `relations`           — input row counts for each base relation
    /// * `equality_predicates` — pairs of relation indices joined by `=`
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64;
}

/// Cartesian-product upper bound. Sound but very loose.
pub struct ProductBound;

impl UpperBound for ProductBound {
    fn ceiling(&self, relations: &[u64], _eq: &[(usize, usize)]) -> u64 {
        relations.iter().fold(1u64, |acc, &n| acc.saturating_mul(n))
    }
}

/// Coarse AGM-style upper bound for equi-joins.
///
/// Returns `min(product, |R_min| * |R_max|)` when at least one equality
/// predicate exists, otherwise falls back to [`ProductBound`]. This is a
/// placeholder approximation; the true AGM / LpBound bound requires
/// fractional edge cover / LP relaxation.
pub struct AgmBound;

impl UpperBound for AgmBound {
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }
        let product: u64 = relations.iter().fold(1u64, |acc, &n| acc.saturating_mul(n));
        let min_r = *relations.iter().min().unwrap_or(&0);
        let max_r = *relations.iter().max().unwrap_or(&0);
        product.min(min_r.saturating_mul(max_r))
    }
}

/// Clamp an estimate to a ceiling. Returns [`Error::LpBoundExceeded`]
/// if the estimate exceeds the ceiling — this signals a correction-layer
/// bug, since corrections must respect the envelope.
pub fn clamp_estimate(estimate: f64, ceiling: u64) -> Result<u64> {
    let clamped = estimate.max(0.0).min(u64::MAX as f64) as u64;
    if clamped <= ceiling {
        Ok(clamped)
    } else {
        Err(Error::LpBoundExceeded {
            estimate,
            ceiling: ceiling as f64,
        })
    }
}

/// Clamp without erroring; saturates to `ceiling`. Use this in production
/// paths where a misbehaving corrector must never crash the engine.
pub fn saturating_clamp(estimate: f64, ceiling: u64) -> u64 {
    let clamped = estimate.max(0.0).min(u64::MAX as f64) as u64;
    clamped.min(ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_bound_two_relations() {
        assert_eq!(ProductBound.ceiling(&[100, 200], &[]), 20_000);
    }

    #[test]
    fn product_bound_overflow_saturates() {
        assert_eq!(ProductBound.ceiling(&[u64::MAX, 2], &[]), u64::MAX);
    }

    #[test]
    fn product_bound_empty_relations() {
        assert_eq!(ProductBound.ceiling(&[], &[]), 1);
    }

    #[test]
    fn agm_no_predicates_falls_back_to_product() {
        assert_eq!(AgmBound.ceiling(&[10, 20, 30], &[]), 10 * 20 * 30);
    }

    #[test]
    fn agm_with_predicates_tighter_than_product() {
        let r = [1_000u64, 1_000_000];
        let bound = AgmBound.ceiling(&r, &[(0, 1)]);
        let product = ProductBound.ceiling(&r, &[]);
        assert!(bound <= product);
    }

    #[test]
    fn clamp_within_ceiling() {
        assert_eq!(clamp_estimate(500.0, 1000).unwrap(), 500);
    }

    #[test]
    fn clamp_exceeds_ceiling_errors() {
        let err = clamp_estimate(1500.0, 1000).unwrap_err();
        match err {
            Error::LpBoundExceeded { estimate, ceiling } => {
                assert_eq!(estimate, 1500.0);
                assert_eq!(ceiling, 1000.0);
            }
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn saturating_clamp_saturates() {
        assert_eq!(saturating_clamp(500.0, 1000), 500);
        assert_eq!(saturating_clamp(2000.0, 1000), 1000);
        assert_eq!(saturating_clamp(-5.0, 1000), 0);
        assert_eq!(saturating_clamp(f64::NAN, 1000), 0);
    }
}
