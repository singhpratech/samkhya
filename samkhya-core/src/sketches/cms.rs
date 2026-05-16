//! Count-Min Sketch — heavy-hitter / frequency estimation.
//!
//! Useful for detecting skewed values in join keys. Given depth `d` and
//! width `w`, the sketch uses `d × w` u32 counters. Frequency estimate
//! for an item is the minimum count across the `d` rows hashed to.
//!
//! Memory: `4 × d × w` bytes. With `d = 5` and `w = 1024` (defaults),
//! ~20KB per sketch. Relative error bounded by `2 * total_count / w`
//! with probability at least `1 - 0.5^d`.

use std::hash::Hasher;

use serde::{Deserialize, Serialize};
use twox_hash::XxHash64;

use crate::sketches::Sketch;
use crate::{Error, Result};

/// Count-Min Sketch — sub-linear-space frequency estimator.
///
/// Memory is `4 * depth * width` bytes. `estimate(item)` returns an upper
/// bound on the true frequency of `item`; it never undercounts.
///
/// # Examples
///
/// ```
/// use samkhya_core::sketches::CountMinSketch;
///
/// let mut cms = CountMinSketch::with_defaults();
/// cms.add(b"alice", 1);
/// cms.add(b"alice", 1);
/// cms.add(b"bob", 1);
/// assert!(cms.estimate(b"alice") >= 2);
/// assert!(cms.estimate(b"bob") >= 1);
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CountMinSketch {
    depth: u32,
    width: u32,
    counters: Vec<u32>, // length = depth * width, row-major
    total: u64,
}

impl CountMinSketch {
    /// Fallible constructor. Validates `depth > 0`, `width > 0`, and
    /// that `depth * width` fits in `usize` BEFORE allocating the
    /// counter array. Preferred public entry point.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::CountMinSketch;
    ///
    /// let cms = CountMinSketch::try_new(5, 1024).unwrap();
    /// assert_eq!(cms.depth(), 5);
    /// assert_eq!(cms.width(), 1024);
    /// assert!(CountMinSketch::try_new(0, 100).is_err());
    /// ```
    pub fn try_new(depth: u32, width: u32) -> Result<Self> {
        if depth == 0 || width == 0 {
            return Err(Error::InvalidSketch(
                "CMS depth and width must be > 0".into(),
            ));
        }
        let size = (depth as usize)
            .checked_mul(width as usize)
            .ok_or_else(|| Error::InvalidSketch("CMS depth*width overflows usize".into()))?;
        Ok(Self {
            depth,
            width,
            counters: vec![0u32; size],
            total: 0,
        })
    }

    /// Source-compatible alias for [`try_new`]. Preserved so that
    /// downstream call sites continue to compile unchanged.
    pub fn new(depth: u32, width: u32) -> Result<Self> {
        Self::try_new(depth, width)
    }

    /// Sensible defaults: depth 5, width 1024 → 20 KB per sketch.
    pub fn with_defaults() -> Self {
        Self::try_new(5, 1024).expect("defaults are valid")
    }

    /// Validate the structural invariants of a deserialised payload.
    fn validate(&self) -> Result<()> {
        if self.depth == 0 || self.width == 0 {
            return Err(Error::InvalidSketch(
                "CMS decoded depth/width must both be > 0".into(),
            ));
        }
        let expected = (self.depth as usize)
            .checked_mul(self.width as usize)
            .ok_or_else(|| Error::InvalidSketch("CMS decoded depth*width overflows".into()))?;
        if self.counters.len() != expected {
            return Err(Error::InvalidSketch(format!(
                "CMS counter length {} != depth*width = {}",
                self.counters.len(),
                expected
            )));
        }
        Ok(())
    }

    fn hash(item: &[u8], row: u32) -> u64 {
        // Seed-per-row to get d independent hash functions.
        let mut h = XxHash64::with_seed(0x1010_d017 ^ u64::from(row));
        h.write(item);
        h.finish()
    }

    /// Add `count` occurrences of `item` to the sketch. Saturates at
    /// `u32::MAX` per counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::CountMinSketch;
    ///
    /// let mut cms = CountMinSketch::with_defaults();
    /// cms.add(b"event-A", 5);
    /// cms.add(b"event-A", 3);
    /// assert!(cms.estimate(b"event-A") >= 8);
    /// assert_eq!(cms.total(), 8);
    /// ```
    pub fn add(&mut self, item: &[u8], count: u32) {
        for row in 0..self.depth {
            let idx = (Self::hash(item, row) % u64::from(self.width)) as usize;
            let pos = (row as usize) * (self.width as usize) + idx;
            self.counters[pos] = self.counters[pos].saturating_add(count);
        }
        self.total = self.total.saturating_add(u64::from(count));
    }

    /// Estimate the frequency of `item`. Always an upper bound under
    /// CMS semantics — never undercounts.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::CountMinSketch;
    ///
    /// let mut cms = CountMinSketch::with_defaults();
    /// for _ in 0..7 { cms.add(b"key", 1); }
    /// // CMS never undercounts: estimate is at least the true count.
    /// assert!(cms.estimate(b"key") >= 7);
    /// // An item never inserted estimates to 0 (with high probability).
    /// assert_eq!(cms.estimate(b"never-seen"), 0);
    /// ```
    pub fn estimate(&self, item: &[u8]) -> u32 {
        (0..self.depth)
            .map(|row| {
                let idx = (Self::hash(item, row) % u64::from(self.width)) as usize;
                let pos = (row as usize) * (self.width as usize) + idx;
                self.counters[pos]
            })
            .min()
            .unwrap_or(0)
    }

