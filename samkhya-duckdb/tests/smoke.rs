//! End-to-end smoke test for the embedded DuckDB integration.
//!
//! Gated behind the `bundled` feature so that the default
//! `cargo test -p samkhya-duckdb` invocation reports zero tests (and
//! therefore still passes) on machines without a C++ toolchain.

#![cfg(feature = "bundled")]

use duckdb::Connection;
use samkhya_duckdb::feedback::capture_observation;
use samkhya_duckdb::sketcher::{build_bloom_from_query, build_hll_from_query};

/// Seed an in-memory DuckDB with 1000 rows whose `id` column has roughly
/// 500 distinct values (each id appears ~twice). Distinct count is
/// exactly 500 because `i % 500` is deterministic.
fn seed_table(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE t(id INTEGER);\n\
         INSERT INTO t SELECT (i % 500)::INTEGER FROM range(0, 1000) tbl(i);",
    )
    .expect("seed table");
}

#[test]
fn hll_from_duckdb_within_ten_percent() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    seed_table(&conn);

    let hll = build_hll_from_query(&conn, "SELECT id FROM t", 12).expect("build hll");
    let est = hll.estimate() as f64;
    let actual = 500.0_f64;
    let err = (est - actual).abs() / actual;
    assert!(
        err < 0.10,
        "HLL distinct-count estimate {est} off from {actual} by {err}"
    );
}

#[test]
fn bloom_from_duckdb_round_trips_membership() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    seed_table(&conn);

    let bloom = build_bloom_from_query(&conn, "SELECT id FROM t", 1000, 0.01).expect("build bloom");

    // Every id in [0, 500) was inserted at least once, so the filter
    // must report contains() = true (no false negatives).
    for id in 0..500_i32 {
        let key = id.to_string();
        assert!(bloom.contains(key.as_bytes()), "bloom missing id={id}");
    }
}

#[test]
fn capture_observation_returns_actual_row_count() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    seed_table(&conn);

    let obs = capture_observation(&conn, "SELECT id FROM t", "tpl-1", "plan-1")
        .expect("capture observation");
    assert_eq!(obs.actual_rows, 1000);
    assert_eq!(obs.template_hash, "tpl-1");
    assert_eq!(obs.plan_fingerprint, "plan-1");
}
