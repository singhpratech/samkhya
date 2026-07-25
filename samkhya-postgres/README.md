# samkhya-postgres

[![crates.io](https://img.shields.io/crates/v/samkhya-postgres.svg)](https://crates.io/crates/samkhya-postgres)
[![docs.rs](https://docs.rs/samkhya-postgres/badge.svg)](https://docs.rs/samkhya-postgres)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

A **scaffold** for a PostgreSQL adapter to
[samkhya](https://github.com/singhpratech/samkhya). The planner-hook
integration is **not implemented**: this crate does not install
`get_relation_info_hook` or any other planner hook, so the PostgreSQL
planner never sees a samkhya row estimate. Nothing here changes a query
plan.

What it contains is the extension surface: crate layout, pgrx feature
gating, and two read-only SQL functions that expose samkhya's portable
sketch and Puffin sidecar readers, so you can check from a Postgres
session that statistics another engine wrote are readable here.
samkhya's provable join-cardinality ceiling lives in
[`samkhya_core::degree`](https://docs.rs/samkhya-core/latest/samkhya_core/degree/)
and is **not** exposed to SQL by this crate.

## Two build modes

Default: an `rlib` with one symbol, `version()`, needing no PostgreSQL
headers. Extension: a `cdylib` loadable module, needing pgrx and PG 17
server headers.

The extension is double-gated: the `pg_extension` Cargo feature **and**
the `samkhya_pgrx_enabled` rustc cfg. Both are required. The cfg gate
exists so `cargo check --workspace --all-features` stays green on hosts
with no PostgreSQL development headers — under `--all-features`,
`pg_extension` alone is a deliberate no-op, because the pgrx dependency
sits under `[target.'cfg(samkhya_pgrx_enabled)'.dependencies]` and is
dropped from the dependency graph.

There are no `pg13`..`pg16` features. As of 1.2.1 the crate pins
`pgrx/pg17` only; pgrx 0.12's build script panics when several
`pg$VERSION` features are active at once, which is what a workspace-wide
`--all-features` gate would do.

## Default build

```bash
cargo check -p samkhya-postgres
```

Compiles in seconds, no PostgreSQL headers. The only public item is:

```rust
assert!(!samkhya_postgres::version().is_empty()); // crate version string
```

## Extension build

```bash
cargo install --locked cargo-pgrx --version 0.12.9
cargo pgrx init --pg17 download

RUSTFLAGS="--cfg=samkhya_pgrx_enabled" \
  cargo pgrx run pg17 --features pg_extension --package samkhya-postgres

# then, in the psql session cargo pgrx run opens:
#   CREATE EXTENSION samkhya_postgres;
```

Omitting `RUSTFLAGS` silently builds the stub `rlib` instead of the
extension — the most common way to get a confusing "function does not
exist" from `psql`.

**Known gap (1.2.1):** the pgrx module imports `serde_json`, which this
crate does not declare as a dependency, so the extension path does not
compile as published. Add `serde_json = "1"` under
`[target.'cfg(samkhya_pgrx_enabled)'.dependencies]` to build it locally.
CI only builds the default mode, which is why this went unnoticed.

## SQL surface

`samkhya_hll_count(input anyarray) -> bigint` — builds a
`samkhya_core::sketches::HllSketch` at precision 14 (~16 KiB registers,
~0.81% relative standard error) over the array elements and returns the
distinct-count estimate. NULL elements are skipped. Elements are hashed
by their raw datum bytes, so two values count as equal iff their
in-memory representation is bitwise equal — correct for fixed-width
types, but pre-canonicalize varlena inputs.

```sql
SELECT samkhya_hll_count(ARRAY[1, 2, 2, 3, 3, 3]::int[]::anyarray);
```

`samkhya_puffin_inspect(path text) -> jsonb` — opens an Iceberg
[Puffin](https://iceberg.apache.org/puffin-spec/) sidecar on the server
filesystem and returns per-blob metadata: `kind`, `fields`, `offset`,
`length`, `compression_codec`.

```sql
SELECT samkhya_puffin_inspect('/srv/iceberg/sketches/orders.puffin');
-- {"blobs":[{"kind":"samkhya.hll-v1","fields":[7],"offset":4,
--            "length":16384,"compression_codec":null}]}
```

`path` is read with the postmaster's filesystem privileges. Do not grant
`EXECUTE` on it to untrusted roles.

## Scope and caveats

- No planner hook, no `pg_statistic` writer, no cost-model changes.
  Deferred deliberately: it needs deeper pgrx hook plumbing than belongs
  in a scaffold.
- PostgreSQL 17 only. pgrx 0.12.
- The extension path is not covered by CI and has no integration tests
  beyond one `#[pg_test]` sanity check on `samkhya_hll_count`.
- `cargo pgrx test pg17 --features pg_extension,pg_test` runs that test;
  it needs the same `RUSTFLAGS` cfg.

## License

Apache-2.0, inherited from the workspace.
