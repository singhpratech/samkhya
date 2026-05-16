# Changelog

All notable changes to **samkhya** are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
honors [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-05-16

Second wave of the same scaffolding session. Real implementations replace
several v0.0.1 stubs; the architectural skeleton is now an actually-running
end-to-end pipeline against DataFusion.

### Added

- **samkhya-core**
  - `residual::gbt` submodule behind the `gbt` cargo feature. `GbtCorrector`
    trains on `Observation` history; targets `log(actual/est)` regression;
    predictions clamp via `lpbound::saturating_clamp`. Backed by `gbdt-rs`
    (Baidu, pure-Rust). 4 additional tests under `--features gbt`.
  - `puffin` zstd compression behind the `zstd` cargo feature.
    `CompressionCodec::{None,Zstd}` enum; `add_blob_compressed` /
    `read_blob_decompressed` methods; metadata-driven codec dispatch.
    3 additional tests under `--features zstd`.
  - `CorrectionFeatures::to_vec()` + `FEATURE_LEN` — stable feature
    vector layout for residual model inputs (append-only).
  - `benches/sketches.rs` (9 cases) + `benches/puffin.rs` (3 cases) —
    criterion microbenchmarks. `cargo bench --no-run` compiles cleanly.
  - `tests/properties.rs` — 9 proptest properties (HLL relative error /
    merge commutativity / round-trip, Bloom no-FN / round-trip, Puffin
    round-trip, LpBound monotonicity / clamp invariants).
  - `tests/integration.rs` — end-to-end pipeline integration test
    (HLL → Puffin → ColumnStats → FeedbackStore → lpbound).
  - `examples/sketch_to_puffin.rs` — demo binary that exercises the
    sketch → Puffin → reopen → recover path and prints relative error.
- **samkhya-datafusion**
  - `SamkhyaTableProvider<T>` — primary integration pattern. Wraps any
    `Arc<dyn TableProvider>` and overrides `statistics()` with samkhya
    corrections. Builder API: `.with_column_stats(col_idx, ColumnStats)`.
    `stats_call_count()` test hook. All values marked `Precision::Inexact`.
  - `tests/wrap_provider.rs` — integration test confirming the wrapper
    is consulted via the `TableProvider` trait surface.
  - Documented caveat: DataFusion 46's mainline planner does not yet
    drive `TableProvider::statistics()`; the hook is shaped for
    downstream optimizer rules or future DF versions.
- **samkhya-bench**
  - Real DataFusion runner. Generates a deterministic synthetic retail
    OLAP schema (customers/products/orders/order_items at 1k/200/10k/30k
    rows) and registers it via `SessionContext`. In samkhya-corrected
    mode, wraps each MemTable with `SamkhyaTableProvider`.
  - For each query: builds the physical plan to extract the optimizer's
    row estimate, executes the query, counts actual rows, computes
    multiplicative q-error, records the observation to a
    `FeedbackStore`. Prints a per-query comparison table and
    avg/max q-error.
  - New `Synthetic` suite with 5 queries (S1–S5) covering single-filter
    and 2-/3-/4-join shapes with correlated predicates.
  - `run --feedback <path>` flag to persist observations to SQLite.
  - `report --feedback <path>` subcommand — summarizes the store
    per-template; lists every observation with q-error and latency.
  - `train --feedback <path> --template <hash>` subcommand stub —
    documents the path to wire the GBT corrector against feedback
    history once `samkhya-core --features gbt` is enabled in a
    downstream build.

### Changed

- `samkhya-core/Cargo.toml` grew optional `zstd` and `gbt` features,
  plus `criterion`, `tempfile`, and `proptest` dev-deps.
- `samkhya-datafusion/Cargo.toml` added `async-trait` dep.
- `samkhya-bench/Cargo.toml` added `datafusion 46`, `samkhya-datafusion`,
  `rand`, and `tokio` deps; the binary is now `#[tokio::main]`-ish
  (uses a manually-built multi-thread runtime).

### Confirmed (the gap samkhya targets)

Running the synthetic suite against DataFusion 46 reveals:

| query | estimated | actual | q-error |
|---:|---:|---:|---:|
| S1 (single-filter) | 2000 | 3925 | 1.96 |
| S2 (2-join) | 0 | 300 | ∞ |
| S3 (2-join) | 0 | 6924 | ∞ |
| S4 (4-join) | 0 | 761 | ∞ |
| S5 (3-join) | 0 | 5223 | ∞ |

DataFusion 46 returns 0 for the multi-join cardinality estimates — i.e.
no estimate at all — for queries that actually return hundreds to
thousands of rows. This is precisely the embedded-engine cardinality
estimation gap the project targets.

### Tests

- 51 tests pass workspace-wide on the default build.
- Adding `--features gbt zstd` adds 7 more (4 GBT + 3 zstd).
- `cargo clippy -- -D warnings` passes.

### Known limitations carried over

- DataFusion 46's mainline planner does not yet propagate
  `TableProvider::statistics()` into cardinality estimates, so today
  the baseline and samkhya-wrapped runs report the same numbers.
  Resolution paths: (a) a custom DataFusion `OptimizerRule` that rewrites
  scan stats, (b) waiting for a DF release that consumes the hook, or
  (c) wrapping at the `ExecutionPlan::statistics()` layer instead.
- LpBound is still the coarse AGM approximation; full LP solver pending.
- DuckDB extension remains a stub.

## [0.0.1] — 2026-05-16

Initial scaffolding release. Sets the architectural skeleton; most layers
are wired with minimal correct implementations rather than full
production behavior. The 90-day MVP plan in `samkhya.md` §4 governs what
graduates into v0.1.0.

### Added

- **Workspace** — Cargo workspace with 5 member crates, edition 2024,
  pinned to Rust 1.94 via `rust-toolchain.toml`.
- **samkhya-core**
  - `sketches::hll` — HyperLogLog (precision 4-18, xxhash, small-range
    correction, serde-backed wire format).
  - `sketches::bloom` — Bloom filter (Kirsch-Mitzenmacher double-hashing,
    serde).
  - `sketches::Sketch` — uniform `to_bytes` / `from_bytes` codec trait
    with stable `KIND` tags so blobs round-trip cross-engine.
  - `puffin` — Iceberg Puffin sidecar reader/writer with magic / footer
    JSON / blob index. Streaming writer + lazy reader.
  - `feedback` — SQLite-backed `(plan, estimate, actual)` observation
    store. In-memory and on-disk modes. q-error helper.
  - `lpbound` — `UpperBound` trait + `ProductBound` + coarse `AgmBound`
    + `clamp_estimate` / `saturating_clamp` helpers. Pessimistic
    envelope ensures correction can never breach the ceiling.
  - `residual` — `Corrector` trait + `IdentityCorrector` baseline.
    Real backends (GBT, TabPFN) deferred.
  - `stats::ColumnStats` — engine-agnostic column statistics surface
    (superset of DataFusion's `ColumnStatistics` and DuckDB's
    `BaseStatistics`).
  - `error::Error` — thiserror-based error type with `LpBoundExceeded`
    variant for envelope violations.
- **samkhya-datafusion** — `SamkhyaOptimizerRule` against DataFusion
  46.0.1 (`ApplyOrder::BottomUp`, `supports_rewrite = true`). Walks
  TableScans and is observe-only at v0.0.1 — returns `Transformed::no`,
  cold-start-safe. `stats_provider` converts `samkhya_core::ColumnStats`
  to DataFusion's `ColumnStatistics` with `Precision::Inexact`
  throughout per the LpBound conservative posture.
- **samkhya-py** — PyO3 0.22 bindings exposing `HllSketch`,
  `BloomFilter`, `ColumnStats`, plus a `SamkhyaError` Python exception.
  `crate-type = ["cdylib", "rlib"]`, abi3-py39 for a single wheel
  covering CPython 3.9+. maturin build config.
- **samkhya-bench** — clap CLI with `list-queries` / `run --suite
  <job-slow|tpc-h|stats-ceb> [--baseline]` / `report` subcommands.
  Five hand-written JOB-Slow queries bundled (1a, 2b, 6a, 17a, 29a).
  TPC-H + STATS-CEB placeholders.
- **samkhya-duckdb** — stub crate. Full DuckDB extension (Rust ↔ C++
  via cxx) is a samkhya.md §4 Months 4-6 deliverable.
- **CI** — GitHub Actions workflow: cargo fmt, clippy `-D warnings`,
  test on push/PR. Excludes `samkhya-duckdb` (C++ toolchain) and
  `samkhya-py` (Python deps). Swatinem/rust-cache@v2.
- **Docs** — `README.md`, `ARCHITECTURE.md` (422 lines + mermaid
  diagrams), `CONTRIBUTING.md`, `samkhya.md` (full research bootstrap,
  ~400 lines, 40-entry annotated bibliography).
- **Paper drafts** — `paper/abstract.md` (236-word arXiv abstract),
  `paper/title-options.md`, `paper/outline.md` for CIDR 2027 6-page
  submission (deadline 2026-08-04).
- **Quality config** — `rustfmt.toml`, `clippy.toml`, PR template, bug
  + feature issue templates.

### Tests

- 31 unit + integration tests pass workspace-wide
  - samkhya-core: 26 (sketches 6, puffin 7, feedback 4, lpbound 8,
    residual 1)
  - samkhya-datafusion: 4 unit + 2 smoke
- `cargo clippy --workspace --exclude samkhya-py --exclude samkhya-duckdb
   -- -D warnings` passes.
- `cargo fmt --all -- --check` passes (modulo nightly-only rustfmt
  warnings).

### Naming

- Project name locked to **Samkhya** (सांख्य — "enumeration / counting").
  Originally proposed as "Drishti" during the May 2026 research sweep;
  renamed for clean PyPI / crates.io / GitHub namespace and stronger
  semantic fit. Full reasoning in `samkhya.md` §3 and `CHANGELOG`
  v0.0.1 commit history.

### Known limitations

- **LpBound** — the shipped envelope is a coarse AGM approximation.
  Full ℓp-norm LP solver port from Zhang et al. SIGMOD 2025 is a
  v0.1.0 target.
- **DataFusion rule** — observe-only; does not yet inject corrected
  estimates into the optimizer beyond placeholder column stats.
- **Residual** — no real backends shipped; identity passthrough only.
- **JOB-Slow** — five queries bundled; full set (~113 queries) is
  pending. No baseline-vs-corrected runner yet.
- **DuckDB extension** — placeholder; cxx integration pending.
- **PyO3 0.22 + edition 2024** — produces benign warnings under Rust
  1.94 (`unsafe-op-in-unsafe-fn` from `#[pymethods]` macro). Tracked
  upstream in pyo3-rs/pyo3. No functional impact.

[Unreleased]: https://github.com/singhpratech/samkhya/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/singhpratech/samkhya/releases/tag/v0.0.1
