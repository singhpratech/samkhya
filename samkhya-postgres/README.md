# samkhya-postgres

PostgreSQL adapter for [samkhya](../).

## Status

Scaffold. The integration model is settled in shape — PostgreSQL exposes
a rich set of planner / executor hooks, and the prior art is mature —
but the wiring lands in a later milestone. This crate declares the
surface today and reserves the workspace member slot.

## Prior art

- [`postgrespro/aqo`](https://github.com/postgrespro/aqo) — the closest
  working analogue. AQO hooks the planner and executor, captures
  `(plan, estimated_rows, actual_rows)` after every query, and feeds
  the deltas back into selectivity estimates on subsequent runs.
  samkhya targets the same `(plan, est, actual)` capture shape but
  keeps the sketches portable (Puffin / `samkhya_core::sketches`)
  rather than baked into a PostgreSQL-specific store.
- [`pg_qualstats`](https://github.com/powa-team/pg_qualstats) — per-qual
  selectivity statistics; useful template for how to surface the
  collected stats as SQL-queryable views.
- [`pg_hint_plan`](https://github.com/ossc-db/pg_hint_plan) — explicit
  cardinality hints via SQL comments; complementary to samkhya's
  feedback-driven correction path.

## Planned integration patterns

1. **PG extension via [pgrx](https://github.com/pgcentralfoundation/pgrx)**
   — register `planner_hook` to inject corrected cardinalities into
   `RelOptInfo::rows`, and `ExecutorEnd_hook` to capture actual row
   counts post-execution. Persist observations into a `samkhya` schema
   mirroring the `aqo_data` / `aqo_queries` layout.
2. **libpq-driven sidecar** — for environments that cannot load native
   extensions, a `tokio-postgres` sidecar scrapes
   `pg_stat_statements` plus `EXPLAIN (ANALYZE, FORMAT JSON)` output
   and writes sketches into a samkhya-managed schema, surfaced back
   into the planner via `pg_hint_plan`-style hints.
3. **Sketches in a `samkhya` schema** — HLL / Bloom / Count-Min /
   EquiDepthHistogram serialized via
   `samkhya_core::sketches::Sketch` and stored as `bytea` keyed by
   `(table_oid, attnum)`, queryable from SQL for debugging.

## Why this crate exists today

The samkhya architecture lists PostgreSQL alongside DataFusion,
DuckDB, Polars, and gpudb as integration targets. Even as a stub, this
crate:

- Reserves the workspace member slot and crate name on crates.io.
- Lets downstream users add `samkhya-postgres` to their `Cargo.toml`
  today and pick up the real integration once it lands.
- Documents the planned hook shape so contributors can map
  `postgrespro/aqo`-style prior art onto samkhya's portable,
  feedback-driven, self-correcting cardinality model.

## License

Apache-2.0, inherited from the workspace.
