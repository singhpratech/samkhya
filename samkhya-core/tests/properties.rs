//! Property-based integration tests for samkhya-core.
//!
//! Uses `proptest` to validate invariants across sketches, Puffin I/O,
//! and the LpBound envelope.

use std::io::Cursor;

use proptest::collection::{hash_set, vec as pvec};
use proptest::prelude::*;

use samkhya_core::lpbound::{
    ChainBound, ProductBound, UpperBound, clamp_estimate, saturating_clamp,
};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{BloomFilter, CountMinSketch, HllSketch, Sketch};

// ---- Helpers ---------------------------------------------------------------

fn build_hll_from_u32_set(items: &std::collections::HashSet<u32>, precision: u8) -> HllSketch {
    let mut hll = HllSketch::new(precision).unwrap();
    for x in items {
        hll.add(&x.to_le_bytes());
    }
    hll
}

fn build_hll_from_byte_slices(slices: &[Vec<u8>], precision: u8) -> HllSketch {
    let mut hll = HllSketch::new(precision).unwrap();
    for s in slices {
        hll.add(s);
    }
    hll
}

// ---- HLL properties --------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// HLL must not overestimate by more than 2× the true cardinality.
    /// A small absolute slack accommodates the linear-counting regime where
    /// the estimator's natural error floor dominates at very small N.
    #[test]
    fn hll_no_overestimate_more_than_2x(
        items in hash_set(any::<u32>(), 1..=1000usize),
    ) {
        let hll = build_hll_from_u32_set(&items, 12);
        let est = hll.estimate() as f64;
        let truth = items.len() as f64;
        // Allow an absolute slack of 8 items to absorb tiny-N estimator noise.
        let upper = (2.0 * truth).max(truth + 8.0);
        prop_assert!(
            est <= upper,
            "HLL est {est} exceeds 2x of truth {truth} (upper={upper})"
        );
    }

    /// HLL merge is commutative on registers (registers store the elementwise max,
    /// which is commutative). We check by serializing both merge orderings.
    #[test]
    fn hll_merge_commutative(
        a_items in hash_set(any::<u32>(), 0..=500usize),
        b_items in hash_set(any::<u32>(), 0..=500usize),
    ) {
        let hll_a = build_hll_from_u32_set(&a_items, 12);
        let hll_b = build_hll_from_u32_set(&b_items, 12);

        let mut ab = hll_a.clone();
        ab.merge(&hll_b).unwrap();

        let mut ba = hll_b.clone();
        ba.merge(&hll_a).unwrap();

        // Bincode encoding is deterministic over the same struct values, so
        // equal serialized bytes ⇒ equal registers + precision.
        let ab_bytes = ab.to_bytes().unwrap();
        let ba_bytes = ba.to_bytes().unwrap();
        prop_assert_eq!(ab_bytes, ba_bytes);
    }

    /// to_bytes → from_bytes round-trip yields an identical sketch.
    #[test]
    fn hll_round_trip_preserves_registers(
        slices in pvec(pvec(any::<u8>(), 0..=32usize), 0..=200usize),
    ) {
        let hll = build_hll_from_byte_slices(&slices, 12);
        let bytes = hll.to_bytes().unwrap();
        let hll2 = HllSketch::from_bytes(&bytes).unwrap();
        let bytes2 = hll2.to_bytes().unwrap();
        prop_assert_eq!(bytes, bytes2);
        prop_assert_eq!(hll.precision(), hll2.precision());
        prop_assert_eq!(hll.estimate(), hll2.estimate());
    }
}

// ---- Bloom properties ------------------------------------------------------

proptest! {
    /// Bloom filters must never produce false negatives.
    #[test]
    fn bloom_no_false_negatives(
        items in pvec(pvec(any::<u8>(), 0..=32usize), 1..=500usize),
    ) {
        let mut bf = BloomFilter::new(items.len(), 0.01);
        for it in &items {
            bf.insert(it);
        }
        for it in &items {
            prop_assert!(bf.contains(it), "false negative for {:?}", it);
        }
    }

    /// Serialization round-trip preserves membership for all inserted items.
    #[test]
    fn bloom_round_trip_preserves_state(
        items in pvec(pvec(any::<u8>(), 0..=32usize), 1..=500usize),
    ) {
        let mut bf = BloomFilter::new(items.len(), 0.01);
        for it in &items {
            bf.insert(it);
        }
        let bytes = bf.to_bytes().unwrap();
        let bf2 = BloomFilter::from_bytes(&bytes).unwrap();
        prop_assert_eq!(bf.num_bits(), bf2.num_bits());
        prop_assert_eq!(bf.num_hashes(), bf2.num_hashes());
        for it in &items {
            prop_assert_eq!(bf.contains(it), bf2.contains(it));
            prop_assert!(bf2.contains(it));
        }
    }
}

// ---- CMS properties --------------------------------------------------------

