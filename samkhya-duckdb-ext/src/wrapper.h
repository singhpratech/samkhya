// =====================================================================
// samkhya-duckdb-ext / wrapper.h
//
// C++ surface that DuckDB will link against in v1.1.
//
// v1.0 ships ONLY the cxx-bridge primitives (HllHandle, puffin_inspect)
// already declared on the Rust side. This header is the place where the
// DuckDB-side `samkhya_register(DatabaseInstance &)` hook lives — it is
// forward-declared today and filled in once DuckDB Issue #11638
// (OptimizerExtension API for cardinality overrides) lands upstream.
//
// Intentional design choice: we do NOT include any DuckDB headers
// here. The forward-declared `DatabaseInstance` is an opaque type from
// the caller's perspective, and the v1.1 implementation file will
// pull in `duckdb.hpp` privately. Keeping wrapper.h header-only and
// dependency-free means `cargo build -p samkhya-duckdb-ext` works on
// any image with a C++17 compiler, no DuckDB SDK required.
//
// License: Apache-2.0. Sole author: Prateek Singh.
// =====================================================================
#pragma once

namespace duckdb {
// Forward declaration. The real type lives in duckdb.hpp; we deliberately
// do not include that header here so v1.0 builds without the DuckDB SDK.
class DatabaseInstance;
}  // namespace duckdb

namespace samkhya {

// Registration entry point for the DuckDB optimizer extension.
//
// v1.0: stub — performs no work, returns immediately. The function is
// declared so v1.1 can implement it without touching the cxx bridge
// layer in lib.rs / wrapper.cc.
//
// v1.1: walks `LogicalGet` nodes, looks up sketches in a per-table
// `_samkhya_stats` map, and overrides the planner's row-count estimate
// using the corrected number from `samkhya-core`'s corrector chain.
//
// Reference: https://github.com/duckdb/duckdb/issues/11638
void samkhya_register(::duckdb::DatabaseInstance &db);

}  // namespace samkhya
