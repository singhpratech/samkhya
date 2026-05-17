# samkhya-cli

[![crates.io](https://img.shields.io/crates/v/samkhya-cli.svg)](https://crates.io/crates/samkhya-cli)
[![docs.rs](https://docs.rs/samkhya-cli/badge.svg)](https://docs.rs/samkhya-cli)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

The operator-facing CLI for samkhya. Surfaces the same primitives that
`samkhya-core` exposes to embedded engines — Puffin sidecars, sketches,
feedback stores — so operators can debug a production sidecar, inspect a
feedback database, or build a sketch from a CSV without writing any Rust.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## What this crate provides

A single binary, `samkhya`, with four top-level subcommands:

```
samkhya
├── inspect <path>           dump a Puffin sidecar
├── stats <path>             summarize a FeedbackStore SQLite file
├── sketch
│   ├── hll                  HyperLogLog (distinct count)
│   ├── bloom                Bloom filter (membership)
│   ├── cms                  Count-Min sketch (frequency)
│   └── histogram            equi-depth histogram (range)
└── puffin
    ├── pack                 bundle sketch payloads into one .puffin file
    └── verify               full structural validation
```

Every sketch builder reads a CSV by 0-based column index. Pass `--header`
when the CSV has a header row.

## Quick start

```sh
# Inspect any Puffin sidecar — footer JSON plus decoded sketch summaries.
samkhya inspect ./stats.puffin

# Build an HLL sketch from column 3 of a CSV.
samkhya sketch hll \
    --input rows.csv \
    --column 3 \
    --precision 14 \
    --header \
    --output col3.hll

# Bundle several sketch payloads into one Puffin sidecar.
samkhya puffin pack stats.puffin \
    --hll col3.hll \
    --bloom col3.bloom \
    --cms col3.cms \
    --histogram col0.hist

# Validate a sidecar end-to-end (footer, every blob, every decoded payload).
samkhya puffin verify stats.puffin
```

## Subcommand reference

### `inspect <path>`

Dump the sidecar's footer (JSON) and decode every blob whose `kind` matches
a known samkhya sketch. Unknown kinds are listed but not decoded — that's
the Puffin coexistence contract.

### `stats <path>`

Open a `FeedbackStore` SQLite file and print total observations, distinct
template hashes, latency percentiles, and per-template avg/max q-error.

### `sketch bloom`

```sh
samkhya sketch bloom \
    --input rows.csv --column 3 \
    --capacity 1000000 --fp-rate 0.01 \
    --header --output col3.bloom
```

### `sketch cms`

```sh
samkhya sketch cms \
    --input rows.csv --column 3 \
    --depth 5 --width 1024 \
    --header --output col3.cms
```

### `sketch histogram`

Numeric-only: column cells must parse as `f64`; empty cells are skipped.

```sh
samkhya sketch histogram \
    --input rows.csv --column 0 \
    --buckets 64 \
    --header --output col0.hist
```

### `puffin pack`

Wrap one or more sketch payload files (produced by `samkhya sketch ...
--output`) into a single Puffin sidecar with the correct KIND tags. Any
flag may be repeated to bundle multiple sketches of the same kind. The
packer decodes each payload through the matching `Sketch::from_bytes`
before writing, so a corrupt input fails fast.

### `puffin verify`

Full structural validation — parses the footer, reads every blob, and
re-decodes any known-kind payload. Exits non-zero on the first failure.

## Feature flags

This crate has no cargo features. It depends on `samkhya-core` and `clap`;
the binary builds with a stock Rust toolchain and links no native
libraries beyond what `rusqlite` already vendors.

## Exit codes

- `0` on success
- `1` on any operational error (invalid sketch, missing file, decode
  failure, verify rejection)
- `2` on CLI usage error (clap-driven)

## Integration

The CLI is the operator escape hatch: every primitive an embedded engine
adapter uses (sketch construction, Puffin pack/verify, FeedbackStore
introspection) is also reachable from the shell. A typical workflow is to
build sketches in a nightly ELT batch with `samkhya sketch ... --output`,
bundle them with `samkhya puffin pack`, then verify the resulting sidecar
in CI with `samkhya puffin verify`.

## License

Apache-2.0. Sole author: Prateek Singh.
