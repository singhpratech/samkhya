# samkhya-arrow

Engine-agnostic Apache Arrow integration for [samkhya](../README.md)
sketches. Given an `arrow::array::Array` or an
`arrow::record_batch::RecordBatch`, this crate produces ready-to-serialize
HyperLogLog, Bloom, Count-Min, and equi-depth histogram sketches.

## Role in the workspace

`samkhya-core` defines the sketches. `samkhya-arrow` sits one layer above
and converts Arrow's columnar batches into the byte streams those
sketches expect — once, with a single hashing convention, so every
Arrow-aware consumer agrees on the keys.

It deliberately does **not** depend on DataFusion, DuckDB, Polars, or
any other engine. Each of those crates (`samkhya-datafusion`,
`samkhya-duckdb`, `samkhya-polars`, …) can take an Arrow batch out of
its native pipeline and hand it to `samkhya-arrow`. The result is a
single ingestion path: sketches built from a DataFusion `RecordBatch`
hash to the same keys as sketches built from a Polars DataFrame's
Arrow chunks.

## Hash-key conventions

| Arrow type                   | Bytes fed to the sketch |
|------------------------------|--------------------------|
| `Int8` … `Int64`             | little-endian            |
| `UInt8` … `UInt64`           | little-endian            |
| `Float32`, `Float64`         | little-endian (`to_le_bytes`) |
| `Utf8`, `LargeUtf8`          | raw UTF-8 bytes          |
| `Binary`, `LargeBinary`      | bytes as-is              |
| `Date32`, `Date64`           | little-endian of the underlying int |
| `Timestamp(Nanosecond, …)`   | little-endian of `i64`   |
| `Boolean`                    | `[0]` for false, `[1]` for true |

These match the byte form `samkhya-core` sketches consume directly, so
values added through this crate and values added directly via the core
API hash to the same key.

## API surface

- `ingest::ingest_array_into_hll(array, hll)`
- `ingest::ingest_array_into_bloom(array, bloom)`
- `ingest::ingest_array_into_cms(array, cms, count_per_value)`
- `ingest::ingest_array_into_histogram_values(array) -> Result<Vec<f64>>`
- `batch::build_column_sketches(batch, precision) -> Result<Vec<HllSketch>>`
- `batch::build_blooms(batch, fp_rate) -> Result<Vec<BloomFilter>>`
- `batch::build_histograms(batch, buckets) -> Result<Vec<Option<EquiDepthHistogram>>>`

The HLL/Bloom/CMS ingest helpers silently skip unsupported Arrow types
so a per-column fan-out doesn't have to pre-audit the schema. The
histogram path returns `Err` for non-numeric types — there's no
meaningful histogram interpretation over strings or bytes.

## Arrow version

Pinned to `arrow = "54"`, matching the major DataFusion 46 vendors, so
workspaces that pull in DataFusion don't end up with two parallel Arrow
stacks.

## License

Apache-2.0.
