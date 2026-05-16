//! Column-level statistics that samkhya-* adapters inject into native optimizers.

use serde::{Deserialize, Serialize};

/// Canonical column statistics surface.
///
/// Intentionally a superset of DataFusion's `ColumnStatistics` and DuckDB's
/// internal `BaseStatistics` so a single instance can satisfy either engine.
///
/// # Examples
///
/// ```
/// use samkhya_core::ColumnStats;
///
/// let stats = ColumnStats::new()
///     .with_row_count(10_000)
///     .with_distinct_count(8_421)
///     .with_null_count(7)
///     .with_upper_bound(10_000);
/// assert_eq!(stats.row_count, Some(10_000));
/// assert_eq!(stats.distinct_count, Some(8_421));
/// assert_eq!(stats.null_count, Some(7));
/// assert_eq!(stats.upper_bound_rows, Some(10_000));
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColumnStats {
    pub row_count: Option<u64>,
    pub null_count: Option<u64>,
    pub distinct_count: Option<u64>,
    pub min: Option<Bound>,
    pub max: Option<Bound>,
    /// Inclusive ceiling derived from LpBound; correctors must not exceed this.
    pub upper_bound_rows: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Bound {
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
}

impl ColumnStats {
    /// Construct an empty stats record (all fields `None`).
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::ColumnStats;
    ///
    /// let s = ColumnStats::new();
    /// assert!(s.row_count.is_none());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style setter for `row_count`.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::ColumnStats;
    ///
    /// let s = ColumnStats::new().with_row_count(100);
    /// assert_eq!(s.row_count, Some(100));
    /// ```
    pub fn with_row_count(mut self, n: u64) -> Self {
        self.row_count = Some(n);
        self
    }

    /// Builder-style setter for `distinct_count`.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::ColumnStats;
    ///
    /// let s = ColumnStats::new().with_distinct_count(42);
    /// assert_eq!(s.distinct_count, Some(42));
    /// ```
    pub fn with_distinct_count(mut self, n: u64) -> Self {
        self.distinct_count = Some(n);
        self
    }

    /// Builder-style setter for `null_count`.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::ColumnStats;
    ///
    /// let s = ColumnStats::new().with_null_count(3);
    /// assert_eq!(s.null_count, Some(3));
    /// ```
    pub fn with_null_count(mut self, n: u64) -> Self {
        self.null_count = Some(n);
        self
    }

    /// Builder-style setter for `upper_bound_rows`. The LpBound ceiling
    /// that downstream correctors must respect.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::ColumnStats;
    ///
    /// let s = ColumnStats::new().with_upper_bound(10_000);
    /// assert_eq!(s.upper_bound_rows, Some(10_000));
    /// ```
    pub fn with_upper_bound(mut self, n: u64) -> Self {
        self.upper_bound_rows = Some(n);
        self
    }
}
