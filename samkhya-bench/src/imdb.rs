//! IMDb schema registration for the Join Order Benchmark (JOB).
//!
//! Registers the 21 IMDb tables shipped with the JOB CSV dump as DataFusion
//! tables. The schema mirrors the PostgreSQL-formatted dump from
//! <https://homepages.cwi.nl/~boncz/job/imdb.tgz>, with column orderings
//! taken from the canonical schema at
//! <https://github.com/winkyao/join-order-benchmark> (file
//! `schema.sql`).
//!
//! ## Entry points
//!
//! - [`register_imdb_tables`] is the single function callers use. It picks
//!   the source format automatically: if `csv_dir/parquet/` exists, the
//!   pre-Parquet'd files are read from there; otherwise the raw `*.csv`
//!   files under `csv_dir` are read.
//! - [`imdb_schemas`] returns the table-name → Arrow schema map. Useful
//!   for sanity-checking the on-disk files before running queries.
//!
//! ## On declared schemas
//!
//! IMDb CSV files have no header row and use the column orderings declared
//! in `schema.sql`. We supply explicit schemas via `CsvReadOptions::schema`
//! so DataFusion does not have to infer column types from a multi-GB scan,
//! and so downstream cardinality estimates land on stable types regardless
//! of how the CSV happens to be sampled.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::datasource::TableProvider;
use datafusion::prelude::{CsvReadOptions, ParquetReadOptions, SessionContext};
use samkhya_core::Result;
use samkhya_core::error::Error;
use samkhya_core::puffin::PuffinReader;
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::SamkhyaTableProvider;

/// All 21 IMDb tables referenced by the JOB query corpus, in the order
/// matching the canonical `schema.sql`.
pub const TABLES: &[&str] = &[
    "aka_name",
    "aka_title",
    "cast_info",
    "char_name",
    "comp_cast_type",
    "company_name",
    "company_type",
    "complete_cast",
    "info_type",
    "keyword",
    "kind_type",
    "link_type",
    "movie_companies",
    "movie_info",
    "movie_info_idx",
    "movie_keyword",
    "movie_link",
    "name",
    "person_info",
    "role_type",
    "title",
];

/// Register every IMDb table on the given context.
///
/// Resolution order for each table `T`:
///
/// 1. If `csv_dir/parquet/T.parquet` exists, register it as Parquet.
/// 2. Else if `csv_dir/T.csv` exists, register it as a header-less CSV
///    with the declared schema from [`imdb_schemas`].
/// 3. Else return an error naming the missing file.
///
/// The CSV reader is configured with the IMDb dump's quirks:
/// `has_header = false`, escape `\\`, no header row, comma-separated, and
/// values containing newlines (a few `movie_info.note` rows trip this).
///
/// This is the synchronous (no-runtime-active) entry. If a tokio runtime
/// is already active (e.g. inside `Runner::run_async`), call
/// [`register_imdb_tables_async`] instead — nesting a second runtime
/// panics in tokio.
pub fn register_imdb_tables(ctx: &SessionContext, csv_dir: &Path) -> Result<()> {
    if !csv_dir.exists() {
        return Err(Error::Feedback(format!(
            "imdb: csv_dir {} does not exist",
            csv_dir.display()
        )));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::from)?;
    rt.block_on(register_imdb_tables_async(ctx, csv_dir))
}

/// Async variant of [`register_imdb_tables`]; safe to call from inside an
/// already-active tokio runtime (the runner's `run_async` path).
///
/// Wraps every registered table with a `SamkhyaTableProvider` when a
/// `<csv_dir>/<table>.puffin` sidecar exists. Pass `baseline=true` to
/// skip the wrapping entirely (the "native DataFusion 46" head-to-head
/// arm — sidecars are still expected on disk but ignored).
pub async fn register_imdb_tables_async(ctx: &SessionContext, csv_dir: &Path) -> Result<()> {
    register_imdb_tables_async_with_baseline(ctx, csv_dir, false).await
}

