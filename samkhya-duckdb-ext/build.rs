//! Build script for samkhya-duckdb-ext.
//!
//! Without the `extension` feature, this is a no-op: a plain
//! `cargo check -p samkhya-duckdb-ext` succeeds without a C++ toolchain
//! and without DuckDB headers being present on disk.
//!
//! With `--features extension`, we invoke `cxx_build` to:
//!   1. Generate the C++ side of the bridge from the `#[cxx::bridge]`
//!      module in `src/lib.rs`.
//!   2. Compile `src/extension.cpp`, the DuckDB-facing stub that
//!      registers samkhya's sketches as DuckDB scalar / aggregate
//!      functions.
//!   3. Link the result into the final cdylib that DuckDB will load.
//!
//! DuckDB's own headers are consumed by `extension.cpp` only. We expect
//! the consumer to have run the standard DuckDB extension-template
//! workflow (see README.md) so that the include path resolves. If
//! `DUCKDB_INCLUDE_DIR` is set in the environment we forward it as a
//! `-I` flag; otherwise we rely on the system include path.

#[cfg(feature = "extension")]
fn main() {
    use std::env;

    // Tell cargo to rerun the build script if the sources change.
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/extension.cpp");
    println!("cargo:rerun-if-env-changed=DUCKDB_INCLUDE_DIR");

    let mut build = cxx_build::bridge("src/lib.rs");
    build.file("src/extension.cpp").std("c++17");

    // Forward the DuckDB extension headers if the consumer set the env
    // var. Without this, compilation of extension.cpp will fail at the
    // `#include "duckdb.hpp"` line — that failure is intentional and
    // documented in README.md.
    if let Ok(duckdb_inc) = env::var("DUCKDB_INCLUDE_DIR") {
        build.include(duckdb_inc);
    }

    build.compile("samkhya_duckdb_ext_bridge");
}

#[cfg(not(feature = "extension"))]
fn main() {
    // No-op. The cxx-build dependency is gated behind the `extension`
    // feature so it isn't even compiled in this configuration.
}
