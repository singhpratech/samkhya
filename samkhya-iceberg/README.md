# samkhya-iceberg

[![crates.io](https://img.shields.io/crates/v/samkhya-iceberg.svg)](https://crates.io/crates/samkhya-iceberg)
[![docs.rs](https://docs.rs/samkhya-iceberg/badge.svg)](https://docs.rs/samkhya-iceberg)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Bridge between Apache Iceberg snapshots and samkhya's Puffin sidecars. Walks
an Iceberg snapshot's `statistics-files` entries, resolves the Puffin paths,
and surfaces samkhya `KIND`-tagged blobs as engine-neutral `ColumnStats`.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## How it fits the Iceberg model

Iceberg already has a Puffin sidecar concept: every snapshot's manifest
carries `statistics` and `partition-statistics` entries that list Puffin
files written alongside the table. Each Puffin file holds typed blobs
identified by a `KIND` tag. Iceberg's own blob kinds
(`apache-datasketches-theta-v1`, `deletion-vector-v1`, …) live in the same
physical file as samkhya's blob kinds (`samkhya.hll-v1`, `samkhya.bloom-v1`,
`samkhya.cms-v1`, `samkhya.equi-depth-v1`, `samkhya.correlated-2d-v1`).
Per the Puffin v1 spec, readers ignore kinds they do not understand — so an
Iceberg-native reader transparently coexists with samkhya-produced sidecars.

samkhya already knows how to write Puffin files and how to bundle the
deserialized sketches into `ColumnStats`. This crate supplies the missing
*snapshot-aware* link: "for this current table snapshot, here are the
sidecar paths samkhya should look at."

## What this crate provides

- **`SnapshotPuffinPaths`** (always available, no cargo features required) —
  a list of Puffin sidecar paths discovered from a snapshot manifest, plus
  the resolved snapshot id. Constructible from any source via
  `SnapshotPuffinPaths::from_strings`, so a test harness or a Puffin-only
  pipeline that does not own an Iceberg table can still use the loader.
- **`Schema` / `SchemaField`** — a lightweight `(field_id, name)` projection
  of the Iceberg schema; the minimum needed to map Puffin blob metadata back
  onto `ColumnStats`.
- **`column_stats_from_paths`** — the loader that combines a
  `SnapshotPuffinPaths` set with `samkhya_core::puffin::PuffinReader` to
  produce a `HashMap<field_id, ColumnStats>`.
- **`snapshot::discover_puffin_sidecars`** (feature `iceberg`) — reads the
  current snapshot's `StatisticsFile` entries from a live
  `iceberg::table::Table` and returns a `SnapshotPuffinPaths`.
- **`snapshot::load_column_stats`** (feature `iceberg`) — the async
  end-to-end walker: discover sidecars, decode samkhya blobs, return
  `ColumnStats` keyed by Iceberg field id.

## Quick start

```rust
use samkhya_iceberg::{Schema, SnapshotPuffinPaths, try_column_stats_from_paths};

let paths = SnapshotPuffinPaths::from_strings(
    Some(42),
    ["orders.puffin", "lineitem.puffin"],
);

let schema = Schema::from_fields([(1, "order_id"), (2, "customer_id")]);

let stats = try_column_stats_from_paths(&paths, &schema)?;
for (field_id, col_stats) in &stats {
    println!("field {field_id}: distinct ~= {:?}", col_stats.distinct_count);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Feature flags

- `iceberg` (off by default) — pulls in the apache `iceberg` crate (0.9.x)
  plus `tokio`, and enables the async snapshot walker in the `snapshot`
  module. Off by default because the iceberg dependency tree is large
  (opendal, parquet, arrow, …). Downstream crates that only need
  `SnapshotPuffinPaths` as a contract type can depend on `samkhya-iceberg`
  without paying for that tree.

## Compatibility

Tested against `iceberg = "0.9.1"`. Builds cleanly on Rust 1.94 (the iceberg
crate's declared minimum is 1.92). Earlier design notes referenced 0.4; the
snapshot / statistics-file shape is substantively the same across the two,
but module paths moved — this crate insulates callers from that drift. If
upstream shifts again, only the body of `snapshot::discover_puffin_sidecars`
changes; the public `SnapshotPuffinPaths` contract is independent of the
iceberg crate.

## Integration

A nightly ELT job writes a Puffin sidecar containing samkhya HLL / Bloom /
histogram blobs and appends a `statistics-files` entry to the new Iceberg
snapshot. At query time, any engine adapter (`samkhya-datafusion`,
`samkhya-duckdb`) uses `samkhya-iceberg` to walk the current snapshot, locate
sidecars, and load `ColumnStats` — without coupling the engine itself to the
iceberg crate. The two ecosystems share the file format and ignore each
other's blob kinds; that is the portability moat.

## License

Apache-2.0. Sole author: Prateek Singh.
