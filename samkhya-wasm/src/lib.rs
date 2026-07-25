//! samkhya for JavaScript and TypeScript.
//!
//! A Rust engine inside, a plain JS API outside. This crate exposes the two
//! parts of samkhya that are genuinely portable — the sketches and the
//! provable join ceiling — compiled to WebAssembly with no server, no native
//! module, and no build step for the consumer.
//!
//! # What is here
//!
//! * [`HllSketch`], [`BloomFilter`], [`CountMinSketch`] — the sketches, with
//!   the same byte format the Rust and Python packages read. A sketch built
//!   in a browser deserialises unchanged in a Rust query engine.
//! * [`join_ceiling`] — a number the join provably cannot exceed, computed
//!   from row counts and distinct counts. Not an estimate.
//!
//! # What is not here
//!
//! The SQLite feedback store, and therefore corrector training. SQLite is not
//! available on `wasm32-unknown-unknown`, so `samkhya-core` is built with
//! `--no-default-features`. Train a model with the Rust or Python tooling and
//! serve it from there; this package is for computing statistics and bounds
//! where the data already is.
//!
//! # Example
//!
//! ```js
//! import init, { HllSketch, joinCeiling } from 'samkhya';
//!
//! await init();
//!
//! const hll = new HllSketch(12);
//! for (const row of rows) hll.add(row.orderKey);
//! const distinct = hll.estimate();
//!
//! // 10 orders joined to 100 line items over `distinct` keys.
//! const ceiling = joinCeiling([10, 100], [0, 1], [distinct, distinct]);
//! ```

use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_core::lpbound::{ProductBound, UpperBound};
use samkhya_core::sketches::{
    BloomFilter as CoreBloom, CountMinSketch as CoreCms, HllSketch as CoreHll, Sketch,
};
use wasm_bindgen::prelude::*;

fn js_err(e: samkhya_core::Error) -> JsError {
    JsError::new(&e.to_string())
}

/// HyperLogLog distinct-count sketch.
///
/// Precision `p` selects `2^p` registers; relative error is about
/// `1.04 / sqrt(2^p)`. Valid range is 4 to 18.
#[wasm_bindgen]
pub struct HllSketch {
    inner: CoreHll,
}

#[wasm_bindgen]
impl HllSketch {
    /// Build an empty sketch at precision `p`.
    #[wasm_bindgen(constructor)]
    pub fn new(p: u8) -> Result<HllSketch, JsError> {
        CoreHll::new(p)
            .map(|inner| HllSketch { inner })
            .map_err(js_err)
    }

    /// Add a value. Strings are hashed as UTF-8 bytes.
    pub fn add(&mut self, item: &str) {
        self.inner.add(item.as_bytes());
    }

    /// Add raw bytes, for callers that already have a stable encoding.
    #[wasm_bindgen(js_name = addBytes)]
    pub fn add_bytes(&mut self, item: &[u8]) {
        self.inner.add(item);
    }

    /// Current distinct-count estimate.
    ///
    /// This is a two-sided estimate: it lands above the truth about half the
    /// time. Use [`Self::distinct_floor`] where a value that is never above
    /// the truth is required — deriving a join ceiling, for instance.
    pub fn estimate(&self) -> f64 {
        self.inner.estimate() as f64
    }

    /// A distinct count that is never above the truth.
    ///
    /// Every value hashes to exactly one register, so a register is non-zero
    /// only if some distinct value reached it. Collisions only push the count
    /// further down. Weak, but sound — which is what a provable bound needs.
    #[wasm_bindgen(js_name = distinctFloor)]
    pub fn distinct_floor(&self) -> f64 {
        self.inner.nonzero_registers() as f64
    }

    /// Merge another sketch of the same precision into this one.
    pub fn merge(&mut self, other: &HllSketch) -> Result<(), JsError> {
        self.inner.merge(&other.inner).map_err(js_err)
    }

    /// Serialise to the portable payload every samkhya binding reads.
    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Result<Vec<u8>, JsError> {
        self.inner.to_bytes().map_err(js_err)
    }

    /// Restore a sketch from [`Self::to_bytes`], validating it first.
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(data: &[u8]) -> Result<HllSketch, JsError> {
        CoreHll::from_bytes(data)
            .map(|inner| HllSketch { inner })
            .map_err(js_err)
    }

    /// Precision the sketch was built at.
    #[wasm_bindgen(getter)]
    pub fn precision(&self) -> u8 {
        self.inner.precision()
    }
}

/// Bloom filter sized for `n_items` at a target false-positive rate.
#[wasm_bindgen]
pub struct BloomFilter {
    inner: CoreBloom,
}

