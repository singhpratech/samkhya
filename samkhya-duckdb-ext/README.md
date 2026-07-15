# samkhya-duckdb-ext

**v1.0 cxx FFI scaffold** linking samkhya's portable cardinality
primitives into DuckDB. The Rust <-> C++ bridge is complete and
linkable today; a DuckDB optimizer hook that consumes the bridge remains a
future deliverable, pending [DuckDB Issue #11638][issue-11638]
("OptimizerExtension API for cardinality overrides").

[issue-11638]: https://github.com/duckdb/duckdb/issues/11638

## What v1.0 ships

A `staticlib + rlib` Rust crate that, when linked from a C++ build,
exposes the following symbols under the `samkhya::` C++ namespace:

| Symbol | Purpose |
|---|---|
| `samkhya::hll_new(p)` | Construct an `HllHandle` at precision `p` (4..=18; fallback p=12). |
| `samkhya::hll_add(h, bytes)` | Insert one byte-string item into the sketch. |
| `samkhya::hll_estimate(h)` | Return the current cardinality estimate as `double`. |
| `samkhya::puffin_inspect(path)` | Read a Puffin sidecar and return its blob metadata. |
| `samkhya::samkhya_register(db)` | Reserved future entrypoint; declared today with a no-op body. |

The FFI surface is declared exactly once, in `src/lib.rs`, inside a
`#[cxx::bridge(namespace = "samkhya")]` module. The cxx-build crate
generates the matching C++ header at build time; `src/wrapper.cc`
includes it and re-exposes the symbols through `src/wrapper.h`.

## Future optimizer integration

The actual DuckDB integration:

1. **Optimizer extension.** A subclass of `duckdb::OptimizerExtension`
   that walks `LogicalGet` nodes, matches `(schema, table, column)`
   tuples against a `_samkhya_stats` metadata table, and overrides
   the planner's cardinality estimate when a match is found.
2. **`_samkhya_stats` table.** Catalog-registered metadata table with
   `(schema, table, column, distinct_count, sketch_blob)` columns.
3. **`register_puffin(table, path)` SQL function.** Reads a Puffin
   sidecar via the bridge's `puffin_inspect`, hydrates the sketches,
   and inserts rows into `_samkhya_stats`.

The future implementation belongs inside `samkhya::samkhya_register` and
the new `duckdb::OptimizerExtension` subclass — the cxx bridge itself
does not need to change.

The blocker is upstream: DuckDB's `OptimizerExtension` does not yet
expose a stable hook for cardinality overrides. Issue #11638 tracks
that work; once it lands we'll pin to the resulting DuckDB tag and
fill in the body of `samkhya_register`.

## Today's call shape (from C++)

```cpp
#include "samkhya-duckdb-ext/src/lib.rs.h"

// Build an HLL sketch over an in-memory column.
auto h = samkhya::hll_new(12);
samkhya::hll_add(*h, rust::Slice<const uint8_t>{ptr, len});
double ndv = samkhya::hll_estimate(*h);

// Inspect a Puffin sidecar.
auto blobs = samkhya::puffin_inspect(rust::Str{"events.puffin"});
for (const auto& b : blobs) {
    // b.kind, b.offset, b.length
}
```

## Building

### Default (requires a C++17 toolchain)

```bash
cargo check -p samkhya-duckdb-ext
cargo build -p samkhya-duckdb-ext --release
```

The default build invokes `cxx_build`, generates the bridge header,
and compiles `src/wrapper.cc` with `-std=c++17`. It needs a C++17
compiler (clang++ >= 7 or g++ >= 7) on PATH. It does **not** need any
DuckDB headers — those only become relevant when
`samkhya_register` gains a body.

### Minimal CI (`no_cxx` feature)

For sandboxed CI runners that lack a C++ toolchain entirely:

```bash
cargo check -p samkhya-duckdb-ext --features no_cxx
```

With `--features no_cxx` the `build.rs` is a no-op, the cxx bridge
module is excluded from compilation, and the crate's Rust API stays
fully exercised by the unit tests. The C++ surface is unreachable in
that configuration — the trade-off is intentional and documented in
`Cargo.toml`.

## Output

```text
target/release/libsamkhya_duckdb_ext.a   # staticlib for the DuckDB C++ build
target/release/libsamkhya_duckdb_ext.rlib # rlib for sibling Rust crates
```

The staticlib is what future `samkhya-duckdb-ext` wiring can link
into the DuckDB extension binary. The rlib lets sibling crates
(notably `samkhya-bench`'s planned `--engine duckdb` runner) call the
Rust API directly without going through C++.

## Two DuckDB crates — which is which

This crate is **not** the same as `samkhya-duckdb`:

| Crate | Role |
|---|---|
| `samkhya-duckdb` | Rust *client* that opens a DuckDB connection from outside the engine. |
| `samkhya-duckdb-ext` *(this crate)* | FFI shipped *into* DuckDB; currently bridge-only. |

`samkhya-duckdb` is the workaround tier from earlier releases;
`samkhya-duckdb-ext` is the possible graduation path once DuckDB exposes a
stable cardinality override hook. Until then, cross-engine v1.1 coverage uses
the honest client-side consumer in `samkhya-duckdb`.

## License

Apache-2.0. Sole author: Prateek Singh.
