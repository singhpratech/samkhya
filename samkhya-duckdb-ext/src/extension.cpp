// samkhya-duckdb-ext — C++ side of the cxx bridge.
//
// SCAFFOLDING NOTICE
// ==================
//
// This file is intentionally a stub. The Rust side (src/lib.rs) declares
// a working cxx bridge that wraps samkhya's portable sketches; this file
// is where DuckDB's extension API gets wired up to those sketches.
//
// The actual DuckDB function-registration syntax depends on the DuckDB
// extension API version (it changed shape between v1.0, v1.1, and v1.2).
// We pin to a specific DuckDB tag in CI — see README.md — but the calls
// below are written against the v1.2.x surface and will need adjustment
// if the upstream API shifts again.
//
// What this file is expected to do once completed:
//
//   1. Register a scalar function `hll_sketch_create(precision)` that
//      returns a `BLOB` holding a serialized empty HLL sketch.
//   2. Register an aggregate function `hll_sketch_build(col)` that
//      builds an HLL sketch over the values of `col` and returns a BLOB.
//   3. Register a scalar function `hll_sketch_estimate(blob)` that
//      deserializes a sketch BLOB and returns the cardinality estimate
//      as `UBIGINT`.
//   4. Register the corresponding Bloom-filter surface
//      (`bloom_filter_create`, `bloom_filter_insert`, `bloom_filter_contains`).
//   5. Install an `OptimizerExtension` that, on every planned query,
//      walks the logical plan and looks up `(schema, table, column)`
//      tuples in a `_samkhya_stats` metadata table. When a match is
//      found, the corresponding `distinct_count` override is fed to the
//      DuckDB cardinality estimator.
//
// Until then, this file declares the entrypoint that DuckDB will call
// (`samkhya_duckdb_ext_init`) and demonstrates how to reach the Rust
// side of the bridge. It is structured so that the next iteration can
// fill in the DuckDB API calls without touching the Rust crate.

// The cxx-generated header. Path is what cxx-build emits by default.
#include "samkhya-duckdb-ext/src/lib.rs.h"

// DuckDB extension API. Only available when the build environment has
// the DuckDB headers on its include path (set via DUCKDB_INCLUDE_DIR —
// see build.rs and README.md). When those headers are missing this
// file will fail to compile, which is the documented behaviour: see
// README.md "Prerequisites" for the install path.
//
// The `#ifdef` guard lets a future CI job exercise a "syntax only"
// pass that confirms the file parses without DuckDB present, by
// defining `SAMKHYA_DUCKDB_HEADERS_PRESENT` to 0 explicitly. Real
// extension builds will leave it undefined and pick up the headers.
#ifndef SAMKHYA_DUCKDB_HEADERS_PRESENT
#define SAMKHYA_DUCKDB_HEADERS_PRESENT 1
#endif

#if SAMKHYA_DUCKDB_HEADERS_PRESENT
#include "duckdb.hpp"
#include "duckdb/main/extension_util.hpp"
#include "duckdb/parser/parsed_data/create_scalar_function_info.hpp"
#include "duckdb/parser/parsed_data/create_aggregate_function_info.hpp"
#endif

// ---------------------------------------------------------------------
// DuckDB function-registration stubs.
//
// These are placeholders that demonstrate the call shape. Each one
// reaches into the Rust bridge to confirm linkage works end-to-end,
// then returns. The full function-vector / aggregate-state-machine
// wiring is the next round's job.
// ---------------------------------------------------------------------

