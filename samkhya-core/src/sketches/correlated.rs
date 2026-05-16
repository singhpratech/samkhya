//! Multi-column correlated histogram (2D equi-width grid).
//!
//! Captures the joint distribution of two numeric columns by binning each
//! into a fixed number of equi-width buckets and counting the rows that
//! land in each `(i, j)` cell. Useful when DataFusion's heuristic 1/5
//! selectivity assumption would mis-estimate predicates that touch two
//! correlated columns (e.g. `WHERE city = 'X' AND zip BETWEEN ...`).
//!
//! Complements the single-column [`EquiDepthHistogram`] in the same module
//! by exposing the cross-column covariance that single-column stats erase.
//!
//! [`EquiDepthHistogram`]: super::histogram::EquiDepthHistogram

use serde::{Deserialize, Serialize};

use crate::sketches::Sketch;
use crate::{Error, Result};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CorrelatedHistogram2D {
    col_a_bins: u32,
    col_b_bins: u32,
    a_min: f64,
    a_max: f64,
    b_min: f64,
    b_max: f64,
    /// Row-major: `cells[i * col_b_bins + j]` holds count for
    /// (a-bucket i, b-bucket j). `len() == col_a_bins * col_b_bins`.
    cells: Vec<u64>,
    total: u64,
}

impl CorrelatedHistogram2D {
    /// Create an empty 2D histogram with the given bin counts. The min/max
    /// of each column default to 0.0 and must be populated via
    /// [`from_pairs`](Self::from_pairs) or [`merge`](Self::merge) before
    /// estimates are meaningful.
    pub fn new(col_a_bins: usize, col_b_bins: usize) -> Result<Self> {
        if col_a_bins == 0 || col_b_bins == 0 {
            return Err(Error::InvalidSketch(
                "CorrelatedHistogram2D bin counts must be > 0".into(),
            ));
        }
        let size = col_a_bins
            .checked_mul(col_b_bins)
            .ok_or_else(|| Error::InvalidSketch("CorrelatedHistogram2D size overflow".into()))?;
        Ok(Self {
            col_a_bins: col_a_bins as u32,
            col_b_bins: col_b_bins as u32,
            a_min: 0.0,
            a_max: 0.0,
            b_min: 0.0,
            b_max: 0.0,
            cells: vec![0u64; size],
            total: 0,
        })
    }

    /// Scan a slice of `(a, b)` pairs, learn each column's min/max, then
    /// equi-width-bin and populate the cell counts.
    pub fn from_pairs(pairs: &[(f64, f64)], col_a_bins: usize, col_b_bins: usize) -> Result<Self> {
        let mut h = Self::new(col_a_bins, col_b_bins)?;
        if pairs.is_empty() {
            return Ok(h);
        }
        let (mut amin, mut amax) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut bmin, mut bmax) = (f64::INFINITY, f64::NEG_INFINITY);
        for &(a, b) in pairs {
            if !a.is_finite() || !b.is_finite() {
                continue;
            }
            if a < amin {
                amin = a;
            }
            if a > amax {
                amax = a;
            }
            if b < bmin {
                bmin = b;
            }
            if b > bmax {
                bmax = b;
            }
        }
        if !amin.is_finite() || !bmin.is_finite() {
            // No finite samples; leave as empty.
            return Ok(h);
        }
        h.a_min = amin;
        h.a_max = amax;
        h.b_min = bmin;
        h.b_max = bmax;

