# samkhya-postgres

PostgreSQL extension adapter for [samkhya](../) — portable
cardinality correction for embedded analytical engines.

This crate ships a [pgrx](https://github.com/pgcentralfoundation/pgrx)
based PostgreSQL extension that exposes samkhya's portable sketch and
Puffin sidecar primitives to SQL.

## Build modes

The crate has two build modes, controlled by the `pg_extension` Cargo
feature:

- **Default (`pg_extension` off)**: empty `rlib`. Compiles in seconds
  without PostgreSQL development headers. This is what
  `cargo check --workspace` builds in CI. Suitable for downstream
  crates that want `samkhya-postgres` in their dependency graph
  without forcing every consumer to install `libpq-dev`.
- **`pg_extension` on**: pulls in pgrx and compiles the real
  PostgreSQL loadable module. Requires PostgreSQL development headers
  (`postgresql-server-dev-NN` on Debian/Ubuntu, `postgresql-devel` on
  RHEL/Fedora) and the matching `cargo-pgrx` toolchain.

## Quickstart (extension build)

```bash
# 1. Install the pgrx CLI.
cargo install --locked cargo-pgrx

# 2. One-time pgrx init — downloads and builds the supported PG
#    majors into ~/.pgrx (skip the ones you don't need with --pg16
#    etc.). Pick the version you plan to develop against.
cargo pgrx init

# 3. From the workspace root, run the extension inside an ephemeral
#    PostgreSQL 16 (or 17) instance with psql attached.
cargo pgrx run pg16 --features pg_extension,pg16 \
    --package samkhya-postgres

# Inside psql:
#   CREATE EXTENSION samkhya_postgres;
```

## SQL surface

### `samkhya_hll_count(input anyarray) -> bigint`

Builds a samkhya `HllSketch` (precision 14) from the input array and
returns its estimated distinct-element count.

```sql
SELECT samkhya_hll_count(array_agg(id)) FROM foo;
SELECT samkhya_hll_count(ARRAY[1, 2, 2, 3, 3, 3]::int[]);
```

### `samkhya_puffin_inspect(path text) -> jsonb`

Opens an Iceberg [Puffin](https://iceberg.apache.org/puffin-spec/)
sidecar file on the server filesystem and returns per-blob metadata
(`kind`, `fields`, `offset`, `length`, `compression_codec`).

```sql
SELECT samkhya_puffin_inspect('/srv/iceberg/sketches/orders.puffin');
```

Output shape:

```json
{
  "blobs": [
    {
      "kind": "samkhya.hll-v1",
      "fields": [7],
      "offset": 4,
      "length": 16384,
      "compression_codec": null
    }
  ]
}
```

## Scope

This is the v1.0 scaffold. It establishes the extension surface,
crate layout, and pgrx feature gating. The operator-side cardinality
hook (replacing `get_relation_info_hook` so the planner picks up
samkhya's corrected row estimates without per-query SQL changes) is a
**v1.1** target.

## Testing

```bash
# Default-feature check (no PG headers required).
cargo check -p samkhya-postgres

# Extension-side unit tests (requires cargo-pgrx).
cargo pgrx test pg16 --features pg_extension,pg16,pg_test \
    --package samkhya-postgres
```

## License

Apache-2.0, inherited from the workspace.
