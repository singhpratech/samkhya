//! `SamkhyaOptimizerRule` — DataFusion integration point for samkhya's
//! cardinality corrections.
//!
//! # Two-trait surface
//!
//! In DataFusion 46.0 the rule implements **both** [`OptimizerRule`]
//! (logical) and [`PhysicalOptimizerRule`] (physical). Cardinality
//! information cannot be injected from a logical rule alone: DataFusion's
//! mainline planner does not call [`TableProvider::statistics()`] when it
//! constructs the leaf [`ExecutionPlan`] for a scan, and the
//! `LogicalPlan::TableScan` node has no slot to attach a `Statistics`
//! value. The only reliable hook is the physical layer, where we wrap
//! scan execs with [`SamkhyaStatsExec`] so their `statistics()` returns
//! samkhya-corrected values that propagate up through filters,
//! projections, and joins.
//!
//! ## Logical pass: observe-only
//!
//! The logical-side `rewrite` traverses the plan and counts `TableScan`
//! nodes. It does not mutate the plan (returns `Transformed::no`); the
//! traversal is retained as telemetry — it exercises the corrected-stats
//! helper end-to-end and gives downstream code a stable hook into the
//! optimizer pass without changing the logical tree.
//!
//! ## Physical pass: the actual injection
//!
//! The physical-side `optimize` walks the [`ExecutionPlan`] tree and
//! records how many [`SamkhyaStatsExec`] leaves it observes. Those
//! wrappers were installed by [`SamkhyaTableProvider::scan`] when the
//! planner asked each table provider for its scan exec. The rule does
//! not need to add wrappers itself — by the time `optimize` runs they
//! are already in place — but it does validate the wiring and surfaces
//! a `name()` so [`SessionStateBuilder::with_physical_optimizer_rule`]
//! has something to register.
//!
//! # Why the rule registers as a physical rule even though it doesn't
//! mutate
//!
//! Registering the rule against the session is the integration ceremony.
//! It is the explicit, named contract that *samkhya is in the loop*,
//! visible in `EXPLAIN VERBOSE` output and in the
//! `SessionState::physical_optimizers()` slice. The rule's `name()` is
//! `samkhya_cardinality_correction` for that telemetry. Whether or not
//! the rule physically rewrites the plan on any given query, its
//! presence in the optimizer chain is what an operator audits to confirm
//! samkhya is wired in.
//!
//! This is the cold-start-safe posture required by samkhya's design: the
//! rule cannot make plans worse, only equal-or-better.
//!
//! [`OptimizerRule`]: datafusion::optimizer::OptimizerRule
//! [`PhysicalOptimizerRule`]: datafusion::physical_optimizer::PhysicalOptimizerRule
//! [`TableProvider::statistics()`]: datafusion::datasource::TableProvider::statistics
//! [`ExecutionPlan`]: datafusion::physical_plan::ExecutionPlan
//! [`SamkhyaStatsExec`]: crate::physical_plan::SamkhyaStatsExec
//! [`SamkhyaTableProvider::scan`]: crate::SamkhyaTableProvider::scan
//! [`SessionStateBuilder::with_physical_optimizer_rule`]: datafusion::execution::session_state::SessionStateBuilder::with_physical_optimizer_rule

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use datafusion::common::Result;
use datafusion::common::config::ConfigOptions;
use datafusion::common::tree_node::{Transformed, TreeNode, TreeNodeRecursion};
use datafusion::logical_expr::LogicalPlan;
use datafusion::optimizer::optimizer::{ApplyOrder, OptimizerConfig, OptimizerRule};
use datafusion::physical_optimizer::PhysicalOptimizerRule;
use datafusion::physical_plan::ExecutionPlan;
use samkhya_core::stats::ColumnStats;

use crate::physical_plan::SamkhyaStatsExec;
use crate::stats_provider::to_datafusion_column_statistics;

/// DataFusion adapter rule that bridges samkhya's corrected statistics
/// into the optimizer.
///
/// Implements both [`OptimizerRule`] (logical observe-only) and
/// [`PhysicalOptimizerRule`] (physical validator). The physical pass is
/// the visible integration point; the actual scan-wrapping that makes
/// `physical.statistics()?.num_rows` reflect samkhya's corrections is
/// done by [`crate::SamkhyaTableProvider::scan`] at plan-construction
/// time.
///
/// Register with both
/// [`SessionStateBuilder::with_optimizer_rule`] and
/// [`SessionStateBuilder::with_physical_optimizer_rule`].
///
/// [`SessionStateBuilder::with_optimizer_rule`]: datafusion::execution::session_state::SessionStateBuilder::with_optimizer_rule
/// [`SessionStateBuilder::with_physical_optimizer_rule`]: datafusion::execution::session_state::SessionStateBuilder::with_physical_optimizer_rule
#[derive(Debug, Default)]
pub struct SamkhyaOptimizerRule {
    /// Number of `SamkhyaStatsExec` wrappers observed on the most recent
    /// physical-optimize pass. Exposed for tests / diagnostics; not part
    /// of the public optimization contract.
    samkhya_leaves_seen: AtomicUsize,
}

impl Clone for SamkhyaOptimizerRule {
    fn clone(&self) -> Self {
        Self {
            samkhya_leaves_seen: AtomicUsize::new(self.samkhya_leaves_seen.load(Ordering::SeqCst)),
        }
    }
}

