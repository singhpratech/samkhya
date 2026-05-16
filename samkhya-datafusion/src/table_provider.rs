//! `SamkhyaTableProvider` — the primary integration point for injecting
//! samkhya-corrected column statistics into DataFusion's query planning.
//!
//! # Wrapping point: `TableProvider::statistics()`
//!
//! DataFusion attaches statistics to table providers, not to logical-plan
//! nodes. The [`TableProvider`] trait exposes a `statistics()` hook
//! (returning `Option<Statistics>`) that the planner consults when reasoning
//! about cardinality, join order, and filter selectivity. Rewriting a
//! `LogicalPlan` to "inject" stats is the wrong layer — that is observe-only
//! plumbing. The right layer is a `TableProvider` shim that delegates every
//! method to an inner provider *except* `statistics()`, where it folds in
//! samkhya's feedback-driven corrections.
//!
//! We considered three wrapping points and chose the first:
//!
//! 1. **`TableProvider::statistics()`** (this module). Clean, stable surface
//!    in DataFusion 46. The planner calls it during analysis. Every adapter
//!    (Parquet, CSV, MemTable, Iceberg) flows through the same hook, so the
//!    shim is provider-agnostic.
//! 2. `ExecutionPlan::statistics()`. Lower in the stack — would require
//!    wrapping the scan-side `ExecutionPlan` returned from `scan()`. Useful
//!    when the inner provider's logical stats are absent but its physical
//!    plan has them; not our situation today.
//! 3. `OptimizerRule` rewriting `TableScan::source`. The original scaffold
//!    direction. The rewrite must construct a new `TableSource` (the logical
//!    counterpart of `TableProvider`) — duplicate state, version-fragile,
//!    and never propagates into the physical layer where the planner
//!    actually consults stats. Kept around as observe-only telemetry
//!    ([`crate::SamkhyaOptimizerRule`]).
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
use datafusion::common::{ColumnStatistics, Constraints, Result, Statistics};
use datafusion::datasource::{TableProvider, TableType};
use datafusion::logical_expr::dml::InsertOp;
use datafusion::logical_expr::{Expr, LogicalPlan, TableProviderFilterPushDown};
use datafusion::physical_plan::ExecutionPlan;
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
        let override_row_count = self.overrides.values().filter_map(|s| s.row_count).max();
        if let Some(rc) = override_row_count {
            base.num_rows = Precision::Inexact(rc as usize);
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
/// base. Fields where the override carries an `Inexact` value win. This
/// mirrors how the optimizer treats partial stats: any signal is better
/// than no signal, but never overwrite a known value with unknown.
fn merge_column_stats(base: ColumnStatistics, ovr: ColumnStatistics) -> ColumnStatistics {
    ColumnStatistics {
        null_count: pick(base.null_count, ovr.null_count),
        max_value: pick(base.max_value, ovr.max_value),
        min_value: pick(base.min_value, ovr.min_value),
        sum_value: pick(base.sum_value, ovr.sum_value),
        distinct_count: pick(base.distinct_count, ovr.distinct_count),
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

    #[test]
    fn builder_records_overrides() {
        let inner = tiny_mem_table();
        let wrapped = SamkhyaTableProvider::new(inner)
            .with_column_stats(0, ColumnStats::new().with_row_count(999));
        assert_eq!(wrapped.overrides().len(), 1);
        assert_eq!(wrapped.overrides()[&0].row_count, Some(999));
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
}
