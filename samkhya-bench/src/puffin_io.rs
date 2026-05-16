//! Build Puffin sidecars from synthetic data and load them back into
//! `ColumnStats` overrides for the runner.
//!
//! This closes the architectural loop: instead of hardcoding distinct
//! counts inside the runner, the bench can write sketches to per-table
//! Puffin files (one file per table) and reload them on a subsequent
//! run. The samkhya-corrected mode then sources its overrides from the
//! sidecars — the actual production data flow.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use datafusion::arrow::array::Array;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::prelude::{CsvReadOptions, SessionContext};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{BloomFilter, HllSketch, Sketch};
use samkhya_core::stats::ColumnStats;
use samkhya_core::{Error, Result};

use crate::imdb::{self, ROW_COUNT_KIND};
use crate::synthetic;

const HLL_PRECISION: u8 = 12;

/// Build Puffin sidecars for every synthetic table and write them to
/// `output_dir`. One file per table: customers.puffin, products.puffin,
/// orders.puffin, order_items.puffin.
pub fn build_puffin_sidecars(output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;

    let tables = [
        ("customers", synthetic::N_CUSTOMERS, build_customers()?),
        ("products", synthetic::N_PRODUCTS, build_products()?),
        ("orders", synthetic::N_ORDERS, build_orders()?),
        (
            "order_items",
            synthetic::N_ORDER_ITEMS,
            build_order_items()?,
        ),
    ];

    for (name, _row_count, table) in tables {
        let path = output_dir.join(format!("{name}.puffin"));
        write_table_sidecar(&path, &table)?;
        println!(
            "wrote {} ({} blobs)",
            path.display(),
            schema_field_count(&table)
        );
    }
    Ok(())
}

/// Load Puffin sidecars from `input_dir` and return per-table column
/// stats indexed by table name.
pub fn load_column_stats_from_sidecars(
    input_dir: &Path,
) -> Result<std::collections::HashMap<String, Vec<(usize, ColumnStats)>>> {
    let mut out = std::collections::HashMap::new();
    for name in ["customers", "products", "orders", "order_items"] {
        let path = input_dir.join(format!("{name}.puffin"));
        if !path.exists() {
            continue;
        }
        let file = File::open(&path)?;
        let mut reader = PuffinReader::open(file)?;
        let mut overrides = Vec::new();
        for (i, meta) in reader.blobs().to_vec().iter().enumerate() {
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
        out.insert(name.to_string(), overrides);
    }
    Ok(out)
}

fn write_table_sidecar(path: &PathBuf, table: &Arc<MemTable>) -> Result<()> {
    let file = File::create(path)?;
    let mut writer = PuffinWriter::new(file);

    let schema = table.schema();
    let batches = collect_batches(table)?;
    for (col_idx, _) in schema.fields().iter().enumerate() {
        let mut hll = HllSketch::new(HLL_PRECISION)?;
        for batch in &batches {
            let array = batch.column(col_idx);
            ingest_into_hll(&mut hll, array.as_ref());
        }
        let payload = hll.to_bytes()?;
        writer.add_blob(Blob::new(HllSketch::KIND, vec![col_idx as i32], &payload))?;
    }
    writer.finish()?;
    Ok(())
}

fn collect_batches(
    table: &Arc<MemTable>,
) -> Result<Vec<datafusion::arrow::record_batch::RecordBatch>> {
    // Reuse the MemTable's in-memory partitions directly via the public scan API
    // would require a SessionContext; instead, we exploit that MemTable was built
    // from a single batch in synthetic.rs. The bench knows the construction shape.
    // For a v0 puffin sidecar we accept this coupling.
    use datafusion::execution::context::SessionContext;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::from)?;
    rt.block_on(async {
        let ctx = SessionContext::new();
        ctx.register_table(
            "__t",
            Arc::clone(table) as Arc<dyn datafusion::datasource::TableProvider>,
        )
        .map_err(|e| Error::Feedback(format!("register: {e}")))?;
        let df = ctx
            .sql("SELECT * FROM __t")
            .await
            .map_err(|e| Error::Feedback(format!("sql: {e}")))?;
        df.collect()
            .await
            .map_err(|e| Error::Feedback(format!("collect: {e}")))
    })
}