/// Variant of [`register_imdb_tables_async`] that respects a `baseline`
/// flag: when `baseline=true`, sidecars are NOT applied and the
/// registered providers are the unmodified DataFusion ListingTable
/// providers (the head-to-head's native-DataFusion arm).
pub async fn register_imdb_tables_async_with_baseline(
    ctx: &SessionContext,
    csv_dir: &Path,
    baseline: bool,
) -> Result<()> {
    if !csv_dir.exists() {
        return Err(Error::Feedback(format!(
            "imdb: csv_dir {} does not exist",
            csv_dir.display()
        )));
    }
    let parquet_dir = csv_dir.join("parquet");
    let prefer_parquet = parquet_dir.exists();
    let schemas = imdb_schemas();
    for &table in TABLES {
        let schema = schemas
            .get(table)
            .ok_or_else(|| Error::Feedback(format!("imdb: no schema for {table}")))?;

        let parquet_path = parquet_dir.join(format!("{table}.parquet"));
        let csv_path = csv_dir.join(format!("{table}.csv"));

        if prefer_parquet && parquet_path.exists() {
            let opts = ParquetReadOptions::default();
            ctx.register_parquet(table, parquet_path.to_string_lossy().as_ref(), opts)
                .await
                .map_err(df_err)?;
        } else if csv_path.exists() {
            let opts = CsvReadOptions::new()
                .has_header(false)
                .escape(b'\\')
                .delimiter(b',')
                .schema(schema)
                .newlines_in_values(true);
            ctx.register_csv(table, csv_path.to_string_lossy().as_ref(), opts)
                .await
                .map_err(df_err)?;
        } else {
            return Err(Error::Feedback(format!(
                "imdb: missing source for {table} (looked at {} and {})",
                parquet_path.display(),
                csv_path.display()
            )));
        }

        // baseline=true short-circuits the wrapping entirely so the
        // "native DataFusion 46" arm in the head-to-head sees no samkhya
        // stats injection.
        if baseline {
            continue;
        }
        // Optional: wrap with SamkhyaTableProvider when a sidecar exists.
        // Mirrors `samkhya-bench/src/tpch.rs::register_tpch_tables_async`'s
        // graceful-fallback pattern: corrupt sidecars do not error the whole
        // registration, they fall back to the default ListingTable.
        let sidecar = csv_dir.join(format!("{table}.puffin"));
        if !sidecar.exists() {
            println!(
                "[imdb] no samkhya stats sidecar for table {}, falling back to default",
                table
            );
            continue;
        }
        match wrap_imdb_table_with_sidecar(ctx, table, &sidecar).await {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "[imdb] failed to wrap table {} with samkhya sidecar ({}), falling back to default",
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
/// name. The sidecar layout is one HLL blob per column (column index in
/// the blob metadata's `fields[0]`), as produced by `build-puffin
/// --imdb-dir`.
async fn wrap_imdb_table_with_sidecar(
    ctx: &SessionContext,
    table: &str,
    sidecar_path: &Path,
) -> Result<()> {
    let overrides = load_imdb_table_sidecar(sidecar_path)?;
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
    ctx.deregister_table(table).map_err(df_err)?;
    ctx.register_table(table, wrapped).map_err(df_err)?;
    Ok(())
}

/// Load a single IMDb Puffin sidecar and return per-column
/// `(field_index, ColumnStats)` overrides. Mirrors the loop in
/// `tpch::load_table_sidecar` but the sidecar may carry an additional
/// `row_count` HLL-marker blob (kind = `"samkhya.imdb.row_count"`) whose
/// integer payload is stamped into every column's `row_count` field
/// downstream.
fn load_imdb_table_sidecar(path: &Path) -> Result<Vec<(usize, ColumnStats)>> {
    let file = File::open(path)?;
    let mut reader = PuffinReader::open(file)?;
    let mut overrides: Vec<(usize, ColumnStats)> = Vec::new();
    let mut row_count: Option<u64> = None;
    let metas = reader.blobs().to_vec();
    // First pass: pick up the row-count marker if present.
    for (i, meta) in metas.iter().enumerate() {
        if meta.kind == ROW_COUNT_KIND {
            let payload = reader.read_blob(i)?;
            if payload.len() >= 8 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&payload[..8]);
                row_count = Some(u64::from_le_bytes(buf));
            }
        }
    }
    // Second pass: collect HLL-based distinct counts per column.
    for (i, meta) in metas.iter().enumerate() {
        if meta.kind != HllSketch::KIND {
            continue;
        }
        let payload = reader.read_blob(i)?;
        let hll = HllSketch::from_bytes(&payload)?;
        let distinct = hll.estimate();
        for field_idx in &meta.fields {
            let mut s = ColumnStats::new().with_distinct_count(distinct);
            if let Some(rc) = row_count {
                s = s.with_row_count(rc);
            }
            overrides.push((*field_idx as usize, s));
        }
    }
    Ok(overrides)
}

/// Custom blob kind used by `build-puffin --imdb-dir` to stamp the
/// table-level row count into the sidecar. Eight-byte little-endian u64
/// payload. Kept inline (rather than promoting into `samkhya-core`) so the
/// v1.0 core surface stays frozen.
pub const ROW_COUNT_KIND: &str = "samkhya.imdb.row_count";

/// Map of table name → Arrow schema for the JOB IMDb dump.
///
/// Column orderings mirror `schema.sql` from the upstream JOB repo. Numeric
/// keys are `Int32` (they fit) and free-form text is `Utf8` with nullability
/// where the dump permits empty fields.
pub fn imdb_schemas() -> HashMap<&'static str, Schema> {
    let mut m = HashMap::new();

    m.insert(
        "aka_name",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("person_id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
            Field::new("imdb_index", DataType::Utf8, true),
            Field::new("name_pcode_cf", DataType::Utf8, true),
            Field::new("name_pcode_nf", DataType::Utf8, true),
            Field::new("surname_pcode", DataType::Utf8, true),
            Field::new("md5sum", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "aka_title",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("title", DataType::Utf8, true),
            Field::new("imdb_index", DataType::Utf8, true),
            Field::new("kind_id", DataType::Int32, true),
            Field::new("production_year", DataType::Int32, true),
            Field::new("phonetic_code", DataType::Utf8, true),
            Field::new("episode_of_id", DataType::Int32, true),
            Field::new("season_nr", DataType::Int32, true),
            Field::new("episode_nr", DataType::Int32, true),
            Field::new("note", DataType::Utf8, true),
            Field::new("md5sum", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "cast_info",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("person_id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("person_role_id", DataType::Int32, true),
            Field::new("note", DataType::Utf8, true),
            Field::new("nr_order", DataType::Int32, true),
            Field::new("role_id", DataType::Int32, false),
        ]),
    );
    m.insert(
        "char_name",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("imdb_index", DataType::Utf8, true),
            Field::new("imdb_id", DataType::Int32, true),
            Field::new("name_pcode_nf", DataType::Utf8, true),
            Field::new("surname_pcode", DataType::Utf8, true),
            Field::new("md5sum", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "comp_cast_type",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("kind", DataType::Utf8, false),
        ]),
    );
    m.insert(
        "company_name",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("country_code", DataType::Utf8, true),
            Field::new("imdb_id", DataType::Int32, true),
            Field::new("name_pcode_nf", DataType::Utf8, true),
            Field::new("name_pcode_sf", DataType::Utf8, true),
            Field::new("md5sum", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "company_type",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("kind", DataType::Utf8, false),
        ]),
    );
    m.insert(
        "complete_cast",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, true),
            Field::new("subject_id", DataType::Int32, false),
            Field::new("status_id", DataType::Int32, false),
        ]),
    );
    m.insert(
        "info_type",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("info", DataType::Utf8, false),
        ]),
    );
    m.insert(
        "keyword",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("keyword", DataType::Utf8, false),
            Field::new("phonetic_code", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "kind_type",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("kind", DataType::Utf8, false),
        ]),
    );
    m.insert(
        "link_type",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("link", DataType::Utf8, false),
        ]),
    );
    m.insert(
        "movie_companies",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("company_id", DataType::Int32, false),
            Field::new("company_type_id", DataType::Int32, false),
            Field::new("note", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "movie_info",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("info_type_id", DataType::Int32, false),
            Field::new("info", DataType::Utf8, false),
            Field::new("note", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "movie_info_idx",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("info_type_id", DataType::Int32, false),
            Field::new("info", DataType::Utf8, false),
            Field::new("note", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "movie_keyword",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("keyword_id", DataType::Int32, false),
        ]),
    );
    m.insert(
        "movie_link",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("movie_id", DataType::Int32, false),
            Field::new("linked_movie_id", DataType::Int32, false),
            Field::new("link_type_id", DataType::Int32, false),
        ]),
    );
    m.insert(
        "name",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("imdb_index", DataType::Utf8, true),
            Field::new("imdb_id", DataType::Int32, true),
            Field::new("gender", DataType::Utf8, true),
            Field::new("name_pcode_cf", DataType::Utf8, true),
            Field::new("name_pcode_nf", DataType::Utf8, true),
            Field::new("surname_pcode", DataType::Utf8, true),
            Field::new("md5sum", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "person_info",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("person_id", DataType::Int32, false),
            Field::new("info_type_id", DataType::Int32, false),
            Field::new("info", DataType::Utf8, false),
            Field::new("note", DataType::Utf8, true),
        ]),
    );
    m.insert(
        "role_type",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("role", DataType::Utf8, false),
        ]),
    );
    m.insert(
        "title",
        Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("title", DataType::Utf8, false),
            Field::new("imdb_index", DataType::Utf8, true),
            Field::new("kind_id", DataType::Int32, false),
            Field::new("production_year", DataType::Int32, true),
            Field::new("imdb_id", DataType::Int32, true),
            Field::new("phonetic_code", DataType::Utf8, true),
            Field::new("episode_of_id", DataType::Int32, true),
            Field::new("season_nr", DataType::Int32, true),
            Field::new("episode_nr", DataType::Int32, true),
            Field::new("series_years", DataType::Utf8, true),
            Field::new("md5sum", DataType::Utf8, true),
        ]),
    );

    m
}

