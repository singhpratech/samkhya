//! Bloom filter using Kirsch-Mitzenmacher double-hashing.

use std::hash::Hasher;

use serde::{Deserialize, Serialize};
use twox_hash::XxHash64;

use crate::sketches::Sketch;
use crate::{Error, Result};

/// Probabilistic set-membership filter using Kirsch-Mitzenmacher
/// double-hashing on top of [`twox_hash::XxHash64`].
///
/// Inserts and lookups are O(`num_hashes`). False positives occur at the
/// configured target rate; false negatives are impossible.
///
/// # Examples
///
/// ```
/// use samkhya_core::sketches::BloomFilter;
///
/// let mut bf = BloomFilter::new(1000, 0.01);
/// bf.insert(b"alice");
/// assert!(bf.contains(b"alice"));
/// // An item that was never inserted is (with high probability) not present.
/// assert!(!bf.contains(b"unseen-key-xyz"));
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BloomFilter {
    bits: Vec<u8>,
    num_hashes: u32,
    num_bits: u64,
}

/// Cap on filter size in bits. 2^33 bits = 1 GiB allocation; well above
/// any realistic single-column sketch but small enough that an attacker-
/// supplied `fp_rate ≈ 0` cannot drive a multi-EiB allocation that the
/// allocator would abort on. [`try_new`](BloomFilter::try_new) refuses
/// any sizing that exceeds this cap.
const MAX_NUM_BITS: u64 = 1u64 << 33;

impl BloomFilter {
    /// Fallible constructor. Validates `capacity > 0` and
    /// `fp_rate ∈ (0, 1)` BEFORE computing the bit-array size, then
    /// also refuses sizings that exceed [`MAX_NUM_BITS`] (a guard
    /// against pathological `fp_rate` values that would otherwise drive
    /// the allocator into a multi-EiB request and SIGABRT the process).
    ///
    /// This is the preferred public constructor. The infallible
    /// [`new`](Self::new) is retained for source compatibility with
    /// existing call sites that have already pre-validated their
    /// arguments; it clamps rather than erroring.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::BloomFilter;
    ///
    /// let bf = BloomFilter::try_new(10_000, 0.01).unwrap();
    /// assert!(bf.num_bits() >= 64);
    /// assert!(bf.num_hashes() >= 1);
    /// // Invalid configurations error rather than panicking.
    /// assert!(BloomFilter::try_new(0, 0.01).is_err());
    /// assert!(BloomFilter::try_new(100, 0.0).is_err());
    /// assert!(BloomFilter::try_new(100, 1.5).is_err());
    /// ```
    pub fn try_new(capacity: usize, fp_rate: f64) -> Result<Self> {
        if capacity == 0 {
            return Err(Error::InvalidSketch("Bloom capacity must be > 0".into()));
        }
        if !fp_rate.is_finite() || fp_rate <= 0.0 || fp_rate >= 1.0 {
            return Err(Error::InvalidSketch(format!(
                "Bloom fp_rate must be a finite value in (0, 1), got {fp_rate}"
            )));
        }
        let cap_f = capacity as f64;
        let ln2_sq = std::f64::consts::LN_2 * std::f64::consts::LN_2;
        let num_bits_f = (-cap_f * fp_rate.ln() / ln2_sq).ceil();
        if !num_bits_f.is_finite() || num_bits_f < 0.0 {
            return Err(Error::InvalidSketch(format!(
                "Bloom sizing produced a non-finite num_bits ({num_bits_f}) \
                 for capacity={capacity}, fp_rate={fp_rate}"
            )));
        }
        let num_bits = (num_bits_f as u64).max(64);
        if num_bits > MAX_NUM_BITS {
            return Err(Error::InvalidSketch(format!(
                "Bloom sizing {num_bits} bits exceeds cap {MAX_NUM_BITS} \
                 (fp_rate={fp_rate} too aggressive for capacity={capacity})"
            )));
        }
        let num_hashes = ((num_bits as f64 / cap_f) * std::f64::consts::LN_2)
            .ceil()
            .max(1.0) as u32;
        let byte_len = num_bits.div_ceil(8) as usize;
        Ok(Self {
            bits: vec![0u8; byte_len],
            num_hashes,
            num_bits,
        })
    }

    /// Build a filter sized for `capacity` items at the given false-positive rate.
    ///
    /// Infallible, lossy on bad input: out-of-range parameters are
    /// clamped to a safe envelope (`capacity ≥ 1`, `fp_rate ∈
    /// [1e-12, 0.5]`). Prefer [`try_new`](Self::try_new), which
    /// returns a structured [`Error`] instead of clamping; this
    /// alias exists so that pre-existing call sites do not break.
    pub fn new(capacity: usize, fp_rate: f64) -> Self {
        // m = -n * ln(p) / (ln 2)^2  is the classical Bloom-filter sizing.
        // An earlier version used 1.44 (the *k*-formula constant 1/ln 2) where
        // 2.0814 ≈ 1/(ln 2)^2 (the *m*-formula constant) is required; B04 of
        // the empirical campaign caught the resulting ~30.8% under-allocation.
        let capacity = capacity.max(1);
        let fp = if !fp_rate.is_finite() || fp_rate <= 0.0 {
            1e-12
        } else if fp_rate >= 1.0 {
            0.5
        } else {
            fp_rate
        };
        // try_new always succeeds for the clamped envelope above.
        Self::try_new(capacity, fp).unwrap_or_else(|_| {
            // Belt-and-braces fallback: 64-bit, single hash, empty filter.
            Self {
                bits: vec![0u8; 8],
                num_hashes: 1,
                num_bits: 64,
            }
        })
    }

