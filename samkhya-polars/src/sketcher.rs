//! Build samkhya sketches directly from a [`polars::series::Series`].
//!
//! Each helper iterates the series once, hashing values into the
//! appropriate sketch. Numeric values are fed as their little-endian
//! byte representation; non-numeric values are converted to a UTF-8
//! string and fed as bytes. Nulls are skipped.
//!
//! All helpers return [`samkhya_core::Result`] so failures (e.g. an
//! invalid precision, a non-numeric column passed to the histogram
//! builder) propagate uniformly with the rest of `samkhya-core`.

use polars::datatypes::{AnyValue, DataType};
use polars::series::Series;

use samkhya_core::sketches::{BloomFilter, CountMinSketch, EquiDepthHistogram, HllSketch};
use samkhya_core::{Error, Result};

/// Encode a single [`AnyValue`] into the byte slice we hash into a sketch.
///
/// - Numeric variants → little-endian bytes (matches the encoding used
///   by `samkhya-core`'s own sketch test suites).
/// - Boolean → single byte.
/// - Strings / binary → underlying bytes.
/// - Anything else → debug-format string bytes; stable enough for
///   distinct-count / membership purposes.
///
/// Returns `None` for nulls so callers can skip them.
fn anyvalue_to_bytes(av: &AnyValue<'_>) -> Option<Vec<u8>> {
    match av {
        AnyValue::Null => None,
        AnyValue::Boolean(b) => Some(vec![u8::from(*b)]),
        AnyValue::UInt8(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::UInt16(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::UInt32(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::UInt64(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::Int8(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::Int16(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::Int32(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::Int64(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::Float32(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::Float64(v) => Some(v.to_le_bytes().to_vec()),
        AnyValue::String(s) => Some(s.as_bytes().to_vec()),
        AnyValue::StringOwned(s) => Some(s.as_str().as_bytes().to_vec()),
        AnyValue::Binary(b) => Some(b.to_vec()),
        AnyValue::BinaryOwned(b) => Some(b.clone()),
        other => Some(format!("{other:?}").into_bytes()),
    }
}

/// Iterate `series` as `AnyValue`, after rechunking so that
/// `Series::iter()` is safe to call.
fn iter_anyvalues(series: &Series) -> Vec<AnyValue<'_>> {
    // Series::iter() panics unless chunks().len() == 1. Rechunk first so
    // multi-chunk inputs from a LazyFrame.collect() path work.
    if series.n_chunks() > 1 {
        // SAFETY: rechunk returns a new Series; we drop it after copying values.
        let rechunked = series.rechunk();
        rechunked.iter().map(|av| av.into_static()).collect()
    } else {
        series.iter().collect()
    }
}

/// Build an HLL sketch from a Polars `Series` at the given precision.
pub fn hll_from_series(series: &Series, precision: u8) -> Result<HllSketch> {
    let mut hll = HllSketch::new(precision)?;
    for av in iter_anyvalues(series) {
        if let Some(bytes) = anyvalue_to_bytes(&av) {
            hll.add(&bytes);
        }
    }
    Ok(hll)
}

/// Build a Bloom filter sized for `series.len()` items at the target
/// false-positive rate.
pub fn bloom_from_series(series: &Series, fp_rate: f64) -> Result<BloomFilter> {
    let mut bf = BloomFilter::new(series.len(), fp_rate);
    for av in iter_anyvalues(series) {
        if let Some(bytes) = anyvalue_to_bytes(&av) {
            bf.insert(&bytes);
        }
    }
    Ok(bf)
}

/// Build a Count-Min sketch from a Polars `Series` at the given depth/width.
pub fn cms_from_series(series: &Series, depth: u32, width: u32) -> Result<CountMinSketch> {
    let mut cms = CountMinSketch::new(depth, width)?;
    for av in iter_anyvalues(series) {
        if let Some(bytes) = anyvalue_to_bytes(&av) {
            cms.add(&bytes, 1);
        }
    }
    Ok(cms)
}

/// Build an equi-depth histogram from a numeric Polars `Series`.
///
/// Returns [`Error::InvalidSketch`] if the column dtype is not a basic
/// numeric type (use [`DataType::is_numeric`] semantics).
pub fn histogram_from_series(series: &Series, buckets: usize) -> Result<EquiDepthHistogram> {
    if !series.dtype().is_numeric() {
        return Err(Error::InvalidSketch(format!(
            "histogram_from_series: column dtype {:?} is not numeric",
            series.dtype()
        )));
    }
    let f64_series = series
        .cast(&DataType::Float64)
        .map_err(|e| Error::InvalidSketch(format!("cast to Float64 failed: {e}")))?;
    let ca = f64_series
        .f64()
        .map_err(|e| Error::InvalidSketch(format!("downcast to f64 failed: {e}")))?;

    // `iter()` rather than `into_iter()`: polars 0.54 dropped the
    // `IntoIterator` impl on `&ChunkedArray`. `flatten()` skips nulls, which
    // is what an equi-depth histogram wants — a null is an absent value, not
    // a zero.
    let mut values: Vec<f64> = Vec::with_capacity(ca.len());
    for v in ca.iter().flatten() {
        values.push(v);
    }
    EquiDepthHistogram::from_values(&values, buckets)
}