fn ingest_into_hll(hll: &mut HllSketch, array: &dyn Array) {
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::Int32Array>()
    {
        for v in arr.iter().flatten() {
            hll.add(&v.to_le_bytes());
        }
        return;
    }
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::Int64Array>()
    {
        for v in arr.iter().flatten() {
            hll.add(&v.to_le_bytes());
        }
        return;
    }
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::Float64Array>()
    {
        for v in arr.iter().flatten() {
            hll.add(&v.to_le_bytes());
        }
        return;
    }
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::StringArray>()
    {
        for v in arr.iter().flatten() {
            hll.add(v.as_bytes());
        }
    }
    // Other types fall through as no-op for v0; extend as new synthetic columns land.
}

fn schema_field_count(table: &Arc<MemTable>) -> usize {
    table.schema().fields().len()
}

fn build_customers() -> Result<Arc<MemTable>> {
    synthetic::customers_table(synthetic::N_CUSTOMERS).map_err(df_err)
}
fn build_products() -> Result<Arc<MemTable>> {
    synthetic::products_table(synthetic::N_PRODUCTS).map_err(df_err)
}
fn build_orders() -> Result<Arc<MemTable>> {
    synthetic::orders_table(synthetic::N_ORDERS, synthetic::N_CUSTOMERS).map_err(df_err)
}
fn build_order_items() -> Result<Arc<MemTable>> {
    synthetic::order_items_table(
        synthetic::N_ORDER_ITEMS,
        synthetic::N_ORDERS,
        synthetic::N_PRODUCTS,
    )
    .map_err(df_err)
}

fn df_err(e: datafusion::error::DataFusionError) -> Error {
    Error::Feedback(format!("datafusion: {e}"))
}

// ---------- IMDb / JOB sidecar build path ----------

/// Bloom false-positive rate for FK-column bloom filters. Conservative
/// 1% per Cormode-Muthukrishnan; gives a small payload (~12 KiB for 1e6
/// items) without false-positive blow-up.
const BLOOM_FP_RATE: f64 = 0.01;

