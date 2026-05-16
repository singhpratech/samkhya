//! Equi-depth histogram for range / inequality predicate selectivity.
//!
//! Given a sorted column population, partitions the values into `b`
//! buckets each holding approximately the same number of items. For a
//! range predicate `lo ≤ x ≤ hi`, the histogram returns the estimated
//! number of items in that range by linearly interpolating within the
//! buckets that straddle the range endpoints.
//!
//! Pairs with HLL (distinct count) and Bloom (membership) to cover
//! the three optimizer-relevant selectivity classes:
//! - equality ⇒ HLL distinct count ⇒ selectivity 1/D
//! - membership ⇒ Bloom contains
//! - range ⇒ this histogram

use serde::{Deserialize, Serialize};

use crate::sketches::Sketch;
use crate::{Error, Result};

/// Single-column equi-depth histogram for range-predicate selectivity.
///
/// Partitions the sorted input into `b` buckets, each holding approximately
/// the same number of rows. Range estimates interpolate linearly inside the
/// partial-overlap buckets at each endpoint.
///
/// # Examples
///
/// ```
/// use samkhya_core::sketches::EquiDepthHistogram;
///
/// let values: Vec<f64> = (0..1000).map(|i| i as f64).collect();
/// let h = EquiDepthHistogram::from_values(&values, 10).unwrap();
/// // Whole-support range recovers the full count.
/// assert_eq!(h.estimate_range(0.0, 999.0), 1000);
/// // A clearly disjoint range estimates to 0.
/// assert_eq!(h.estimate_range(10_000.0, 20_000.0), 0);
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EquiDepthHistogram {
    /// Sorted bucket boundaries. `len() == buckets + 1`.
    boundaries: Vec<f64>,
    /// Per-bucket count. `len() == buckets`.
    counts: Vec<u64>,
    /// Total items represented; sum(counts).
    total: u64,
}

impl EquiDepthHistogram {
    /// Fallible parameter-only constructor: produce an empty
    /// histogram shell with `buckets` zero-count bins and degenerate
    /// `[0, 0]` boundaries. Validates `buckets > 0` before allocating.
    /// Useful when the caller wants a typed receptacle to merge into
    /// rather than building from a value population.
    pub fn try_new(buckets: usize) -> Result<Self> {
        if buckets == 0 {
            return Err(Error::InvalidSketch(
                "EquiDepthHistogram buckets must be > 0".into(),
            ));
        }
        // boundaries.len() == buckets + 1, all zero; counts all zero.
        Ok(Self {
            boundaries: vec![0.0; buckets + 1],
            counts: vec![0u64; buckets],
            total: 0,
        })
    }

    /// Build a histogram from a slice of sorted (or unsorted; we sort it
    /// here) `f64` values, partitioned into `buckets` equi-depth bins.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::EquiDepthHistogram;
    ///
    /// let h = EquiDepthHistogram::from_values(&[1.0, 2.0, 3.0, 4.0], 2).unwrap();
    /// assert_eq!(h.total(), 4);
    /// assert_eq!(h.buckets(), 2);
    /// // Empty population is handled without erroring.
    /// let empty = EquiDepthHistogram::from_values(&[], 5).unwrap();
    /// assert_eq!(empty.total(), 0);
    /// ```
    pub fn from_values(values: &[f64], buckets: usize) -> Result<Self> {
        if buckets == 0 {
            return Err(Error::InvalidSketch(
                "EquiDepthHistogram buckets must be > 0".into(),
            ));
        }
        if values.is_empty() {
            return Ok(Self {
                boundaries: vec![0.0, 0.0],
                counts: vec![0],
                total: 0,
            });
        }
        let mut sorted: Vec<f64> = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        let buckets = buckets.min(n);

        let per_bucket = n / buckets;
        let mut remainder = n % buckets;

        let mut boundaries = Vec::with_capacity(buckets + 1);
        let mut counts = Vec::with_capacity(buckets);
        boundaries.push(sorted[0]);
        let mut cursor = 0usize;
        for _ in 0..buckets {
            let mut take = per_bucket;
            if remainder > 0 {
                take += 1;
                remainder -= 1;
            }
            cursor += take;
            let edge_idx = cursor.min(n) - 1;
            boundaries.push(sorted[edge_idx]);
            counts.push(take as u64);
        }
        Ok(Self {
            boundaries,
            counts,
            total: n as u64,
        })
    }

