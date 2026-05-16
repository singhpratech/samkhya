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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CountMinSketch {
    depth: u32,
    width: u32,
    counters: Vec<u32>, // length = depth * width, row-major
    total: u64,
}

impl CountMinSketch {
    pub fn new(depth: u32, width: u32) -> Result<Self> {
        if depth == 0 || width == 0 {
            return Err(Error::InvalidSketch(
                "CMS depth and width must be > 0".into(),
            ));
        }
        let size = (depth as usize)
            .checked_mul(width as usize)
            .ok_or_else(|| Error::InvalidSketch("CMS size overflow".into()))?;
        Ok(Self {
            depth,
            width,
            counters: vec![0u32; size],
            total: 0,
        })
    }

    /// Sensible defaults: depth 5, width 1024 → 20 KB per sketch.
    pub fn with_defaults() -> Self {
        Self::new(5, 1024).expect("defaults are valid")
    }

    fn hash(item: &[u8], row: u32) -> u64 {
        // Seed-per-row to get d independent hash functions.
        let mut h = XxHash64::with_seed(0x1010_d017 ^ u64::from(row));
        h.write(item);
        h.finish()
    }

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
        bincode::deserialize(bytes).map_err(Into::into)
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
            heavy_est >= 10_000 && heavy_est < 11_000,
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
}
