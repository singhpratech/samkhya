# samkhya-duckdb

[![crates.io](https://img.shields.io/crates/v/samkhya-duckdb.svg)](https://crates.io/crates/samkhya-duckdb)
[![docs.rs](https://docs.rs/samkhya-duckdb/badge.svg)](https://docs.rs/samkhya-duckdb)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Client-side DuckDB integration for samkhya. Runs SQL against an embedded
DuckDB connection and digests the result rows into samkhya's portable HLL
and Bloom-filter sketches, which then serialize through the same Puffin-blob
path used by every other engine adapter.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## What this crate provides

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
  and no system-installed `libduckdb` is required. With the feature
  disabled, this crate exposes no symbols and a bare
  `cargo check -p samkhya-duckdb` builds in seconds with no C++ toolchain
  in scope — deliberate so workspace CI can exclude the heavy build until
  it's needed.

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

Tested against `duckdb = "1.0"`. The sketches this crate produces are
byte-for-byte compatible with sketches built by `samkhya-datafusion`,
`samkhya-polars`, or any other adapter — that is the point of pushing the
digest into `samkhya-core` rather than into engine-specific code.

## Roadmap

- True server-side `.duckdb_extension` with scalar / aggregate UDFs
  (`samkhya_hll_*`, `samkhya_bloom_*`) registered through `cxx`, so DuckDB
  builds sketches inside vectorized operators with no row-by-row hop into
  Rust. Scheduled for the next minor.
- Logical-type-aware value hashing (see caveats above).
- Plan-fingerprint extraction from `EXPLAIN (FORMAT JSON)` instead of
  scraping the textual operator tree.

## Integration

A standalone application embedding DuckDB pulls in `samkhya-duckdb` with
`features = ["bundled"]`, builds sketches against its tables, and writes
them to Puffin sidecars via `samkhya-core::puffin`. Another DuckDB process
(or DataFusion, or Polars) reads the same sidecars later — without
re-scanning, and without coupling either engine to the other.

## License

Apache-2.0. Sole author: Prateek Singh.
