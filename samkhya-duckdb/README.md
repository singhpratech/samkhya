# samkhya-duckdb

Client-side DuckDB integration for [samkhya](https://github.com/singhpratech/samkhya):
runs SQL against an embedded DuckDB connection and digests the rows into
samkhya's portable HyperLogLog and Bloom-filter sketches, which can then
be serialized through the same Puffin-blob path used by every other
engine adapter.

This crate is intentionally **client-side**. A true `.duckdb_extension`
(server-side scalar / aggregate functions registered via `cxx`, so that
DuckDB can build sketches without round-tripping rows to Rust) is on the
roadmap but not delivered here.

## Cargo feature: `bundled`

The `duckdb` crate dependency is **optional** and gated behind the
`bundled` feature flag:

```toml
[dependencies]
samkhya-duckdb = { version = "0.0.1", features = ["bundled"] }
```

- **Off by default.** A bare `cargo check -p samkhya-duckdb` (and the
  matching `cargo test`) reports zero items and builds in seconds with
  no C++ toolchain in scope. This is deliberate so workspace CI can
  exclude the heavy build until it's specifically needed.
- **With `--features bundled`** the `duckdb` crate is pulled in with its
  own `bundled` flag enabled, so `libduckdb` is compiled from source by
  `duckdb-sys` — no system-installed `libduckdb` is required.

## Integration pattern

```rust,ignore
use duckdb::Connection;
use samkhya_duckdb::sketcher::{build_hll_from_query, build_bloom_from_query};
use samkhya_duckdb::feedback::capture_observation;

let conn = Connection::open_in_memory()?;
// ... populate `conn` with tables ...

// Cardinality sketch over one column.
let hll = build_hll_from_query(&conn, "SELECT customer_id FROM orders", 14)?;
println!("distinct customers ≈ {}", hll.estimate());

// Membership filter for join pre-filtering.
let bloom = build_bloom_from_query(
    &conn,
    "SELECT customer_id FROM orders WHERE region = 'EMEA'",
    100_000,
    0.01,
)?;

// Feedback-driven self-correction: capture (estimated, actual) pairs so
// the residual corrector in `samkhya-core` can learn the engine's bias.
let obs = capture_observation(&conn, "SELECT * FROM orders", "tpl-orders", "plan-v1")?;
```

The sketches produced here are byte-for-byte compatible with sketches
built by `samkhya-datafusion`, `samkhya-polars`, or any other adapter —
that's the point of pushing the digest into `samkhya-core` rather than
into engine-specific code.

### v0 hashing caveat

The current `value_bytes` helper renders every DuckDB `ValueRef` into
its textual form before hashing. That's portable across all column
types but means floats and timestamps are hashed by their *display*
representation, not their binary layout. If you need binary fidelity,
cast the column explicitly in SQL first (`CAST(col AS BLOB)`). A future
revision will switch on logical type to avoid this caveat.

## Future work

- True server-side DuckDB extension (`.duckdb_extension`) with scalar
  and aggregate UDFs for `samkhya_hll_*` / `samkhya_bloom_*`, registered
  through `cxx`. That removes the row-by-row hop into Rust and lets
  DuckDB build sketches inside vectorized operators.
- Logical-type-aware value hashing.
- Plan-fingerprint extraction from `EXPLAIN (FORMAT JSON)` instead of
  scraping the textual operator tree.

## License

Apache-2.0.
