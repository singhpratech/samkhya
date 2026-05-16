//! TPC-H schema registration for the benchmark harness.
//!
//! Registers the 8 TPC-H tables (region, nation, customer, supplier, part,
//! partsupp, orders, lineitem) as Parquet-backed DataFusion tables. The
//! schemas are inferred from the on-disk Parquet files produced by
//! `tpchgen-cli` (or DuckDB's `dbgen` + `EXPORT DATABASE (FORMAT PARQUET)`),
//! so this module does not declare schemas explicitly — Parquet carries its
//! own.
//!
//! ## Entry points
//!
//! - [`build_tpch_context`] is the convenience constructor: it builds a
//!   fresh `SessionContext` and registers every TPC-H table from
//!   `parquet_dir`.
//! - [`register_tpch_tables`] registers TPC-H tables on a pre-built
//!   context (for callers that need to share a context with other
//!   sources).
//! - [`probe_tpch_dir`] sanity-checks `parquet_dir` for the expected
//!   files before the runner attempts a full registration. Mirrors the
//!   shape of [`crate::imdb::probe_imdb_dir`].
//!
//! ## On-disk layout
//!
//! ```text
//! <parquet_dir>/
//!   region.parquet
//!   nation.parquet
//!   customer.parquet
//!   supplier.parquet
//!   part.parquet
//!   partsupp.parquet
//!   orders.parquet
//!   lineitem.parquet
//! ```
//!
//! This is the default output of `tpchgen-cli -s 1 --format=parquet
//! --output-dir=<parquet_dir>`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use datafusion::datasource::TableProvider;
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use samkhya_core::Result;
use samkhya_core::error::Error;
use samkhya_core::puffin::PuffinReader;
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::SamkhyaTableProvider;

/// The 8 canonical TPC-H tables, in the conventional roster order
/// (small-to-large for readability; registration order is irrelevant to
/// DataFusion).
pub const TABLES: &[&str] = &[
    "region", "nation", "customer", "supplier", "part", "partsupp", "orders", "lineitem",
];

/// Build a fresh `SessionContext` with every TPC-H table registered from
/// `parquet_dir`.
///
/// Errors if `parquet_dir` does not exist or any expected `*.parquet`
/// file is missing.
pub async fn build_tpch_context(parquet_dir: &Path) -> Result<SessionContext> {
    let ctx = SessionContext::new();
    register_tpch_tables_async(&ctx, parquet_dir).await?;
    Ok(ctx)
}

/// Synchronous wrapper around [`register_tpch_tables_async`] for callers
/// that already own a runtime entry point (mirrors the shape of
/// [`crate::imdb::register_imdb_tables`]).
pub fn register_tpch_tables(ctx: &SessionContext, parquet_dir: &Path) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::from)?;
    rt.block_on(register_tpch_tables_async(ctx, parquet_dir))
}

/// Register every TPC-H table on the given context. Each table is read
/// as Parquet with default options — Parquet's embedded schema is the
/// source of truth.
///
/// After the default `ListingTable` is registered, this function looks
/// for a samkhya stats sidecar at `<parquet_dir>/<table>.puffin`. If
/// found, the listing provider is deregistered, wrapped in
/// [`SamkhyaTableProvider`] with the sidecar's per-column distinct
/// counts, and re-registered — so samkhya stats flow into DataFusion's
/// optimizer statistics path on subsequent planning. If absent, the
/// default listing provider is left in place and a one-line INFO notice
/// is printed so operators know which tables fell back.
pub async fn register_tpch_tables_async(ctx: &SessionContext, parquet_dir: &Path) -> Result<()> {
    if !parquet_dir.exists() {
        return Err(Error::Feedback(format!(
            "tpch: parquet_dir {} does not exist",
            parquet_dir.display()
        )));
    }
    for &table in TABLES {
        let path = parquet_dir.join(format!("{table}.parquet"));
        if !path.exists() {
            return Err(Error::Feedback(format!(
                "tpch: missing {} (expected at {})",
                table,
                path.display()
            )));
        }
        let opts = ParquetReadOptions::default();
        ctx.register_parquet(table, path.to_string_lossy().as_ref(), opts)
            .await
            .map_err(df_err)?;

        // Optional: wrap with SamkhyaTableProvider when a sidecar exists.
        // Falls back gracefully when there is no `<table>.puffin` next to
        // the Parquet file.
        let sidecar = parquet_dir.join(format!("{table}.puffin"));
        if !sidecar.exists() {
            // INFO-level note. We use `println!` here because samkhya-bench
            // does not pull in a logging crate; the rest of the harness
            // emits its progress on stdout the same way.
            println!(
                "[tpch] no samkhya stats sidecar for table {}, falling back to default",
                table
            );
            continue;
        }
        match wrap_table_with_sidecar(ctx, table, &sidecar).await {
            Ok(()) => {}
            Err(e) => {
                // Wrapping failed (corrupt sidecar, schema mismatch, etc.):
                // keep the default ListingTable, do not error the whole
                // registration. This preserves the "fall back gracefully"
                // contract from the task spec.
                println!(
                    "[tpch] failed to wrap table {} with samkhya sidecar ({}), falling back to default",
                    table, e
                );
            }
        }
    }
    Ok(())
}

