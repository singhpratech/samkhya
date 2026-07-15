# Repository Guidelines

## Project Structure & Module Organization

This is a 13-crate Cargo workspace. `samkhya-core/src/` contains engine-neutral sketches, statistics, Puffin I/O, feedback, LpBound, and corrector primitives. Engine adapters live in `samkhya-{datafusion,duckdb,duckdb-ext,polars,postgres,iceberg,arrow,gpudb}/src/`; tools are in `samkhya-{cli,bench,it}/`. Rust integration tests sit in each crate's `tests/`, while core examples, Criterion benches, and fuzz targets are under `samkhya-core/{examples,benches,fuzz}/`. Python bindings use `samkhya-py/src/`, `samkhya-py/python/samkhya/`, and `samkhya-py/tests/`. Documentation and measured artifacts belong in `docs/` and `bench-results/`; do not commit generated `target/`, virtualenv, or cache contents.

## Build, Test, and Development Commands

- `cargo build --locked --workspace --exclude samkhya-py` builds the standard workspace; Python bindings require PyO3 prerequisites, while DuckDB's native path is opt-in.
- `cargo fmt --all -- --check` verifies formatting.
- `cargo clippy --locked --workspace --exclude samkhya-py -- -D warnings` runs the CI lint gate.
- `cargo test --locked --workspace --exclude samkhya-py` runs the default test suite.
- `cargo run -p samkhya-bench -- list-queries` exercises the benchmark CLI without external data.

For Python work, activate a virtualenv, run `maturin develop -m samkhya-py/Cargo.toml`, then `pytest samkhya-py/tests`.

## Coding Style & Naming Conventions

The pinned developer toolchain is Rust 1.94; the workspace uses edition 2024 and a 100-column `rustfmt` limit. Use four-space indentation, `snake_case` for modules/functions/tests, `PascalCase` for types and traits, and `SCREAMING_SNAKE_CASE` for constants. Keep `samkhya-core` engine-independent. Add rustdoc for public API changes. Prefer fixing Clippy findings; any allowance must be narrow and explained.

## Testing Guidelines

Place unit tests beside implementations and cross-module behavior in `tests/*.rs`. Follow existing names such as `smoke.rs`, `fortress.rs`, and `property_*.rs`; core invariants use `proptest`, and Python uses pytest. Run one crate with `cargo test -p samkhya-core`, or filter an integration test with `cargo test -p samkhya-core --test <name> -- <pattern>`. Sketch-math changes must update statistical/property assertions and documented error bounds. No numeric coverage threshold is enforced, but changed behavior requires tests.

## Commit & Pull Request Guidelines

Conventional Commits are optional; history commonly uses subjects like `docs: ...` and `ci(deny): ...`. Write an imperative subject no longer than 72 characters and explain why in the body. Use kebab-case branches such as `fix/hll-bounds` or `feat/new-adapter`, and reference issues with `Fixes #N` or `Refs #N`.

PRs must include summary, motivation, changes, test evidence, related issues, and explicit semver impact. Discuss large redesigns or new integrations first. Include benchmark results when measurement code changes, following `bench-results/METHODOLOGY.md`; obtain one approving review before merge. Report vulnerabilities privately through GitHub Security Advisories.
