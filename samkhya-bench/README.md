# samkhya-bench

Benchmark harness for [samkhya](../), the portable cardinality correction
library for embedded analytical engines. Compares samkhya-corrected query
plans against the engine's native plans on three corpora:

- **JOB-Slow** — the worst-case subset of the Join Order Benchmark on the
  IMDb schema.
- **TPC-H** — the canonical decision-support benchmark; the harness
  initially focuses on Q1, Q5, Q9, Q17, and Q21.
- **STATS-CEB** — the cardinality-estimation benchmark on the
  StackExchange schema.

## Status

Scaffolding. The CLI wiring, query corpus, and runner trait are in place;
the engine adapters (DataFusion, DuckDB) are wired up in sibling crates and
will be invoked once the optimizer hooks land.

## Usage

```sh
# List bundled suites and query counts.
cargo run -p samkhya-bench -- list-queries

# Run a suite with samkhya cardinality correction enabled.
cargo run -p samkhya-bench -- run --suite job-slow

# Run the engine's native plan only, as a baseline.
cargo run -p samkhya-bench -- run --suite tpc-h --baseline

# Render a report from a previous run.
cargo run -p samkhya-bench -- report
```

## Layout

- `src/main.rs` — clap-derive CLI entry point.
- `src/lib.rs` — library facade exposing `queries` and `runner`.
- `src/queries/` — per-suite query corpora as `&'static str` (hermetic,
  no network fetch at runtime).
- `src/runner.rs` — runner struct that will drive the engine adapters.

## License

Apache-2.0, inherited from the workspace.