#[wasm_bindgen]
impl BloomFilter {
    /// Build a filter. `fp_rate` must be in `(0, 1)`.
    #[wasm_bindgen(constructor)]
    pub fn new(n_items: usize, fp_rate: f64) -> Result<BloomFilter, JsError> {
        CoreBloom::try_new(n_items, fp_rate)
            .map(|inner| BloomFilter { inner })
            .map_err(js_err)
    }

    /// Insert a value.
    pub fn add(&mut self, item: &str) {
        self.inner.insert(item.as_bytes());
    }

    /// `true` if the filter may contain the value, `false` if it definitely
    /// does not. False positives are possible; false negatives are not.
    pub fn contains(&self, item: &str) -> bool {
        self.inner.contains(item.as_bytes())
    }

    /// Serialise to the portable payload.
    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Result<Vec<u8>, JsError> {
        self.inner.to_bytes().map_err(js_err)
    }

    /// Restore from [`Self::to_bytes`].
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(data: &[u8]) -> Result<BloomFilter, JsError> {
        CoreBloom::from_bytes(data)
            .map(|inner| BloomFilter { inner })
            .map_err(js_err)
    }
}

/// Count-Min frequency sketch.
#[wasm_bindgen]
pub struct CountMinSketch {
    inner: CoreCms,
}

#[wasm_bindgen]
impl CountMinSketch {
    /// Build a sketch with `depth` hash rows of `width` counters each.
    #[wasm_bindgen(constructor)]
    pub fn new(depth: u32, width: u32) -> Result<CountMinSketch, JsError> {
        CoreCms::try_new(depth, width)
            .map(|inner| CountMinSketch { inner })
            .map_err(js_err)
    }

    /// Add `count` occurrences of a value.
    pub fn add(&mut self, item: &str, count: u32) {
        self.inner.add(item.as_bytes(), count);
    }

    /// Frequency estimate. Never below the truth unless the sketch has
    /// saturated — check [`Self::is_saturated`].
    pub fn estimate(&self, item: &str) -> f64 {
        f64::from(self.inner.estimate(item.as_bytes()))
    }

    /// Whether any counter has reached its maximum, which is the one
    /// condition under which the never-undercount guarantee fails.
    #[wasm_bindgen(js_name = isSaturated)]
    pub fn is_saturated(&self) -> bool {
        self.inner.is_saturated()
    }

    /// An upper bound on the frequency of the most frequent value, without
    /// needing to know which value that is. `null` when saturated.
    #[wasm_bindgen(js_name = maxFrequencyBound)]
    pub fn max_frequency_bound(&self) -> Option<f64> {
        self.inner.max_frequency_bound().map(f64::from)
    }

    /// Serialise to the portable payload.
    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Result<Vec<u8>, JsError> {
        self.inner.to_bytes().map_err(js_err)
    }

    /// Restore from [`Self::to_bytes`].
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(data: &[u8]) -> Result<CountMinSketch, JsError> {
        CoreCms::from_bytes(data)
            .map(|inner| CountMinSketch { inner })
            .map_err(js_err)
    }
}

/// A number the join provably cannot exceed.
///
/// `rows[i]` is the row count of relation `i`. `edges` is a flat list of
/// relation-index pairs — `[0, 1, 1, 2]` means relation 0 joins relation 1,
/// and relation 1 joins relation 2. `distinct_counts[i]`, when supplied, is
/// the number of distinct join-key values in relation `i`.
///
/// # The distinct counts must not be over-stated
///
/// The bound derives a maximum degree as `rows - distinct + 1`, so it
/// subtracts the distinct count. A value above the truth produces a ceiling
/// *below* the truth, which defeats the point. Pass
/// [`HllSketch::distinct_floor`], not [`HllSketch::estimate`].
///
/// Pass an empty array to omit them entirely: the result is then the
/// Cartesian product, which is sound and useless — the honest answer when
/// nothing better is known.
///
/// # Example
///
/// ```js
/// // 10 orders, 100 line items, 10 distinct order keys on both sides.
/// joinCeiling([10, 100], [0, 1], [10, 10]);   // 100 — exactly the truth
/// joinCeiling([10, 100], [0, 1], []);         // 1000 — the product
/// ```
#[wasm_bindgen(js_name = joinCeiling)]
pub fn join_ceiling(rows: Vec<f64>, edges: Vec<u32>, distinct_counts: Vec<f64>) -> f64 {
    let to_u64 = |v: f64| -> u64 {
        if !v.is_finite() || v < 0.0 {
            0
        } else if v >= u64::MAX as f64 {
            u64::MAX
        } else {
            v as u64
        }
    };

    let row_counts: Vec<u64> = rows.iter().copied().map(to_u64).collect();
    if row_counts.is_empty() {
        return 0.0;
    }
    let distinct: Vec<u64> = distinct_counts.iter().copied().map(to_u64).collect();

    let mut relations: Vec<JoinRelation> =
        row_counts.iter().map(|&n| JoinRelation::new(n)).collect();

    // `edges` arrives flattened; a trailing unpaired index is ignored.
    let pairs: Vec<(usize, usize)> = edges
        .chunks_exact(2)
        .map(|c| (c[0] as usize, c[1] as usize))
        .collect();

    for (attribute, &(i, j)) in pairs.iter().enumerate() {
        for endpoint in [i, j] {
            if endpoint >= row_counts.len() {
                continue;
            }
            let degree = match distinct.get(endpoint) {
                Some(&d) => AttributeDegree::from_distinct(row_counts[endpoint], d),
                None => AttributeDegree::unknown(row_counts[endpoint]),
            };
            relations[endpoint] = std::mem::replace(
                &mut relations[endpoint],
                JoinRelation::new(row_counts[endpoint]),
            )
            .with_degree(attribute as u32, degree);
        }
    }

    let mut graph = JoinGraph::new(relations);
    for (attribute, &(i, j)) in pairs.iter().enumerate() {
        graph = graph.with_edge(i, j, attribute as u32);
    }
    graph.ceiling() as f64
}

