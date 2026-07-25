# samkhya-duckdb-ext

**This crate is a scaffold, not a DuckDB extension.** It ships a cxx
Rust/C++ bridge that makes two samkhya primitives callable from C++ — an
HLL sketch and a Puffin sidecar reader — and an empty registration hook.
There is no optimizer extension, no `_samkhya_stats` catalog table, no SQL
function, and nothing you can `LOAD` from a DuckDB shell. If you want
samkhya statistics with DuckDB today, use the client-side crate
[`samkhya-duckdb`](https://docs.rs/samkhya-duckdb), which works from
outside the engine.

The missing half is blocked upstream: DuckDB's `OptimizerExtension` has no
stable hook for plan-time cardinality overrides. That is
[DuckDB issue #11638](https://github.com/duckdb/duckdb/issues/11638).
Until it lands there is nothing to plug into.

## What the bridge actually exposes

Declared once, in `src/lib.rs`, inside `#[cxx::bridge(namespace =
"samkhya")]`:

| Symbol | Behaviour |
|---|---|
| `hll_new(p)` | `rust::Box<HllHandle>` at precision `p`; falls back to `p = 12` if `p` is outside `4..=18`. |
| `hll_add(h, bytes)` | Insert one byte string. |
| `hll_estimate(h)` | Two-sided cardinality estimate, widened to `double`. |
| `puffin_inspect(path)` | Per-blob `{kind, offset, length}`; empty vector on any I/O or parse failure. |

`samkhya::samkhya_register(duckdb::DatabaseInstance &)` is declared in
`src/wrapper.h` with an empty body in `src/wrapper.cc`. It exists so a
future implementation does not have to reshape the bridge. It does
nothing, and it never dereferences the reference it is passed —
`duckdb.hpp` is not included anywhere in this crate.

## The join-cardinality ceiling is not on this bridge

samkhya's provable join-cardinality ceiling lives in
`samkhya_core::degree` (`JoinGraph`, `JoinRelation`, `AttributeDegree`).
None of it is reachable from C++ through this crate.

If you extend the bridge yourself, note the soundness rule: `hll_estimate`
is a two-sided estimate and must **not** be fed in as a degree. Sound
degree sources are `AttributeDegree::from_hll_floor` (a distinct-count
floor, derived from nonzero registers) and `AttributeDegree::from_count_min`
(Count-Min never undercounts). Using an estimate that can land below the
truth makes the ceiling unsound.

## Calling it from C++

```cpp
#include "samkhya-duckdb-ext/src/lib.rs.h"

auto h = samkhya::hll_new(12);
samkhya::hll_add(*h, rust::Slice<const uint8_t>{ptr, len});
double ndv = samkhya::hll_estimate(*h);

auto blobs = samkhya::puffin_inspect(rust::Str{"events.puffin"});
for (const auto &b : blobs) {
    // b.kind, b.offset, b.length
}
```

The include path is cxx-build's default layout,
`<crate-name>/src/lib.rs.h`, which is what `src/wrapper.cc` uses.

## Building

```bash
cargo build -p samkhya-duckdb-ext --release
```

The default build runs `cxx_build`, generates the bridge header, and
compiles `src/wrapper.cc` with `-std=c++17`. It needs a C++17 compiler
(clang++ >= 7 or g++ >= 7) on PATH. It does not need DuckDB headers or the
DuckDB SDK.

For images with no C++ toolchain:

```bash
cargo check -p samkhya-duckdb-ext --features no_cxx
```

Under `no_cxx` the build script is a no-op and the bridge module is
excluded. The Rust functions keep their signatures and the unit tests
still run; the C++ surface is simply absent.

## Output and crate types

Crate types are `staticlib` and `rlib`:

```text
target/release/libsamkhya_duckdb_ext.a      # link from a C++ build
target/release/libsamkhya_duckdb_ext.rlib   # depend on from Rust
```

There is deliberately no `cdylib`, which is another way of saying runtime
`LOAD` is out of scope until #11638 resolves.

## Two DuckDB crates

| Crate | Role |
|---|---|
| `samkhya-duckdb` | Client-side integration that opens a connection from outside the engine. Usable now. |
| `samkhya-duckdb-ext` (this crate) | FFI meant to be linked *into* DuckDB. Bridge only. |

## License

Apache-2.0. Sole author: Prateek Singh.
