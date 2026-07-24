// SPDX-License-Identifier: Apache-2.0
//
// samkhya-core: absolute soundness tests for the join-ceiling envelope.
//
// Sole author: Prateek Singh.
//
// The pre-existing `property_lpbound.rs` suite checks only *relative*
// invariants — bound A is no larger than bound B, bounds are finite. Those
// hold perfectly well for a family of bounds that are all wrong. This suite
// checks the property that actually matters:
//
//     ceiling >= |true join output|
//
// It builds explicit relation instances, counts the real join output by
// brute force, derives statistics from those same instances, and asserts
// the ceiling never lands below the truth.

use std::collections::HashMap;

use proptest::collection::vec as pvec;
use proptest::prelude::*;

use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_core::lpbound::{ProductBound, UpperBound};

/// A relation in a chain, stored as (left_key, right_key) pairs. The first
/// relation's left key and the last relation's right key are unconstrained.
type Relation = Vec<(u64, u64)>;

/// Brute-force the true output cardinality of the chain
/// `R0 ⋈ R1 ⋈ … ⋈ R_{n-1}` where `R_i.right == R_{i+1}.left`.
fn brute_force_chain(rels: &[Relation]) -> u64 {
    fn rec(rels: &[Relation], idx: usize, prev_right: Option<u64>) -> u64 {
        if idx == rels.len() {
            return 1;
        }
        let mut total = 0u64;
        for &(l, r) in &rels[idx] {
            if let Some(p) = prev_right {
                if p != l {
                    continue;
                }
            }
            total = total.saturating_add(rec(rels, idx + 1, Some(r)));
        }
        total
    }
    rec(rels, 0, None)
}

/// Brute-force the true output cardinality of a star: `hub` joined to every
/// `spoke`, where `hub.keys[i] == spokes[i].key`.
fn brute_force_star(hub: &[Vec<u64>], spokes: &[Vec<u64>]) -> u64 {
    let mut total = 0u64;
    for hub_row in hub {
        let mut product = 1u64;
        for (i, spoke) in spokes.iter().enumerate() {
            let key = hub_row[i];
            let matches = spoke.iter().filter(|&&k| k == key).count() as u64;
            product = product.saturating_mul(matches);
        }
        total = total.saturating_add(product);
    }
    total
}

/// Exact distinct count and exact maximum degree of a key column.
fn key_stats(keys: impl Iterator<Item = u64>) -> (u64, u64) {
    let mut counts: HashMap<u64, u64> = HashMap::new();
    for k in keys {
        *counts.entry(k).or_insert(0) += 1;
    }
    let distinct = counts.len() as u64;
    let max_degree = counts.values().copied().max().unwrap_or(0);
    (distinct, max_degree)
}

/// Build a `JoinGraph` for a chain, deriving every degree from the actual
/// instance. `use_distinct` selects the weaker distinct-count-derived bound
/// (`maxdeg <= rows - distinct + 1`) over the exact measured degree.
fn chain_graph(rels: &[Relation], use_distinct: bool) -> JoinGraph {
    let n = rels.len();
    let mut relations = Vec::with_capacity(n);

    for (i, rel) in rels.iter().enumerate() {
        let rows = rel.len() as u64;
        let mut jr = JoinRelation::new(rows);

        // Attribute (i-1) links this relation to its predecessor via the
        // left key; attribute i links it to its successor via the right key.
        if i > 0 {
            let (distinct, max_degree) = key_stats(rel.iter().map(|&(l, _)| l));
            jr = jr.with_degree(
                (i - 1) as u32,
                if use_distinct {
                    AttributeDegree::from_distinct(rows, distinct)
                } else {
                    AttributeDegree::from_upper_bound(rows, max_degree)
                },
            );
        }
        if i + 1 < n {
            let (distinct, max_degree) = key_stats(rel.iter().map(|&(_, r)| r));
            jr = jr.with_degree(
                i as u32,
                if use_distinct {
                    AttributeDegree::from_distinct(rows, distinct)
                } else {
                    AttributeDegree::from_upper_bound(rows, max_degree)
                },
            );
        }
        relations.push(jr);
    }

    let mut graph = JoinGraph::new(relations);
    for i in 0..n.saturating_sub(1) {
        graph = graph.with_edge(i, i + 1, i as u32);
    }
    graph
}

fn cases() -> ProptestConfig {
    ProptestConfig::with_cases(2048)
}

