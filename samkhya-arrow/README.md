# samkhya-arrow

[![crates.io](https://img.shields.io/crates/v/samkhya-arrow.svg)](https://crates.io/crates/samkhya-arrow)
[![docs.rs](https://docs.rs/samkhya-arrow/badge.svg)](https://docs.rs/samkhya-arrow)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Engine-agnostic Apache Arrow integration for samkhya sketches. Feed an
`arrow::array::Array` or a `RecordBatch` in, get back ready-to-serialize HLL,
Bloom, Count-Min, and equi-depth-histogram sketches.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## What this crate provides

- **`ingest`** — array-level helpers that dispatch once on `DataType`,
  downcast to the concrete primitive / byte array, and walk values into a
  sketch:
  - `ingest_array_into_hll(array, &mut HllSketch)`
  - `ingest_array_into_bloom(array, &mut BloomFilter)`
  - `ingest_array_into_cms(array, &mut CountMinSketch, count_per_value)`
  - `ingest_array_into_histogram_values(array) -> Result<Vec<f64>>`
- **`batch`** — `RecordBatch`-level convenience wrappers that fan out the
  array helpers across every column:
  - `build_column_sketches(batch, precision) -> Result<Vec<HllSketch>>`
  - `build_blooms(batch, fp_rate) -> Result<Vec<BloomFilter>>`
  - `build_histograms(batch, buckets) -> Result<Vec<Option<EquiDepthHistogram>>>`

The crate intentionally does *not* depend on DataFusion, DuckDB, Polars, or
any other engine — only on `arrow` itself — so any Arrow-aware caller can use
it. Sketches built from a DataFusion `RecordBatch` hash to the same keys as
sketches built from a Polars DataFrame's Arrow chunks.

## Hash-key conventions

All ingestion paths hash a column value by its canonical byte form:

| Arrow type                                  | Bytes fed to the sketch              |
|---------------------------------------------|--------------------------------------|
| `Int8` … `Int64`, `UInt8` … `UInt64`        | little-endian primitive bytes        |
| `Float32`, `Float64`                        | little-endian (`to_le_bytes`)        |
| `Utf8`, `LargeUtf8`                         | raw UTF-8 bytes                      |
| `Binary`, `LargeBinary`                     | bytes as-is                          |
| `Date32`, `Date64`, `TimestampNanosecond`   | little-endian of the underlying int  |
| `Boolean`                                   | `[0]` for false, `[1]` for true      |

These match the byte form `samkhya-core` sketches consume directly, so values
added through this crate and values added directly via the core API hash to
the same key.

## Quick start

```rust
use arrow::array::{Int64Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use std::sync::Arc;

use samkhya_arrow::batch::build_column_sketches;

let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
let batch = RecordBatch::try_new(
    schema,
    vec![Arc::new(Int64Array::from(vec![1, 2, 3, 1, 2]))],
)?;

let sketches = build_column_sketches(&batch, 12)?;
let approx_distinct = sketches[0].estimate();
println!("approx_distinct = {approx_distinct}"); // ~3
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Feature flags

This crate has no cargo features. The `arrow` dependency is pinned to the
major version DataFusion 46 vendors (`arrow = "54"`), so consumers that
already pull DataFusion never end up with two parallel Arrow stacks linked
into the same binary.

## Behavior on unsupported types

- HLL / Bloom / CMS helpers silently skip arrays whose `DataType` is not
  recognized (e.g. nested `Struct`, `List`, `Dictionary`). A generalized
  "build sketches for every column" caller can fan out without first
  auditing the schema.
- The histogram helper is stricter: it requires a numeric column and returns
  `Error::InvalidSketch` for non-numeric input rather than producing a
  meaningless empty histogram.

## Integration

Any caller that already speaks Arrow and wants engine-neutral cardinality
stats. Inside the samkhya workspace, this is the path adapters take when they
receive data as Arrow `RecordBatch` rather than as engine-native rows —
keeping sketch construction in one tested place rather than re-implemented
per engine.

## License

Apache-2.0. Sole author: Prateek Singh.
