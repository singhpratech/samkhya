//! samkhya-datafusion — DataFusion adapter for samkhya-core.
//!
//! # Integration model
//!
//! DataFusion attaches statistics to **table providers**, not to logical-plan
//! nodes, so the primary integration point is a
//! [`SamkhyaTableProvider`](table_provider::SamkhyaTableProvider): a wrapper
//! that delegates every [`TableProvider`] method to an inner provider but
//! overrides `statistics()` to fold in samkhya-corrected
//! [`ColumnStatistics`]. The planner consults `statistics()` during
//! analysis, so the corrections reach the cost-based optimizer through the
//! engine's own stable surface — no fork of DataFusion, no rewriting of
//! `TableScan` nodes.
//!
//! ```ignore
//! use std::sync::Arc;
//! use samkhya_datafusion::SamkhyaTableProvider;
//! use samkhya_core::stats::ColumnStats;
//!
//! let wrapped = SamkhyaTableProvider::new(inner_provider)
//!     .with_column_stats(0, ColumnStats::new().with_row_count(1_000_000));
//! ctx.register_table("t", Arc::new(wrapped))?;
//! ```
//!
//! All values translated into DataFusion's `Precision<T>` are marked
//! [`Precision::Inexact`] — samkhya's corrections are feedback-driven,
//! clamped by the LpBound pessimistic ceiling, and never exact catalog
//! counts. This is the conservative posture the safety envelope requires.
//!
//! # Observe-only telemetry
//!
//! The [`SamkhyaOptimizerRule`] also walks the `LogicalPlan` and visits
//! every `TableScan`, but it is **not** the injection path. The rule is
//! retained as observe-only telemetry: it counts scans, exercises the
//! corrected-stats helper, and returns `Transformed::no(plan)`. Use it
//! when you want a hook into the optimizer pass without changing the
//! plan; use [`SamkhyaTableProvider`](table_provider::SamkhyaTableProvider)
//! when you want corrections to reach the planner.
//!
//! # Compatibility
//!
//! Compiled and tested against **DataFusion 46.0** (released March 2025).
//! Version 46 is the first release with a stable `OptimizerRule` trait
//! surface (`name`, `apply_order`, `supports_rewrite`, `rewrite`) and the
//! `Precision<T>` / `ColumnStatistics` / `Statistics` types we depend on
//! for cardinality correction. The `TableProvider::statistics()` hook has
//! the signature `fn statistics(&self) -> Option<Statistics>` in 46;
//! newer versions should also work, with any signature drift caught by
//! the `wrap_provider` integration test.
//!
//! [`OptimizerRule`]: datafusion::optimizer::OptimizerRule
//! [`TableProvider`]: datafusion::datasource::TableProvider
//! [`ColumnStatistics`]: datafusion::common::ColumnStatistics
//! [`Precision::Inexact`]: datafusion::common::stats::Precision::Inexact

pub mod optimizer_rule;
pub mod physical_plan;
pub mod stats_provider;
pub mod table_provider;

pub use optimizer_rule::SamkhyaOptimizerRule;
pub use stats_provider::to_datafusion_column_statistics;
pub use table_provider::SamkhyaTableProvider;
