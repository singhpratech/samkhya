//! `RecordBatch`-level helpers: build one sketch per column in a single
//! call. These are thin convenience wrappers around
//! [`crate::ingest`] for the common "summarize every column" path.

use arrow::record_batch::RecordBatch;
use samkhya_core::Result;
use samkhya_core::sketches::{BloomFilter, EquiDepthHistogram, HllSketch};

use crate::ingest::{
    ingest_array_into_bloom, ingest_array_into_hll, ingest_array_into_histogram_values,
};

/// Build one HLL sketch per column of `batch`, in schema field order.
///
/// Every sketch uses the same `precision`. Columns whose Arrow type is
/// unsupported by the ingestion path produce an empty (zero-cardinality)
/// HLL — they aren't an error here, because the caller may legitimately
/// want a per-column sketch vector aligned with the schema.
pub fn build_column_sketches(batch: &RecordBatch, precision: u8) -> Result<Vec<HllSketch>> {
    let mut sketches = Vec::with_capacity(batch.num_columns());
    for col_idx in 0..batch.num_columns() {
        let mut hll = HllSketch::new(precision)?;
        ingest_array_into_hll(batch.column(col_idx).as_ref(), &mut hll);
        sketches.push(hll);
    }
    Ok(sketches)
}

/// Build one Bloom filter per column of `batch`, sized for the batch's
/// row count at the requested false-positive rate.
pub fn build_blooms(batch: &RecordBatch, fp_rate: f64) -> Result<Vec<BloomFilter>> {
    let capacity = batch.num_rows();
    let mut blooms = Vec::with_capacity(batch.num_columns());
    for col_idx in 0..batch.num_columns() {
        let mut bloom = BloomFilter::new(capacity, fp_rate);
        ingest_array_into_bloom(batch.column(col_idx).as_ref(), &mut bloom);
        blooms.push(bloom);
    }
    Ok(blooms)
}

/// Build one equi-depth histogram per numeric column of `batch`.
///
/// The returned vector is aligned with the schema (`vec.len() ==
/// batch.num_columns()`); non-numeric columns slot in as `None` so the
/// caller can index by column position without a separate mapping.
pub fn build_histograms(
    batch: &RecordBatch,
    buckets: usize,
) -> Result<Vec<Option<EquiDepthHistogram>>> {
    let mut hists = Vec::with_capacity(batch.num_columns());
    for col_idx in 0..batch.num_columns() {
        match ingest_array_into_histogram_values(batch.column(col_idx).as_ref()) {
            Ok(values) => {
                let h = EquiDepthHistogram::from_values(&values, buckets)?;
                hists.push(Some(h));
            }
            Err(_) => hists.push(None),
        }
    }
    Ok(hists)
}
