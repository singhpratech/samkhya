# samkhya-cli

`samkhya` is the operator-facing CLI for the samkhya cardinality-correction
toolkit. It surfaces the same primitives that `samkhya-core` exposes to
embedded engines (Puffin sidecars, sketches, feedback stores), so operators
can debug a production sidecar, inspect a feedback database, or build a
sketch from a CSV without writing any Rust.

## Build

```sh
cargo build -p samkhya-cli              # debug binary → target/debug/samkhya
cargo build -p samkhya-cli --release    # release binary → target/release/samkhya
```

## Top-level layout

```
samkhya
├── inspect <path>           dump a Puffin sidecar
├── stats <path>             summarize a FeedbackStore SQLite file
├── sketch
│   ├── hll                  HyperLogLog (distinct count)
│   ├── bloom                Bloom filter (membership)
│   ├── cms                  Count-Min Sketch (frequency)
│   └── histogram            Equi-depth histogram (range)
└── puffin
    ├── pack                 bundle sketch payloads into one .puffin file
    └── verify               full structural validation
```

Every sketch builder reads a CSV by 0-based column index. Pass `--header`
when the CSV has a header row.

## inspect

Dump a sidecar's footer (JSON) and decode every blob whose `kind` matches
a known samkhya sketch.

```sh
samkhya inspect ./stats.puffin
```

## stats

Open a `FeedbackStore` SQLite file and print total observations, distinct
template hashes, latency percentiles, and per-template avg/max q-error.

```sh
samkhya stats ./feedback.db
```

## sketch hll

```sh
samkhya sketch hll \
  --input rows.csv \
  --column 3 \
  --precision 14 \
  --header \
  --output col3.hll
```

## sketch bloom

```sh
samkhya sketch bloom \
  --input rows.csv \
  --column 3 \
  --capacity 1000000 \
  --fp-rate 0.01 \
  --header \
  --output col3.bloom
```

## sketch cms

```sh
samkhya sketch cms \
  --input rows.csv \
  --column 3 \
  --depth 5 \
  --width 1024 \
  --header \
  --output col3.cms
```

## sketch histogram

The numeric-only path: column cells must parse as `f64`. Empty cells are
skipped.

```sh
samkhya sketch histogram \
  --input rows.csv \
  --column 0 \
  --buckets 64 \
  --header \
  --output col0.hist
```

## puffin pack

Wrap one or more sketch payload files (produced by `samkhya sketch ...
--output`) into a single Puffin sidecar with the correct KIND tags. Any
flag may be repeated.

```sh
samkhya puffin pack stats.puffin \
  --hll col3.hll \
  --bloom col3.bloom \
  --cms col3.cms \
  --histogram col0.hist
```

The packer decodes each payload through the matching `Sketch::from_bytes`
before writing, so a corrupt input fails fast.

## puffin verify

Full structural validation — parses the footer, reads every blob, and
re-decodes any known-kind payload. Exits non-zero on the first failure.

```sh
samkhya puffin verify stats.puffin
```

## Exit codes

- `0` on success
- `1` on any operational error (invalid sketch, missing file, decode
  failure, verify rejection)
- `2` on CLI usage error (clap-driven)