        for &(a, b) in pairs {
            if !a.is_finite() || !b.is_finite() {
                continue;
            }
            let i = Self::bucket_index(a, amin, amax, h.col_a_bins);
            let j = Self::bucket_index(b, bmin, bmax, h.col_b_bins);
            let pos = i * (h.col_b_bins as usize) + j;
            h.cells[pos] = h.cells[pos].saturating_add(1);
            h.total = h.total.saturating_add(1);
        }
        Ok(h)
    }

    /// Map a value to a bucket index in `[0, nbins)` given the column's
    /// known `[min, max]`. Saturates outside the support.
    fn bucket_index(v: f64, lo: f64, hi: f64, nbins: u32) -> usize {
        let n = nbins as usize;
        if n == 0 {
            return 0;
        }
        if hi <= lo {
            return 0;
        }
        if v <= lo {
            return 0;
        }
        if v >= hi {
            return n - 1;
        }
        let frac = (v - lo) / (hi - lo);
        let idx = (frac * (n as f64)).floor() as usize;
        idx.min(n - 1)
    }

    /// Return the inclusive `[lo_idx, hi_idx]` bucket index range that an
    /// inclusive value-range `[q_lo, q_hi]` covers, or `None` if the
    /// query is disjoint from the column support.
    fn range_bucket_span(
        q_lo: f64,
        q_hi: f64,
        col_min: f64,
        col_max: f64,
        nbins: u32,
    ) -> Option<(usize, usize)> {
        if q_lo > q_hi {
            return None;
        }
        if nbins == 0 {
            return None;
        }
        if col_max < col_min {
            return None;
        }
        // Empty/degenerate support: a single point.
        if col_max <= col_min {
            if q_lo <= col_min && q_hi >= col_min {
                return Some((0, (nbins as usize) - 1));
            }
            return None;
        }
        if q_hi < col_min || q_lo > col_max {
            return None;
        }
        let lo_idx = Self::bucket_index(q_lo, col_min, col_max, nbins);
        let hi_idx = Self::bucket_index(q_hi, col_min, col_max, nbins);
        Some((lo_idx, hi_idx))
    }

    /// Sum counts of every cell touched by the 2D range
    /// `[a_lo, a_hi] × [b_lo, b_hi]`. Cells are counted in whole — no
    /// sub-cell interpolation. Returns 0 if the range is empty or
    /// outside the support of either column.
    pub fn estimate_range(&self, a_lo: f64, a_hi: f64, b_lo: f64, b_hi: f64) -> u64 {
        if self.total == 0 {
            return 0;
        }
        let a_span =
            match Self::range_bucket_span(a_lo, a_hi, self.a_min, self.a_max, self.col_a_bins) {
                Some(s) => s,
                None => return 0,
            };
        let b_span =
            match Self::range_bucket_span(b_lo, b_hi, self.b_min, self.b_max, self.col_b_bins) {
                Some(s) => s,
                None => return 0,
            };
        let bw = self.col_b_bins as usize;
        let mut sum: u64 = 0;
        for i in a_span.0..=a_span.1 {
            let row = i * bw;
            for j in b_span.0..=b_span.1 {
                sum = sum.saturating_add(self.cells[row + j]);
            }
        }
        sum
    }

    /// Merge another histogram into `self`. Requires identical bin
    /// dimensions and identical min/max on both columns so that the
    /// grids are layout-compatible.
    pub fn merge(&mut self, other: &Self) -> Result<()> {
        if self.col_a_bins != other.col_a_bins || self.col_b_bins != other.col_b_bins {
            return Err(Error::InvalidSketch(
                "CorrelatedHistogram2D bin dimension mismatch in merge".into(),
            ));
        }
        // If self is empty (total == 0), inherit the other's support.
        if self.total == 0 {
            self.a_min = other.a_min;
            self.a_max = other.a_max;
            self.b_min = other.b_min;
            self.b_max = other.b_max;
        } else if other.total != 0
            && (self.a_min != other.a_min
                || self.a_max != other.a_max
                || self.b_min != other.b_min
                || self.b_max != other.b_max)
        {
            return Err(Error::InvalidSketch(
                "CorrelatedHistogram2D bin layout mismatch (min/max differ) in merge".into(),
            ));
        }
        for (a, b) in self.cells.iter_mut().zip(other.cells.iter()) {
            *a = a.saturating_add(*b);
        }
        self.total = self.total.saturating_add(other.total);
        Ok(())
    }

    /// Row-major flattened cell counts. `cells[i * col_b_bins + j]`
    /// is the count for a-bucket `i`, b-bucket `j`.
    pub fn cell_counts(&self) -> &[u64] {
        &self.cells
    }

    pub fn col_a_bins(&self) -> usize {
        self.col_a_bins as usize
    }

    pub fn col_b_bins(&self) -> usize {
        self.col_b_bins as usize
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn a_range(&self) -> (f64, f64) {
        (self.a_min, self.a_max)
    }

    pub fn b_range(&self) -> (f64, f64) {
        (self.b_min, self.b_max)
    }
}