/// Deregister `table` from `ctx`, wrap its provider in a
/// [`SamkhyaTableProvider`] populated from the Puffin sidecar at
/// `sidecar_path`, then re-register the wrapped provider under the same
/// name. The sidecar is expected to contain one HLL blob per column,
/// matching the layout produced by [`crate::puffin_io`].
async fn wrap_table_with_sidecar(
    ctx: &SessionContext,
    table: &str,
    sidecar_path: &Path,
) -> Result<()> {
    let overrides = load_table_sidecar(sidecar_path)?;

    // Pull the previously registered ListingTable provider out of the
    // context, wrap it, and re-register under the same name.
    let inner: Arc<dyn TableProvider> = ctx.table_provider(table).await.map_err(df_err)?;

    let row_count = overrides.iter().filter_map(|(_, s)| s.row_count).max();
    let n_fields = inner.schema().fields().len();

    let mut wrapper = SamkhyaTableProvider::new(inner);
    for (col_idx, stats) in overrides {
        if col_idx >= n_fields {
            continue;
        }
        let merged = match row_count {
            Some(rc) => stats.with_row_count(rc),
            None => stats,
        };
        wrapper = wrapper.with_column_stats(col_idx, merged);
    }
    let wrapped: Arc<dyn TableProvider> = Arc::new(wrapper);

    // deregister returns the old provider; we drop it.
    ctx.deregister_table(table).map_err(df_err)?;
    ctx.register_table(table, wrapped).map_err(df_err)?;
    Ok(())
}

/// Load a single TPC-H Puffin sidecar and return per-column
/// `(field_index, ColumnStats)` overrides. Mirrors the loop in
/// `puffin_io::load_column_stats_from_sidecars` but works for a single
/// arbitrary sidecar path (the bench-level helper is hard-wired to the
/// synthetic table names).
fn load_table_sidecar(path: &Path) -> Result<Vec<(usize, ColumnStats)>> {
    let file = File::open(path)?;
    let mut reader = PuffinReader::open(file)?;
    let mut overrides = Vec::new();
    let metas = reader.blobs().to_vec();
    for (i, meta) in metas.iter().enumerate() {
        if meta.kind != HllSketch::KIND {
            continue;
        }
        let payload = reader.read_blob(i)?;
        let hll = HllSketch::from_bytes(&payload)?;
        let distinct = hll.estimate();
        for field_idx in &meta.fields {
            overrides.push((
                *field_idx as usize,
                ColumnStats::new().with_distinct_count(distinct),
            ));
        }
    }
    Ok(overrides)
}

/// Probe `parquet_dir` for a TPC-H dump. Returns `Ok(())` if every
/// expected `*.parquet` file is present; otherwise a descriptive
/// `Error`. Lighter-weight than [`register_tpch_tables`]: no
/// context is built and no schema is opened.
pub fn probe_tpch_dir(parquet_dir: &Path) -> Result<()> {
    if !parquet_dir.exists() {
        return Err(Error::Feedback(format!(
            "tpch: directory does not exist: {}",
            parquet_dir.display()
        )));
    }
    let mut missing = Vec::new();
    for &t in TABLES {
        let p = parquet_dir.join(format!("{t}.parquet"));
        if !p.exists() {
            missing.push(t);
        }
    }
    if !missing.is_empty() {
        return Err(Error::Feedback(format!(
            "tpch: missing tables under {}: {}",
            parquet_dir.display(),
            missing.join(", ")
        )));
    }
    Ok(())
}

/// Where the runner expects the TPC-H dump to live by default. Callers
/// may override via `--tpch-dir`.
pub fn default_tpch_dir() -> PathBuf {
    PathBuf::from("samkhya-bench/data/tpch")
}

fn df_err(e: impl std::fmt::Display) -> Error {
    Error::Feedback(format!("datafusion: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_roster_is_eight() {
        assert_eq!(TABLES.len(), 8);
    }

    #[test]
    fn probe_rejects_missing_dir() {
        let bogus = PathBuf::from("/tmp/samkhya-bench-no-such-tpch-dir-xyzzy");
        assert!(probe_tpch_dir(&bogus).is_err());
    }
}