    /// Validate the structural invariants of a deserialised payload:
    /// `num_bits > 0`, `num_hashes > 0`, byte vector matches
    /// `ceil(num_bits / 8)`, and `num_bits ≤ MAX_NUM_BITS`. Used by
    /// [`Sketch::from_bytes`] to reject adversarial byte streams that
    /// bincode-decode but violate the type contract.
    fn validate(&self) -> Result<()> {
        if self.num_bits == 0 {
            return Err(Error::InvalidSketch("Bloom num_bits must be > 0".into()));
        }
        if self.num_hashes == 0 {
            return Err(Error::InvalidSketch("Bloom num_hashes must be > 0".into()));
        }
        if self.num_bits > MAX_NUM_BITS {
            return Err(Error::InvalidSketch(format!(
                "Bloom num_bits {} exceeds cap {}",
                self.num_bits, MAX_NUM_BITS
            )));
        }
        let expected = self.num_bits.div_ceil(8) as usize;
        if self.bits.len() != expected {
            return Err(Error::InvalidSketch(format!(
                "Bloom bits length {} != ceil(num_bits/8) = {}",
                self.bits.len(),
                expected
            )));
        }
        Ok(())
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

    /// Set every bit that `item` hashes to. After this returns,
    /// `self.contains(item)` is guaranteed to be `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::BloomFilter;
    ///
    /// let mut bf = BloomFilter::new(100, 0.01);
    /// bf.insert(b"x");
    /// assert!(bf.contains(b"x"));
    /// ```
    pub fn insert(&mut self, item: &[u8]) {
        let (h1, h2) = Self::double_hash(item);
        for i in 0..self.num_hashes {
            let idx = Self::bit_index(h1, h2, i, self.num_bits);
            self.bits[(idx / 8) as usize] |= 1u8 << (idx % 8);
        }
    }

    /// Test whether `item` is *probably* in the filter. False positives are
    /// possible (at the configured rate); false negatives are not.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::BloomFilter;
    ///
    /// let mut bf = BloomFilter::new(100, 0.01);
    /// bf.insert(b"hello");
    /// assert!(bf.contains(b"hello"));        // present → always true
    /// // Absent items return false with high probability.
    /// assert!(!bf.contains(b"definitely-not-here"));
    /// ```
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
        let s: Self = bincode::deserialize(bytes).map_err(Error::from)?;
        s.validate()?;
        Ok(s)
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
        // Tight envelope: empirical FPR must land within 1.5× of configured
        // target (B04 acceptance: ≤ 1.1× at n ≥ 10^4, 1.5× here as a single-
        // shot unit-test allowance for finite-sample variance at n=10k).
        assert!(rate < 0.015, "fp rate {rate} too high for target 0.01");
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

    #[test]
    fn try_new_rejects_zero_capacity() {
        assert!(BloomFilter::try_new(0, 0.01).is_err());
    }

    #[test]
    fn try_new_rejects_out_of_range_fp_rate() {
        for fp in [
            -1.0,
            0.0,
            1.0,
            1.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(
                BloomFilter::try_new(1000, fp).is_err(),
                "try_new accepted fp_rate {fp}"
            );
        }
    }

    #[test]
    fn try_new_rejects_oversize_sizing() {
        // fp_rate ≈ 1e-100 with capacity 10^9 would request ~5e11 bits =
        // ~60 GB, well over the 1 GiB cap. Must Err rather than allocate.
        assert!(BloomFilter::try_new(1_000_000_000, 1e-100).is_err());
    }

    #[test]
    fn try_new_accepts_valid_arguments() {
        let bf = BloomFilter::try_new(1000, 0.01).unwrap();
        assert!(bf.num_bits() > 0);
        assert!(bf.num_hashes() > 0);
    }

    #[test]
    fn new_clamps_pathological_inputs_no_panic() {
        // Historical fortress finding: BloomFilter::new(_, 0.0) used to
        // request 2 EiB and SIGABRT. The infallible alias now clamps.
        let bf = BloomFilter::new(1000, 0.0);
        assert!(bf.num_bits() > 0);
        let bf2 = BloomFilter::new(0, f64::NAN);
        assert!(bf2.num_bits() > 0);
        let bf3 = BloomFilter::new(1000, 2.0);
        assert!(bf3.num_bits() > 0);
    }

    #[test]
    fn from_bytes_rejects_all_zero_payload() {
        // Bincode-decodes an empty struct; validate catches num_bits==0.
        for n in [4usize, 8, 16, 24, 32, 64, 128] {
            let zeros = vec![0u8; n];
            assert!(
                BloomFilter::from_bytes(&zeros).is_err(),
                "all-zero len {n} accepted by from_bytes"
            );
        }
    }

    #[test]
    fn from_bytes_accepts_valid_payload() {
        let bf = BloomFilter::try_new(256, 0.01).unwrap();
        let bytes = bf.to_bytes().unwrap();
        let decoded = BloomFilter::from_bytes(&bytes).unwrap();
        assert_eq!(bf.num_bits, decoded.num_bits);
        assert_eq!(bf.num_hashes, decoded.num_hashes);
    }
}
