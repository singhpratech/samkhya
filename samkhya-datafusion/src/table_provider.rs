//! `SamkhyaTableProvider` — the primary integration point for injecting
//! samkhya-corrected column statistics into DataFusion's query planning.
//!
//! # Wrapping point: scan-time `ExecutionPlan::statistics()`
//!
//! DataFusion 46 exposes `TableProvider::statistics()` for downstream users,
//! but its mainline physical planner relies on statistics from the
//! [`ExecutionPlan`] returned by [`TableProvider::scan`]. This wrapper folds
//! samkhya overrides into the provider statistics surface and also wraps the
//! scan result in [`crate::SamkhyaStatsExec`], which publishes the same values
//! at the physical layer. The logical optimizer rule remains observe-only.
//!
//! Portable snapshots carry Iceberg field IDs, whereas DataFusion addresses
//! columns by zero-based schema ordinal. [`SamkhyaTableProvider::try_with_portable_stats`]
//! requires both values explicitly so schema evolution cannot silently bind a
//! sketch to the wrong column.
//!
//! # LpBound posture
//!
//! Every value translated into DataFusion's `Precision<T>` is wrapped as
//! [`Precision::Inexact`]. samkhya's corrections are feedback-driven
//! estimates clamped by the LpBound pessimistic ceiling; they are never
//! exact catalog counts. `Inexact` is the precision DataFusion's
//! cost-based optimizer treats as "use this, but do not assume zero error".

use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use datafusion::arrow::datatypes::SchemaRef;
use datafusion::catalog::Session;
use datafusion::common::stats::Precision;
use datafusion::common::{ColumnStatistics, Constraints, DataFusionError, Result, Statistics};
use datafusion::datasource::{TableProvider, TableType};
use datafusion::logical_expr::dml::InsertOp;
use datafusion::logical_expr::{Expr, LogicalPlan, TableProviderFilterPushDown};
use datafusion::physical_plan::ExecutionPlan;
use samkhya_core::portable::PortableStatsSnapshot;
use samkhya_core::stats::ColumnStats;

use crate::physical_plan::SamkhyaStatsExec;
use crate::stats_provider::to_datafusion_column_statistics;

/// A [`TableProvider`] wrapper that overrides `statistics()` with
/// samkhya-corrected column statistics while delegating every other method
/// to the inner provider.
///
/// # Builder
///
/// ```ignore
/// use std::sync::Arc;
/// use samkhya_datafusion::SamkhyaTableProvider;
/// use samkhya_core::stats::ColumnStats;
///
/// let wrapped = SamkhyaTableProvider::new(Arc::new(inner))
///     .with_column_stats(0, ColumnStats::new().with_row_count(999).with_distinct_count(42));
/// ```
///
/// # Stats fold semantics
///
/// `statistics()` builds a `Statistics` whose per-column entries come from
/// the samkhya override map where present, falling back to the inner
/// provider's stats (or `ColumnStatistics::new_unknown()` if the inner
/// provider returns `None`). Table-level `num_rows` is taken from the
/// override map's most authoritative `row_count`: the maximum across all
/// override entries, since samkhya's per-column stats describe the same
/// underlying relation. If no override carries a row count, the inner
/// provider's `num_rows` is preserved.
#[derive(Debug)]
pub struct SamkhyaTableProvider {
    inner: Arc<dyn TableProvider>,
    overrides: HashMap<usize, ColumnStats>,
    /// Number of times `statistics()` has been invoked by the planner.
    /// Exposed for integration tests; not part of the public optimization
    /// contract.
    stats_calls: AtomicUsize,
}

impl SamkhyaTableProvider {
    /// Wrap an existing provider. No overrides are installed until
    /// [`Self::with_column_stats`] is called.
    pub fn new(inner: Arc<dyn TableProvider>) -> Self {
        Self {
            inner,
            overrides: HashMap::new(),
            stats_calls: AtomicUsize::new(0),
        }
    }

    /// Install a samkhya override for the column at `col_idx`.
    ///
    /// Indices refer to positions in the inner provider's [`SchemaRef`].
    /// Existing overrides for the same index are replaced.
    pub fn with_column_stats(mut self, col_idx: usize, stats: ColumnStats) -> Self {
        self.overrides.insert(col_idx, stats);
        self
    }

