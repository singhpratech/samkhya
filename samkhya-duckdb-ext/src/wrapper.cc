// =====================================================================
// samkhya-duckdb-ext / wrapper.cc
//
// C++ side of the cxx bridge. Two responsibilities:
//
//   1. Pull in the cxx-generated bridge header so the Rust primitives
//      declared in lib.rs (HllHandle / puffin_inspect / ...) are
//      available to C++ callers under the `samkhya::` namespace.
//
//   2. Provide the body of `samkhya::samkhya_register` — a stub in
//      v1.0, filled in once DuckDB exposes an OptimizerExtension
//      cardinality-override API (Issue #11638).
//
// Intentional: this file does NOT include `duckdb.hpp`. The
// `DatabaseInstance` parameter is treated opaquely (we never
// dereference it in v1.0), which keeps the v1.0 build dependency-free.
//
// License: Apache-2.0. Sole author: Prateek Singh.
// =====================================================================

#include "wrapper.h"

// cxx generates this header from the #[cxx::bridge] block in src/lib.rs.
// The path matches cxx_build's default layout: <crate-name>/src/lib.rs.h.
#include "samkhya-duckdb-ext/src/lib.rs.h"

namespace samkhya {

void samkhya_register(::duckdb::DatabaseInstance & /*db*/) {
  // v1.0 stub. The real registration body lands in v1.1 against the
  // DuckDB OptimizerExtension surface tracked by Issue #11638.
  //
  // We intentionally take an unused reference so the v1.1 signature
  // doesn't need to change when the body lands.
}

}  // namespace samkhya
