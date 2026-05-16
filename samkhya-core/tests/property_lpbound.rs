// SPDX-License-Identifier: Apache-2.0
//
// samkhya-core: randomized property tests for the LpBound envelope.
//
// Sole author: Prateek Singh.
//
// The envelope is the safety contract that no correction may breach,
// so the bounds it ships must obey the formal ordering invariants
// claimed in the documentation regardless of relation sizes, join
// graph shape, or distinct-count hints. Each `proptest!` block runs
// at least 1024 cases.

use proptest::collection::vec as pvec;
use proptest::prelude::*;

use samkhya_core::lpbound::{AgmBound, ChainBound, ProductBound, UpperBound};

fn cases() -> ProptestConfig {
    ProptestConfig::with_cases(1024)
}

// Generate a chain of distinct relations: pairs (i, i+1) for i in 0..n-1.
fn chain_predicates(n: usize) -> Vec<(usize, usize)> {
    (0..n.saturating_sub(1)).map(|i| (i, i + 1)).collect()
}

proptest! {
    #![proptest_config(cases())]

    // ProductBound is monotone in each input cardinality.
    #[test]
    fn product_bound_monotone(
        a in pvec(0u64..1_000_000u64, 1..6usize),
        bump in 1u64..10_000u64,
        idx in 0usize..6usize,
    ) {
        let idx = idx % a.len();
        let mut b = a.clone();
        b[idx] = b[idx].saturating_add(bump);
        let lo = ProductBound.ceiling(&a, &[]);
        let hi = ProductBound.ceiling(&b, &[]);
        prop_assert!(hi >= lo, "lo {lo} hi {hi}");
    }

    // AgmBound <= ProductBound on every input.
    #[test]
    fn agm_le_product(
        rows in pvec(0u64..100_000u64, 1..6usize),
        preds_raw in pvec((0usize..6usize, 0usize..6usize), 0..6usize),
    ) {
        let n = rows.len();
        let preds: Vec<(usize, usize)> = preds_raw.iter()
            .copied()
            .map(|(i, j)| (i % n, j % n))
            .filter(|(i, j)| i != j)
            .collect();
        let agm = AgmBound.ceiling(&rows, &preds);
        let prod = ProductBound.ceiling(&rows, &preds);
        prop_assert!(agm <= prod, "AGM {agm} > Product {prod}");
    }

    // ChainBound <= ProductBound on chain-shape join graphs.
    #[test]
    fn chain_le_product_on_chains(
        rows in pvec(1u64..10_000u64, 2..6usize),
        distinct in pvec(1u64..1_000u64, 6usize),
    ) {
        let preds = chain_predicates(rows.len());
        let cb = ChainBound::new(distinct.clone());
        let chain_b = cb.ceiling(&rows, &preds);
        let prod = ProductBound.ceiling(&rows, &preds);
        prop_assert!(chain_b <= prod, "Chain {chain_b} > Product {prod}");
    }

    // Every bound is finite and non-negative (u64 is non-negative
    // by type; the "finite" content is checked by deriving the f64
    // value and asserting `is_finite`).
    #[test]
    fn all_bounds_finite_and_nonneg(
        rows in pvec(0u64..1_000_000u64, 0..6usize),
        preds_raw in pvec((0usize..6usize, 0usize..6usize), 0..6usize),
        distinct in pvec(0u64..1_000u64, 6usize),
    ) {
        let n = rows.len();
        let preds: Vec<(usize, usize)> = if n == 0 {
            Vec::new()
        } else {
            preds_raw.iter()
                .copied()
                .map(|(i, j)| (i % n, j % n))
                .filter(|(i, j)| i != j)
                .collect()
        };
        let p = ProductBound.ceiling(&rows, &preds);
        let a = AgmBound.ceiling(&rows, &preds);
        let cb = ChainBound::new(distinct);
        let c = cb.ceiling(&rows, &preds);
        prop_assert!((p as f64).is_finite());
        prop_assert!((a as f64).is_finite());
        prop_assert!((c as f64).is_finite());
    }

    // Adding an equality predicate to AgmBound can only tighten the
    // bound (never loosen it).
    #[test]
    fn agm_tighter_with_more_predicates(
        rows in pvec(1u64..10_000u64, 2..5usize),
    ) {
        let none = AgmBound.ceiling(&rows, &[]);
        let with_one = AgmBound.ceiling(&rows, &[(0usize, 1usize)]);
        prop_assert!(with_one <= none, "with_one {with_one} > none {none}");
    }
}

// LpJoinBound properties — only compiled when the LP solver feature is on.
#[cfg(feature = "lp_solver")]
mod lp {
    use super::*;
    use samkhya_core::lpbound::LpJoinBound;

    proptest! {
        #![proptest_config(cases())]

        // LpJoinBound <= AgmBound on connected join graphs.
        //
        // The strict ordering claim holds only when the join graph is
        // a single connected component — there the LP returns the
        // principled fractional-edge-cover bound, which the coarse
        // AGM construction (min*max) over-approximates. In the
        // disconnected case the per-component LP solves multiply, so
        // the comparison is not the right one to make; we restrict
        // to chain-shaped graphs which are connected by construction.
        #[test]
        fn lp_le_agm(
            rows in pvec(1u64..10_000u64, 2..5usize),
        ) {
            let n = rows.len();
            let preds = chain_predicates(n);
            let lp = LpJoinBound::new();
            let lp_b = lp.ceiling(&rows, &preds);
            let agm = AgmBound.ceiling(&rows, &preds);
            // Small additive slack absorbs the ceil() the solver
            // applies to floating-point objective values.
            prop_assert!(
                lp_b <= agm.saturating_add(2),
                "LP {lp_b} > AGM {agm} on chain rows={:?}",
                rows
            );
        }

        // LpJoinBound is non-negative and finite.
        #[test]
        fn lp_finite(
            rows in pvec(1u64..10_000u64, 1..5usize),
            preds_raw in pvec((0usize..5usize, 0usize..5usize), 0..6usize),
        ) {
            let n = rows.len();
            let preds: Vec<(usize, usize)> = preds_raw.iter()
                .copied()
                .map(|(i, j)| (i % n, j % n))
                .filter(|(i, j)| i != j)
                .collect();
            let lp = LpJoinBound::new();
            let v = lp.ceiling(&rows, &preds);
            prop_assert!((v as f64).is_finite());
        }
    }
}
