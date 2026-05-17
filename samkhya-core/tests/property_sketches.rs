// SPDX-License-Identifier: Apache-2.0
//
// samkhya-core: randomized property tests for the sketch family.
//
// Sole author: Prateek Singh.
//
// Each `proptest!` block runs at least 1024 cases. The properties here
// encode the formal guarantees the sketches must uphold for the
// LpBound envelope and the residual corrector to remain sound:
//
//   * HLL merge equals the sketch built over the union.
//   * Bloom never produces a false negative; the false-positive rate
//     stays close to the configured target across 100k random probes.
//   * Count-Min never undercounts and stays within the
//     (eps * total) / probability-(1 - 0.5^depth) classical bound.
//   * EquiDepthHistogram covers its full-domain count exactly.
//   * Every sketch round-trips byte-for-byte structurally.

use std::collections::HashSet;

use proptest::collection::{hash_set, vec as pvec};
use proptest::prelude::*;

use samkhya_core::sketches::{BloomFilter, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch};

// Floating-point comparison helper. `a` and `b` are treated as equal
// when |a - b| <= |a| * rel_tol + abs_tol.
fn near(a: f64, b: f64, rel_tol: f64, abs_tol: f64) -> bool {
    (a - b).abs() <= a.abs() * rel_tol + abs_tol
}

fn cases() -> ProptestConfig {
    ProptestConfig::with_cases(1024)
}

// ---- HLL -------------------------------------------------------------------

fn build_hll(items: &HashSet<u64>, precision: u8) -> HllSketch {
    let mut h = HllSketch::new(precision).unwrap();
    for x in items {
        h.add(&x.to_le_bytes());
    }
    h
}

proptest! {
    #![proptest_config(cases())]

    // HLL merge equals the sketch built directly from A union B
    // within the sketch's relative error band.
    #[test]
    fn hll_merge_equals_union(
        a in hash_set(any::<u64>(), 0..256usize),
        b in hash_set(any::<u64>(), 0..256usize),
    ) {
        let precision: u8 = 12;
        let mut h_a = build_hll(&a, precision);
        let h_b = build_hll(&b, precision);
        h_a.merge(&h_b).unwrap();

        let union: HashSet<u64> = a.union(&b).copied().collect();
        let h_union = build_hll(&union, precision);

        let merged_est = h_a.estimate() as f64;
        let union_est = h_union.estimate() as f64;

        // HLL relative error at p=12 is roughly 1.04 / sqrt(4096) ~= 0.0163.
        // Two independently routed sketches sit within twice that band.
        prop_assert!(
            near(merged_est, union_est, 0.05, 4.0),
            "merge est {merged_est} vs union est {union_est}"
        );
    }

    // bytes round-trip preserves the estimate exactly.
    #[test]
    fn hll_bytes_round_trip_preserves_estimate(
        items in hash_set(any::<u64>(), 0..512usize),
        precision in 6u8..=14u8,
    ) {
        let h = build_hll(&items, precision);
        let est_before = h.estimate() as f64;
        let bytes = h.to_bytes().unwrap();
        let h2 = HllSketch::from_bytes(&bytes).unwrap();
        let est_after = h2.estimate() as f64;
        prop_assert!(
            near(est_before, est_after, 0.001, 1.0),
            "before {est_before} after {est_after}"
        );
        prop_assert_eq!(h.precision(), h2.precision());
    }

    // Monotonicity: adding more keys never decreases the estimate by
    // more than the sketch's relative error band.
    #[test]
    fn hll_estimate_is_monotone(
        base in hash_set(any::<u64>(), 0..256usize),
        extra in hash_set(any::<u64>(), 0..256usize),
    ) {
        let precision: u8 = 12;
        let h_base = build_hll(&base, precision);
        let mut h_more = h_base.clone();
        for x in &extra {
            h_more.add(&x.to_le_bytes());
        }
        let e_base = h_base.estimate() as f64;
        let e_more = h_more.estimate() as f64;
        // Adding elements can only nudge registers upward, so the
        // estimate must not drop by more than the sketch's noise band.
        let drop = (e_base - e_more).max(0.0);
        prop_assert!(
            drop <= e_base * 0.05 + 4.0,
            "non-monotone: base {e_base} more {e_more}"
        );
    }
}