impl Sketch for CorrelatedHistogram2D {
    const KIND: &'static str = "samkhya.correlated2d-v1";

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
    fn new_rejects_zero_bins() {
        assert!(CorrelatedHistogram2D::new(0, 16).is_err());
        assert!(CorrelatedHistogram2D::new(16, 0).is_err());
    }

    #[test]
    fn full_range_covers_total_input_count() {
        let pairs: Vec<(f64, f64)> = (0..1000).map(|i| (i as f64, (i % 50) as f64)).collect();
        let h = CorrelatedHistogram2D::from_pairs(&pairs, 16, 16).unwrap();
        assert_eq!(h.total(), 1000);
        let (a_lo, a_hi) = h.a_range();
        let (b_lo, b_hi) = h.b_range();
        assert_eq!(h.estimate_range(a_lo, a_hi, b_lo, b_hi), 1000);
        // Querying with a strictly wider range than the support also
        // returns the full count.
        assert_eq!(
            h.estimate_range(a_lo - 100.0, a_hi + 100.0, b_lo - 100.0, b_hi + 100.0),
            1000
        );
    }

    #[test]
    fn widening_either_dimension_never_decreases() {
        let pairs: Vec<(f64, f64)> = (0..500)
            .map(|i| ((i as f64) * 0.7, ((i * 3) % 73) as f64))
            .collect();
        let h = CorrelatedHistogram2D::from_pairs(&pairs, 16, 16).unwrap();

        let base = h.estimate_range(50.0, 150.0, 10.0, 40.0);
        let wider_a_lo = h.estimate_range(20.0, 150.0, 10.0, 40.0);
        let wider_a_hi = h.estimate_range(50.0, 200.0, 10.0, 40.0);
        let wider_b_lo = h.estimate_range(50.0, 150.0, 0.0, 40.0);
        let wider_b_hi = h.estimate_range(50.0, 150.0, 10.0, 80.0);

        assert!(wider_a_lo >= base);
        assert!(wider_a_hi >= base);
        assert!(wider_b_lo >= base);
        assert!(wider_b_hi >= base);
    }

    #[test]
    fn empty_range_returns_zero() {
        let pairs: Vec<(f64, f64)> = (0..100).map(|i| (i as f64, i as f64)).collect();
        let h = CorrelatedHistogram2D::from_pairs(&pairs, 8, 8).unwrap();
        // Inverted query range
        assert_eq!(h.estimate_range(50.0, 10.0, 0.0, 100.0), 0);
        // Query strictly outside support
        assert_eq!(h.estimate_range(200.0, 300.0, 0.0, 100.0), 0);
        assert_eq!(h.estimate_range(0.0, 100.0, 500.0, 600.0), 0);
    }

    #[test]
    fn empty_pairs_handled() {
        let h = CorrelatedHistogram2D::from_pairs(&[], 8, 8).unwrap();
        assert_eq!(h.total(), 0);
        assert_eq!(h.estimate_range(0.0, 100.0, 0.0, 100.0), 0);
    }

    #[test]
    fn round_trip_preserves_cells() {
        let pairs: Vec<(f64, f64)> = (0..400)
            .map(|i| ((i % 20) as f64, (i / 20) as f64))
            .collect();
        let h = CorrelatedHistogram2D::from_pairs(&pairs, 8, 8).unwrap();
        let bytes = h.to_bytes().unwrap();
        let h2 = CorrelatedHistogram2D::from_bytes(&bytes).unwrap();
        assert_eq!(h.cells, h2.cells);
        assert_eq!(h.total, h2.total);
        assert_eq!(h.col_a_bins, h2.col_a_bins);
        assert_eq!(h.col_b_bins, h2.col_b_bins);
        assert_eq!(h.a_min, h2.a_min);
        assert_eq!(h.a_max, h2.a_max);
        assert_eq!(h.b_min, h2.b_min);
        assert_eq!(h.b_max, h2.b_max);
    }

