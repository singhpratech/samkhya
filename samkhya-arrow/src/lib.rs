//! Engine-agnostic Arrow integration for samkhya sketches.
//!
//! This crate is the bridge between Apache Arrow data and samkhya's
//! cardinality / membership / range sketches. Consumers (DataFusion,
//! DuckDB extensions reading Arrow, Polars, custom Arrow pipelines)
//! feed an [`arrow::array::Array`] or an [`arrow::record_batch::RecordBatch`]
//! in, and get back ready-to-serialize sketches.
//!
//! The crate intentionally does **not** depend on DataFusion or any
//! other compute engine — only on `arrow` itself — so it stays usable
//! from any Arrow-aware caller.
//!
//! # Hash-key conventions
//!
//! All ingestion paths hash a column value by its canonical byte form:
//!
//! - Numeric types: little-endian bytes of the underlying primitive.
//! - `Utf8` / `LargeUtf8`: the raw UTF-8 bytes of the string.
//! - `Binary` / `LargeBinary`: the bytes as-is.
//! - `Date32` / `Date64` / `TimestampNanosecond`: little-endian bytes
//!   of the underlying integer.
//! - `Boolean`: a single byte, `0` for false, `1` for true.
//!
//! These conventions match the byte-form `samkhya-core` sketches already
//! consume (see `HllSketch::add`, `BloomFilter::insert`,
//! `CountMinSketch::add`), so values added through this crate and values
//! added directly via the core API hash to the same key.
#![deny(rustdoc::broken_intra_doc_links)]

pub mod batch;
pub mod ingest;
