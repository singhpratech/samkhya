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
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_core::stats::ColumnStats;
use samkhya_core::{Error, Result};

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
