//! samkhya-datafusion — DataFusion adapter for samkhya-core.
//!
//! First integration target. Implements [`OptimizerRule`] and consumes
//! [`ColumnStatistics`] to inject Puffin-backed estimates into the DataFusion
//! optimizer.
//!
//! # Compatibility
//!
//! Compiled and tested against **DataFusion 46.0** (released March 2025).
//! Version 46 is the first release with a stable `OptimizerRule` trait surface
//! (`name`, `apply_order`, `supports_rewrite`, `rewrite`) and the
//! `Precision<T>` / `ColumnStatistics` types we depend on for cardinality
//! correction. Newer versions (47/48) should also work; if a future bump
//! breaks the trait shape, update the `rewrite` signature accordingly.
//!
//! [`OptimizerRule`]: datafusion::optimizer::OptimizerRule
//! [`ColumnStatistics`]: datafusion::common::ColumnStatistics

pub mod optimizer_rule;
pub mod stats_provider;

pub use optimizer_rule::SamkhyaOptimizerRule;
pub use stats_provider::to_datafusion_column_statistics;