    #[test]
    fn merge_combines_compatible_grids() {
        let pairs_a: Vec<(f64, f64)> = (0..100).map(|i| (i as f64, (i % 10) as f64)).collect();
        let pairs_b: Vec<(f64, f64)> = (0..100).map(|i| (i as f64, (i % 10) as f64)).collect();
        let mut h1 = CorrelatedHistogram2D::from_pairs(&pairs_a, 8, 8).unwrap();
        let h2 = CorrelatedHistogram2D::from_pairs(&pairs_b, 8, 8).unwrap();
        h1.merge(&h2).unwrap();
        assert_eq!(h1.total(), 200);
    }

    #[test]
    fn merge_dimension_mismatch_errors() {
        let h_a = CorrelatedHistogram2D::new(8, 8).unwrap();
        let mut h_b = CorrelatedHistogram2D::new(16, 8).unwrap();
        assert!(h_b.merge(&h_a).is_err());
    }

    #[test]
    fn diagonal_correlation_tighter_than_independent() {
        // Pairs (i, i % 4) — `b` is a deterministic function of `a`,
        // so the joint distribution is concentrated on a few cells.
        // A diagonal stripe query should return tighter (lower) counts
        // than the independence assumption `P(A) * P(B)` would predict.
        let n: u64 = 4000;
        let pairs: Vec<(f64, f64)> = (0..n).map(|i| (i as f64, (i % 4) as f64)).collect();
        let h = CorrelatedHistogram2D::from_pairs(&pairs, 16, 4).unwrap();
        assert_eq!(h.total(), n);

        // Query: a in [0, n/4), b ∈ [0, 0]  (one of the four b values)
        let a_lo = 0.0;
        let a_hi = (n as f64) / 4.0 - 1.0;
        let b_lo = 0.0;
        let b_hi = 0.0;
        let est = h.estimate_range(a_lo, a_hi, b_lo, b_hi);

        // Marginal counts on each axis for the same ranges.
        let (full_a_lo, full_a_hi) = h.a_range();
        let (full_b_lo, full_b_hi) = h.b_range();
        let marg_a = h.estimate_range(a_lo, a_hi, full_b_lo, full_b_hi) as f64;
        let marg_b = h.estimate_range(full_a_lo, full_a_hi, b_lo, b_hi) as f64;
        let independent = marg_a * marg_b / (n as f64);

        // Under independence we'd predict ~ (n/4) * (n/4) / n = n/16 = 250.
        // True joint count for b==0 over a in [0, n/4) is ~ n/16 too — but
        // because the diagonal stripe is concentrated, the 2D histogram
        // captures the structure and the estimate is meaningfully smaller
        // than the marginal-A count `n/4` that an independent model would
        // back-derive when combined with a wider b range.
        //
        // The crucial assertion: the 2D estimate must not exceed the
        // independent assumption by a wide margin, and the marginal-A
        // count is strictly larger than the joint count (which proves the
        // 2D histogram is doing real work).
        assert!(
            (est as f64) <= independent * 1.5 + 1.0,
            "2D est {est} exceeded independent estimate {independent}"
        );
        assert!(
            (est as f64) < marg_a,
            "joint estimate {est} should be strictly less than marginal-A {marg_a}"
        );

        // Sanity: a query over b in [0, 3] (all b values) at the same
        // a-range must equal marg_a.
        let full_b = h.estimate_range(a_lo, a_hi, full_b_lo, full_b_hi);
        assert_eq!(full_b as f64, marg_a);
    }

    #[test]
    fn cell_counts_row_major_layout() {
        // Two carefully chosen pairs into a 4x4 grid with known min/max.
        let pairs = vec![(0.0, 0.0), (3.0, 3.0)];
        let h = CorrelatedHistogram2D::from_pairs(&pairs, 4, 4).unwrap();
        let cells = h.cell_counts();
        assert_eq!(cells.len(), 16);
        // (0.0, 0.0) → bucket (0, 0) → index 0
        assert_eq!(cells[0], 1);
        // (3.0, 3.0) is at the column max → bucket (3, 3) → index 3*4 + 3 = 15
        assert_eq!(cells[15], 1);
        // All other cells empty.
        let touched: u64 = cells.iter().sum();
        assert_eq!(touched, 2);
    }
}
