# Changelog

All notable changes to **samkhya** are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
honors [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
