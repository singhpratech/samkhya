//! Array-level ingestion helpers: feed an [`arrow::array::Array`] into a
//! samkhya sketch.
//!
//! Each `ingest_array_into_*` function dispatches once on the array's
//! `DataType`, downcasts to the concrete primitive / byte array, and
//! walks the values. Nulls are skipped. Unsupported types are silently
//! ignored (for HLL/Bloom/CMS) so a generalized "build sketches for
//! every column" caller can fan out without first auditing the schema.
//!
//! The histogram helper is the exception: it only makes sense for
//! numeric columns, so it surfaces an [`Error::InvalidSketch`] for
//! non-numeric input rather than producing an empty / meaningless
//! histogram.

use arrow::array::{
    Array, BinaryArray, BooleanArray, Date32Array, Date64Array, Float32Array, Float64Array,
    Int8Array, Int16Array, Int32Array, Int64Array, LargeBinaryArray, LargeStringArray, StringArray,
    TimestampNanosecondArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow::datatypes::{DataType, TimeUnit};
use samkhya_core::sketches::{BloomFilter, CountMinSketch, HllSketch};
use samkhya_core::{Error, Result};

/// Apply `f` to every non-null primitive value in `array`, downcast as
/// `$arr_ty`, converting each to its little-endian byte representation.
///
/// Implemented as a macro because Arrow's `PrimitiveArray<T>` instances
/// are distinct generic types — a single generic function would have to
/// thread `ArrowPrimitiveType` plumbing more than we need.
macro_rules! le_walk {
    ($array:expr, $arr_ty:ty, $f:expr) => {{
        let arr = $array
            .as_any()
            .downcast_ref::<$arr_ty>()
            .expect("downcast guarded by data_type match arm");
        for v in arr.iter().flatten() {
            ($f)(&v.to_le_bytes());
        }
    }};
}

macro_rules! bytes_walk {
    ($array:expr, $arr_ty:ty, $f:expr) => {{
        let arr = $array
            .as_any()
            .downcast_ref::<$arr_ty>()
            .expect("downcast guarded by data_type match arm");
        for v in arr.iter().flatten() {
            ($f)(v.as_ref());
        }
    }};
}

macro_rules! str_walk {
    ($array:expr, $arr_ty:ty, $f:expr) => {{
        let arr = $array
            .as_any()
            .downcast_ref::<$arr_ty>()
            .expect("downcast guarded by data_type match arm");
        for v in arr.iter().flatten() {
            ($f)(v.as_bytes());
        }
    }};
}

/// Drive a per-value byte-slice callback over every supported Arrow
/// array type. Returns `true` if the array's `DataType` was recognized,
/// `false` otherwise — callers that need to flag unsupported types
/// (e.g. the histogram path) check the return value.
fn for_each_value<F: FnMut(&[u8])>(array: &dyn Array, mut f: F) -> bool {
    match array.data_type() {
        DataType::Int8 => le_walk!(array, Int8Array, &mut f),
        DataType::Int16 => le_walk!(array, Int16Array, &mut f),
        DataType::Int32 => le_walk!(array, Int32Array, &mut f),
        DataType::Int64 => le_walk!(array, Int64Array, &mut f),
        DataType::UInt8 => le_walk!(array, UInt8Array, &mut f),
        DataType::UInt16 => le_walk!(array, UInt16Array, &mut f),
        DataType::UInt32 => le_walk!(array, UInt32Array, &mut f),
        DataType::UInt64 => le_walk!(array, UInt64Array, &mut f),
        DataType::Float32 => le_walk!(array, Float32Array, &mut f),
        DataType::Float64 => le_walk!(array, Float64Array, &mut f),
        DataType::Utf8 => str_walk!(array, StringArray, &mut f),
        DataType::LargeUtf8 => str_walk!(array, LargeStringArray, &mut f),
        DataType::Binary => bytes_walk!(array, BinaryArray, &mut f),
        DataType::LargeBinary => bytes_walk!(array, LargeBinaryArray, &mut f),
        DataType::Date32 => le_walk!(array, Date32Array, &mut f),
        DataType::Date64 => le_walk!(array, Date64Array, &mut f),
        DataType::Timestamp(TimeUnit::Nanosecond, _) => {
            le_walk!(array, TimestampNanosecondArray, &mut f)
        }
        DataType::Boolean => {
            let arr = array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .expect("downcast guarded by data_type match arm");
            for v in arr.iter().flatten() {
                let byte: u8 = u8::from(v);
                f(&[byte]);
            }
        }
        _ => return false,
    }
    true
}

/// Ingest every non-null value of `array` into `hll`, hashing as the
/// canonical byte form described at the crate root. Unsupported types
/// are silently skipped — they contribute zero values to the sketch.
pub fn ingest_array_into_hll(array: &dyn Array, hll: &mut HllSketch) {
    let _ = for_each_value(array, |bytes| hll.add(bytes));
}

/// Ingest every non-null value of `array` into `bloom`.
pub fn ingest_array_into_bloom(array: &dyn Array, bloom: &mut BloomFilter) {
    let _ = for_each_value(array, |bytes| bloom.insert(bytes));
}

/// Ingest every non-null value of `array` into `cms`, with a fixed
/// per-value count weight. Use `count_per_value = 1` to count
/// occurrences directly; use a higher weight to pre-aggregate.
pub fn ingest_array_into_cms(array: &dyn Array, cms: &mut CountMinSketch, count_per_value: u32) {
    let _ = for_each_value(array, |bytes| cms.add(bytes, count_per_value));
}

/// Pull non-null primitive values out of an Arrow array and append them
/// to `out` after casting to `f64`. Used by the histogram path.
macro_rules! collect_primitive_as_f64 {
    ($array:expr, $arr_ty:ty, $out:expr) => {{
        let arr = $array
            .as_any()
            .downcast_ref::<$arr_ty>()
            .expect("downcast guarded by data_type match arm");
        for v in arr.iter().flatten() {
            $out.push(v as f64);
        }
    }};
}

/// Extract non-null numeric values from `array` as `f64`, ready to feed
/// into [`samkhya_core::sketches::EquiDepthHistogram::from_values`].
/// Returns an [`Error::InvalidSketch`] for non-numeric arrays — the
/// histogram has no meaningful interpretation over strings / bytes /
/// booleans.
pub fn ingest_array_into_histogram_values(array: &dyn Array) -> Result<Vec<f64>> {
    let mut out: Vec<f64> = Vec::with_capacity(array.len());
    match array.data_type() {
        DataType::Int8 => collect_primitive_as_f64!(array, Int8Array, out),
        DataType::Int16 => collect_primitive_as_f64!(array, Int16Array, out),
        DataType::Int32 => collect_primitive_as_f64!(array, Int32Array, out),
        DataType::Int64 => collect_primitive_as_f64!(array, Int64Array, out),
        DataType::UInt8 => collect_primitive_as_f64!(array, UInt8Array, out),
        DataType::UInt16 => collect_primitive_as_f64!(array, UInt16Array, out),
        DataType::UInt32 => collect_primitive_as_f64!(array, UInt32Array, out),
        DataType::UInt64 => collect_primitive_as_f64!(array, UInt64Array, out),
        DataType::Float32 => collect_primitive_as_f64!(array, Float32Array, out),
        DataType::Float64 => collect_primitive_as_f64!(array, Float64Array, out),
        // Date / timestamp columns are integer-backed and order-preserving
        // under the f64 cast, so they remain meaningful for range
        // selectivity. Nanosecond timestamps past 2^53 lose precision,
        // but the equi-depth histogram is a lossy summary anyway.
        DataType::Date32 => collect_primitive_as_f64!(array, Date32Array, out),
        DataType::Date64 => collect_primitive_as_f64!(array, Date64Array, out),
        DataType::Timestamp(TimeUnit::Nanosecond, _) => {
            collect_primitive_as_f64!(array, TimestampNanosecondArray, out)
        }
        other => {
            return Err(Error::InvalidSketch(format!(
                "histogram requires a numeric Arrow type, got {other:?}"
            )));
        }
    }
    Ok(out)
}