/// The Cartesian product of the row counts — the ceiling that holds when
/// nothing is known about the join keys.
#[wasm_bindgen(js_name = productBound)]
pub fn product_bound(rows: Vec<f64>) -> f64 {
    let counts: Vec<u64> = rows
        .iter()
        .map(|&v| {
            if !v.is_finite() || v < 0.0 {
                0
            } else if v >= u64::MAX as f64 {
                u64::MAX
            } else {
                v as u64
            }
        })
        .collect();
    ProductBound.ceiling(&counts, &[]) as f64
}

/// Version of the underlying samkhya crate.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_key_join_bounds_exactly() {
        // 10 orders, 100 line items, 10 distinct keys: the true output is 100.
        assert_eq!(
            join_ceiling(vec![10.0, 100.0], vec![0, 1], vec![10.0, 10.0]),
            100.0
        );
    }

    #[test]
    fn no_distinct_counts_gives_the_product() {
        assert_eq!(
            join_ceiling(vec![10.0, 100.0], vec![0, 1], Vec::new()),
            1_000.0
        );
    }

    #[test]
    fn a_chain_stays_below_the_product() {
        let ceiling = join_ceiling(
            vec![100.0, 100.0, 100.0],
            vec![0, 1, 1, 2],
            vec![100.0, 100.0, 100.0],
        );
        assert!(ceiling <= product_bound(vec![100.0, 100.0, 100.0]));
    }

    #[test]
    fn empty_input_is_zero() {
        assert_eq!(join_ceiling(Vec::new(), Vec::new(), Vec::new()), 0.0);
    }

    #[test]
    fn out_of_range_edges_are_dropped_not_panicked_on() {
        // A misbuilt join graph must degrade the ceiling, never crash it.
        let ceiling = join_ceiling(vec![10.0, 20.0], vec![0, 99], vec![10.0, 20.0]);
        assert!(ceiling > 0.0);
    }

    #[test]
    fn a_trailing_unpaired_edge_index_is_ignored() {
        assert_eq!(
            join_ceiling(vec![10.0, 100.0], vec![0, 1, 1], vec![10.0, 10.0]),
            100.0
        );
    }

    #[test]
    fn sketches_round_trip_through_bytes() {
        let mut hll = HllSketch::new(12).unwrap();
        for i in 0..500 {
            hll.add(&format!("key-{i}"));
        }
        let bytes = hll.to_bytes().unwrap();
        let restored = HllSketch::from_bytes(&bytes).unwrap();
        assert_eq!(hll.estimate(), restored.estimate());
        assert_eq!(restored.precision(), 12);
    }

    #[test]
    fn the_distinct_floor_never_exceeds_the_truth() {
        let mut hll = HllSketch::new(12).unwrap();
        for i in 0..500 {
            hll.add(&format!("key-{i}"));
        }
        assert!(hll.distinct_floor() <= 500.0);
    }

    #[test]
    fn count_min_bounds_the_hottest_key() {
        let mut cms = CountMinSketch::new(5, 1024).unwrap();
        for _ in 0..9 {
            cms.add("hot", 1);
        }
        cms.add("cold", 1);
        assert!(!cms.is_saturated());
        assert!(cms.max_frequency_bound().unwrap() >= 9.0);
    }

    #[test]
    fn bloom_has_no_false_negatives() {
        let mut bloom = BloomFilter::new(1_000, 0.01).unwrap();
        for i in 0..100 {
            bloom.add(&format!("v{i}"));
        }
        for i in 0..100 {
            assert!(bloom.contains(&format!("v{i}")));
        }
    }
}