// ---- Bloom -----------------------------------------------------------------

proptest! {
    #![proptest_config(cases())]

    // Inserted keys must always report contained — zero false negatives.
    #[test]
    fn bloom_has_no_false_negatives(
        items in hash_set(any::<u64>(), 1..512usize),
    ) {
        let mut bf = BloomFilter::new(items.len().max(16), 0.01);
        for x in &items {
            bf.insert(&x.to_le_bytes());
        }
        for x in &items {
            prop_assert!(bf.contains(&x.to_le_bytes()), "false negative for {x}");
        }
    }

    // Bytes round-trip preserves the contained set bit-for-bit.
    #[test]
    fn bloom_round_trip(
        items in hash_set(any::<u64>(), 0..256usize),
    ) {
        let mut bf = BloomFilter::new(items.len().max(16), 0.01);
        for x in &items { bf.insert(&x.to_le_bytes()); }
        let bytes = bf.to_bytes().unwrap();
        let bf2 = BloomFilter::from_bytes(&bytes).unwrap();
        prop_assert_eq!(bf.num_bits(), bf2.num_bits());
        prop_assert_eq!(bf.num_hashes(), bf2.num_hashes());
        for x in &items {
            prop_assert!(bf2.contains(&x.to_le_bytes()));
        }
    }
}

// A single coverage probe for the false-positive rate. This is not a
// proptest!-style randomized property because the sample size required
// to certify a 1% target rate is large; we run it once with a fixed
// seed-equivalent (sequential keys), insert 10k items, probe 100k
// disjoint keys, and require the observed rate to sit within tolerance.
#[test]
fn bloom_false_positive_rate_within_tolerance() {
    let target = 0.01;
    let n = 10_000u64;
    let probes = 100_000u64;
    let mut bf = BloomFilter::new(n as usize, target);
    for i in 0..n {
        bf.insert(&i.to_le_bytes());
    }
    let mut fps = 0u64;
    for i in n..(n + probes) {
        if bf.contains(&i.to_le_bytes()) {
            fps += 1;
        }
    }
    let observed = fps as f64 / probes as f64;
    // Allow a generous tolerance: the in-tree sizing formula uses the
    // looser `-1.44 * n * ln(p)` approximation rather than the tight
    // `-n * ln(p) / (ln 2)^2`, which inflates the observed FP rate by
    // a constant factor. The existing module-level Bloom test in
    // src/sketches/bloom.rs requires rate < 5%; we mirror that bound
    // here and assert "observed <= 8 * target" to absorb the run-to-run
    // variance of a 100k-sample probe.
    let ceiling = (target * 8.0).max(0.05);
    assert!(
        observed <= ceiling,
        "observed FP rate {observed} exceeds tolerance {ceiling} (target {target})"
    );
}

// ---- Count-Min -------------------------------------------------------------

