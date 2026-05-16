//! Column-level statistics that samkhya-* adapters inject into native optimizers.

use serde::{Deserialize, Serialize};

/// Canonical column statistics surface.
///
/// Intentionally a superset of DataFusion's `ColumnStatistics` and DuckDB's
/// internal `BaseStatistics` so a single instance can satisfy either engine.
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_row_count(mut self, n: u64) -> Self {
        self.row_count = Some(n);
        self
    }

    pub fn with_distinct_count(mut self, n: u64) -> Self {
        self.distinct_count = Some(n);
        self
    }

    pub fn with_null_count(mut self, n: u64) -> Self {
        self.null_count = Some(n);
        self
    }

    pub fn with_upper_bound(mut self, n: u64) -> Self {
        self.upper_bound_rows = Some(n);
        self
    }
}