    pub fn merge(&mut self, other: &Self) -> Result<()> {
        if self.depth != other.depth || self.width != other.width {
            return Err(Error::InvalidSketch(
                "CMS depth/width mismatch in merge".into(),
            ));
        }
        for (a, b) in self.counters.iter_mut().zip(other.counters.iter()) {
            *a = a.saturating_add(*b);
        }
        self.total = self.total.saturating_add(other.total);
        Ok(())
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

impl Sketch for CountMinSketch {
    const KIND: &'static str = "samkhya.cms-v1";

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
    fn never_undercounts() {
        let mut cms = CountMinSketch::new(5, 1024).unwrap();
        for i in 0..1000u32 {
            for _ in 0..5 {
                cms.add(&i.to_le_bytes(), 1);
            }
        }
        for i in 0..1000u32 {
            assert!(
                cms.estimate(&i.to_le_bytes()) >= 5,
                "undercount for {i}: {}",
                cms.estimate(&i.to_le_bytes())
            );
        }
    }

    #[test]
    fn heavy_hitter_detected() {
        let mut cms = CountMinSketch::with_defaults();
        // 1000 light items at count 1, 1 heavy item at count 10_000
        for i in 0..1000u32 {
            cms.add(&i.to_le_bytes(), 1);
        }
        cms.add(b"heavy", 10_000);
        let heavy_est = cms.estimate(b"heavy");
        let light_est = cms.estimate(&42u32.to_le_bytes());
        assert!(
            (10_000..11_000).contains(&heavy_est),
            "heavy est {heavy_est} out of range"
        );
        assert!(light_est < 50, "light est {light_est} too high");
    }

    #[test]
    fn merge_adds_counts() {
        let mut a = CountMinSketch::new(3, 100).unwrap();
        let mut b = CountMinSketch::new(3, 100).unwrap();
        a.add(b"x", 5);
        b.add(b"x", 3);
        a.merge(&b).unwrap();
        assert!(a.estimate(b"x") >= 8);
    }

    #[test]
    fn merge_mismatched_dimensions_errors() {
        let mut a = CountMinSketch::new(3, 100).unwrap();
        let b = CountMinSketch::new(4, 100).unwrap();
        assert!(a.merge(&b).is_err());
    }

    #[test]
    fn round_trip() {
        let mut cms = CountMinSketch::with_defaults();
        for i in 0..100u32 {
            cms.add(&i.to_le_bytes(), 1);
        }
        let bytes = cms.to_bytes().unwrap();
        let cms2 = CountMinSketch::from_bytes(&bytes).unwrap();
        for i in 0..100u32 {
            assert_eq!(
                cms.estimate(&i.to_le_bytes()),
                cms2.estimate(&i.to_le_bytes())
            );
        }
        assert_eq!(cms.total, cms2.total);
    }

    #[test]
    fn invalid_dimensions_error() {
        assert!(CountMinSketch::new(0, 100).is_err());
        assert!(CountMinSketch::new(5, 0).is_err());
    }

    #[test]
    fn try_new_rejects_each_invalid_dimension() {
        assert!(CountMinSketch::try_new(0, 0).is_err());
        assert!(CountMinSketch::try_new(0, 100).is_err());
        assert!(CountMinSketch::try_new(5, 0).is_err());
    }

    #[test]
    fn try_new_accepts_valid_dimensions() {
        let cms = CountMinSketch::try_new(4, 256).unwrap();
        assert_eq!(cms.depth(), 4);
        assert_eq!(cms.width(), 256);
    }

    #[test]
    fn from_bytes_rejects_all_zero_payload() {
        // Bincode-decodes {depth:0, width:0, counters:[], total:0},
        // which fails the validate() depth/width > 0 check.
        for n in [4usize, 8, 16, 24, 32, 64, 128, 256] {
            let zeros = vec![0u8; n];
            assert!(
                CountMinSketch::from_bytes(&zeros).is_err(),
                "all-zero len {n} accepted by from_bytes"
            );
        }
    }

    #[test]
    fn from_bytes_rejects_counter_length_mismatch() {
        // Craft a payload with valid dims (depth=2,width=4) but a
        // 3-element counter vector instead of 8. Bincode layout for
        // this struct: u32 + u32 + Vec<u32>(len u64 + data) + u64.
        let depth: u32 = 2;
        let width: u32 = 4;
        let mut payload = Vec::new();
        payload.extend_from_slice(&depth.to_le_bytes());
        payload.extend_from_slice(&width.to_le_bytes());
        let bad_len: u64 = 3;
        payload.extend_from_slice(&bad_len.to_le_bytes());
        for _ in 0..3 {
            payload.extend_from_slice(&0u32.to_le_bytes());
        }
        payload.extend_from_slice(&0u64.to_le_bytes());
        assert!(
            CountMinSketch::from_bytes(&payload).is_err(),
            "from_bytes accepted counter-length mismatch"
        );
    }

    #[test]
    fn from_bytes_accepts_valid_payload() {
        let cms = CountMinSketch::with_defaults();
        let bytes = cms.to_bytes().unwrap();
        let decoded = CountMinSketch::from_bytes(&bytes).unwrap();
        assert_eq!(cms.depth(), decoded.depth());
        assert_eq!(cms.width(), decoded.width());
    }
}
