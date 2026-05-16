//! samkhya-postgres — PostgreSQL adapter for samkhya.
//!
//! # Status
//!
//! Scaffolding. The PostgreSQL integration shape is well-trodden — the
//! prior art is [`postgrespro/aqo`][aqo], which hooks into the planner
//! and executor to capture `(plan, estimated_rows, actual_rows)` tuples
//! after each query and feed them back into selectivity estimates. The
//! `pg_qualstats` and `pg_hint_plan` extensions illustrate the two
//! companion knobs: per-qual statistics collection and explicit hint
//! injection.
//!
//! [aqo]: https://github.com/postgrespro/aqo
//!
//! # Planned integration patterns
//!
//! - **PG extension via [pgrx]**: build samkhya as a first-class
//!   PostgreSQL extension that registers `planner_hook` and
//!   `ExecutorEnd_hook` callbacks. The planner hook injects samkhya's
//!   corrected cardinalities into `RelOptInfo::rows`; the executor hook
//!   captures actual row counts post-execution and persists them as
//!   `Observation`s in a `samkhya` schema (mirrors the
//!   `aqo_data` / `aqo_queries` table layout used by postgrespro/aqo).
//! - **libpq-driven sidecar**: for deployments that cannot load native
//!   extensions, a `tokio-postgres` sidecar process scrapes
//!   `pg_stat_statements` plus `EXPLAIN (ANALYZE, FORMAT JSON)` output
//!   and writes sketches into a samkhya-managed schema, surfaced back
//!   to the planner through `pg_hint_plan`-style hints.
//! - **Sketch storage in a `samkhya` schema**: HLL / Bloom / Count-Min
//!   / EquiDepthHistogram sketches serialized via
//!   `samkhya_core::sketches::Sketch` and stored as `bytea` rows keyed
//!   by `(table_oid, attnum)`, queryable from SQL for debugging.
//!
//! [pgrx]: https://github.com/pgcentralfoundation/pgrx
//!
//! Once the extension surface lands, this crate gains a real
//! `planner_hook` shim comparable in spirit to
//! `samkhya_datafusion::SamkhyaTableProvider`.

use samkhya_core::Result;
use samkhya_core::stats::ColumnStats;

/// Placeholder accessor for a future PostgreSQL-side stats provider.
///
/// Returns the column statistics that samkhya would inject into a
/// PostgreSQL planner pass once the pgrx-based `planner_hook` shim is
/// in place. Looks up by `(table, column)` name pair; the real
/// implementation will resolve these to `(table_oid, attnum)` via the
/// extension's catalog access.
pub fn column_stats_for(_table: &str, _col: &str) -> Result<Option<ColumnStats>> {
    Ok(None)
}

/// Install hook for the future PostgreSQL extension entry point.
///
/// Stubbed until the pgrx dependency lands; the real version will
/// register `planner_hook` and `ExecutorEnd_hook` callbacks against
/// the running PostgreSQL backend and create the `samkhya` schema
/// with the sketch / observation tables on first load.
pub fn install_extension_stub() {
    // Intentionally a no-op until the pgrx integration is wired in.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_stats_returns_none() {
        assert!(column_stats_for("t", "c").unwrap().is_none());
    }

    #[test]
    fn install_hook_is_callable() {
        install_extension_stub();
    }
}
