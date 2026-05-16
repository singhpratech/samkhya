//! `SamkhyaStatsExec` — the [`ExecutionPlan`]-layer wrapper that actually
//! flows samkhya-corrected statistics into DataFusion 46's physical plan.
//!
//! # Why this exists
//!
//! In DataFusion 46.0.1 the mainline physical planner does **not** call
//! [`TableProvider::statistics()`] when it constructs the leaf
//! `ExecutionPlan` for a scan: it just invokes
//! [`TableProvider::scan()`] and uses whatever exec is returned. The
//! exec's own [`ExecutionPlan::statistics()`] is what later operators
//! (`FilterExec`, `ProjectionExec`, `HashJoinExec`, …) propagate up the
//! tree.
//!
//! To get samkhya's corrections into the row-count numbers reported by
//! `ctx.state().create_physical_plan(&plan).await?.statistics()`, we
//! therefore need to override `statistics()` at the *physical* layer.
//! `SamkhyaStatsExec` is a thin passthrough wrapper for that single
//! purpose:
//!
//! * everything (schema, partitioning, equivalence, execute) delegates to
//!   the inner [`ExecutionPlan`];
//! * `statistics()` returns the override [`Statistics`] supplied at
//!   construction time, marked `Precision::Inexact` per the LpBound
//!   conservative posture;
//! * `with_new_children` rebuilds the wrapper around the new child,
//!   preserving the override — so subsequent physical optimizer rules can
//!   reshape the tree without losing the corrected stats.
//!
//! # Where it gets installed
//!
//! [`crate::SamkhyaTableProvider`] installs this wrapper from inside its
//! `scan()` implementation: the inner provider returns its native exec,
//! and the table provider wraps it in `SamkhyaStatsExec` carrying the
//! samkhya-corrected `Statistics`. The mainline planner then sees the
//! wrapped exec as the scan leaf, so its `statistics()` are the samkhya
//! values — and propagation through filters / projections / joins
//! produces a final plan whose `.statistics()?.num_rows` reflects the
//! corrections.
//!
//! [`TableProvider::statistics()`]: datafusion::datasource::TableProvider::statistics
//! [`TableProvider::scan()`]: datafusion::datasource::TableProvider::scan

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::common::{Result, Statistics};
use datafusion::execution::TaskContext;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, PlanProperties, SendableRecordBatchStream,
};

/// Passthrough [`ExecutionPlan`] wrapper that publishes samkhya-corrected
/// statistics from `statistics()` while delegating every other method to
/// the inner exec.
///
/// Construct via [`SamkhyaStatsExec::new`]; the wrapper holds an
/// `Arc<dyn ExecutionPlan>` plus the `Statistics` it should report.
#[derive(Debug, Clone)]
pub struct SamkhyaStatsExec {
    input: Arc<dyn ExecutionPlan>,
    stats: Statistics,
}

impl SamkhyaStatsExec {
    /// Wrap `input` so that calls to [`ExecutionPlan::statistics()`] on
    /// the result return `stats` rather than the input's defaults.
    pub fn new(input: Arc<dyn ExecutionPlan>, stats: Statistics) -> Self {
        Self { input, stats }
    }

    /// Borrow the inner plan.
    pub fn input(&self) -> &Arc<dyn ExecutionPlan> {
        &self.input
    }

    /// Borrow the override statistics.
    pub fn override_statistics(&self) -> &Statistics {
        &self.stats
    }
}

impl DisplayAs for SamkhyaStatsExec {
    fn fmt_as(&self, t: DisplayFormatType, f: &mut fmt::Formatter) -> fmt::Result {
        match t {
            DisplayFormatType::Default | DisplayFormatType::Verbose => {
                write!(f, "SamkhyaStatsExec: num_rows={:?}", self.stats.num_rows)
            }
        }
    }
}

impl ExecutionPlan for SamkhyaStatsExec {
    fn name(&self) -> &str {
        "SamkhyaStatsExec"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> SchemaRef {
        self.input.schema()
    }

    fn properties(&self) -> &PlanProperties {
        // Passthrough: our output partitioning / ordering / equivalence
        // match the inner plan exactly. Borrowing the inner cache keeps
        // the wrapper allocation-free and avoids drift if the inner plan
        // is itself rewritten by other physical-optimizer rules.
        self.input.properties()
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        vec![&self.input]
    }

    fn maintains_input_order(&self) -> Vec<bool> {
        // Passthrough preserves ordering trivially.
        vec![true]
    }

    fn benefits_from_input_partitioning(&self) -> Vec<bool> {
        vec![false]
    }

    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        // Preserve the override across rewrites — other physical
        // optimizer rules will call `with_new_children` to swap our
        // input, and we want the corrected stats to ride along.
        let new_input = children
            .into_iter()
            .next()
            .expect("SamkhyaStatsExec has exactly one child");
        Ok(Arc::new(SamkhyaStatsExec::new(
            new_input,
            self.stats.clone(),
        )))
    }

    fn execute(
        &self,
        partition: usize,
        context: Arc<TaskContext>,
    ) -> Result<SendableRecordBatchStream> {
        self.input.execute(partition, context)
    }

    fn statistics(&self) -> Result<Statistics> {
        Ok(self.stats.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::Int64Array;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::arrow::record_batch::RecordBatch;
    use datafusion::common::stats::Precision;
    use datafusion::datasource::{MemTable, TableProvider};
    use datafusion::execution::context::SessionContext;

    async fn tiny_input_exec() -> Arc<dyn ExecutionPlan> {
        let schema = Arc::new(Schema::new(vec![Field::new("a", DataType::Int64, false)]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(Int64Array::from(vec![1, 2, 3]))],
        )
        .unwrap();
        let mem = Arc::new(MemTable::try_new(Arc::clone(&schema), vec![vec![batch]]).unwrap());
        let ctx = SessionContext::new();
        let state = ctx.state();
        let session: &dyn datafusion::catalog::Session = &state;
        mem.scan(session, None, &[], None).await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wrapper_reports_override_stats() {
        let inner = tiny_input_exec().await;
        let mut stats = Statistics::new_unknown(inner.schema().as_ref());
        stats.num_rows = Precision::Inexact(42);
        let wrapped: Arc<dyn ExecutionPlan> = Arc::new(SamkhyaStatsExec::new(inner, stats));
        let s = wrapped.statistics().expect("stats present");
        assert_eq!(s.num_rows, Precision::Inexact(42));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_new_children_preserves_override() {
        let inner = tiny_input_exec().await;
        let mut stats = Statistics::new_unknown(inner.schema().as_ref());
        stats.num_rows = Precision::Inexact(7);
        let wrapped: Arc<dyn ExecutionPlan> =
            Arc::new(SamkhyaStatsExec::new(Arc::clone(&inner), stats));
        // Swap the child for an identical plan — the override must ride
        // through the rebuild, otherwise downstream physical optimizer
        // rules would erase samkhya's corrections.
        let rebuilt = Arc::clone(&wrapped)
            .with_new_children(vec![inner])
            .expect("rebuild");
        assert_eq!(
            rebuilt.statistics().unwrap().num_rows,
            Precision::Inexact(7)
        );
    }
}
