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
    /// Build a histogram from a slice of sorted (or unsorted; we sort it
    /// here) `f64` values, partitioned into `buckets` equi-depth bins.
    pub fn from_values(values: &[f64], buckets: usize) -> Result<Self> {
        if buckets == 0 {
            return Err(Error::InvalidSketch("buckets must be > 0".into()));
        }
        if values.is_empty() {
            return Ok(Self {
                boundaries: vec![0.0, 0.0],
                counts: vec![0],
                total: 0,
            });
        }
        let mut sorted: Vec<f64> = values.iter().copied().collect();
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
}

impl Sketch for EquiDepthHistogram {
    const KIND: &'static str = "samkhya.histogram-equidepth-v1";

    fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(Into::into)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(Into::into)
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
}
