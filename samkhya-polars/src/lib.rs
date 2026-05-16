//! samkhya-polars — Polars adapter for samkhya.
//!
//! # Status
//!
//! Polars has no public optimizer-rule extension API yet (tracked
//! upstream in [pola-rs/polars#23345][issue]), so the integration model
//! differs from `samkhya-datafusion`'s `TableProvider` wrapper. Today
//! this crate ships two pieces, both gated behind the `engine` feature
//! flag (off by default to keep workspace builds lean):
//!
//! - [`sketcher`] — pure-Rust helpers that consume a `polars::Series`
//!   and produce HLL / Bloom / Count-Min / EquiDepthHistogram sketches
//!   from `samkhya-core`, ready to serialize via the `Sketch` trait into
//!   an Iceberg Puffin sidecar.
//! - [`feedback_wrapper`] — a thin wrapper around `LazyFrame::collect()`
//!   that records `(template_hash, est_rows, actual_rows, latency_ms)`
//!   observations into a `samkhya_core::feedback::FeedbackStore`. The
//!   estimated row count is set to `0` because Polars does not expose
//!   plan-level row estimates through its public API at this version.
//!
//! [issue]: https://github.com/pola-rs/polars/issues/23345
//!
//! Once Polars exposes optimizer hooks, this crate will gain a real
//! injection point comparable to `SamkhyaTableProvider` in
//! `samkhya-datafusion`.

#[cfg(feature = "engine")]
pub mod feedback_wrapper;
#[cfg(feature = "engine")]
pub mod sketcher;

use samkhya_core::Result;
use samkhya_core::stats::ColumnStats;

/// Placeholder accessor for a future Polars-side stats provider.
///
/// Returns the column statistics that samkhya would inject into a
/// Polars `LazyFrame` plan once Polars exposes an optimizer rule API.
pub fn column_stats_for(_table: &str, _col: &str) -> Result<Option<ColumnStats>> {
    Ok(None)
}

/// Legacy stub kept for backwards compatibility with crates that
/// pre-date the `engine` feature flag.
///
/// Real Series → Sketch helpers live in [`sketcher`] behind the
/// `engine` feature.
pub fn build_sketches_from_series_stub() {
    // Intentionally a no-op; see `sketcher` for the real builders.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_stats_returns_none() {
        assert!(column_stats_for("t", "c").unwrap().is_none());
    }

    #[test]
    fn sketch_builder_is_callable() {
        build_sketches_from_series_stub();
    }
}
