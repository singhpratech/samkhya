# samkhya-arrow

[![crates.io](https://img.shields.io/crates/v/samkhya-arrow.svg)](https://crates.io/crates/samkhya-arrow)
[![docs.rs](https://docs.rs/samkhya-arrow/badge.svg)](https://docs.rs/samkhya-arrow)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Builds samkhya sketches from Apache Arrow data. Hand it an
`arrow::array::Array` or a `RecordBatch`, get back HLL, Bloom, Count-Min,
and equi-depth-histogram sketches that serialize and travel. It depends on
`arrow` and `samkhya-core` only — not DataFusion, not DuckDB, not Polars —
so the same column produces the same sketch bytes whichever engine handed
you the buffers.

## Install

`samkhya-arrow = "1.2"`. The `arrow` dependency is pinned to major version
54, the one DataFusion 46 vendors, so a consumer that already pulls
DataFusion does not link two Arrow stacks.

## API

`ingest` — array level, one dispatch on `DataType`, nulls skipped:

- `ingest_array_into_hll(&dyn Array, &mut HllSketch)`
- `ingest_array_into_bloom(&dyn Array, &mut BloomFilter)`
- `ingest_array_into_cms(&dyn Array, &mut CountMinSketch, count_per_value: u32)`
- `ingest_array_into_histogram_values(&dyn Array) -> Result<Vec<f64>>`

`batch` — fan those across every column of a `RecordBatch`:

- `build_column_sketches(&RecordBatch, precision: u8) -> Result<Vec<HllSketch>>`
- `build_blooms(&RecordBatch, fp_rate: f64) -> Result<Vec<BloomFilter>>`
- `build_histograms(&RecordBatch, buckets: usize) -> Result<Vec<Option<EquiDepthHistogram>>>`

## Example: sketches to a provable join ceiling

Sketches built here are inputs to samkhya's provable join-cardinality
ceiling in `samkhya_core::degree`. Ten orders joined to a hundred line
items over ten distinct keys:

```rust
use arrow::array::{Int64Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use std::sync::Arc;

use samkhya_arrow::batch::build_column_sketches;
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};

let schema = Arc::new(Schema::new(vec![Field::new("order_id", DataType::Int64, false)]));
let col = |v: Vec<i64>| {
    RecordBatch::try_new(schema.clone(), vec![Arc::new(Int64Array::from(v))]).unwrap()
};

let orders = col((0..10i64).collect());
let line_items = col((0..100i64).map(|i| i % 10).collect());

let order_hll = &build_column_sketches(&orders, 12)?[0];
let item_hll = &build_column_sketches(&line_items, 12)?[0];

const ORDER_ID: u32 = 0;
let graph = JoinGraph::new(vec![
    JoinRelation::new(10).with_degree(ORDER_ID,
        AttributeDegree::from_hll_floor(10, order_hll)),
    JoinRelation::new(100).with_degree(ORDER_ID,
        AttributeDegree::from_hll_floor(100, item_hll)),
])
.with_edge(0, 1, ORDER_ID);

assert_eq!(graph.ceiling(), 100); // the Cartesian product would say 1000
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `AttributeDegree::from_hll_floor`, never `HllSketch::estimate`: the
point estimate is two-sided, so it exceeds the true distinct count about
half the time, and since the degree arithmetic subtracts that count an
over-stated one yields a ceiling below the truth. `from_hll_floor` reads a
distinct-count floor instead. On high-cardinality columns a Count-Min
sketch bounds degrees far more tightly — build one with
`ingest_array_into_cms(array, &mut cms, 1)` and pass it to
`AttributeDegree::from_count_min`, sound because Count-Min never
undercounts.

## Hash-key conventions

Every path hashes a value by its canonical byte form — the same form
`samkhya-core` sketches consume directly, so a value added through this
crate and one added via the core API hash to the same key:

| Arrow type                          | Bytes fed to the sketch       |
|-------------------------------------|-------------------------------|
| `Int8`..`Int64`, `UInt8`..`UInt64`  | little-endian primitive bytes |
| `Float32`, `Float64`                | little-endian bytes           |
| `Utf8`, `LargeUtf8`                 | raw UTF-8 bytes               |
| `Binary`, `LargeBinary`             | bytes as-is                   |
| `Date32`, `Date64`, `Timestamp(ns)` | little-endian underlying int  |
| `Boolean`                           | `[0]` false, `[1]` true       |

## Scope and caveats

- The HLL / Bloom / CMS helpers silently skip arrays of unrecognized
  `DataType` — `Struct`, `List`, `Dictionary`, non-nanosecond timestamps.
  Such a column yields an empty sketch, not an error. Check the schema
  yourself if an empty sketch would be wrong for you.
- `ingest_array_into_histogram_values` is the exception: non-numeric input
  returns `Error::InvalidSketch`, which `build_histograms` turns into a
  `None` in its schema-aligned vector.
- `build_blooms` sizes each filter for the row count of the batch it was
  handed. Sketching a table in chunks gives per-chunk filters, not one
  filter sized for the table.
- This crate builds sketches. It does not plan, rewrite, or execute
  anything, and carries no engine integration; those live in
  `samkhya-datafusion`, `samkhya-duckdb-ext`, and the other adapters.

## 1.2

The bound family shipped through 1.1 was found unsound and was repaired in
1.2; `AgmBound` is deprecated. This crate's API is unchanged, but what to
do with its output is not: feed distinct-count floors and Count-Min
sketches to `samkhya_core::degree`, never a two-sided point estimate. See
https://github.com/singhpratech/samkhya/blob/main/CHANGELOG.md

## License

Apache-2.0. Sole author: Prateek Singh.
