//! Bloom filter using Kirsch-Mitzenmacher double-hashing.

use std::hash::Hasher;

use serde::{Deserialize, Serialize};
use twox_hash::XxHash64;

use crate::Result;
use crate::sketches::Sketch;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BloomFilter {
    bits: Vec<u8>,
    num_hashes: u32,
    num_bits: u64,
}

impl BloomFilter {
    /// Build a filter sized for `capacity` items at the given false-positive rate.
    pub fn new(capacity: usize, fp_rate: f64) -> Self {
        let capacity = capacity.max(1) as f64;
        let num_bits = ((-1.44 * capacity * fp_rate.ln()).ceil() as u64).max(64);
        let num_hashes = ((num_bits as f64 / capacity) * std::f64::consts::LN_2)
            .ceil()
            .max(1.0) as u32;
        Self {
            bits: vec![0u8; num_bits.div_ceil(8) as usize],
            num_hashes,
            num_bits,
        }
    }

    fn double_hash(item: &[u8]) -> (u64, u64) {
        let mut h = XxHash64::with_seed(0xc0ffee);
        h.write(item);
        let h1 = h.finish();
        let mut h = XxHash64::with_seed(0xbeef);
        h.write(item);
        let h2 = h.finish();
        (h1, h2)
    }

    fn bit_index(h1: u64, h2: u64, i: u32, m: u64) -> u64 {
        h1.wrapping_add((i as u64).wrapping_mul(h2)) % m
    }

    pub fn insert(&mut self, item: &[u8]) {
        let (h1, h2) = Self::double_hash(item);
        for i in 0..self.num_hashes {
            let idx = Self::bit_index(h1, h2, i, self.num_bits);
            self.bits[(idx / 8) as usize] |= 1u8 << (idx % 8);
        }
    }

    pub fn contains(&self, item: &[u8]) -> bool {
        let (h1, h2) = Self::double_hash(item);
        for i in 0..self.num_hashes {
            let idx = Self::bit_index(h1, h2, i, self.num_bits);
            if self.bits[(idx / 8) as usize] & (1u8 << (idx % 8)) == 0 {
                return false;
            }
        }
        true
    }

    pub fn num_bits(&self) -> u64 {
        self.num_bits
    }

    pub fn num_hashes(&self) -> u32 {
        self.num_hashes
    }
}

impl Sketch for BloomFilter {
    const KIND: &'static str = "samkhya.bloom-v1";

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
    fn no_false_negatives() {
        let mut bf = BloomFilter::new(1000, 0.01);
        for i in 0..1000u32 {
            bf.insert(&i.to_le_bytes());
        }
        for i in 0..1000u32 {
            assert!(bf.contains(&i.to_le_bytes()), "fn for {i}");
        }
    }

    #[test]
    fn fp_rate_close_to_target() {
        let mut bf = BloomFilter::new(10_000, 0.01);
        for i in 0..10_000u32 {
            bf.insert(&i.to_le_bytes());
        }
        let mut fps = 0u32;
        for i in 10_000u32..20_000 {
            if bf.contains(&i.to_le_bytes()) {
                fps += 1;
            }
        }
        let rate = fps as f64 / 10_000.0;
        assert!(rate < 0.05, "fp rate {rate} too high for target 0.01");
    }

    #[test]
    fn round_trip() {
        let mut bf = BloomFilter::new(100, 0.01);
        for i in 0..100u32 {
            bf.insert(&i.to_le_bytes());
        }
        let bytes = bf.to_bytes().unwrap();
        let bf2 = BloomFilter::from_bytes(&bytes).unwrap();
        for i in 0..100u32 {
            assert!(bf2.contains(&i.to_le_bytes()));
        }
        assert_eq!(bf.num_bits, bf2.num_bits);
        assert_eq!(bf.num_hashes, bf2.num_hashes);
    }
}