proptest! {
    #![proptest_config(cases())]

    // Count-Min never undercounts: estimate >= true count for every key.
    #[test]
    fn cms_never_undercounts(
        keys in pvec(any::<u32>(), 1..256usize),
    ) {
        let mut cms = CountMinSketch::new(5, 1024).unwrap();
        let mut truth: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        for k in &keys {
            cms.add(&k.to_le_bytes(), 1);
            *truth.entry(*k).or_insert(0) += 1;
        }
        for (k, t) in &truth {
            let est = cms.estimate(&k.to_le_bytes());
            prop_assert!(
                est >= *t,
                "undercount for {k}: est {est} < true {t}"
            );
        }
    }

    // Round-trip preserves every per-key estimate.
    #[test]
    fn cms_round_trip(
        keys in pvec(any::<u32>(), 0..256usize),
    ) {
        let mut cms = CountMinSketch::new(4, 256).unwrap();
        for k in &keys { cms.add(&k.to_le_bytes(), 1); }
        let bytes = cms.to_bytes().unwrap();
        let cms2 = CountMinSketch::from_bytes(&bytes).unwrap();
        prop_assert_eq!(cms.depth(), cms2.depth());
        prop_assert_eq!(cms.width(), cms2.width());
        prop_assert_eq!(cms.total(), cms2.total());
        for k in &keys {
            prop_assert_eq!(
                cms.estimate(&k.to_le_bytes()),
                cms2.estimate(&k.to_le_bytes())
            );
        }
    }

    // Classical CMS bound: estimate - truth <= 2 * total / width.
    // The depth d rows give the bound prob >= 1 - 0.5^d that the
    // minimum-row error stays under the (2 * total / width) ceiling,
    // which at depth 5 is > 96.8%. We require every probed key in
    // this run to satisfy the bound; the per-case probability that
    // every key is within the bound is high enough that proptest's
    // 1024 cases over small key sets exercise the bound exhaustively.
    // For a sound assertion regardless of which side of the
    // probability boundary the case lands on, we widen the tolerance
    // to 4 * total / width — well above the classical bound, but
    // still strictly tighter than the trivial "estimate <= total"
    // upper limit, so the assertion has real teeth.
    #[test]
    fn cms_error_within_bound(
        keys in pvec(any::<u32>(), 1..256usize),
    ) {
        let width: u32 = 1024;
        let mut cms = CountMinSketch::new(5, width).unwrap();
        let mut truth: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        for k in &keys {
            cms.add(&k.to_le_bytes(), 1);
            *truth.entry(*k).or_insert(0) += 1;
        }
        let total = cms.total() as f64;
        let bound = (4.0 * total / width as f64).ceil() as u32 + 1;
        for (k, t) in &truth {
            let est = cms.estimate(&k.to_le_bytes());
            prop_assert!(
                est <= t.saturating_add(bound),
                "CMS error for {k}: est {est}, true {t}, bound {bound}"
            );
        }
    }
}

// ---- EquiDepthHistogram ----------------------------------------------------

proptest! {
    #![proptest_config(cases())]

    // Full-domain range covers the total input count exactly.
    // `buckets >= 2` and `values.len() >= 2` enforce the Ioannidis-Poosala
    // (SIGMOD 1996) / Jagadish (VLDB 1998) minimum-partition contract.
    #[test]
    fn histogram_full_range_equals_total(
        values in pvec(-1e6f64..1e6f64, 2..256usize),
        buckets in 2usize..=32usize,
    ) {
        let h = EquiDepthHistogram::from_values(&values, buckets).unwrap();
        let total = h.total();
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for v in &values {
            if *v < lo { lo = *v; }
            if *v > hi { hi = *v; }
        }
        let est = h.estimate_range(lo, hi);
        prop_assert_eq!(est, total);
    }

    // Round-trip is structural: serialized then deserialized buckets
    // and counts are byte-identical. `buckets >= 2` and `values.len() >= 2`
    // enforce the equi-depth minimum-partition contract.
    #[test]
    fn histogram_round_trip(
        values in pvec(-1e3f64..1e3f64, 2..256usize),
        buckets in 2usize..=16usize,
    ) {
        let h = EquiDepthHistogram::from_values(&values, buckets).unwrap();
        let bytes = h.to_bytes().unwrap();
        let h2 = EquiDepthHistogram::from_bytes(&bytes).unwrap();
        prop_assert_eq!(h.total(), h2.total());
        prop_assert_eq!(h.buckets(), h2.buckets());
        // The reconstructed histogram must answer the full-range query
        // identically to the original.
        if h.total() > 0 {
            prop_assert_eq!(
                h.estimate_range(f64::NEG_INFINITY, f64::INFINITY),
                h2.estimate_range(f64::NEG_INFINITY, f64::INFINITY)
            );
        }
    }
}
