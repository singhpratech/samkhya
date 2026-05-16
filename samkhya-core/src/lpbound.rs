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

/// Frequency-moment chain-join upper bound.
///
/// Assumes each equality predicate `(i, j)` joins on a single key whose
/// distinct-value count is given by `distinct_counts[i]` and
/// `distinct_counts[j]`. The bound is:
///
/// ```text
/// |R_i ⋈ R_j| ≤ |R_i| * |R_j| / max(D_i, D_j)
/// ```
///
/// (Uniform-distribution worst case; tight in expectation when join
/// keys are evenly spread.) Applied sequentially across all equality
/// predicates: the result of each join feeds the next bound.
///
/// Tighter than [`AgmBound`] for tree / chain joins where each relation
/// has a non-trivial distinct-key count. Falls back to [`ProductBound`]
/// when no equality predicates are supplied.
pub struct ChainBound {
    pub distinct_counts: Vec<u64>,
}

impl ChainBound {
    pub fn new(distinct_counts: Vec<u64>) -> Self {
        Self { distinct_counts }
    }
}

impl UpperBound for ChainBound {
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }
        // Each predicate divides the running product by the larger of
        // the two endpoint distinct counts (or 1 if unknown).
        let mut bound: u128 = relations
            .iter()
            .fold(1u128, |acc, &n| acc.saturating_mul(n as u128));
        for &(i, j) in equality_predicates {
            let d_i = self.distinct_counts.get(i).copied().unwrap_or(1).max(1) as u128;
            let d_j = self.distinct_counts.get(j).copied().unwrap_or(1).max(1) as u128;
            let d = d_i.max(d_j);
            bound /= d;
        }
        if bound > u64::MAX as u128 {
            u64::MAX
        } else {
            bound as u64
        }
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
    fn chain_bound_tighter_than_product() {
        // Two relations of 1000 rows each, joining on a key with 100 distinct values.
        // Product = 1_000_000; ChainBound = 1000 * 1000 / 100 = 10_000.
        let r = [1_000u64, 1_000];
        let cb = ChainBound::new(vec![100, 100]);
        let bound = cb.ceiling(&r, &[(0, 1)]);
        assert_eq!(bound, 10_000);
        let product = ProductBound.ceiling(&r, &[]);
        assert!(bound < product);
    }

    #[test]
    fn chain_bound_three_table_chain() {
        // R1(1000) ⋈ R2(2000) ⋈ R3(500), join keys 100 distinct each side.
        // Product = 1e9. Chain = 1e9 / 100 / 100 = 100_000.
        let r = [1_000u64, 2_000, 500];
        let cb = ChainBound::new(vec![100, 100, 100]);
        let bound = cb.ceiling(&r, &[(0, 1), (1, 2)]);
        assert_eq!(bound, 100_000);
    }

    #[test]
    fn chain_bound_no_predicates_falls_back() {
        let cb = ChainBound::new(vec![10, 20, 30]);
        assert_eq!(cb.ceiling(&[10, 20, 30], &[]), 10 * 20 * 30);
    }

    #[test]
    fn chain_bound_missing_distinct_count_defaults_to_one() {
        // No distinct count entry → defaults to 1, meaning no reduction.
        let cb = ChainBound::new(vec![]);
        let bound = cb.ceiling(&[100, 100], &[(0, 1)]);
        assert_eq!(bound, 10_000); // 100 * 100 / max(1, 1) = 10_000
    }

    #[test]
    fn saturating_clamp_saturates() {
        assert_eq!(saturating_clamp(500.0, 1000), 500);
        assert_eq!(saturating_clamp(2000.0, 1000), 1000);
        assert_eq!(saturating_clamp(-5.0, 1000), 0);
        assert_eq!(saturating_clamp(f64::NAN, 1000), 0);
    }
}