/// Build Puffin sidecars for the 21 IMDb tables and write them next to
/// the CSVs at `<imdb_dir>/<table>.puffin`. For every column we compute:
///
/// - HLL precision-12 NDV sketch (per [`HLL_PRECISION`])
/// - For foreign-key columns (column name ends in `_id`, or column name
///   `id`): Bloom filter at 1% FPR
/// - Table-level row count is stamped as a [`ROW_COUNT_KIND`] marker blob
///
/// Errors on individual tables are logged and the build continues — the
/// caller picks up whichever sidecars succeeded.
pub fn build_puffin_sidecars_imdb(imdb_dir: &Path) -> Result<()> {
    if !imdb_dir.exists() {
        return Err(Error::Feedback(format!(
            "build-puffin: imdb_dir {} does not exist",
            imdb_dir.display()
        )));
    }
    let schemas = imdb::imdb_schemas();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::from)?;

    for &table in imdb::TABLES {
        let csv_path = imdb_dir.join(format!("{table}.csv"));
        let out_path = imdb_dir.join(format!("{table}.puffin"));
        if !csv_path.exists() {
            println!(
                "[build-puffin][imdb] skipping {} (no CSV at {})",
                table,
                csv_path.display()
            );
            continue;
        }
        let schema = match schemas.get(table) {
            Some(s) => Arc::new(s.clone()),
            None => {
                println!("[build-puffin][imdb] no schema for {}; skipping", table);
                continue;
            }
        };
        let start = std::time::Instant::now();
        let result: Result<()> = rt.block_on(async {
            let ctx = SessionContext::new();
            let opts = CsvReadOptions::new()
                .has_header(false)
                .escape(b'\\')
                .delimiter(b',')
                .schema(schema.as_ref())
                .newlines_in_values(true);
            ctx.register_csv(table, csv_path.to_string_lossy().as_ref(), opts)
                .await
                .map_err(|e| Error::Feedback(format!("register: {e}")))?;
            let df = ctx
                .sql(&format!("SELECT * FROM {table}"))
                .await
                .map_err(|e| Error::Feedback(format!("sql: {e}")))?;
            let batches = df
                .collect()
                .await
                .map_err(|e| Error::Feedback(format!("collect: {e}")))?;

            let n_cols = schema.fields().len();
            let mut hlls: Vec<HllSketch> = (0..n_cols)
                .map(|_| HllSketch::new(HLL_PRECISION))
                .collect::<Result<Vec<_>>>()?;
            // For FK columns (and the primary key `id`), build a 1% Bloom.
            // Capacity is unknown ahead of time; we'll insert and accept
            // the worst-case fp rate growth — Bloom over-insertion just
            // bumps fp rate, never errors.
            let mut blooms: Vec<Option<BloomFilter>> = schema
                .fields()
                .iter()
                .map(|f| {
                    let name = f.name();
                    if is_fk_column(name) {
                        // Capacity heuristic: 4 million unique items for
                        // the largest IMDb tables (cast_info has ~36M
                        // rows but fewer unique person_ids; this is an
                        // over-allocation but keeps the bloom under 5 MiB
                        // per column). Falls back to fp inflation if
                        // exceeded, which is acceptable for v1.0.
                        BloomFilter::try_new(4_000_000, BLOOM_FP_RATE).ok()
                    } else {
                        None
                    }
                })
                .collect();

            let mut total_rows: u64 = 0;
            for batch in &batches {
                total_rows = total_rows.saturating_add(batch.num_rows() as u64);
                for col_idx in 0..n_cols {
                    let array = batch.column(col_idx);
                    ingest_into_hll(&mut hlls[col_idx], array.as_ref());
                    if let Some(bf) = blooms[col_idx].as_mut() {
                        ingest_into_bloom(bf, array.as_ref());
                    }
                }
            }

            // Write the sidecar.
            let file = File::create(&out_path)?;
            let mut writer = PuffinWriter::new(file);
            // Row-count marker blob first.
            let mut rc_bytes = vec![0u8; 8];
            rc_bytes.copy_from_slice(&total_rows.to_le_bytes());
            writer.add_blob(Blob::new(ROW_COUNT_KIND, vec![], &rc_bytes))?;
            for (col_idx, hll) in hlls.iter().enumerate() {
                let payload = hll.to_bytes()?;
                writer.add_blob(Blob::new(HllSketch::KIND, vec![col_idx as i32], &payload))?;
            }
            for (col_idx, bf_opt) in blooms.iter().enumerate() {
                if let Some(bf) = bf_opt {
                    let payload = bf.to_bytes()?;
                    writer.add_blob(Blob::new(
                        BloomFilter::KIND,
                        vec![col_idx as i32],
                        &payload,
                    ))?;
                }
            }
            writer.finish()?;
            Ok(())
        });
        match result {
            Ok(()) => {
                let elapsed = start.elapsed();
                println!(
                    "[build-puffin][imdb] wrote {} (rows from CSV scan; {:.2}s)",
                    out_path.display(),
                    elapsed.as_secs_f64()
                );
            }
            Err(e) => {
                println!("[build-puffin][imdb] FAILED {} ({}); continuing", table, e);
            }
        }
    }
    Ok(())
}

fn is_fk_column(name: &str) -> bool {
    name == "id" || name.ends_with("_id")
}

fn ingest_into_bloom(bf: &mut BloomFilter, array: &dyn Array) {
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::Int32Array>()
    {
        for v in arr.iter().flatten() {
            bf.insert(&v.to_le_bytes());
        }
        return;
    }
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::Int64Array>()
    {
        for v in arr.iter().flatten() {
            bf.insert(&v.to_le_bytes());
        }
        return;
    }
    if let Some(arr) = array
        .as_any()
        .downcast_ref::<datafusion::arrow::array::StringArray>()
    {
        for v in arr.iter().flatten() {
            bf.insert(v.as_bytes());
        }
    }
}
