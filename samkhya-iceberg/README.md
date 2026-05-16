# samkhya-iceberg

Bridge between Apache Iceberg table snapshots and samkhya's Puffin
sidecars.

## Integration model

Iceberg has had a Puffin sidecar concept since the v2 spec: every
snapshot's manifest carries a `statistics` entry that lists Puffin
files written alongside the table. samkhya already knows how to
write Puffin files
([`samkhya_core::puffin`](../samkhya-core/src/puffin.rs)) and how to
bundle the deserialized sketches into
[`ColumnStats`](../samkhya-core/src/stats.rs); what it has been
missing is the *snapshot-aware* link that says: "for this current
table snapshot, here are the sidecar paths samkhya should look at".

This crate is that link.

Concretely, it provides:

- **`SnapshotPuffinPaths`** — an always-on contract type (no cargo
  features required) listing the Puffin sidecar paths discovered
  from a snapshot manifest. Downstream code can depend on this type
  without pulling the heavy `iceberg` crate.
- **`Schema`** — a lightweight `(field_id, name)` view used by the
  always-on `column_stats_from_paths` projection.
- **`snapshot::discover_puffin_sidecars`** (feature `iceberg`) —
  reads the current snapshot's `StatisticsFile` entries from a live
  `iceberg::table::Table` and returns a `SnapshotPuffinPaths`.
- **`snapshot::load_column_stats`** (feature `iceberg`) — composes
  `discover_puffin_sidecars` with `samkhya_core::puffin::PuffinReader`
  to deserialize samkhya sketches and project them into a
  `HashMap<usize, ColumnStats>` keyed by Iceberg field id.

## How to enable

The `iceberg` crate is large (it transitively pulls `opendal`,
`arrow`, `parquet`, `tokio`, etc.). To keep default builds slim,
the live Iceberg walker is hidden behind a non-default cargo
feature:

```toml
[dependencies]
samkhya-iceberg = { version = "0.0.1", features = ["iceberg"] }
```

Without the feature, you can still construct `SnapshotPuffinPaths`
by hand (from any source — a test harness, a Puffin-only ELT
pipeline that does not own an Iceberg table) and feed it to
`column_stats_from_paths`. This makes the crate cheap to depend
on as a *contract*: a downstream consumer can write code against
`SnapshotPuffinPaths` without committing to compiling the iceberg
dependency tree.

## Puffin-side compatibility

samkhya's Puffin blob kinds (`samkhya.hll-v1`, `samkhya.bloom-v1`,
`samkhya.cms-v1`, `samkhya.equi-depth-v1`,
`samkhya.correlated-2d-v1`) live alongside Iceberg's own blob
kinds (`apache-datasketches-theta-v1`, `deletion-vector-v1`, etc.)
in the same physical sidecar file. The Puffin v1 spec is explicit
about this: readers MUST ignore blob kinds they do not understand.

This means:

- An Iceberg-native reader that knows nothing about samkhya will
  still read its own `apache-datasketches-theta-v1` blobs out of
  a samkhya-produced sidecar.
- samkhya-iceberg's loader skips any blob kind it does not
  recognize, so it co-exists cleanly with sidecars written by
  Iceberg's own statistics-file writers (Trino, Spark, etc.).

The two ecosystems share the file format and ignore each other's
blob kinds — that is the portability moat.

## Iceberg version

This crate is currently built and tested against `iceberg = 0.9.1`
(latest at the time of writing). The original design note referenced
`0.4`; the snapshot / statistics-file shape is substantively the
same across the two, but module paths have moved. If the iceberg
crate's "current snapshot's stats files" accessor shifts again,
only the body of `snapshot::discover_puffin_sidecars` needs to
change — the public `SnapshotPuffinPaths` contract is independent
of the iceberg crate.

## Status

Working scaffold. The path-walking and Puffin-projection logic are
both implemented and exercised by `tests/smoke.rs`. A full
end-to-end test against a real in-memory `iceberg::table::Table`
fixture is gated behind `#[ignore]` until iceberg-rust ships a
stable `TableBuilder::build_for_test`-style helper for attaching
`StatisticsFile` entries to a snapshot.

## License

Apache-2.0, inherited from the workspace.