impl SamkhyaOptimizerRule {
    /// Create a new rule with zeroed counters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap in an `Arc` for registration with `SessionStateBuilder`.
    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Number of `SamkhyaStatsExec` wrappers seen by the most recent
    /// physical-optimize pass.
    ///
    /// Useful in tests to confirm the physical pass actually observed
    /// the wrappers installed by `SamkhyaTableProvider::scan`.
    pub fn samkhya_leaves_seen(&self) -> usize {
        self.samkhya_leaves_seen.load(Ordering::SeqCst)
    }
}

// -----------------------------------------------------------------------
// Logical-plan side: observe-only walk.
// -----------------------------------------------------------------------

impl OptimizerRule for SamkhyaOptimizerRule {
    fn name(&self) -> &str {
        "samkhya_cardinality_correction"
    }

    fn apply_order(&self) -> Option<ApplyOrder> {
        Some(ApplyOrder::BottomUp)
    }

    fn supports_rewrite(&self) -> bool {
        true
    }

    fn rewrite(
        &self,
        plan: LogicalPlan,
        _config: &dyn OptimizerConfig,
    ) -> Result<Transformed<LogicalPlan>> {
        // Walk the logical plan and exercise the corrected-stats helper
        // for every TableScan we find. This is observe-only — the actual
        // injection happens at the physical layer via `SamkhyaStatsExec`.
        let mut scan_count = 0usize;
        plan.apply(|node| {
            if let LogicalPlan::TableScan(scan) = node {
                scan_count += 1;
                let n_cols = scan.projected_schema.fields().len();
                for col_idx in 0..n_cols {
                    let corrected = compute_corrected_stats(&scan.table_name.to_string(), col_idx);
                    let _df_stats = to_datafusion_column_statistics(&corrected);
                }
            }
            Ok(TreeNodeRecursion::Continue)
        })?;

        let _ = scan_count;
        Ok(Transformed::no(plan))
    }
}

// -----------------------------------------------------------------------
// Physical-plan side: the injection path.
// -----------------------------------------------------------------------

impl PhysicalOptimizerRule for SamkhyaOptimizerRule {
    fn name(&self) -> &str {
        "samkhya_cardinality_correction"
    }

    fn schema_check(&self) -> bool {
        // The wrappers installed by `SamkhyaTableProvider::scan` are
        // passthrough: they do not change the output schema, only the
        // statistics. Keep the schema invariant on.
        true
    }

    fn optimize(
        &self,
        plan: Arc<dyn ExecutionPlan>,
        _config: &ConfigOptions,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        // Walk the physical tree and count `SamkhyaStatsExec` wrappers.
        // The wrappers are installed up-front by
        // `SamkhyaTableProvider::scan`; this pass validates the wiring
        // and exposes a count for diagnostics.
        //
        // We deliberately do not mutate the plan here: replanting
        // wrappers post hoc would either duplicate them (if naive) or
        // require a registry of which scans are samkhya-managed (the
        // logical-plan TableScan node carries no link back to the
        // TableProvider after `source_as_provider`). The scan-time
        // wrap-on-return path is the reliable mechanism.
        let mut seen = 0usize;
        plan.apply(|node| {
            if node.as_any().downcast_ref::<SamkhyaStatsExec>().is_some() {
                seen += 1;
            }
            Ok(TreeNodeRecursion::Continue)
        })?;
        self.samkhya_leaves_seen.store(seen, Ordering::SeqCst);
        Ok(plan)
    }
}

/// Placeholder for the Puffin-backed cardinality correction lookup.
///
/// In the production wiring this will:
/// 1. Resolve the table to its Iceberg/Parquet location.
/// 2. Locate the companion Puffin sidecar.
/// 3. Read the relevant blob (HLL / theta / bloom) for `col_idx`.
/// 4. Apply the LpBound-clamped, feedback-driven correction.
///
/// For the scaffold it returns fake-but-typed stats so the integration
/// surface compiles and runs end-to-end.
pub fn compute_corrected_stats(_table: &str, _col_idx: usize) -> ColumnStats {
    ColumnStats::new()
        .with_row_count(1_000)
        .with_distinct_count(100)
        .with_null_count(0)
        .with_upper_bound(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_has_stable_name() {
        let r = SamkhyaOptimizerRule::new();
        assert_eq!(
            <SamkhyaOptimizerRule as OptimizerRule>::name(&r),
            "samkhya_cardinality_correction"
        );
        assert_eq!(
            <SamkhyaOptimizerRule as PhysicalOptimizerRule>::name(&r),
            "samkhya_cardinality_correction"
        );
        assert!(r.supports_rewrite());
        assert!(matches!(r.apply_order(), Some(ApplyOrder::BottomUp)));
    }

    #[test]
    fn placeholder_stats_are_populated() {
        let s = compute_corrected_stats("t", 0);
        assert_eq!(s.row_count, Some(1_000));
        assert_eq!(s.distinct_count, Some(100));
        assert_eq!(s.upper_bound_rows, Some(10_000));
    }

    #[test]
    fn leaves_seen_starts_at_zero() {
        let r = SamkhyaOptimizerRule::new();
        assert_eq!(r.samkhya_leaves_seen(), 0);
    }
}
