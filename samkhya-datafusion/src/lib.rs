//! samkhya-datafusion — DataFusion adapter for samkhya-core.
//!
//! First integration target. Implements `OptimizerRule` and consumes
//! `ColumnStatistics` to inject Puffin-backed estimates into the DataFusion
//! optimizer.