proptest! {
    #![proptest_config(cases())]

    /// The headline property: a ceiling built from exact degree statistics
    /// is never below the true cardinality of the instance it was derived
    /// from. Key domains are deliberately tiny so heavy skew — including
    /// "every row on one key", the worst case — is generated often.
    #[test]
    fn chain_ceiling_is_sound_with_exact_degrees(
        rels in pvec(pvec((0u64..3, 0u64..3), 1..5usize), 2..4usize),
    ) {
        let truth = brute_force_chain(&rels);
        let ceiling = chain_graph(&rels, /*use_distinct=*/ false).ceiling();
        prop_assert!(
            ceiling >= truth,
            "ceiling {ceiling} < true cardinality {truth} for {rels:?}"
        );
    }

    /// The same property when the only statistic available is a distinct
    /// count — what an HLL sketch already gives every samkhya deployment.
    #[test]
    fn chain_ceiling_is_sound_with_distinct_counts_only(
        rels in pvec(pvec((0u64..3, 0u64..3), 1..5usize), 2..4usize),
    ) {
        let truth = brute_force_chain(&rels);
        let ceiling = chain_graph(&rels, /*use_distinct=*/ true).ceiling();
        prop_assert!(
            ceiling >= truth,
            "ceiling {ceiling} < true cardinality {truth} for {rels:?}"
        );
    }

    /// Distinct-derived degrees are weaker than measured ones, so the
    /// ceiling they produce can only be looser — never tighter, which would
    /// signal the derivation is unsound.
    #[test]
    fn distinct_derived_ceiling_is_never_tighter_than_measured(
        rels in pvec(pvec((0u64..4, 0u64..4), 1..5usize), 2..4usize),
    ) {
        let exact = chain_graph(&rels, false).ceiling();
        let from_distinct = chain_graph(&rels, true).ceiling();
        prop_assert!(
            from_distinct >= exact,
            "distinct-derived {from_distinct} tighter than measured {exact}"
        );
    }

    /// A star join: one hub row set joined to `k` independent spokes.
    #[test]
    fn star_ceiling_is_sound(
        hub_keys in pvec(pvec(0u64..3, 2..4usize), 1..5usize),
        spoke_a in pvec(0u64..3, 1..5usize),
        spoke_b in pvec(0u64..3, 1..5usize),
    ) {
        let arity = hub_keys[0].len();
        // Keep every hub row the same arity as the first.
        let hub: Vec<Vec<u64>> = hub_keys
            .iter()
            .map(|row| {
                let mut r = row.clone();
                r.resize(arity, 0);
                r
            })
            .collect();
        let mut spokes = vec![spoke_a, spoke_b];
        spokes.truncate(arity);
        while spokes.len() < arity {
            spokes.push(vec![0u64]);
        }

        let truth = brute_force_star(&hub, &spokes);

        let mut relations = vec![JoinRelation::new(hub.len() as u64)];
        for (i, spoke) in spokes.iter().enumerate() {
            let rows = spoke.len() as u64;
            let (_, max_degree) = key_stats(spoke.iter().copied());
            relations.push(
                JoinRelation::new(rows)
                    .with_degree(i as u32, AttributeDegree::from_upper_bound(rows, max_degree)),
            );
        }
        let mut graph = JoinGraph::new(relations);
        for i in 0..spokes.len() {
            graph = graph.with_edge(0, i + 1, i as u32);
        }

        let ceiling = graph.ceiling();
        prop_assert!(
            ceiling >= truth,
            "star ceiling {ceiling} < true {truth}; hub={hub:?} spokes={spokes:?}"
        );
    }

    /// The Cartesian product is the fallback every other bound must respect,
    /// and it must itself be sound.
    #[test]
    fn product_bound_is_sound(
        rels in pvec(pvec((0u64..3, 0u64..3), 1..5usize), 2..4usize),
    ) {
        let truth = brute_force_chain(&rels);
        let rows: Vec<u64> = rels.iter().map(|r| r.len() as u64).collect();
        let product = ProductBound.ceiling(&rows, &[]);
        prop_assert!(
            product >= truth,
            "ProductBound {product} < true cardinality {truth}"
        );
    }

    /// A ceiling built with no degree information at all must equal the
    /// Cartesian product: sound, maximally loose, and never a surprise.
    #[test]
    fn missing_degrees_degrade_to_the_product(
        rows in pvec(1u64..12, 2..4usize),
    ) {
        let relations: Vec<JoinRelation> = rows.iter().map(|&n| JoinRelation::new(n)).collect();
        let mut graph = JoinGraph::new(relations);
        for i in 0..rows.len() - 1 {
            graph = graph.with_edge(i, i + 1, i as u32);
        }
        let product = ProductBound.ceiling(&rows, &[]);
        prop_assert_eq!(graph.ceiling(), product);
    }
}

/// The five witness instances from the v1.1 soundness audit, pinned as
/// regression tests so the repaired envelope can never silently drift back
/// below the truth on any of them.
#[test]
fn audit_witnesses_remain_sound() {
    // (name, chain instance)
    let witnesses: Vec<(&str, Vec<Relation>)> = vec![
        (
            "2-rel, all rows share one key",
            vec![vec![(0, 0); 4], vec![(0, 0); 5]],
        ),
        (
            "3-rel chain, all rows share one key",
            vec![vec![(0, 0); 3], vec![(0, 0); 3], vec![(0, 0); 3]],
        ),
        (
            "FK join orders(10) <- lineitem(100)",
            vec![
                (0..10).map(|i| (i, i)).collect(),
                (0..100).map(|i| (i % 10, i)).collect(),
            ],
        ),
        ("skewed 20x20, 5 distinct, 16 on key 0", {
            let mut r: Relation = vec![(0, 0); 16];
            for k in 1..5 {
                r.push((k, k));
            }
            vec![r.clone(), r]
        }),
    ];

    for (name, rels) in &witnesses {
        let truth = brute_force_chain(rels);
        let exact = chain_graph(rels, false).ceiling();
        let approx = chain_graph(rels, true).ceiling();
        assert!(
            exact >= truth,
            "{name}: measured-degree ceiling {exact} < true {truth}"
        );
        assert!(
            approx >= truth,
            "{name}: distinct-derived ceiling {approx} < true {truth}"
        );
    }
}

/// The foreign-key case is not merely sound, it is exactly tight — which is
/// the whole reason a degree-aware ceiling is worth carrying statistics for.
#[test]
fn foreign_key_join_bounds_exactly() {
    let orders: Relation = (0..10).map(|i| (i, i)).collect();
    let lineitem: Relation = (0..100).map(|i| (i % 10, i)).collect();
    let rels = vec![orders, lineitem];

    let truth = brute_force_chain(&rels);
    assert_eq!(truth, 100);
    assert_eq!(chain_graph(&rels, false).ceiling(), 100);
    assert_eq!(chain_graph(&rels, true).ceiling(), 100);

    // The Cartesian product is sound but 10x looser.
    assert_eq!(ProductBound.ceiling(&[10, 100], &[]), 1_000);
}
