# samkhya-duckdb

[![crates.io](https://img.shields.io/crates/v/samkhya-duckdb.svg)](https://crates.io/crates/samkhya-duckdb)
[![docs.rs](https://docs.rs/samkhya-duckdb/badge.svg)](https://docs.rs/samkhya-duckdb)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Client-side DuckDB integration for samkhya. It consumes validated portable
Puffin statistics without requiring DuckDB, and optionally runs SQL against an
embedded DuckDB connection to build HLL and Bloom-filter sketches.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## What this crate provides

- **`sidecar::decode_portable_column(snapshot, field_id)`** — decode the
  canonical `ColumnStats`, HLL, and equi-depth histogram for one Iceberg field
  ID. This always-on API does not require the `bundled` feature.
- **`sketcher::build_hll_from_query(conn, sql, precision)`** — execute SQL
  against an embedded DuckDB connection, digest column 0 of each row, and
  return an `HllSketch` ready to serialize.
- **`sketcher::build_bloom_from_query(conn, sql, capacity, fp_rate)`** —
  same shape, but builds a `BloomFilter` sized for the expected cardinality.
- **`feedback::capture_observation(conn, sql, template, plan)`** — runs the
  query, times the execution, and emits an `Observation` matching the
  schema of `samkhya_core::feedback::FeedbackStore` so the residual
  corrector can train on real `(estimated, actual)` pairs.

This is a deliberately **client-side** integration: callers pre-shape the
SQL (`SELECT col FROM t`, optionally with `WHERE` / `GROUP BY`) and we feed
the values in. A true server-side `.duckdb_extension` (with cxx-bridged
sketch aggregates that DuckDB builds inside vectorized operators) is on the
roadmap but not delivered here.

## Consume portable Puffin statistics

Load a `PortableStatsSnapshot` through `samkhya-iceberg`, then select an
Iceberg field ID through this crate:

```rust
use samkhya_duckdb::sidecar::decode_portable_column;

let column = decode_portable_column(&snapshot, 7)?;
if let Some(column) = column {
    println!("NDV: {:?}", column.column_stats().distinct_count);
    if let Some(histogram) = column.histogram() {
        println!("rows in [10, 20]: {}", histogram.estimate_range(10.0, 20.0));
    }
}
# Ok::<(), samkhya_core::Error>(())
```

This exposes client-side statistics only. It does not register a DuckDB
catalog object or alter DuckDB optimizer estimates.

## Quick start

```rust
use duckdb::Connection;
use samkhya_duckdb::sketcher::{build_hll_from_query, build_bloom_from_query};

let conn = Connection::open_in_memory()?;
conn.execute_batch(
    "CREATE TABLE orders(id INTEGER, customer_id INTEGER);
     INSERT INTO orders VALUES (1, 100), (2, 100), (3, 200);",
)?;

let hll = build_hll_from_query(&conn, "SELECT customer_id FROM orders", 14)?;
println!("approx distinct customers = {}", hll.estimate());

let bloom = build_bloom_from_query(
    &conn,
    "SELECT customer_id FROM orders WHERE id > 1",
    1_000,
    0.01,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Feature flags

- `bundled` (off by default) — pulls in the `duckdb` crate with its own
  `bundled` feature, so `libduckdb` is compiled from source by `duckdb-sys`
  and no system-installed `libduckdb` is required. With the feature disabled,
  portable sidecar decoding remains available and a bare
  `cargo check -p samkhya-duckdb` needs no DuckDB C++ build.

## v0 hashing caveat

The current value-byte helper renders every DuckDB `ValueRef` into its
textual form before hashing — portable across all column types, but with
two known caveats:

- Values that print identically but have different logical types (e.g.
  integer `1` vs. boolean `true` rendered as `1`) collide.
- Floating-point precision of the textual rendering matters; callers that
  need binary fidelity should cast in SQL first (`CAST(col AS VARCHAR)` or
  `CAST(col AS BLOB)`).

A future revision will switch on logical type to avoid these caveats. The
limits are acceptable for the cardinality-estimation use case (HLL is
already approximate) and will tighten over time.

## DuckDB version

Tested against `duckdb = "1.0"`. Serialized sketches use the shared
`samkhya-core` codecs. Producers must still use the same value-byte convention:
the current DuckDB helper renders numeric values as text, so numeric sketches
are not byte-identical to adapters that hash little-endian primitives. Raw
UTF-8 and binary values do share their byte representation.

## Roadmap

- True server-side `.duckdb_extension` with scalar / aggregate UDFs
  (`samkhya_hll_*`, `samkhya_bloom_*`) registered through `cxx`, so DuckDB
  builds sketches inside vectorized operators with no row-by-row hop into
  Rust. Scheduled for the next minor.
- Logical-type-aware value hashing (see caveats above).
- Plan-fingerprint extraction from `EXPLAIN (FORMAT JSON)` instead of
  scraping the textual operator tree.

## Integration

A standalone application can load a snapshot through `samkhya-iceberg` and
inspect it through `sidecar::decode_portable_column`. With `bundled` enabled,
it can also build sketches from DuckDB queries and persist them through
`samkhya-core::puffin`. These are client APIs; native DuckDB optimizer
injection remains outside this crate's current contract.

## License

Apache-2.0. Sole author: Prateek Singh.