    /// Estimate the number of items in the inclusive range `[lo, hi]`.
    ///
    /// Interpolates linearly within partial buckets. Returns the total
    /// count if the range spans the whole histogram, 0 if the range is
    /// empty or outside the support.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::EquiDepthHistogram;
    ///
    /// let values: Vec<f64> = (0..1000).map(|i| i as f64).collect();
    /// let h = EquiDepthHistogram::from_values(&values, 10).unwrap();
    /// // Roughly half the support contains roughly half the rows.
    /// let est = h.estimate_range(0.0, 500.0);
    /// assert!((400..=600).contains(&est), "got {est}");
    /// // Inverted range (lo > hi) returns 0.
    /// assert_eq!(h.estimate_range(500.0, 0.0), 0);
    /// ```
    pub fn estimate_range(&self, lo: f64, hi: f64) -> u64 {
        if lo > hi || self.counts.is_empty() {
            return 0;
        }
        let nb = self.counts.len();
        let mut estimate = 0.0f64;
        for i in 0..nb {
            let b_lo = self.boundaries[i];
            let b_hi = self.boundaries[i + 1];
            let cnt = self.counts[i] as f64;
            if b_hi < lo || b_lo > hi {
                continue;
            }
            // Bucket is entirely inside the query range
            if b_lo >= lo && b_hi <= hi {
                estimate += cnt;
                continue;
            }
            // Partial overlap — linear interpolation
            let overlap_lo = b_lo.max(lo);
            let overlap_hi = b_hi.min(hi);
            let bucket_width = (b_hi - b_lo).max(f64::EPSILON);
            let overlap_width = (overlap_hi - overlap_lo).max(0.0);
            estimate += cnt * (overlap_width / bucket_width);
        }
        estimate.max(0.0) as u64
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn buckets(&self) -> usize {
        self.counts.len()
    }

    /// Validate the structural invariants of a deserialised payload:
    /// at least one bucket (so boundaries.len() >= 2), boundaries.len()
    /// matches counts.len() + 1, no NaN bin edges, and the boundaries
    /// vector is non-decreasing. Used by [`Sketch::from_bytes`] to
    /// reject adversarial byte streams that bincode-decode but violate
    /// the type contract.
    fn validate(&self) -> Result<()> {
        if self.counts.is_empty() {
            return Err(Error::InvalidSketch(
                "EquiDepthHistogram must have ≥ 1 bucket".into(),
            ));
        }
        if self.boundaries.len() != self.counts.len() + 1 {
            return Err(Error::InvalidSketch(format!(
                "EquiDepthHistogram boundaries.len() {} != counts.len()+1 = {}",
                self.boundaries.len(),
                self.counts.len() + 1
            )));
        }
        // No NaN boundary edges.
        for (i, b) in self.boundaries.iter().enumerate() {
            if b.is_nan() {
                return Err(Error::InvalidSketch(format!(
                    "EquiDepthHistogram boundary[{i}] is NaN"
                )));
            }
        }
        // Monotone non-decreasing boundary edges.
        for w in self.boundaries.windows(2) {
            if w[0] > w[1] {
                return Err(Error::InvalidSketch(format!(
                    "EquiDepthHistogram boundaries not non-decreasing: {} > {}",
                    w[0], w[1]
                )));
            }
        }
        // Sum of bucket counts must equal total.
        let sum: u64 = self.counts.iter().fold(0u64, |a, &c| a.saturating_add(c));
        if sum != self.total {
            return Err(Error::InvalidSketch(format!(
                "EquiDepthHistogram total {} != sum(counts) {}",
                self.total, sum
            )));
        }
        Ok(())
    }
}

impl Sketch for EquiDepthHistogram {
    const KIND: &'static str = "samkhya.histogram-equidepth-v1";

    fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(Into::into)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let s: Self = bincode::deserialize(bytes).map_err(Error::from)?;
        s.validate()?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_whole_range_correctly() {
        let values: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let h = EquiDepthHistogram::from_values(&values, 10).unwrap();
        assert_eq!(h.estimate_range(0.0, 999.0), 1000);
    }

    #[test]
    fn estimates_half_range_approximately() {
        let values: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let h = EquiDepthHistogram::from_values(&values, 10).unwrap();
        let est = h.estimate_range(0.0, 500.0);
        // True count is 501 (0..=500). Equi-depth interpolation should be close.
        assert!(
            (400..=600).contains(&est),
            "half-range estimate {est} too far"
        );
    }

