//! HyperLogLog cardinality sketch (basic; not HLL++).
//!
//! Sized by precision `p` ∈ [4, 18] → `2^p` 8-bit registers.
//! Relative error ≈ 1.04 / √(2^p) at large cardinalities.

use std::hash::Hasher;

use serde::{Deserialize, Serialize};
use twox_hash::XxHash64;

use crate::sketches::Sketch;
use crate::{Error, Result};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HllSketch {
    precision: u8,
    registers: Vec<u8>,
}

impl HllSketch {
    pub fn new(precision: u8) -> Result<Self> {
        if !(4..=18).contains(&precision) {
            return Err(Error::InvalidSketch(format!(
                "precision {precision} not in [4, 18]"
            )));
        }
        let m = 1usize << precision;
        Ok(Self {
            precision,
            registers: vec![0u8; m],
        })
    }

    fn hash(item: &[u8]) -> u64 {
        let mut h = XxHash64::with_seed(0);
        h.write(item);
        h.finish()
    }

    pub fn add(&mut self, item: &[u8]) {
        let h = Self::hash(item);
        let p = self.precision as u32;
        let idx = (h >> (64 - p)) as usize;
        // Shift out the bucket-index bits; remaining bits land in the top
        // (64-p) positions of `w`. Scan from the MSB to find the leftmost 1.
        let w = h << p;
        let rho = if w == 0 {
            // All remaining bits zero → sentinel rank = (64-p) + 1
            64 - p + 1
        } else {
            // leading_zeros() ∈ [0, 63-p] for non-zero w; rho ∈ [1, 64-p]
            w.leading_zeros() + 1
        };
        let rho_u8 = rho.min(255) as u8;
        if rho_u8 > self.registers[idx] {
            self.registers[idx] = rho_u8;
        }
    }

    pub fn estimate(&self) -> u64 {
        let m = self.registers.len() as f64;
        let alpha = match self.precision {
            4 => 0.673,
            5 => 0.697,
            6 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / m),
        };
        let sum: f64 = self.registers.iter().map(|&r| 2f64.powi(-(r as i32))).sum();
        let raw = alpha * m * m / sum;

        if raw <= 2.5 * m {
            let zeros = self.registers.iter().filter(|&&r| r == 0).count();
            if zeros > 0 {
                return (m * (m / zeros as f64).ln()) as u64;
            }
        }
        raw as u64
    }

    pub fn merge(&mut self, other: &Self) -> Result<()> {
        if self.precision != other.precision {
            return Err(Error::InvalidSketch(
                "HLL precision mismatch in merge".into(),
            ));
        }
        for (a, b) in self.registers.iter_mut().zip(other.registers.iter()) {
            if *b > *a {
                *a = *b;
            }
        }
        Ok(())
    }

    pub fn precision(&self) -> u8 {
        self.precision
    }
}

impl Sketch for HllSketch {
    const KIND: &'static str = "samkhya.hll-v1";

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
    fn counts_within_relative_error() {
        let mut hll = HllSketch::new(14).unwrap();
        for i in 0..10_000u32 {
            hll.add(&i.to_le_bytes());
        }
        let est = hll.estimate();
        let err = (est as f64 - 10_000.0).abs() / 10_000.0;
        assert!(err < 0.05, "estimate {est} off by {err}");
    }

    #[test]
    fn merge_disjoint_sets() {
        let mut a = HllSketch::new(12).unwrap();
        let mut b = HllSketch::new(12).unwrap();
        for i in 0..5_000u32 {
            a.add(&i.to_le_bytes());
        }
        for i in 5_000..10_000u32 {
            b.add(&i.to_le_bytes());
        }
        a.merge(&b).unwrap();
        let est = a.estimate();
        let err = (est as f64 - 10_000.0).abs() / 10_000.0;
        assert!(err < 0.08, "merged estimate {est} off by {err}");
    }

    #[test]
    fn round_trip() {
        let mut hll = HllSketch::new(12).unwrap();
        for i in 0..1000u32 {
            hll.add(&i.to_le_bytes());
        }
        let bytes = hll.to_bytes().unwrap();
        let hll2 = HllSketch::from_bytes(&bytes).unwrap();
        assert_eq!(hll.registers, hll2.registers);
        assert_eq!(hll.precision, hll2.precision);
    }

    #[test]
    fn precision_out_of_range_errors() {
        assert!(HllSketch::new(3).is_err());
        assert!(HllSketch::new(19).is_err());
    }
}