    /// Decode one Iceberg field from a portable snapshot and bind it to an
    /// explicit DataFusion column ordinal.
    ///
    /// Iceberg field IDs are stable schema identifiers, while DataFusion uses
    /// zero-based positions in the provider schema. They are intentionally
    /// separate arguments: this adapter never casts a field ID into an ordinal
    /// or assumes that the two numbering schemes coincide.
    ///
    /// The binding fails when the field ID or ordinal is invalid, the portable
    /// payload is malformed, or the field has no scalar statistic DataFusion
    /// can publish. In particular, an equi-depth histogram remains available
    /// from [`PortableStatsSnapshot`] but cannot by itself populate
    /// DataFusion 46's [`ColumnStatistics`].
    pub fn try_with_portable_stats(
        self,
        snapshot: &PortableStatsSnapshot,
        iceberg_field_id: i32,
        datafusion_column_ordinal: usize,
    ) -> Result<Self> {
        if iceberg_field_id <= 0 {
            return Err(DataFusionError::Plan(format!(
                "Iceberg field id must be positive; got {iceberg_field_id}"
            )));
        }

        let field_count = self.inner.schema().fields().len();
        if datafusion_column_ordinal >= field_count {
            return Err(DataFusionError::Plan(format!(
                "DataFusion column ordinal {datafusion_column_ordinal} is out of range for a \
                 provider with {field_count} columns"
            )));
        }

        let decoded = snapshot
            .decode_column(iceberg_field_id)
            .map_err(|error| DataFusionError::External(Box::new(error)))?
            .ok_or_else(|| {
                DataFusionError::Plan(format!(
                    "portable snapshot has no supported statistics for Iceberg field id \
                     {iceberg_field_id}"
                ))
            })?;
        let stats = decoded.column_stats().clone();
        if !has_datafusion_planner_stats(&stats) {
            return Err(DataFusionError::Plan(format!(
                "portable snapshot has no DataFusion-compatible scalar statistics for Iceberg \
                 field id {iceberg_field_id}"
            )));
        }

        Ok(self.with_column_stats(datafusion_column_ordinal, stats))
    }

    /// Number of times `statistics()` has been called on this wrapper.
    ///
    /// Useful for assertions in integration tests that verify the planner
    /// actually consulted the corrected stats.
    pub fn stats_call_count(&self) -> usize {
        self.stats_calls.load(Ordering::SeqCst)
    }

    /// Borrow the override map. Read-only access for diagnostics.
    pub fn overrides(&self) -> &HashMap<usize, ColumnStats> {
        &self.overrides
    }
}

fn has_datafusion_planner_stats(stats: &ColumnStats) -> bool {
    stats.row_count.is_some()
        || stats.null_count.is_some()
        || stats.distinct_count.is_some()
        || stats.min.is_some()
        || stats.max.is_some()
}