proptest! {
    /// CMS never undercounts: for any item inserted with frequency `f`, the
    /// estimate must be ≥ f (Count-Min upper-bounds the true count).
    #[test]
    fn cms_never_undercounts(
        items in pvec((pvec(any::<u8>(), 1..=16usize), 1u32..=100), 1..=100usize),
    ) {
        let mut cms = CountMinSketch::new(4, 256).unwrap();
        let mut truth = std::collections::HashMap::<Vec<u8>, u32>::new();
        for (k, c) in &items {
            cms.add(k, *c);
            *truth.entry(k.clone()).or_default() = truth.get(k).copied().unwrap_or(0).saturating_add(*c);
        }
        for (k, t) in &truth {
            prop_assert!(cms.estimate(k) >= *t,
                "CMS undercount: estimate={} truth={}", cms.estimate(k), t);
        }
    }

    /// CMS round-trip preserves the estimate for every key.
    #[test]
    fn cms_round_trip_preserves_estimates(
        items in pvec((pvec(any::<u8>(), 1..=16usize), 1u32..=50), 1..=50usize),
    ) {
        let mut cms = CountMinSketch::new(4, 128).unwrap();
        for (k, c) in &items {
            cms.add(k, *c);
        }
        let bytes = cms.to_bytes().unwrap();
        let cms2 = CountMinSketch::from_bytes(&bytes).unwrap();
        for (k, _) in &items {
            prop_assert_eq!(cms.estimate(k), cms2.estimate(k));
        }
    }
}

// ---- Puffin properties -----------------------------------------------------

// Restrict kind strings to non-empty ASCII to avoid degenerate cases, but
// still cover a wide range of values.
fn blob_strategy() -> impl Strategy<Value = (String, Vec<u8>)> {
    ("[a-zA-Z0-9_.-]{1,16}", pvec(any::<u8>(), 0..=128usize))
}

proptest! {
    /// Puffin write → read round-trip preserves every blob payload.
    #[test]
    fn puffin_round_trip_preserves_blobs(
        blobs in pvec(blob_strategy(), 0..=12usize),
    ) {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        for (kind, payload) in &blobs {
            writer
                .add_blob(Blob::new(kind.clone(), vec![0], payload))
                .unwrap();
        }
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        prop_assert_eq!(reader.blobs().len(), blobs.len());

        for (i, (kind, payload)) in blobs.iter().enumerate() {
            let got = reader.read_blob(i).unwrap();
            prop_assert_eq!(&got, payload);
            prop_assert_eq!(&reader.blobs()[i].kind, kind);
        }
    }
}

// ---- LpBound properties ----------------------------------------------------

proptest! {
    /// ProductBound is monotone: if R1[i] ≤ R2[i] for every i, then
    /// ceiling(R1) ≤ ceiling(R2). Both sides saturate at u64::MAX, so the
    /// inequality still holds under overflow.
    #[test]
    fn lpbound_product_monotone_in_inputs(
        // Pairs (a, b) with a ≤ b, element-wise.
        pairs in pvec((any::<u32>(), any::<u32>()), 0..=8usize),
    ) {
        let r1: Vec<u64> = pairs.iter().map(|(a, b)| (*a).min(*b) as u64).collect();
        let r2: Vec<u64> = pairs.iter().map(|(a, b)| (*a).max(*b) as u64).collect();
        let c1 = ProductBound.ceiling(&r1, &[]);
        let c2 = ProductBound.ceiling(&r2, &[]);
        prop_assert!(c1 <= c2, "monotonicity violated: {c1} > {c2}");
    }

    /// saturating_clamp never returns a value above the ceiling, regardless
    /// of the estimate (including NaN, negative, or huge values).
    #[test]
    fn lpbound_saturating_clamp_never_exceeds_ceiling(
        // Permit a broad range of finite + non-finite estimates.
        estimate in prop_oneof![
            Just(f64::NAN),
            Just(f64::INFINITY),
            Just(f64::NEG_INFINITY),
            (-1e20f64..1e20f64),
        ],
        ceiling in any::<u64>(),
    ) {
        let out = saturating_clamp(estimate, ceiling);
        prop_assert!(out <= ceiling, "clamp {out} > ceiling {ceiling}");
    }

    /// When `estimate` is finite, non-negative, and ≤ ceiling, then
    /// `clamp_estimate` succeeds and returns the same value as
    /// `saturating_clamp`.
    #[test]
    fn lpbound_clamp_estimate_consistent_with_saturating(
        ceiling in any::<u64>(),
        // Generate an estimate within [0, ceiling] via a fractional scale.
        scale in 0.0f64..=1.0f64,
    ) {
        let estimate = (ceiling as f64) * scale;
        let sat = saturating_clamp(estimate, ceiling);
        let clamped = clamp_estimate(estimate, ceiling).unwrap();
        prop_assert_eq!(sat, clamped);
        prop_assert!(clamped <= ceiling);
    }

    /// ChainBound with any positive distinct counts is at most the product
    /// of relation sizes (the trivial upper bound).
    #[test]
    fn chainbound_never_exceeds_product(
        relations in pvec(1u64..=10_000, 2..=4),
        distinct_counts in pvec(1u64..=10_000, 2..=4),
    ) {
        // Pair each adjacent relation as an equality predicate.
        let preds: Vec<(usize, usize)> = (0..relations.len() - 1).map(|i| (i, i + 1)).collect();
        let cb = ChainBound::new(distinct_counts.clone());
        let cb_bound = cb.ceiling(&relations, &preds);
        let pb_bound = ProductBound.ceiling(&relations, &[]);
        prop_assert!(cb_bound <= pb_bound,
            "ChainBound {cb_bound} > ProductBound {pb_bound} for relations={relations:?} distinct={distinct_counts:?}");
    }

    /// ChainBound with no equality predicates falls back to ProductBound exactly.
    #[test]
    fn chainbound_no_predicates_equals_product(
        relations in pvec(1u64..=10_000, 1..=4),
        distinct_counts in pvec(1u64..=10_000, 0..=4),
    ) {
        let cb = ChainBound::new(distinct_counts);
        let cb_bound = cb.ceiling(&relations, &[]);
        let pb_bound = ProductBound.ceiling(&relations, &[]);
        prop_assert_eq!(cb_bound, pb_bound);
    }
}
