//! samkhya-datafusion — DataFusion adapter for samkhya-core.
//!
//! # Integration model
//!
//! DataFusion 46.0 has two distinct surfaces that the mainline planner
//! actually consults for cardinality:
//!
//! * [`ExecutionPlan::statistics()`] on each node of the physical plan.
//!   `FilterExec`, `ProjectionExec`, `HashJoinExec` etc. propagate from
//!   their child's `statistics()` upward, so corrections placed at the
//!   leaf scan level reach the top of the plan.
//! * [`TableProvider::statistics()`] is **not** consulted by the
//!   mainline physical planner: it is reserved for downstream forks /
//!   custom optimizer rules. We still implement it for completeness, but
//!   we do not rely on it as the injection path.
//!
//! samkhya therefore wires corrections in at three layers, which together
//! form the integration model:
//!
//! 1. [`SamkhyaTableProvider`] —
//!    a `TableProvider` wrapper that delegates every method to an inner
//!    provider but overrides `statistics()` with samkhya-corrected
//!    [`ColumnStatistics`], and — critically — overrides `scan()` to
//!    return a physical [`SamkhyaStatsExec`]
//!    wrapping the inner provider's exec. The exec wrapper is what
//!    makes `physical.statistics()?.num_rows` reflect samkhya's
//!    corrections, because the mainline planner uses the exec's stats,
//!    not the table provider's.
//! 2. [`SamkhyaStatsExec`] — a
//!    passthrough [`ExecutionPlan`] that overrides `statistics()` to
//!    return a preset `Statistics`, delegating every other method to the
//!    inner exec. This is the physical-layer hook the planner actually
//!    consults.
//! 3. [`SamkhyaOptimizerRule`] —
//!    implements both `OptimizerRule` (logical, observe-only) and
//!    `PhysicalOptimizerRule` (physical, validates the wrappers are in
//!    place and surfaces a diagnostic count of `SamkhyaStatsExec`
//!    leaves seen). Registration of the rule is the explicit integration
//!    ceremony — operators audit the
//!    `SessionState::physical_optimizers()` slice to confirm samkhya is
//!    wired in.
//!
//! ```ignore
//! use std::sync::Arc;
//! use datafusion::execution::session_state::SessionStateBuilder;
//! use datafusion::execution::context::SessionContext;
//! use datafusion::prelude::SessionConfig;
//! use samkhya_datafusion::{SamkhyaOptimizerRule, SamkhyaTableProvider};
//! use samkhya_core::stats::ColumnStats;
//!
//! let rule = Arc::new(SamkhyaOptimizerRule::new());
//! let state = SessionStateBuilder::new()
//!     .with_config(SessionConfig::new())
//!     .with_default_features()
//!     .with_optimizer_rule(rule.clone())
//!     .with_physical_optimizer_rule(rule.clone())
//!     .build();
//! let ctx = SessionContext::new_with_state(state);
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
//! # Compatibility
//!
//! Compiled and tested against **DataFusion 46.0.1** (released March 2025).
//! Version 46 is the first release with a stable `OptimizerRule` trait
//! surface (`name`, `apply_order`, `supports_rewrite`, `rewrite`), the
//! `PhysicalOptimizerRule` trait, and the `Precision<T>` /
//! `ColumnStatistics` / `Statistics` types we depend on for cardinality
//! correction. Newer versions should also work, with any signature drift
//! caught by the `wrap_provider` integration test and the
//! `stats_propagation_demo` example binary.
//!
//! [`OptimizerRule`]: datafusion::optimizer::OptimizerRule
//! [`PhysicalOptimizerRule`]: datafusion::physical_optimizer::PhysicalOptimizerRule
//! [`TableProvider`]: datafusion::datasource::TableProvider
//! [`TableProvider::statistics()`]: datafusion::datasource::TableProvider::statistics
//! [`ExecutionPlan`]: datafusion::physical_plan::ExecutionPlan
//! [`ExecutionPlan::statistics()`]: datafusion::physical_plan::ExecutionPlan::statistics
//! [`ColumnStatistics`]: datafusion::common::ColumnStatistics
//! [`Precision::Inexact`]: datafusion::common::stats::Precision::Inexact

pub mod optimizer_rule;
pub mod physical_plan;
pub mod stats_provider;
pub mod table_provider;

pub use optimizer_rule::SamkhyaOptimizerRule;
pub use physical_plan::SamkhyaStatsExec;
pub use stats_provider::to_datafusion_column_statistics;
pub use table_provider::SamkhyaTableProvider;
