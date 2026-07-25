# samkhya-cli

[![crates.io](https://img.shields.io/crates/v/samkhya-cli.svg)](https://crates.io/crates/samkhya-cli)
[![docs.rs](https://docs.rs/samkhya-cli/badge.svg)](https://docs.rs/samkhya-cli)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

The operator CLI for samkhya's portable statistics. One binary, `samkhya`,
that builds sketches from a CSV column, packs them into an Iceberg Puffin
sidecar, verifies a sidecar you were handed, and summarizes a feedback-store
SQLite file — without writing any Rust.

Scope, up front: this CLI does **not** compute samkhya's provable
join-cardinality ceiling and does not train or run a corrector. It produces
and inspects the inputs those consume. The ceiling lives in
[`samkhya_core::degree`](https://docs.rs/samkhya-core/latest/samkhya_core/degree/index.html);
model training and evaluation live in `samkhya-bench`.

## Install

```sh
cargo install samkhya-cli    # installs a binary named `samkhya`
```

Needs Rust 1.85 or newer (edition 2024) and a working C compiler: `rusqlite`
is built with its `bundled` feature, which compiles SQLite from source.

## Subcommands

```
samkhya
  inspect <path>            dump a Puffin sidecar's footer, decode known kinds
  stats <path>              summarize a FeedbackStore SQLite file
  sketch hll                HyperLogLog          --precision (4..=18)
  sketch bloom              Bloom filter         --capacity --fp-rate
  sketch cms                Count-Min sketch     --depth --width
  sketch histogram          equi-depth histogram --buckets (numeric column)
  puffin pack <out>         bundle payloads into one .puffin file
  puffin verify <path>      full structural validation
```

Every `sketch` subcommand takes `--input <csv> --column <n>` with a 0-based
column index, an optional `--header` to skip the first record, and an
optional `--output <file>` to write the serialized payload. Without
`--output` it prints a summary and writes nothing.

## Example

```sh
# Build a Count-Min sketch over the join column of a CSV extract.
samkhya sketch cms \
    --input orders.csv --column 3 \
    --depth 5 --width 1024 \
    --header --output customer_id.cms

# Bundle it into a sidecar.
samkhya puffin pack orders.puffin --cms customer_id.cms

# Validate the sidecar: footer, every blob, every known payload.
samkhya puffin verify orders.puffin

# Read it back — blob kinds, offsets, decoded parameters.
samkhya inspect orders.puffin
```

`--hll`, `--bloom`, `--cms` and `--histogram` may each be repeated to pack
several payloads of the same kind. `pack` decodes each file through the
matching `Sketch::from_bytes` before writing it, so a corrupt payload fails
before the sidecar exists.

## Notes per subcommand

- `inspect` prints the footer as JSON and decodes any blob whose `kind` is
  `samkhya.hll-v1`, `samkhya.bloom-v1`, `samkhya.cms-v1`, or
  `samkhya.histogram-equidepth-v1`. Unknown kinds are listed with their raw
  length and left alone — that is the Puffin coexistence contract.
- `stats` reads only. A missing path is an error rather than a freshly
  created empty database. Prints total observations, distinct template
  hashes, p50/p90/p99 latency, and per-template average and maximum q-error.
- `sketch bloom` rejects `--capacity 0` and any `--fp-rate` outside
  `(0.0, 1.0)` before allocating, so a typo cannot drive the process into a
  multi-exabyte allocation.
- `sketch histogram` is numeric-only: every non-empty cell must parse as
  `f64`. Empty cells are skipped.

## How this feeds the ceiling

A Count-Min sketch never undercounts, so its largest counter bounds every
key's degree — that is what `AttributeDegree::from_count_min` turns into a
sound input for the join ceiling, and it is why `sketch cms` is the useful
command if a ceiling is your goal. The `estimate:` line that `sketch hll`
prints is a two-sided HyperLogLog estimate and is **not** a sound ceiling
input; the library derives a distinct-count *floor* from the same sketch via
`AttributeDegree::from_hll_floor` instead.

The bound repair shipped in 1.2 — the pre-1.2 bound family returned ceilings
below the true cardinality in 58.8% of measured trials, where 1.2 measures 0
violations — is entirely library-side. This CLI's surface is unchanged by it,
and the sidecars it wrote before 1.2 are read unchanged after. Details:
[bench-results/20_bound_soundness.md](https://github.com/singhpratech/samkhya/blob/main/bench-results/20_bound_soundness.md).

## Exit codes

- `0` success
- `1` operational error — missing file, invalid parameter, decode failure,
  `verify` rejection (the message goes to stderr, prefixed `error:`)
- `2` CLI usage error, from clap

## License

Apache-2.0. Sole author: Prateek Singh.
