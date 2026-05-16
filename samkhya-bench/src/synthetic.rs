//! Synthetic data generators for the in-process benchmark.
//!
//! Generates small in-memory tables that exercise multi-join cardinality
//! estimation under correlated predicates — the regime where DuckDB,
//! Polars, and DataFusion all admit large errors.
//!
//! Schema (loosely modeled on a tiny retail OLAP):
//!
//! - `customers(customer_id, region, segment)`            ~ 1_000 rows
//! - `products (product_id, category, price)`             ~   200 rows
//! - `orders   (order_id, customer_id, status, amount)`   ~10_000 rows
//! - `order_items(order_id, product_id, quantity)`        ~30_000 rows

use std::sync::Arc;

use datafusion::arrow::array::{Float64Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::error::DataFusionError;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const REGIONS: &[&str] = &["US", "EU", "APAC", "LATAM"];
const SEGMENTS: &[&str] = &["consumer", "smb", "enterprise"];
const CATEGORIES: &[&str] = &["electronics", "apparel", "home", "books", "grocery"];
const STATUSES: &[&str] = &["pending", "shipped", "delivered", "returned", "cancelled"];

/// Deterministic seeded RNG so successive runs produce identical data
/// (eases reproducible q-error comparisons).
pub fn rng() -> StdRng {
    StdRng::seed_from_u64(0x5a4b_4859_4159_4144) // "SAMKHYA0"
}

pub fn customers_table(n: usize) -> Result<Arc<MemTable>, DataFusionError> {
    let mut r = rng();
    let ids: Vec<i64> = (0..n as i64).collect();
    let regions: Vec<String> = (0..n)
        .map(|_| REGIONS[r.gen_range(0..REGIONS.len())].to_string())
        .collect();
    let segments: Vec<String> = (0..n)
        .map(|_| SEGMENTS[r.gen_range(0..SEGMENTS.len())].to_string())
        .collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("customer_id", DataType::Int64, false),
        Field::new("region", DataType::Utf8, false),
        Field::new("segment", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(regions)),
            Arc::new(StringArray::from(segments)),
        ],
    )?;
    Ok(Arc::new(MemTable::try_new(schema, vec![vec![batch]])?))
}

pub fn products_table(n: usize) -> Result<Arc<MemTable>, DataFusionError> {
    let mut r = rng();
    let ids: Vec<i64> = (0..n as i64).collect();
    let categories: Vec<String> = (0..n)
        .map(|_| CATEGORIES[r.gen_range(0..CATEGORIES.len())].to_string())
        .collect();
    let prices: Vec<f64> = (0..n).map(|_| r.gen_range(1.0..1000.0)).collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("product_id", DataType::Int64, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("price", DataType::Float64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(categories)),
            Arc::new(Float64Array::from(prices)),
        ],
    )?;
    Ok(Arc::new(MemTable::try_new(schema, vec![vec![batch]])?))
}

pub fn orders_table(n: usize, n_customers: usize) -> Result<Arc<MemTable>, DataFusionError> {
    let mut r = rng();
    let ids: Vec<i64> = (0..n as i64).collect();
    let cust_ids: Vec<i64> = (0..n).map(|_| r.gen_range(0..n_customers as i64)).collect();
    // Correlated: status='delivered' tends to come with higher amount
    let amounts: Vec<f64> = (0..n).map(|_| r.gen_range(10.0..5000.0)).collect();
    let statuses: Vec<String> = amounts
        .iter()
        .map(|&amt| {
            if amt > 3000.0 {
                "delivered".to_string()
            } else {
                STATUSES[r.gen_range(0..STATUSES.len())].to_string()
            }
        })
        .collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("order_id", DataType::Int64, false),
        Field::new("customer_id", DataType::Int64, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("amount", DataType::Float64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(Int64Array::from(cust_ids)),
            Arc::new(StringArray::from(statuses)),
            Arc::new(Float64Array::from(amounts)),
        ],
    )?;
    Ok(Arc::new(MemTable::try_new(schema, vec![vec![batch]])?))
}

pub fn order_items_table(
    n: usize,
    n_orders: usize,
    n_products: usize,
) -> Result<Arc<MemTable>, DataFusionError> {
    let mut r = rng();
    let order_ids: Vec<i64> = (0..n).map(|_| r.gen_range(0..n_orders as i64)).collect();
    let product_ids: Vec<i64> = (0..n).map(|_| r.gen_range(0..n_products as i64)).collect();
    let quantities: Vec<i64> = (0..n).map(|_| r.gen_range(1..10)).collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("order_id", DataType::Int64, false),
        Field::new("product_id", DataType::Int64, false),
        Field::new("quantity", DataType::Int64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(order_ids)),
            Arc::new(Int64Array::from(product_ids)),
            Arc::new(Int64Array::from(quantities)),
        ],
    )?;
    Ok(Arc::new(MemTable::try_new(schema, vec![vec![batch]])?))
}

/// Default table sizes used by the bench harness.
pub const N_CUSTOMERS: usize = 1_000;
pub const N_PRODUCTS: usize = 200;
pub const N_ORDERS: usize = 10_000;
pub const N_ORDER_ITEMS: usize = 30_000;