#[async_trait]
impl TableProvider for SamkhyaTableProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> SchemaRef {
        self.inner.schema()
    }

    fn constraints(&self) -> Option<&Constraints> {
        self.inner.constraints()
    }

    fn table_type(&self) -> TableType {
        self.inner.table_type()
    }

    fn get_table_definition(&self) -> Option<&str> {
        self.inner.get_table_definition()
    }

    fn get_logical_plan(&self) -> Option<Cow<'_, LogicalPlan>> {
        self.inner.get_logical_plan()
    }

    fn get_column_default(&self, column: &str) -> Option<&Expr> {
        self.inner.get_column_default(column)
    }

    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        // Ask the inner provider for its native scan exec, then wrap it
        // in `SamkhyaStatsExec` so the physical layer publishes the
        // samkhya-corrected `Statistics` to every downstream operator.
        //
        // This is the actual injection path: DataFusion 46's mainline
        // planner does not consult `TableProvider::statistics()` when
        // building the physical plan — it calls `scan()` and trusts the
        // returned `ExecutionPlan::statistics()`. So the only reliable
        // way to flow corrected row counts into
        // `physical.statistics()?.num_rows` is to override at the exec
        // level, here.
        //
        // If we have no overrides installed we still wrap, using the
        // statistics() fold as-is — the cost is one cheap delegation
        // call per execute()/statistics() and the inner provider's
        // values are preserved by the merge in `self.statistics()`.
        let inner_plan = self.inner.scan(state, projection, filters, limit).await?;

        // Project the table-level Statistics onto the scan's *output*
        // schema (which honours `projection`), so the wrapped exec
        // reports column_statistics aligned to the columns it actually
        // emits — not the full table schema. This matches what
        // `TableProvider`-aware execs (`DataSourceExec`) already do.
        let full_stats = self
            .statistics()
            .unwrap_or_else(|| Statistics::new_unknown(self.inner.schema().as_ref()));
        let output_stats = full_stats.project(projection);

        Ok(Arc::new(SamkhyaStatsExec::new(inner_plan, output_stats)))
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> Result<Vec<TableProviderFilterPushDown>> {
        self.inner.supports_filters_pushdown(filters)
    }

    /// Fold samkhya overrides into the inner provider's `Statistics`.
    ///
    /// Schema order is preserved: column `i` in the returned
    /// `column_statistics` corresponds to field `i` of `self.schema()`.
    fn statistics(&self) -> Option<Statistics> {
        // Record the call so tests can assert the planner consulted us.
        self.stats_calls.fetch_add(1, Ordering::SeqCst);

        let schema = self.inner.schema();
        let n_fields = schema.fields().len();

        // Start from the inner provider's stats; fall back to an unknown
        // skeleton sized to the schema so we always return Some(_).
        let mut base = self
            .inner
            .statistics()
            .unwrap_or_else(|| Statistics::new_unknown(schema.as_ref()));

        // Defensive: if the inner provider returned a column_statistics vec
        // whose length disagrees with the schema, normalise to schema size.
        if base.column_statistics.len() != n_fields {
            base.column_statistics = Statistics::unknown_column(schema.as_ref());
        }

        // Per-column merge: override wins where present, inner is preserved
        // otherwise. samkhya values are translated as Inexact per the
        // LpBound conservative posture.
        for (col_idx, override_stats) in &self.overrides {
            if *col_idx >= n_fields {
                // Index out of range — skip rather than panic; this can
                // happen if the schema changes under us.
                continue;
            }
            let translated = to_datafusion_column_statistics(override_stats);
            base.column_statistics[*col_idx] =
                merge_column_stats(base.column_statistics[*col_idx].clone(), translated);
        }

        // Table-level row count: take the max row_count across overrides
        // (they all describe the same relation, so any populated value is
        // a corrected estimate of |R|). If no override carries a row
        // count, keep the inner provider's value.
        //
        // WAVE5-RC2: plan-memory-monotonic guard. Never publish a row
        // count smaller than the inner provider's native estimate. The
        // hash-join build-side sizing in DataFusion 46 picks the smaller
        // side as the build side; if samkhya under-estimates and the
        // actual data is much larger, the build hash table grows past
        // its sized allocation and the planner walks into an OOM. Capping
        // the published row count at `max(samkhya, native)` preserves
        // samkhya's win when it has a larger / more accurate NDV-derived
        // row count, while never pushing the planner toward a smaller
        // build side than it would have chosen with no samkhya input.
        // Symmetric guard on `SamkhyaStatsExec::statistics()` enforces
        // the same invariant at the physical layer.
        let override_row_count = self.overrides.values().filter_map(|s| s.row_count).max();
        if let Some(rc) = override_row_count {
            let rc_usize = rc as usize;
            let monotone_rc = match base.num_rows {
                Precision::Exact(n) | Precision::Inexact(n) => rc_usize.max(n),
                Precision::Absent => rc_usize,
            };
            base.num_rows = Precision::Inexact(monotone_rc);
            // Total byte size: if the inner provider reported it, relax to
            // inexact since the row count has shifted; otherwise leave
            // absent.
            base.total_byte_size = match base.total_byte_size {
                Precision::Exact(n) | Precision::Inexact(n) => Precision::Inexact(n),
                Precision::Absent => Precision::Absent,
            };
        }

        Some(base)
    }

    async fn insert_into(
        &self,
        state: &dyn Session,
        input: Arc<dyn ExecutionPlan>,
        insert_op: InsertOp,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        self.inner.insert_into(state, input, insert_op).await
    }
}

