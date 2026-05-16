//! Build samkhya sketches directly from DuckDB query results.
//!
//! The functions in this module take a borrowed [`duckdb::Connection`],
//! execute the caller-supplied SQL, and digest **column 0** of each row
//! into the requested sketch. This keeps the integration surface tiny
//! and engine-agnostic: callers pre-shape the SQL (`SELECT col FROM t`,
//! optionally with `WHERE`/`GROUP BY`) and we feed the values in.
//!
//! ## v0 hashing strategy
//!
//! Sketches hash raw byte slices, but DuckDB columns can be any of a
//! dozen logical types. For this first cut every value is converted to
//! its textual representation via [`duckdb::types::ValueRef`] and
//! `to_string()`-equivalent formatting, then hashed as UTF-8 bytes.
//!
//! That keeps the surface portable across all column types at the cost
//! of two known caveats, which callers should be aware of:
//!
//! - Two values that print identically but have different logical types
//!   (e.g. integer `1` vs. boolean `true` rendered as `1`) collide.
//! - Floating-point precision in the textual rendering matters; callers
//!   that need binary fidelity should cast in SQL first
//!   (e.g. `CAST(col AS VARCHAR)`).
//!
//! These limits are acceptable for the cardinality-estimation use case
//! (HLL is already an approximate sketch) and will be tightened in a
//! future pass that switches on the column's logical type.

use samkhya_core::sketches::{BloomFilter, HllSketch};
use samkhya_core::{Error, Result};

use duckdb::Connection;
use duckdb::types::ValueRef;

/// Materialize a value reference into the UTF-8 bytes we hash. NULLs are
/// represented by the literal token `"\u{0}NULL"` so they don't collide
/// with the empty string.
fn value_bytes(val: ValueRef<'_>) -> Vec<u8> {
    match val {
        ValueRef::Null => b"\x00NULL".to_vec(),
        ValueRef::Boolean(b) => if b { b"1" } else { b"0" }.to_vec(),
        ValueRef::TinyInt(v) => v.to_string().into_bytes(),
        ValueRef::SmallInt(v) => v.to_string().into_bytes(),
        ValueRef::Int(v) => v.to_string().into_bytes(),
        ValueRef::BigInt(v) => v.to_string().into_bytes(),
        ValueRef::HugeInt(v) => v.to_string().into_bytes(),
        ValueRef::UTinyInt(v) => v.to_string().into_bytes(),
        ValueRef::USmallInt(v) => v.to_string().into_bytes(),
        ValueRef::UInt(v) => v.to_string().into_bytes(),
        ValueRef::UBigInt(v) => v.to_string().into_bytes(),
        ValueRef::Float(v) => v.to_string().into_bytes(),
        ValueRef::Double(v) => v.to_string().into_bytes(),
        ValueRef::Decimal(v) => v.to_string().into_bytes(),
        ValueRef::Text(s) => s.to_vec(),
        ValueRef::Blob(b) => b.to_vec(),
        // Anything else (Timestamp, Date, Time, List, Struct, ...) falls
        // back to Debug formatting. Debug is stable within a `duckdb`
        // version, which is enough for the sketch use case where two
        // queries on the same engine version need to agree.
        other => format!("{other:?}").into_bytes(),
    }
}

fn map_duck_err(e: duckdb::Error) -> Error {
    Error::Feedback(format!("duckdb: {e}"))
}

/// Build a [`HllSketch`] of `precision` from the first column of the
/// SQL result set.
///
/// The query may return any number of rows; only column index `0` is
/// consumed. NULL values are folded into a sentinel token (see module
/// docs) so they contribute a single bucket to the cardinality estimate.
pub fn build_hll_from_query(conn: &Connection, sql: &str, precision: u8) -> Result<HllSketch> {
    let mut sketch = HllSketch::new(precision)?;
    let mut stmt = conn.prepare(sql).map_err(map_duck_err)?;
    let mut rows = stmt.query([]).map_err(map_duck_err)?;
    while let Some(row) = rows.next().map_err(map_duck_err)? {
        let val = row.get_ref(0).map_err(map_duck_err)?;
        let bytes = value_bytes(val);
        sketch.add(&bytes);
    }
    Ok(sketch)
}

/// Build a [`BloomFilter`] sized for `capacity` items at `fp_rate` from
/// the first column of the SQL result set.
pub fn build_bloom_from_query(
    conn: &Connection,
    sql: &str,
    capacity: usize,
    fp_rate: f64,
) -> Result<BloomFilter> {
    let mut bloom = BloomFilter::new(capacity, fp_rate);
    let mut stmt = conn.prepare(sql).map_err(map_duck_err)?;
    let mut rows = stmt.query([]).map_err(map_duck_err)?;
    while let Some(row) = rows.next().map_err(map_duck_err)? {
        let val = row.get_ref(0).map_err(map_duck_err)?;
        let bytes = value_bytes(val);
        bloom.insert(&bytes);
    }
    Ok(bloom)
}
