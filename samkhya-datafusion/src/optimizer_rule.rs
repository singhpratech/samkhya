//! `SamkhyaOptimizerRule` — the DataFusion `OptimizerRule` integration point.
//!
//! For this scaffold the rule walks the `LogicalPlan`, locates every
//! `TableScan`, and invokes [`compute_corrected_stats`] to obtain a typed
//! `ColumnStatistics` value per scanned column. The actual Puffin-sidecar
//! read path is a separate task; today the helper returns plausible
//! placeholder stats so the integration surface is exercised end-to-end.
//!
//! The rule is intentionally non-mutating: it observes the plan and returns
//! `Transformed::no(plan)`. Once the cardinality-correction wiring lands
//! (rewriting `TableScan::source` to a wrapped `TableSource` that publishes
//! corrected stats), the same traversal will switch to `Transformed::yes`.
//!
//! This is the cold-start-safe posture required by samkhya's design: the
//! rule cannot make plans worse, only equal-or-better.

use std::sync::Arc;

use datafusion::common::Result;
use datafusion::common::tree_node::{Transformed, TreeNode};
use datafusion::logical_expr::LogicalPlan;
use datafusion::optimizer::optimizer::{ApplyOrder, OptimizerConfig, OptimizerRule};
use samkhya_core::stats::ColumnStats;

use crate::stats_provider::to_datafusion_column_statistics;

/// DataFusion `OptimizerRule` that injects samkhya's corrected column
/// statistics into the optimizer.
///
/// Register with `SessionStateBuilder::with_optimizer_rule(Arc::new(...))`.
#[derive(Debug, Default, Clone)]
pub struct SamkhyaOptimizerRule;

impl SamkhyaOptimizerRule {
    /// Create a new rule with default configuration.
    pub fn new() -> Self {
        Self
    }

    /// Wrap in an `Arc` for registration with `SessionStateBuilder`.
    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

impl OptimizerRule for SamkhyaOptimizerRule {
    fn name(&self) -> &str {
        "samkhya_cardinality_correction"
    }

    fn apply_order(&self) -> Option<ApplyOrder> {
        // We want to inspect every node; do a bottom-up traversal so any
        // future rewrites that depend on a corrected child's stats see them.
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
        // BottomUp apply_order already drives recursion, but we also accept
        // being called on a root with `apply_order: None` semantics by doing
        // a one-shot walk here. Either way, observing each TableScan is
        // idempotent.
        let mut scan_count = 0usize;
        plan.apply(|node| {
            if let LogicalPlan::TableScan(scan) = node {
                scan_count += 1;
                // Pull a placeholder corrected `ColumnStats` per projected
                // column and translate it into DataFusion's surface. The
                // result is discarded until the rewrite path lands.
                let n_cols = scan.projected_schema.fields().len();
                for col_idx in 0..n_cols {
                    let corrected = compute_corrected_stats(&scan.table_name.to_string(), col_idx);
                    let _df_stats = to_datafusion_column_statistics(&corrected);
                }
            }
            Ok(::datafusion::common::tree_node::TreeNodeRecursion::Continue)
        })?;

        // No transformation yet — the rule is in observe-only mode.
        let _ = scan_count;
        Ok(Transformed::no(plan))
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
        assert_eq!(r.name(), "samkhya_cardinality_correction");
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
}
