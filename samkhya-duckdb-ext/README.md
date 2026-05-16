# samkhya-duckdb-ext

Server-side **DuckDB extension** for samkhya — a loadable
`.duckdb_extension` that exposes samkhya's portable sketches as DuckDB
SQL functions and feeds a `_samkhya_stats` metadata table back into
the planner during cardinality estimation.

## Status: scaffolding (v0.7.0 roadmap)

This crate is the v0.7.0 deliverable from `ROADMAP.md` §5. It ships:

- a working Rust ↔ C++ `cxx` bridge surface,
- a C++ stub with the DuckDB extension entrypoint declared,
- the build plumbing wired up behind an opt-in Cargo feature,
- documented hand-off points for the remaining DuckDB API calls.

Full DuckDB function registration plus the optimizer-extension hook is
a multi-week C++ effort tracked separately. The next round should not
need to redesign the integration surface — only fill in the DuckDB API
calls inside `src/extension.cpp`.

## Two DuckDB crates — which is which

This crate is **not** the same as `samkhya-duckdb`. They sit on
opposite sides of the DuckDB process boundary:

| Crate | Role | C++ toolchain required |
|---|---|---|
| `samkhya-duckdb` | Rust *client* that opens a DuckDB connection and reads rows. Builds sketches outside the engine. | Only with the `bundled` feature. |
| `samkhya-duckdb-ext` *(this crate)* | `.duckdb_extension` that DuckDB *loads* at runtime. Runs *inside* the engine. | Yes — for the `extension` feature. |

The client crate is the "workaround tier" from v0.4.0; this crate is
the graduation path.

## Default build (no C++ toolchain needed)

```bash
cargo check -p samkhya-duckdb-ext
cargo build  -p samkhya-duckdb-ext
```

These commands work on a minimal image with **no DuckDB headers and no
C++ compiler**. With the default feature set the crate compiles to an
empty cdylib so the workspace stays buildable on small CI runners.

## Building the actual extension

The `extension` Cargo feature enables the cxx bridge and compiles
`src/extension.cpp`.

### Prerequisites

1. **C++17 toolchain** — recent clang or gcc.
2. **DuckDB extension headers** — the simplest way to obtain them is
   to clone DuckDB's official extension template:

   ```bash
   git clone https://github.com/duckdb/extension-template ~/duckdb-ext-tpl
   cd ~/duckdb-ext-tpl
   git checkout v1.2.x   # pin per ROADMAP §5 risk note
   make pull                                # fetches duckdb/ submodule
   ```

   This leaves the DuckDB headers at
   `~/duckdb-ext-tpl/duckdb/src/include`.

3. **`DUCKDB_INCLUDE_DIR` env var** pointing at those headers:

   ```bash
   export DUCKDB_INCLUDE_DIR=~/duckdb-ext-tpl/duckdb/src/include
   ```

### Build

```bash
cargo build -p samkhya-duckdb-ext --release --features extension
```

The resulting cdylib lands at:

```
target/release/libsamkhya_duckdb_ext.so       # Linux
target/release/libsamkhya_duckdb_ext.dylib    # macOS
target/release/samkhya_duckdb_ext.dll         # Windows
```

If this build fails with `fatal error: 'duckdb.hpp' file not found`,
the DuckDB headers are missing or `DUCKDB_INCLUDE_DIR` is unset. That
is the expected failure mode for environments that haven't set up the
extension template — the default `cargo check` path stays green.

### Install

Copy the cdylib into DuckDB's per-user extension directory, renaming
the suffix to `.duckdb_extension`:

```bash
DUCKDB_VER=1.2.0        # match the DuckDB version you built against
ARCH=$(uname -m)        # e.g. x86_64 / arm64
PLATFORM=linux_amd64    # or osx_arm64 etc. — see DuckDB docs

DEST=~/.duckdb/extensions/v${DUCKDB_VER}/${PLATFORM}
mkdir -p "$DEST"
cp target/release/libsamkhya_duckdb_ext.so \
   "$DEST/samkhya_duckdb_ext.duckdb_extension"
```

Unsigned extensions require DuckDB to run with
`allow_unsigned_extensions=true`:

```bash
duckdb -unsigned
```

or, inside a session:

```sql
SET allow_unsigned_extensions = true;
```

### Load

```sql
LOAD 'samkhya_duckdb_ext';
```

### Use

Once the C++ side is filled in (see "Outstanding work" below), the
SQL surface will look like:

```sql
-- Build a portable HLL sketch over a column.
SELECT hll_sketch_estimate(hll_sketch_build(user_id)) AS approx_users
FROM events;

-- Register a Puffin sidecar so the planner sees samkhya stats.
CALL register_puffin('events', '/data/events.parquet.puffin');
```

The bridged Rust API that backs those SQL functions is declared in
`src/lib.rs` inside the `#[cxx::bridge]` block; the cdylib's
entrypoint is `samkhya_duckdb_ext_init` in `src/extension.cpp`.

## Outstanding work (next round)

The remaining tasks are *DuckDB-side*, not Rust-side:

1. **Scalar / aggregate function registration.** Replace the
   `TODO(v0.7.0-followup)` placeholders in `extension.cpp` with real
   `ScalarFunction` / `AggregateFunction` constructors and call
   `ExtensionUtil::RegisterFunction`. The aggregate state is the
   `rust::Box<samkhya::HllSketch>` produced by `hll_new`; thread it
   through DuckDB's state-init / update / combine / finalize callbacks.
2. **`_samkhya_stats` table.** Create the metadata table on extension
   init via DuckDB's catalog API, with columns
   `(schema, table, column, distinct_count, sketch_blob)`.
3. **`register_puffin` SQL function.** Reads a Puffin sidecar (use
   the `samkhya-core::puffin` reader from the Rust side via a new
   bridge function), inserts rows into `_samkhya_stats`, and bumps a
   "stats version" counter the planner sees.
4. **`OptimizerExtension`.** Subclass `duckdb::OptimizerExtension` and
   register it in `samkhya_duckdb_ext_init`. On invocation, walk the
   `LogicalOperator` tree, match `LogicalGet` nodes against
   `_samkhya_stats`, and inject `distinct_count` overrides.
5. **CI matrix job.** Add a CI configuration that performs the
   `extension`-feature build with the DuckDB headers cached, per the
   `ROADMAP.md` §5 risk note. Expect a >30 min cold run; cache the
   DuckDB source checkout aggressively.

Once those land, the bench harness's `--engine duckdb` path
(`bench run --suite job-slow --engine duckdb`) will exercise the
extension end-to-end against the same Puffin sidecars DataFusion
already consumes — the portability moat the project is built around.

## License

Apache-2.0. See `LICENSE-APACHE` at the repository root.
