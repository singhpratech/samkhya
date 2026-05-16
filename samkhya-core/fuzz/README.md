# samkhya-core fuzz targets

This subdirectory is a **standalone cargo-fuzz workspace** (note the empty
`[workspace] members = []` in `Cargo.toml`). It is intentionally isolated
from the repository-root workspace so the `libfuzzer-sys` dependency — which
requires a nightly toolchain and sanitizer-instrumented builds — never leaks
into `cargo check -p samkhya-core` or any other default-toolchain build.

## Prerequisites

```sh
cargo install cargo-fuzz             # one-time
rustup toolchain install nightly     # one-time
```

## Running

```sh
cd samkhya-core/fuzz

# Puffin sidecar reader — every byte that enters PuffinReader::open
# is attacker-controlled in production (it comes off disk or from
# a DataFusion / DuckDB / Polars caller).
cargo +nightly fuzz run puffin_reader

# Sketch decoders — HLL / Bloom / CMS / EquiDepthHistogram /
# CorrelatedHistogram2D from_bytes paths.
cargo +nightly fuzz run sketch_decoder
```

A useful local budget is **1 hour per target**; CI does not run fuzzing
(too slow for PR feedback). Any crash blocks the next release tag.

## Invariants under test

For every target the invariant is the same: **decoding arbitrary bytes must
return `Err(_)` and never panic**. Panics across the FFI boundaries that
embed samkhya (DuckDB cxx extension in v0.7.0, PyO3 in `samkhya-py`,
DataFusion's `SamkhyaTableProvider`) become aborts or undefined behavior,
so panic-freedom on attacker-controlled bytes is a release-tier guarantee.

## Adding a new target

1. Create `fuzz_targets/<name>.rs` following the existing pattern.
2. Add a `[[bin]]` entry to `Cargo.toml`.
3. Run locally for at least 5 minutes to surface obvious panics before
   committing.

## License

Apache-2.0. Sole author: Prateek Singh.
