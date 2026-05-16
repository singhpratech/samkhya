//! Parse-only smoke test for the JOB / JOB-Slow query corpus.
//!
//! Goal: prove every one of the 113 canonical Leis VLDB 2015 queries (winkyao
//! commit `8e337db25d6b810cacab83cc7131e6b9d75220ee`) parses cleanly through
//! DataFusion 46's SQL frontend. No data, no execution — just the parser.
//!
//! This is intentionally cheap and hermetic so CI catches any future edit
//! that breaks the SQL strings before the heavier `imdb_smoke` test gets
//! anywhere near a CSV dump.

use datafusion::sql::parser::DFParser;
use samkhya_bench::queries::{Suite, job_slow};

#[test]
fn job_slow_roster_has_113_queries() {
    assert_eq!(
        job_slow::QUERIES.len(),
        113,
        "expected 113 canonical JOB queries"
    );
}

#[test]
fn job_slow_no_placeholder_sql_remaining() {
    let placeholders: Vec<&str> = job_slow::QUERIES
        .iter()
        .filter(|q| !job_slow::has_sql(q))
        .map(|q| q.name)
        .collect();
    assert!(
        placeholders.is_empty(),
        "queries still carrying placeholder SQL: {:?}",
        placeholders
    );
}

#[test]
fn every_job_slow_query_parses_via_datafusion() {
    let mut failures: Vec<(String, String)> = Vec::new();
    for q in job_slow::QUERIES {
        match DFParser::parse_sql(q.sql) {
            Ok(_) => {}
            Err(e) => failures.push((q.name.to_string(), format!("{e}"))),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} JOB-Slow queries failed to parse: {:#?}",
        failures.len(),
        job_slow::QUERIES.len(),
        failures
    );
}

#[test]
fn job_slow_real_suite_returns_same_113_queries() {
    let qs = Suite::JobSlowReal.queries();
    assert_eq!(qs.len(), 113);
    // Spot-check a JOB-Slow flagged query is in the roster.
    assert!(qs.iter().any(|q| q.name == "33c"));
}
