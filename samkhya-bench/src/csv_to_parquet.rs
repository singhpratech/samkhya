//! CSV → Parquet converter for the IMDb JOB dump.
//!
//! WAVE-5F adds a Parquet input path so the JOB-Slow real measurement does
//! not pay a CSV re-parse cost on every benchmark execution. This module
//! provides a single entry point, [`convert_imdb_csvs_to_parquet`], that
//! walks the 21 IMDb tables under `imdb_dir`, reads each `<table>.csv`
//! using the declared schema from [`crate::imdb::imdb_schemas`], and
//! writes a `<imdb_dir>/<table>.parquet` sibling file.
//!
//! Implementation:
//!
//! - Reader: [`datafusion::arrow::csv::reader::ReaderBuilder`] with
//!   `has_header = false`, escape `\\`, delimiter `,` and the declared
//!   schema (matching the IMDb dump's PostgreSQL CSV format).
//! - Writer: [`datafusion::parquet::arrow::ArrowWriter`] with default
//!   `WriterProperties` (Snappy compression, default page/row-group
//!   sizes). The conversion is streaming — one record batch at a time —
//!   so even the 1.4 GB `cast_info.csv` does not need to fit in memory.
//!
//! No new crate dependencies are introduced: both `arrow-csv` and
//! `parquet` reach this crate via `datafusion = "46"` re-exports.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use datafusion::arrow::csv::reader::ReaderBuilder;
use datafusion::parquet::arrow::ArrowWriter;
use samkhya_core::Result;
use samkhya_core::error::Error;

use crate::imdb;

/// CSV reader batch size. 64 Ki rows balances memory footprint against
/// per-batch overhead; with ~30 columns and Int32/Utf8 columns the peak
/// resident set is ~16 MiB per batch.
const READ_BATCH_SIZE: usize = 65_536;

/// One row of the conversion audit table.
#[derive(Debug, Clone)]
pub struct ConvertedTable {
    /// Table name (e.g. `"cast_info"`).
    pub table: &'static str,
    /// Path to the source CSV (input).
    pub csv_path: PathBuf,
    /// Path to the destination Parquet (output).
    pub parquet_path: PathBuf,
    /// Rows read from the CSV (sum of `RecordBatch::num_rows`).
    pub rows: u64,
    /// CSV file size in bytes.
    pub csv_bytes: u64,
    /// Parquet file size in bytes.
    pub parquet_bytes: u64,
    /// Wall-clock conversion time for this table.
    pub elapsed: std::time::Duration,
}

/// Convert every IMDb CSV under `imdb_dir` to a sibling Parquet file.
///
/// Tables are processed in [`imdb::TABLES`] order. A missing CSV is
/// logged and skipped (the function does not error). The output file
/// `<imdb_dir>/<table>.parquet` is overwritten if it already exists.
///
/// Returns one [`ConvertedTable`] per successfully converted table. The
/// caller uses this to build the WAVE-5F receipt's row-count audit.
pub fn convert_imdb_csvs_to_parquet(imdb_dir: &Path) -> Result<Vec<ConvertedTable>> {
    if !imdb_dir.exists() {
        return Err(Error::Feedback(format!(
            "convert-imdb-csv-to-parquet: imdb_dir {} does not exist",
            imdb_dir.display()
        )));
    }
    let schemas = imdb::imdb_schemas();
    let mut out: Vec<ConvertedTable> = Vec::new();
    for &table in imdb::TABLES {
        let csv_path = imdb_dir.join(format!("{table}.csv"));
        let parquet_path = imdb_dir.join(format!("{table}.parquet"));
        if !csv_path.exists() {
            println!(
                "[csv-to-parquet] skipping {} (no CSV at {})",
                table,
                csv_path.display()
            );
            continue;
        }
        let schema = schemas
            .get(table)
            .ok_or_else(|| Error::Feedback(format!("imdb: no schema for {table}")))?
            .clone();
        let start = Instant::now();
        let rows = convert_one(&csv_path, &parquet_path, Arc::new(schema))?;
        let elapsed = start.elapsed();
        let csv_bytes = std::fs::metadata(&csv_path).map(|m| m.len()).unwrap_or(0);
        let parquet_bytes = std::fs::metadata(&parquet_path)
            .map(|m| m.len())
            .unwrap_or(0);
        println!(
            "[csv-to-parquet] {} -> {} ({} rows, {} -> {}, {:.2}s)",
            csv_path.display(),
            parquet_path.display(),
            rows,
            human_bytes(csv_bytes),
            human_bytes(parquet_bytes),
            elapsed.as_secs_f64()
        );
        out.push(ConvertedTable {
            table,
            csv_path,
            parquet_path,
            rows,
            csv_bytes,
            parquet_bytes,
            elapsed,
        });
    }
    Ok(out)
}

/// Streaming CSV→Parquet for a single table. Returns the row count read
/// from the CSV.
fn convert_one(
    csv_path: &Path,
    parquet_path: &Path,
    schema: Arc<datafusion::arrow::datatypes::Schema>,
) -> Result<u64> {
    let csv_file = File::open(csv_path)?;
    let csv_reader = ReaderBuilder::new(schema.clone())
        .with_header(false)
        .with_delimiter(b',')
        .with_escape(b'\\')
        .with_batch_size(READ_BATCH_SIZE)
        .build_buffered(BufReader::with_capacity(8 << 20, csv_file))
        .map_err(|e| Error::Feedback(format!("csv-reader: {e}")))?;

    let parquet_file = File::create(parquet_path)?;
    let mut writer = ArrowWriter::try_new(parquet_file, schema, None)
        .map_err(|e| Error::Feedback(format!("parquet-writer: {e}")))?;

    let mut total_rows: u64 = 0;
    for batch in csv_reader {
        let batch = batch.map_err(|e| Error::Feedback(format!("csv-decode: {e}")))?;
        total_rows = total_rows.saturating_add(batch.num_rows() as u64);
        writer
            .write(&batch)
            .map_err(|e| Error::Feedback(format!("parquet-write: {e}")))?;
    }
    writer
        .close()
        .map_err(|e| Error::Feedback(format!("parquet-close: {e}")))?;
    Ok(total_rows)
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{:.1} {}", v, UNITS[u])
}
