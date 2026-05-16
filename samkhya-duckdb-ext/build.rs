//! Build script for samkhya-duckdb-ext.
//!
//! Default path: invoke `cxx_build::bridge("src/lib.rs")` to generate
//! the C++ side of the bridge, compile `src/wrapper.cc` against it with
//! `-std=c++17`, and emit the resulting object code into the crate's
//! staticlib output. This is the scaffold's working configuration: it
//! requires a C++17 compiler on PATH (clang++ or g++) but it does NOT
//! require any DuckDB headers — the wrapper only includes the
//! cxx-generated bridge header and the crate-local wrapper.h.
//!
//! Escape-hatch path (`--features no_cxx`): build.rs becomes a no-op
//! and the bridge module is excluded from compilation. This keeps
//! `cargo check -p samkhya-duckdb-ext --features no_cxx` runnable on
//! minimal images that lack a C++ toolchain entirely (some sandboxed
//! CI runners and the `cargo deny` job in particular).
//!
//! The DuckDB-side optimizer hook (DuckDB Issue #11638) is wired up in
//! v1.1; this build script intentionally does not look for DuckDB
//! headers, because nothing in src/wrapper.cc includes them yet.

#[cfg(not(feature = "no_cxx"))]
fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/wrapper.cc");
    println!("cargo:rerun-if-changed=src/wrapper.h");

    cxx_build::bridge("src/lib.rs")
        .file("src/wrapper.cc")
        .std("c++17")
        .compile("samkhya_duckdb_ext_bridge");
}

#[cfg(feature = "no_cxx")]
fn main() {
    // Escape hatch: skip cxx codegen and C++ compilation entirely.
    // See module-level doc comment for the reasoning.
    println!("cargo:rerun-if-changed=src/lib.rs");
}
