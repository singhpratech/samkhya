//! samkhya-polars — Polars adapter for samkhya.
//!
//! # Status
//!
//! Scaffolding. Polars has no public optimizer-rule extension API yet
//! (tracked upstream in [pola-rs/polars#23345][issue]), so the
//! integration model differs from `samkhya-datafusion`'s
//! `TableProvider` wrapper.
//!
//! [issue]: https://github.com/pola-rs/polars/issues/23345
//!
//! # Planned integration patterns
//!
//! - **Stats sidecar consumer**: load Polars `DataFrame` plus a Puffin
//!   sidecar built by samkhya-core; expose helper functions that
//!   inspect a `LazyFrame` plan and return corrected cardinality hints.
//! - **Feedback wrapper**: wrap `LazyFrame::collect()` to capture
//!   estimated vs actual row counts into a `FeedbackStore`.
//! - **Sketch-from-Series builder**: pure-Rust helpers that build HLL
//!   / Bloom / Count-Min / EquiDepthHistogram sketches directly from a
//!   `polars::Series`, then serialize via the `Sketch` trait.
//!
//! Once Polars exposes optimizer hooks, this crate will gain a real
//! injection point comparable to `SamkhyaTableProvider`.

use samkhya_core::Result;
use samkhya_core::stats::ColumnStats;

/// Placeholder accessor for a future Polars-side stats provider.
///
/// Returns the column statistics that samkhya would inject into a
/// Polars `LazyFrame` plan once Polars exposes an optimizer rule API.
pub fn column_stats_for(_table: &str, _col: &str) -> Result<Option<ColumnStats>> {
    Ok(None)
}

/// Build samkhya sketches from a Polars `Series`.
///
/// Stubbed until the Polars dependency lands; the signature is shaped
/// to match how callers will eventually consume the helper.
pub fn build_sketches_from_series_stub() {
    // Intentionally a no-op until Polars is wired in.
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