/// Merge a samkhya-translated `ColumnStatistics` over a base one.
///
/// Fields where the override is `Precision::Absent` fall through to the
/// base. Fields where the override carries an `Inexact` value win for
/// null_count / max_value / min_value / sum_value. **`distinct_count`
/// applies the WAVE5-RC2 plan-memory-monotonic guard** — the published
/// value is `max(samkhya_ndv, native_ndv)`. NDV drives hash-join
/// build-side sizing; never publishing a smaller distinct count than
/// DataFusion's native estimate prevents the corrected arm from
/// pushing the planner toward a smaller build hash table than it would
/// have chosen with no samkhya input.
fn merge_column_stats(base: ColumnStatistics, ovr: ColumnStatistics) -> ColumnStatistics {
    ColumnStatistics {
        null_count: pick(base.null_count, ovr.null_count),
        max_value: pick(base.max_value, ovr.max_value),
        min_value: pick(base.min_value, ovr.min_value),
        sum_value: pick(base.sum_value, ovr.sum_value),
        distinct_count: pick_max_usize(base.distinct_count, ovr.distinct_count),
    }
}

/// Plan-memory-monotonic merge for `Precision<usize>` cardinality
/// fields. Returns `Precision::Inexact(max(base, ovr))` when both
/// carry a value, the present one when only one does, and
/// `Precision::Absent` when neither does. Used for `distinct_count`
/// merges so samkhya never publishes an NDV smaller than the inner
/// provider would have on its own.
fn pick_max_usize(base: Precision<usize>, ovr: Precision<usize>) -> Precision<usize> {
    let base_val = match base {
        Precision::Exact(n) | Precision::Inexact(n) => Some(n),
        Precision::Absent => None,
    };
    let ovr_val = match ovr {
        Precision::Exact(n) | Precision::Inexact(n) => Some(n),
        Precision::Absent => None,
    };
    match (base_val, ovr_val) {
        (Some(b), Some(o)) => Precision::Inexact(b.max(o)),
        (Some(b), None) => Precision::Inexact(b),
        (None, Some(o)) => Precision::Inexact(o),
        (None, None) => Precision::Absent,
    }
}

