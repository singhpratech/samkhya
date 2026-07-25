# samkhya-duckdb

[![crates.io](https://img.shields.io/crates/v/samkhya-duckdb.svg)](https://crates.io/crates/samkhya-duckdb)
[![docs.rs](https://docs.rs/samkhya-duckdb/badge.svg)](https://docs.rs/samkhya-duckdb)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Client-side DuckDB integration for samkhya. It runs caller-supplied SQL through
the embedded DuckDB Rust client and digests a result column into a samkhya
sketch, decodes portable Puffin statistics written by any engine, and captures
`(estimated, actual)` row-count pairs for the feedback-driven corrector.

It is a client, not a DuckDB extension: nothing here registers a catalog object,
a SQL function, or an optimizer hook, and DuckDB's own estimates are untouched.
What it produces are statistics you feed to samkhya's provable join-cardinality
ceiling in `samkhya_core::degree`, or write to a Puffin sidecar for another
engine to read. Part of [samkhya](https://github.com/singhpratech/samkhya).

## Install

```toml
[dependencies]
samkhya-duckdb = { version = "1.2", features = ["bundled"] }
samkhya-core = "1.2"
```

`bundled` pulls in the `duckdb` crate with its own `bundled` feature, so
`libduckdb` is compiled from source and no system install is needed. It is off
by default; without it the crate still decodes portable statistics, and needs
no C++ toolchain to build.

## Example: DuckDB statistics under the ceiling

```rust
use duckdb::Connection;
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_duckdb::sketcher::build_hll_from_query;

// Caller-assigned attribute id; both sides of the join use the same one.
const ORDER_KEY: u32 = 0;

let conn = Connection::open_in_memory()?;
conn.execute_batch(
    "CREATE TABLE orders(o_key INTEGER);
     INSERT INTO orders SELECT i::INTEGER FROM range(0, 10) t(i);
     CREATE TABLE lineitem(o_key INTEGER);
     INSERT INTO lineitem SELECT (i % 10)::INTEGER FROM range(0, 100) t(i);",
)?;

// 10 rows, 10 distinct keys -- from SELECT count(*), count(DISTINCT o_key).
// An exact distinct count is a valid floor for from_distinct.
let orders = JoinRelation::new(10)
    .with_degree(ORDER_KEY, AttributeDegree::from_distinct(10, 10));

// With only a sketch, use from_hll_floor: it reads a distinct-count floor.
// HllSketch::estimate() is two-sided and would make the ceiling unsound.
let line_keys = build_hll_from_query(&conn, "SELECT o_key FROM lineitem", 14)?;
let lineitem = JoinRelation::new(100)
    .with_degree(ORDER_KEY, AttributeDegree::from_hll_floor(100, &line_keys));

let graph = JoinGraph::new(vec![orders, lineitem]).with_edge(0, 1, ORDER_KEY);
println!("ceiling = {}", graph.ceiling());
```

This prints `ceiling = 100`. The join really does produce 100 rows, so the
ceiling is exactly tight here, where the Cartesian product would say 1,000. It
is a proved upper bound on the output, not an estimate of it.

## API

- `sidecar::decode_portable_column(&PortableStatsSnapshot, i32)` — projects one
  Iceberg field ID out of a validated snapshot into `ColumnStats`, an
  `HllSketch`, and an `EquiDepthHistogram`. `Err` on a corrupt known payload,
  so corrupt statistics cannot pass as an empty column. Needs no DuckDB.
- `sketcher::build_hll_from_query(&Connection, sql, precision)` and
  `sketcher::build_bloom_from_query(&Connection, sql, capacity, fp_rate)`.
- `feedback::estimate_rows`, `feedback::actual_rows`, and
  `feedback::capture_observation(&Connection, sql, template, plan)`, which
  returns a `samkhya_core::feedback::Observation`.

`sketcher` and `feedback` require `bundled`; full signatures are on docs.rs.

## Scope and caveats

- Both sketch builders digest **column 0 only**. Pre-shape the SQL
  (`SELECT col FROM t ...`).
- `from_hll_floor` reads `HllSketch::nonzero_registers`, a floor that saturates
  at `2^p`, so on a high-cardinality column it degrades toward the row count
  rather than toward a wrong answer. `AttributeDegree::from_count_min` bounds
  far tighter under skew; this crate has no Count-Min builder yet, so build a
  `samkhya_core::sketches::CountMinSketch` from the rows yourself.
- Value hashing is textual: every `ValueRef` is rendered to its string form
  before hashing, so values that print alike but differ in logical type
  (integer `1` vs. boolean `true`) collide, and float precision follows the
  rendering. Cast in SQL (`CAST(col AS VARCHAR)` / `AS BLOB`) for binary
  fidelity. Numeric sketches built here are therefore not byte-identical to
  adapters that hash little-endian primitives; UTF-8 and binary values agree.
- `capture_observation` fills `est_rows` by scraping the largest
  `Estimated Cardinality` line out of `EXPLAIN` text as a proxy for the
  plan-root estimate, and `0` when none is found (the corrector reads `0` as a
  missing prior); `actual_rows` runs `SELECT count(*) FROM (<sql>) t`.
  `latency_ms` is always `None` — this crate does not time execution.
- No server-side extension. `samkhya-duckdb-ext` is a cxx scaffold with an
  empty registration hook: no SQL function, nothing to `LOAD`. DuckDB has no
  stable plan-time cardinality-override hook
  ([duckdb/duckdb#11638](https://github.com/duckdb/duckdb/issues/11638)).
- Tested against `duckdb = "1.0"`.

## Changed in 1.2

This crate's own API is unchanged. The bound family that shipped through 1.1 was
found unsound in a 2026-07-24 audit and is deprecated (`AgmBound` above all);
ceilings now come from `samkhya_core::degree`. Code that fed these sketches into
the old bounds should move to `JoinGraph::ceiling`.

## License

Apache-2.0. Sole author: Prateek Singh.
