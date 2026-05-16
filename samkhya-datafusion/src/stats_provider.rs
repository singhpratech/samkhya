//! Conversion from `samkhya_core::stats::ColumnStats` to DataFusion's
//! `ColumnStatistics`.
//!
//! samkhya's canonical `ColumnStats` is intentionally a superset of
//! DataFusion's surface so the same instance can serve both DataFusion and
//! DuckDB. This module translates the `Option<u64>` / `Bound` shape into the
//! `Precision<usize>` / `Precision<ScalarValue>` shape DataFusion expects.
//!
//! All translated values are marked `Precision::Inexact` because samkhya's
//! corrections are feedback-driven estimates, not exact catalog counts. This
//! matches the conservative posture required by the LpBound safety envelope.

use datafusion::common::ColumnStatistics;
use datafusion::common::stats::Precision;
use datafusion::scalar::ScalarValue;
use samkhya_core::stats::{Bound, ColumnStats};

/// Convert a samkhya `ColumnStats` into a DataFusion `ColumnStatistics`.
///
/// Mapping:
/// - `null_count: Option<u64>`   -> `null_count: Precision<usize>`
/// - `distinct_count: Option<u64>` -> `distinct_count: Precision<usize>`
/// - `min: Option<Bound>`        -> `min_value: Precision<ScalarValue>`
/// - `max: Option<Bound>`        -> `max_value: Precision<ScalarValue>`
///
/// `row_count` and `upper_bound_rows` are table-level, not column-level, so
/// they are not carried on `ColumnStatistics`; callers needing them should
/// thread them through `datafusion::common::Statistics::num_rows` separately.
pub fn to_datafusion_column_statistics(src: &ColumnStats) -> ColumnStatistics {
    ColumnStatistics {
        null_count: option_u64_to_precision_usize(src.null_count),
        distinct_count: option_u64_to_precision_usize(src.distinct_count),
        min_value: bound_to_precision_scalar(src.min.as_ref()),
        max_value: bound_to_precision_scalar(src.max.as_ref()),
        sum_value: Precision::Absent,
    }
}

fn option_u64_to_precision_usize(v: Option<u64>) -> Precision<usize> {
    match v {
        Some(n) => Precision::Inexact(n as usize),
        None => Precision::Absent,
    }
}

fn bound_to_precision_scalar(b: Option<&Bound>) -> Precision<ScalarValue> {
    match b {
        Some(Bound::Int(i)) => Precision::Inexact(ScalarValue::Int64(Some(*i))),
        Some(Bound::Float(f)) => Precision::Inexact(ScalarValue::Float64(Some(*f))),
        Some(Bound::Str(s)) => Precision::Inexact(ScalarValue::Utf8(Some(s.clone()))),
        Some(Bound::Bytes(bytes)) => {
            Precision::Inexact(ScalarValue::Binary(Some(bytes.clone())))
        }
        None => Precision::Absent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stats_round_trip_to_absent() {
        let src = ColumnStats::new();
        let dst = to_datafusion_column_statistics(&src);
        assert_eq!(dst.null_count, Precision::Absent);
        assert_eq!(dst.distinct_count, Precision::Absent);
        assert_eq!(dst.min_value, Precision::Absent);
        assert_eq!(dst.max_value, Precision::Absent);
        assert_eq!(dst.sum_value, Precision::Absent);
    }

    #[test]
    fn populated_stats_become_inexact() {
        let src = ColumnStats::new()
            .with_null_count(7)
            .with_distinct_count(42);
        let dst = to_datafusion_column_statistics(&src);
        assert_eq!(dst.null_count, Precision::Inexact(7));
        assert_eq!(dst.distinct_count, Precision::Inexact(42));
    }
}