    #[test]
    fn empty_range_returns_zero() {
        let values: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let h = EquiDepthHistogram::from_values(&values, 5).unwrap();
        assert_eq!(h.estimate_range(50.0, 40.0), 0);
        assert_eq!(h.estimate_range(200.0, 300.0), 0);
    }

    #[test]
    fn empty_population_handled() {
        let h = EquiDepthHistogram::from_values(&[], 5).unwrap();
        assert_eq!(h.total(), 0);
        assert_eq!(h.estimate_range(0.0, 100.0), 0);
    }

    #[test]
    fn round_trip() {
        let values: Vec<f64> = (0..500).map(|i| (i as f64) * 0.3).collect();
        let h = EquiDepthHistogram::from_values(&values, 16).unwrap();
        let bytes = h.to_bytes().unwrap();
        let h2 = EquiDepthHistogram::from_bytes(&bytes).unwrap();
        assert_eq!(h.total, h2.total);
        assert_eq!(h.counts, h2.counts);
        assert_eq!(h.boundaries, h2.boundaries);
    }

    #[test]
    fn zero_buckets_errors() {
        let values = vec![1.0, 2.0, 3.0];
        assert!(EquiDepthHistogram::from_values(&values, 0).is_err());
    }

    #[test]
    fn try_new_rejects_zero_buckets() {
        assert!(EquiDepthHistogram::try_new(0).is_err());
    }

    #[test]
    fn try_new_allocates_empty_shell() {
        let h = EquiDepthHistogram::try_new(8).unwrap();
        assert_eq!(h.buckets(), 8);
        assert_eq!(h.total(), 0);
    }

    #[test]
    fn from_bytes_rejects_all_zero_payload() {
        // Bincode decodes empty Vecs from zero-length prefixes →
        // boundaries=[], counts=[], which validate() rejects (need ≥1
        // bucket and consistent lengths). Covers the EquiDepth half of
        // the H03 shape gap.
        for n in [4usize, 8, 16, 32, 128, 1024, 4 * 1024 * 1024] {
            let zeros = vec![0u8; n];
            assert!(
                EquiDepthHistogram::from_bytes(&zeros).is_err(),
                "all-zero len {n} accepted by from_bytes"
            );
        }
    }

    #[test]
    fn from_bytes_rejects_non_monotone_boundaries() {
        // Build a valid histogram, then mutate its bytes via a
        // bincode encode of an out-of-order EquiDepth.
        #[derive(serde::Serialize)]
        struct Wire {
            boundaries: Vec<f64>,
            counts: Vec<u64>,
            total: u64,
        }
        let bad = Wire {
            boundaries: vec![5.0, 3.0, 7.0], // non-monotone
            counts: vec![1, 1],
            total: 2,
        };
        let bytes = bincode::serialize(&bad).unwrap();
        assert!(
            EquiDepthHistogram::from_bytes(&bytes).is_err(),
            "non-monotone boundaries accepted"
        );
    }

    #[test]
    fn from_bytes_rejects_nan_boundary() {
        #[derive(serde::Serialize)]
        struct Wire {
            boundaries: Vec<f64>,
            counts: Vec<u64>,
            total: u64,
        }
        let bad = Wire {
            boundaries: vec![0.0, f64::NAN, 1.0],
            counts: vec![1, 1],
            total: 2,
        };
        let bytes = bincode::serialize(&bad).unwrap();
        assert!(
            EquiDepthHistogram::from_bytes(&bytes).is_err(),
            "NaN bin edge accepted"
        );
    }

    #[test]
    fn from_bytes_rejects_length_mismatch() {
        #[derive(serde::Serialize)]
        struct Wire {
            boundaries: Vec<f64>,
            counts: Vec<u64>,
            total: u64,
        }
        let bad = Wire {
            boundaries: vec![0.0, 1.0, 2.0],
            counts: vec![5], // counts.len()+1 != boundaries.len()
            total: 5,
        };
        let bytes = bincode::serialize(&bad).unwrap();
        assert!(
            EquiDepthHistogram::from_bytes(&bytes).is_err(),
            "boundaries/counts length mismatch accepted"
        );
    }

    #[test]
    fn from_bytes_accepts_valid_payload() {
        let values: Vec<f64> = (0..500).map(|i| (i as f64) * 0.3).collect();
        let h = EquiDepthHistogram::from_values(&values, 16).unwrap();
        let bytes = h.to_bytes().unwrap();
        let h2 = EquiDepthHistogram::from_bytes(&bytes).unwrap();
        assert_eq!(h.buckets(), h2.buckets());
        assert_eq!(h.total(), h2.total());
    }
}