fn pick<T>(base: Precision<T>, ovr: Precision<T>) -> Precision<T>
where
    T: std::fmt::Debug + Clone + PartialEq + Eq + PartialOrd,
{
    match ovr {
        Precision::Absent => base,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::Int64Array;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::arrow::record_batch::RecordBatch;
    use datafusion::datasource::MemTable;
    use samkhya_core::portable::{PortableSketchBlob, PortableStatsSnapshot};
    use samkhya_core::sketches::{EquiDepthHistogram, HllSketch, Sketch};

    fn tiny_mem_table() -> Arc<MemTable> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("a", DataType::Int64, false),
            Field::new("b", DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![1, 2, 3, 4, 5])),
                Arc::new(Int64Array::from(vec![10, 20, 30, 40, 50])),
            ],
        )
        .unwrap();
        Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap())
    }

    fn portable_hll_snapshot(field_id: i32) -> (PortableStatsSnapshot, u64) {
        let mut hll = HllSketch::new(10).unwrap();
        for value in 0..256_u32 {
            hll.add(&value.to_le_bytes());
        }
        let expected_distinct = hll.estimate();
        let snapshot = PortableStatsSnapshot::new(
            Some(42),
            vec![PortableSketchBlob::new(
                HllSketch::KIND,
                vec![field_id],
                hll.to_bytes().unwrap(),
            )],
        );
        (snapshot, expected_distinct)
    }

    #[test]
    fn builder_records_overrides() {
        let inner = tiny_mem_table();
        let wrapped = SamkhyaTableProvider::new(inner)
            .with_column_stats(0, ColumnStats::new().with_row_count(999));
        assert_eq!(wrapped.overrides().len(), 1);
        assert_eq!(wrapped.overrides()[&0].row_count, Some(999));
    }

    #[test]
    fn portable_stats_bind_field_id_to_explicit_column_ordinal() {
        let (snapshot, expected_distinct) = portable_hll_snapshot(17);
        let wrapped = SamkhyaTableProvider::new(tiny_mem_table())
            .try_with_portable_stats(&snapshot, 17, 1)
            .expect("field 17 should bind to caller-selected ordinal 1");

        assert_eq!(wrapped.overrides().len(), 1);
        assert_eq!(
            wrapped.overrides()[&1].distinct_count,
            Some(expected_distinct)
        );
        assert!(
            !wrapped.overrides().contains_key(&17),
            "Iceberg field ID must never be cast into a DataFusion ordinal"
        );

        let stats = wrapped.statistics().expect("statistics present");
        assert_eq!(stats.column_statistics[0].distinct_count, Precision::Absent);
        assert_eq!(
            stats.column_statistics[1].distinct_count,
            Precision::Inexact(expected_distinct as usize)
        );
    }

    #[test]
    fn portable_stats_reject_missing_supported_field_stats() {
        let snapshot = PortableStatsSnapshot::new(
            Some(42),
            vec![PortableSketchBlob::new(
                "vendor.future-v1",
                vec![17],
                b"opaque".to_vec(),
            )],
        );
        let error = SamkhyaTableProvider::new(tiny_mem_table())
            .try_with_portable_stats(&snapshot, 17, 0)
            .expect_err("unknown kinds must not install an empty override");

        assert!(matches!(&error, DataFusionError::Plan(_)));
        assert!(error.to_string().contains("no supported statistics"));
    }

    #[test]
    fn portable_stats_reject_histogram_without_scalar_planner_stats() {
        let histogram = EquiDepthHistogram::from_values(
            &(0..100).map(|value| value as f64).collect::<Vec<_>>(),
            10,
        )
        .unwrap();
        let snapshot = PortableStatsSnapshot::new(
            Some(42),
            vec![PortableSketchBlob::new(
                EquiDepthHistogram::KIND,
                vec![17],
                histogram.to_bytes().unwrap(),
            )],
        );
        let error = SamkhyaTableProvider::new(tiny_mem_table())
            .try_with_portable_stats(&snapshot, 17, 0)
            .expect_err("a histogram alone has no DataFusion 46 statistics slot");

        assert!(matches!(&error, DataFusionError::Plan(_)));
        assert!(
            error
                .to_string()
                .contains("no DataFusion-compatible scalar statistics")
        );
    }

    #[test]
    fn portable_stats_reject_out_of_range_column_ordinal() {
        let (snapshot, _) = portable_hll_snapshot(17);
        let error = SamkhyaTableProvider::new(tiny_mem_table())
            .try_with_portable_stats(&snapshot, 17, 2)
            .expect_err("two-column schema has no ordinal 2");

        assert!(matches!(&error, DataFusionError::Plan(_)));
        assert!(error.to_string().contains("ordinal 2 is out of range"));
    }

    #[test]
    fn statistics_overrides_row_count() {
        let inner = tiny_mem_table();
        let wrapped = SamkhyaTableProvider::new(inner).with_column_stats(
            0,
            ColumnStats::new()
                .with_row_count(999)
                .with_distinct_count(42),
        );
        let stats = wrapped.statistics().expect("statistics present");
        assert_eq!(stats.num_rows, Precision::Inexact(999));
        assert_eq!(
            stats.column_statistics[0].distinct_count,
            Precision::Inexact(42)
        );
        assert_eq!(wrapped.stats_call_count(), 1);
    }

    #[test]
    fn statistics_falls_back_for_unoverridden_columns() {
        let inner = tiny_mem_table();
        let wrapped = SamkhyaTableProvider::new(inner)
            .with_column_stats(0, ColumnStats::new().with_distinct_count(7));
        let stats = wrapped.statistics().expect("statistics present");
        assert_eq!(
            stats.column_statistics[0].distinct_count,
            Precision::Inexact(7)
        );
        // Column 1 has no override and the inner MemTable does not report
        // stats — so the slot stays at Absent.
        assert_eq!(stats.column_statistics[1].distinct_count, Precision::Absent);
    }

    #[test]
    fn out_of_range_override_is_ignored() {
        let inner = tiny_mem_table();
        let wrapped = SamkhyaTableProvider::new(inner)
            .with_column_stats(99, ColumnStats::new().with_distinct_count(123));
        // No panic, statistics still produced.
        let stats = wrapped.statistics().expect("statistics present");
        assert_eq!(stats.column_statistics.len(), 2);
    }

    /// WAVE5-RC2: when the inner provider's native row count exceeds the
    /// samkhya override, the published value is the inner provider's
    /// estimate, not the (smaller) samkhya value. Prevents the planner
    /// from picking a smaller hash-join build side than baseline would.
    ///
    /// Uses a minimal mock provider that returns a known
    /// `Precision::Inexact(5)` for num_rows, since `MemTable`'s default
    /// stats path leaves num_rows as `Precision::Absent` (which would
    /// trip the fallback branch, not the monotone-cap branch).
    #[test]
    fn statistics_row_count_caps_at_max_of_samkhya_and_native() {
        use async_trait::async_trait;
        use datafusion::catalog::Session;
        use datafusion::common::Result as DfResult;
        use datafusion::datasource::{TableProvider, TableType};
        use datafusion::logical_expr::Expr;
        use datafusion::physical_plan::ExecutionPlan;

        #[derive(Debug)]
        struct MockProvider {
            schema: SchemaRef,
            native_rows: usize,
        }

        #[async_trait]
        impl TableProvider for MockProvider {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn schema(&self) -> SchemaRef {
                Arc::clone(&self.schema)
            }
            fn table_type(&self) -> TableType {
                TableType::Base
            }
            async fn scan(
                &self,
                _state: &dyn Session,
                _projection: Option<&Vec<usize>>,
                _filters: &[Expr],
                _limit: Option<usize>,
            ) -> DfResult<Arc<dyn ExecutionPlan>> {
                unreachable!("scan not exercised by this test")
            }
            fn statistics(&self) -> Option<Statistics> {
                let mut s = Statistics::new_unknown(self.schema.as_ref());
                s.num_rows = Precision::Inexact(self.native_rows);
                Some(s)
            }
        }

        let schema = Arc::new(Schema::new(vec![Field::new("a", DataType::Int64, false)]));
        let inner: Arc<dyn TableProvider> = Arc::new(MockProvider {
            schema: Arc::clone(&schema),
            native_rows: 5,
        });
        let wrapped = SamkhyaTableProvider::new(inner)
            .with_column_stats(0, ColumnStats::new().with_row_count(3));
        let stats = wrapped.statistics().expect("statistics present");
        assert_eq!(
            stats.num_rows,
            Precision::Inexact(5),
            "monotone cap must publish max(samkhya=3, native=5)=5, not the smaller samkhya estimate"
        );
    }

    /// WAVE5-RC2: symmetric column-level guard. When samkhya's NDV
    /// override is smaller than the inner provider's native NDV, publish
    /// the native (larger) value to keep hash-join build sides on the
    /// safe side.
    #[test]
    fn statistics_distinct_count_caps_at_max_of_samkhya_and_native() {
        // Hand-construct a base ColumnStatistics with a known native
        // distinct_count and feed it through merge_column_stats with a
        // smaller samkhya override. Expected: max() wins.
        let base = ColumnStatistics {
            null_count: Precision::Absent,
            max_value: Precision::Absent,
            min_value: Precision::Absent,
            sum_value: Precision::Absent,
            distinct_count: Precision::Inexact(1000),
        };
        let ovr = ColumnStatistics {
            null_count: Precision::Absent,
            max_value: Precision::Absent,
            min_value: Precision::Absent,
            sum_value: Precision::Absent,
            distinct_count: Precision::Inexact(50),
        };
        let merged = merge_column_stats(base, ovr);
        assert_eq!(
            merged.distinct_count,
            Precision::Inexact(1000),
            "merge must publish max(samkhya, native) distinct_count"
        );
    }
}
