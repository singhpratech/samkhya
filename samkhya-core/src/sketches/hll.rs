//! HyperLogLog cardinality sketch (basic; not HLL++).
//!
//! Sized by precision `p` ∈ [4, 18] → `2^p` 8-bit registers.
//! Relative error ≈ 1.04 / √(2^p) at large cardinalities.

use std::hash::Hasher;

use serde::{Deserialize, Serialize};
use twox_hash::XxHash64;

use crate::sketches::Sketch;
use crate::{Error, Result};

/// HyperLogLog cardinality sketch.
///
/// Precision `p ∈ [4, 18]` selects `2^p` 8-bit registers. Relative error is
/// approximately `1.04 / sqrt(2^p)` at large cardinalities.
///
/// # Examples
///
/// ```
/// use samkhya_core::sketches::HllSketch;
///
/// let mut hll = HllSketch::try_new(12).unwrap();
/// for i in 0..1_000u32 {
///     hll.add(&i.to_le_bytes());
/// }
/// let est = hll.estimate();
/// // 12-bit precision: standard error ≈ 1.6%; allow 10% as a safety margin.
/// assert!((est as i64 - 1000).unsigned_abs() < 100, "estimate was {est}");
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HllSketch {
    precision: u8,
    registers: Vec<u8>,
}

impl HllSketch {
    /// Fallible constructor. Validates `precision ∈ [4, 18]` before
    /// allocating the `2^precision` register vector. Preferred public
    /// entry point; `new` is retained for source compatibility and
    /// delegates here.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::HllSketch;
    ///
    /// let hll = HllSketch::try_new(10).unwrap();
    /// assert_eq!(hll.precision(), 10);
    /// // Out-of-range precision returns Err rather than panicking.
    /// assert!(HllSketch::try_new(3).is_err());
    /// assert!(HllSketch::try_new(19).is_err());
    /// ```
    pub fn try_new(precision: u8) -> Result<Self> {
        if !(4..=18).contains(&precision) {
            return Err(Error::InvalidSketch(format!(
                "HLL precision {precision} not in [4, 18]"
            )));
        }
        let m = 1usize << precision;
        Ok(Self {
            precision,
            registers: vec![0u8; m],
        })
    }

    /// Source-compatible alias for [`try_new`]. New code should call
    /// [`try_new`] directly; this entry point is preserved so that
    /// downstream crates (samkhya-cli, samkhya-py) keep compiling.
    pub fn new(precision: u8) -> Result<Self> {
        Self::try_new(precision)
    }

    /// Validate the structural invariants of a deserialised payload:
    /// precision in range and registers vector length matches
    /// `2^precision`. Used by [`Sketch::from_bytes`] to reject
    /// adversarial byte streams that bincode-decodes but violates the
    /// type contract (e.g. the 16-byte all-zero payload that decodes
    /// to `{precision: 0, registers: []}`).
    fn validate(&self) -> Result<()> {
        if !(4..=18).contains(&self.precision) {
            return Err(Error::InvalidSketch(format!(
                "HLL decoded precision {} not in [4, 18]",
                self.precision
            )));
        }
        let expected = 1usize << self.precision;
        if self.registers.len() != expected {
            return Err(Error::InvalidSketch(format!(
                "HLL register length {} != 2^precision = {}",
                self.registers.len(),
                expected
            )));
        }
        Ok(())
    }

    fn hash(item: &[u8]) -> u64 {
        let mut h = XxHash64::with_seed(0);
        h.write(item);
        h.finish()
    }

    /// Hash `item` and update the appropriate register.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::HllSketch;
    ///
    /// let mut hll = HllSketch::try_new(10).unwrap();
    /// hll.add(b"alice");
    /// hll.add(b"bob");
    /// hll.add(b"alice"); // duplicate — distinct count is still 2
    /// assert!(hll.estimate() >= 1);
    /// ```
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

    /// Return the cardinality estimate (count-distinct of inserted items).
    ///
    /// Uses linear counting in the low-cardinality regime and the bias-corrected
    /// harmonic-mean estimator otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::HllSketch;
    ///
    /// let mut hll = HllSketch::try_new(14).unwrap();
    /// for i in 0..5_000u32 {
    ///     hll.add(&i.to_le_bytes());
    /// }
    /// let est = hll.estimate();
    /// assert!((est as i64 - 5_000).unsigned_abs() < 250, "got {est}");
    /// ```
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

    /// Merge `other` into `self` by taking the per-register maximum. Both
    /// sketches must share the same `precision`.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::sketches::HllSketch;
    ///
    /// let mut a = HllSketch::try_new(12).unwrap();
    /// let mut b = HllSketch::try_new(12).unwrap();
    /// for i in 0..500u32 { a.add(&i.to_le_bytes()); }
    /// for i in 500..1000u32 { b.add(&i.to_le_bytes()); }
    /// a.merge(&b).unwrap();
    /// let est = a.estimate();
    /// assert!((est as i64 - 1000).unsigned_abs() < 100, "merged est was {est}");
    /// ```
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
        let s: Self = bincode::deserialize(bytes).map_err(Error::from)?;
        s.validate()?;
        Ok(s)
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

    #[test]
    fn try_new_rejects_each_invalid_precision() {
        for p in [0u8, 1, 2, 3, 19, 20, 64, 255] {
            assert!(
                HllSketch::try_new(p).is_err(),
                "try_new accepted invalid precision {p}"
            );
        }
    }

    #[test]
    fn try_new_accepts_valid_precisions() {
        for p in 4u8..=18 {
            let h = HllSketch::try_new(p).unwrap();
            assert_eq!(h.precision(), p);
            assert_eq!(h.registers.len(), 1usize << p);
        }
    }

    #[test]
    fn from_bytes_rejects_all_zero_payload() {
        // The historical H04 finding: a 16-byte all-zero payload
        // bincode-decodes to {precision: 0, registers: []} and
        // bypassed the [4,18] range check. Validation now rejects it.
        for n in [4usize, 8, 12, 16, 20, 32, 64] {
            let zeros = vec![0u8; n];
            let res = HllSketch::from_bytes(&zeros);
            assert!(res.is_err(), "all-zero len {n} accepted by from_bytes");
        }
    }

    #[test]
    fn from_bytes_rejects_register_length_mismatch() {
        // Hand-craft a payload with valid precision but wrong register
        // length. Bincode default layout: u8 + Vec<u8> (len as u64 LE
        // followed by bytes). precision = 6 → expected 64 registers,
        // we ship 32.
        let precision: u8 = 6;
        let mut payload = Vec::new();
        payload.push(precision);
        let bad_len: u64 = 32;
        payload.extend_from_slice(&bad_len.to_le_bytes());
        payload.extend_from_slice(&[1u8; 32]);
        let res = HllSketch::from_bytes(&payload);
        assert!(res.is_err(), "from_bytes accepted register-length mismatch");
    }

    #[test]
    fn from_bytes_accepts_valid_payload() {
        // Regression guard: the validation must not reject the happy path.
        let mut hll = HllSketch::try_new(10).unwrap();
        for i in 0..256u32 {
            hll.add(&i.to_le_bytes());
        }
        let bytes = hll.to_bytes().unwrap();
        let decoded = HllSketch::from_bytes(&bytes).unwrap();
        assert_eq!(hll.precision(), decoded.precision());
        assert_eq!(hll.registers, decoded.registers);
    }
}