/// Probe `csv_dir` for the IMDb dump. Returns `Ok(())` if at least one
/// CSV or Parquet file from [`TABLES`] is present; otherwise a descriptive
/// `Error`. Useful for early-exit checks before the runner walks the
/// full table set.
pub fn probe_imdb_dir(csv_dir: &Path) -> Result<()> {
    if !csv_dir.exists() {
        return Err(Error::Feedback(format!(
            "imdb: directory does not exist: {}",
            csv_dir.display()
        )));
    }
    let parquet_dir = csv_dir.join("parquet");
    for &t in TABLES {
        let csv = csv_dir.join(format!("{t}.csv"));
        let parq = parquet_dir.join(format!("{t}.parquet"));
        if csv.exists() || parq.exists() {
            return Ok(());
        }
    }
    Err(Error::Feedback(format!(
        "imdb: no expected tables found under {} (try the download in data/job/README.md)",
        csv_dir.display()
    )))
}

/// Where the runner expects the IMDb dump to live by default. Callers may
/// override via `--imdb-dir`.
pub fn default_imdb_dir() -> PathBuf {
    PathBuf::from("samkhya-bench/data/job")
}

fn df_err(e: impl std::fmt::Display) -> Error {
    Error::Feedback(format!("datafusion: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_cover_every_table() {
        let schemas = imdb_schemas();
        for &t in TABLES {
            assert!(schemas.contains_key(t), "missing schema for {t}");
        }
        assert_eq!(schemas.len(), TABLES.len());
    }

    #[test]
    fn probe_rejects_missing_dir() {
        let bogus = PathBuf::from("/tmp/samkhya-bench-no-such-dir-xyzzy");
        assert!(probe_imdb_dir(&bogus).is_err());
    }
}
