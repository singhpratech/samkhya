# samkhya-iceberg

[![crates.io](https://img.shields.io/crates/v/samkhya-iceberg.svg)](https://crates.io/crates/samkhya-iceberg)
[![docs.rs](https://docs.rs/samkhya-iceberg/badge.svg)](https://docs.rs/samkhya-iceberg)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Reads samkhya's portable statistics out of Apache Iceberg Puffin sidecars,
snapshot-aware. Given an open Iceberg table — or just a list of sidecar
paths — it selects the Puffin files attached to the *current* snapshot,
decodes the samkhya blob kinds it understands, and returns engine-neutral
statistics keyed by Iceberg field id.

Part of [samkhya](https://github.com/singhpratech/samkhya): portable,
feedback-driven cardinality correction for embedded analytical engines.

## Install

```toml
[dependencies]
samkhya-iceberg = "1.2"
# To walk a live iceberg::table::Table (pulls iceberg, opendal, arrow):
samkhya-iceberg = { version = "1.2", features = ["iceberg"] }
```

## Example

Load a snapshot's sidecars, project them to scalar stats, and hand a join
key to the provable ceiling in `samkhya_core::degree`.

```rust
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_iceberg::{Schema, SnapshotPuffinPaths, load_portable_stats};
use samkhya_iceberg::try_column_stats_from_paths;

let sidecar = "/warehouse/orders/metadata/stats-42.puffin";
let paths = SnapshotPuffinPaths::from_strings(Some(42), [sidecar]);

// Fails closed: a corrupt payload, a duplicate sketch for one field, or
// a blob whose snapshot-id is not 42 is an error, not silence.
let snapshot = load_portable_stats(&paths)?;

// Keys are Iceberg field ids, not engine column ordinals.
let schema = Schema::from_fields([(3, "customer_id"), (5, "order_date")]);
let stats = try_column_stats_from_paths(&paths, &schema)?;
let customer_distinct: Option<u64> = stats[&3].distinct_count;

// Field 3 is the join key. Derive a *sound* degree from the HLL's
// register floor, not from `distinct_count` (a two-sided estimate).
let orders_rows = 10;
let decoded = snapshot.decode_column(3)?;
let degree = match decoded.as_ref().and_then(|d| d.hll()) {
    Some(hll) => AttributeDegree::from_hll_floor(orders_rows, hll),
    None => AttributeDegree::unknown(orders_rows),
};

let graph = JoinGraph::new(vec![
    JoinRelation::new(orders_rows).with_degree(0, degree),
    JoinRelation::new(100),
])
.with_edge(0, 1, 0);
let ceiling: u64 = graph.ceiling();
```

## What is decoded

Two blob kinds have a typed projection in this release: `samkhya.hll-v1`
(contributes `distinct_count`) and `samkhya.histogram-equidepth-v1`. Every
other kind — samkhya's own `samkhya.bloom-v1`, `samkhya.cms-v1`,
`samkhya.correlated2d-v1`, and Iceberg's `apache-datasketches-theta-v1` or
`deletion-vector-v1` — is skipped without its payload being fetched, which
is the Puffin v1 contract in both directions.

## Snapshot and validation rules

- Discovery keeps only `statistics` entries whose `snapshot_id` equals the
  table's current snapshot; results are sorted and deduplicated. No current
  snapshot yields an empty, snapshot-less set.
- A blob's snapshot-id (and, on the `iceberg` path, sequence-number) must
  match the resolved snapshot or be the `-1` sentinel. Otherwise: error.
- Schema field ids must be positive, and a blob referencing a field absent
  from the schema is an error.
- The `samkhya.schema-version` footer property must be absent or `"1"`.
- `column_stats_from_paths` is the fail-open v1 compatibility shim: it
  swallows the error and returns empty stats. New code should call
  `try_column_stats_from_paths` or `load_portable_stats`.

## Feeding the ceiling

samkhya clamps corrected estimates under a provable join-cardinality
ceiling (`samkhya_core::degree`). That ceiling is sound only if every
degree fed into it is an upper bound. `ColumnStats::distinct_count` is not
one: it comes from `HllSketch::estimate`, which is two-sided and can land
under the truth. Use `AttributeDegree::from_hll_floor` or `from_count_min`
instead. 1.2 repaired this bound family; `AgmBound` is deprecated.

## Scope and caveats

- Read path only. Sidecars are written with `samkhya_core::puffin`.
- The default build reads local files (a `file://` prefix is stripped).
  Object-store and catalog-backed tables need the `iceberg` feature, which
  goes through the table's configured `FileIO`.
- Mapping field ids to engine column ordinals is the adapter's job; use
  `Schema::position_of`.
- The `snapshot` module's entry points are `async` and need a tokio runtime.
- Tested against `iceberg = "0.9.1"`. The default build follows the
  workspace MSRV (1.85); the `iceberg` feature inherits that crate's 1.92.

## License

Apache-2.0. Sole author: Prateek Singh.