namespace samkhya_duckdb_ext {

// Scalar: hll_sketch_create(precision UTINYINT) -> BLOB
//
// SQL signature once completed:
//   CREATE FUNCTION hll_sketch_create(precision UTINYINT) RETURNS BLOB
//
// Body (to be written): construct an HllSketch via the Rust bridge,
// serialize to bytes, return as a DuckDB BLOB value.
static void RegisterHllSketchCreate(/* DatabaseInstance & db */) {
    // Confirm the bridge symbol resolves; the unused-variable warning
    // documents which Rust function this hook will eventually call.
    auto sketch = samkhya::hll_new(12);
    (void)sketch;
    // TODO(v0.7.0-followup): wire up DuckDB's ScalarFunction registration:
    //   ScalarFunction fn("hll_sketch_create", {LogicalType::UTINYINT},
    //                     LogicalType::BLOB, HllSketchCreateImpl);
    //   ExtensionUtil::RegisterFunction(db, fn);
}

// Aggregate: hll_sketch_build(col ANY) -> BLOB
//
// SQL signature once completed:
//   CREATE AGGREGATE hll_sketch_build(col ANY) RETURNS BLOB
//
// State: Box<HllSketch> per group, plumbed through DuckDB's
// AggregateFunction state-init / state-combine / finalize callbacks.
static void RegisterHllSketchBuild(/* DatabaseInstance & db */) {
    // TODO(v0.7.0-followup): implement using AggregateFunction's four
    // callbacks (size, init, update, combine, finalize). The state is
    // the Rust-owned Box<HllSketch>; DuckDB only sees an opaque pointer.
}

// Scalar: hll_sketch_estimate(sketch BLOB) -> UBIGINT
//
// SQL signature once completed:
//   CREATE FUNCTION hll_sketch_estimate(sketch BLOB) RETURNS UBIGINT
//
// Body (to be written): take the input BLOB, call
// `samkhya::hll_from_bytes` to deserialize, then `samkhya::hll_estimate`.
static void RegisterHllSketchEstimate(/* DatabaseInstance & db */) {
    // TODO(v0.7.0-followup): ScalarFunction registration like above.
}

// Bloom filter mirror functions — same pattern.
static void RegisterBloomFilterFunctions(/* DatabaseInstance & db */) {
    // TODO(v0.7.0-followup): bloom_filter_create, bloom_filter_insert,
    // bloom_filter_contains.
}

// The optimizer-extension hook. This is what plugs samkhya's stats
// into DuckDB's planner.
static void RegisterOptimizerHook(/* DatabaseInstance & db */) {
    // TODO(v0.7.0-followup): subclass duckdb::OptimizerExtension and
    // register via db.config.optimizer_extensions.push_back(...).
    // On each invocation, query the `_samkhya_stats` table and supply
    // distinct_count overrides to the cardinality estimator.
}

} // namespace samkhya_duckdb_ext

// ---------------------------------------------------------------------
// DuckDB extension entrypoint.
//
// DuckDB's loader looks up the symbol `<extension_name>_init` (and a
// matching `<extension_name>_version`) when `LOAD '<extension_name>'`
// runs. The signature is `void (DatabaseInstance & db)`.
// ---------------------------------------------------------------------

extern "C" {

#if SAMKHYA_DUCKDB_HEADERS_PRESENT

// The real entrypoint. Picked up by DuckDB when the extension is loaded.
DUCKDB_EXTENSION_API void samkhya_duckdb_ext_init(duckdb::DatabaseInstance &db) {
    samkhya_duckdb_ext::RegisterHllSketchCreate(/* db */);
    samkhya_duckdb_ext::RegisterHllSketchBuild(/* db */);
    samkhya_duckdb_ext::RegisterHllSketchEstimate(/* db */);
    samkhya_duckdb_ext::RegisterBloomFilterFunctions(/* db */);
    samkhya_duckdb_ext::RegisterOptimizerHook(/* db */);
    (void)db;
}

// DuckDB also pins a version string at load time to refuse
// extensions built against an incompatible header set.
DUCKDB_EXTENSION_API const char *samkhya_duckdb_ext_version() {
    return duckdb::DuckDB::LibraryVersion();
}

#else

// Header-free build (used only for the "does this file parse?" CI
// pass): emit a no-op entrypoint with a matching signature so the
// scaffold is self-contained.
void samkhya_duckdb_ext_init(void * /* db */) {
    // Intentionally empty.
}

const char *samkhya_duckdb_ext_version() {
    return "scaffold-no-duckdb-headers";
}

#endif // SAMKHYA_DUCKDB_HEADERS_PRESENT

} // extern "C"
