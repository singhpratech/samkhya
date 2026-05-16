//! Fortress / adversarial harness for samkhya-duckdb (H07).
//!
//! Gated behind the `bundled` feature so `cargo test -p samkhya-duckdb`
//! without features stays a no-op.

#![cfg(feature = "bundled")]

use duckdb::Connection;
use samkhya_duckdb::feedback::capture_observation;
use samkhya_duckdb::sketcher::{build_bloom_from_query, build_hll_from_query};

// ---------- Smoke ----------

#[test]
fn fortress_smoke_count_and_join() {
    let conn = Connection::open_in_memory().expect("open in-memory duckdb");

    conn.execute_batch(
        "CREATE TABLE orders(id INTEGER, customer_id INTEGER);\n\
         INSERT INTO orders VALUES (1,100),(2,100),(3,200),(4,300),(5,300);\n\
         CREATE TABLE customers(customer_id INTEGER, name VARCHAR);\n\
         INSERT INTO customers VALUES (100,'A'),(200,'B'),(300,'C');",
    )
    .expect("seed tables");

    // SELECT COUNT(*)
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM orders", [], |r| r.get(0))
        .expect("count");
    assert_eq!(n, 5);

    // 2-way join
    let m: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM orders o JOIN customers c ON o.customer_id = c.customer_id",
            [],
            |r| r.get(0),
        )
        .expect("join count");
    assert_eq!(m, 5);

    // Exercise samkhya integration paths: sketcher + feedback.
    let hll = build_hll_from_query(&conn, "SELECT customer_id FROM orders", 12).expect("hll");
    let est = hll.estimate();
    assert!(est >= 1, "hll estimate should be >= 1, got {est}");
    // distinct customer_id = 3
    assert!(
        (est as i64 - 3).abs() <= 1,
        "hll estimate {est} far from actual 3"
    );

    let bloom =
        build_bloom_from_query(&conn, "SELECT customer_id FROM orders", 16, 0.01).expect("bloom");
    assert!(bloom.contains(b"100"));
    assert!(bloom.contains(b"200"));
    assert!(bloom.contains(b"300"));

    let obs = capture_observation(
        &conn,
        "SELECT o.id FROM orders o JOIN customers c ON o.customer_id = c.customer_id",
        "tpl-join",
        "plan-join",
    )
    .expect("capture observation");
    assert_eq!(obs.actual_rows, 5);
    assert_eq!(obs.template_hash, "tpl-join");
    assert_eq!(obs.plan_fingerprint, "plan-join");
}

// ---------- Adversarial ----------

/// Garbled SQL must surface as an `Err`, not a panic.
#[test]
fn adversarial_malformed_sql_returns_error() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    let bad = "SELEKT * FORM nope!!"; // intentional typos
    let res = build_hll_from_query(&conn, bad, 10);
    assert!(res.is_err(), "malformed SQL should be Err, got {res:?}");

    let res2 = build_bloom_from_query(&conn, bad, 8, 0.01);
    assert!(res2.is_err(), "malformed SQL should be Err, got {res2:?}");

    let res3 = capture_observation(&conn, bad, "tpl", "plan");
    assert!(
        res3.is_err(),
        "malformed SQL should surface via capture_observation"
    );
}

/// A 1MB-shaped SQL string (lots of literals, still parseable but
/// unreasonably large) must not panic. It can succeed *or* return an
/// `Err` — the point is that we bubble the result, never abort.
#[test]
fn adversarial_oversized_query_no_panic() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1),(2),(3);")
        .expect("seed");

    // Build a query string close to 1 MiB by stacking OR predicates.
    // The predicate is a no-op (x = x) so DuckDB can run it.
    let mut sql = String::with_capacity(1_100_000);
    sql.push_str("SELECT x FROM t WHERE ");
    let chunk = "x = x OR ";
    while sql.len() < 1_000_000 {
        sql.push_str(chunk);
    }
    sql.push_str("x = x");
    assert!(
        sql.len() >= 1_000_000,
        "SQL not large enough: {}",
        sql.len()
    );

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_hll_from_query(&conn, &sql, 10)
    }));
    assert!(
        res.is_ok(),
        "oversized SQL panicked instead of returning Result"
    );
    // Whatever the Result is, we accept it — DuckDB may parse it or err out
    // with a depth / size guard. Both are fine; we just refuse to panic.
    let _ = res.unwrap();
}

/// Syntactically valid SQL that returns zero rows must yield a valid
/// (empty) sketch and an `actual_rows == 0` observation — no panic, no
/// division-by-zero, no NaN.
#[test]
fn adversarial_empty_result_set_is_clean() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1),(2),(3);")
        .expect("seed");

    let sql = "SELECT x FROM t WHERE x > 9999";

    let hll = build_hll_from_query(&conn, sql, 10).expect("empty hll");
    let est = hll.estimate();
    assert_eq!(est, 0, "empty result should give estimate 0, got {est}");

    let bloom = build_bloom_from_query(&conn, sql, 8, 0.01).expect("empty bloom");
    // Nothing inserted, so a random key must NOT be present (no false
    // negatives on absent values — that's a positive-rate guarantee, but
    // for an empty filter we expect this concretely).
    assert!(!bloom.contains(b"42"));

    let obs = capture_observation(&conn, sql, "tpl-empty", "plan-empty").expect("empty obs");
    assert_eq!(obs.actual_rows, 0);
}

/// NULL-only column should be handled cleanly (single sentinel bucket).
#[test]
fn adversarial_null_only_column_collapses_to_one_bucket() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    conn.execute_batch(
        "CREATE TABLE n(v INTEGER); INSERT INTO n VALUES (NULL),(NULL),(NULL),(NULL);",
    )
    .expect("seed nulls");

    let hll = build_hll_from_query(&conn, "SELECT v FROM n", 12).expect("null hll");
    // All NULLs hash to one sentinel — estimate must be 1, not 4.
    let est = hll.estimate();
    assert!(
        est <= 2,
        "NULL sentinel should collapse to ~1 bucket, got {est}"
    );
}

/// DuckDB connection borrowed twice across queries must not deadlock the
/// adapter (we always drop statements before the next call).
#[test]
fn adversarial_sequential_queries_no_borrow_clash() {
    let conn = Connection::open_in_memory().expect("open duckdb");
    conn.execute_batch(
        "CREATE TABLE q(k INTEGER); INSERT INTO q SELECT i FROM range(0, 100) t(i);",
    )
    .expect("seed");

    for _ in 0..5 {
        let _hll = build_hll_from_query(&conn, "SELECT k FROM q", 10).expect("repeat hll");
        let _bloom =
            build_bloom_from_query(&conn, "SELECT k FROM q", 128, 0.01).expect("repeat bloom");
        let obs = capture_observation(&conn, "SELECT k FROM q", "tpl", "plan").expect("repeat obs");
        assert_eq!(obs.actual_rows, 100);
    }
}
