//! Capture `(estimated, actual)` row-count pairs from a DuckDB query
//! and surface them as a portable [`Observation`] for the feedback-driven
//! residual corrector.
//!
//! "Estimated" is sourced from DuckDB's `EXPLAIN` output on a best-effort
//! basis — the optimizer prints an `Estimated Cardinality` line per
//! operator; we scrape the largest value as a proxy for the plan-root
//! estimate. If we can't find one we fall back to `0`, which the
//! corrector treats as a missing prior.
//!
//! "Actual" is captured by running `SELECT count(*) FROM (<sql>) t`,
//! which DuckDB pushes through to the underlying operators without
//! materializing the full result set when the planner can avoid it.
//!
//! These choices keep the integration self-correcting without requiring
//! a DuckDB extension hook: the same client connection answers both
//! questions.

use samkhya_core::feedback::Observation;
use samkhya_core::{Error, Result};

use duckdb::Connection;

fn map_duck_err(e: duckdb::Error) -> Error {
    Error::Feedback(format!("duckdb: {e}"))
}

/// Extract the largest `Estimated Cardinality` number from DuckDB's
/// `EXPLAIN` text output, or `0` if none is found.
fn parse_largest_estimate(explain: &str) -> u64 {
    let needle = "Estimated Cardinality:";
    explain
        .lines()
        .filter_map(|line| {
            line.find(needle).and_then(|idx| {
                let tail = line[idx + needle.len()..].trim();
                let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<u64>().ok()
            })
        })
        .max()
        .unwrap_or(0)
}

/// Run `EXPLAIN <sql>` and return the highest reported estimated
/// cardinality across all printed operators, as a best-effort proxy for
/// the plan-root estimate.
pub fn estimate_rows(conn: &Connection, sql: &str) -> Result<u64> {
    let explain_sql = format!("EXPLAIN {sql}");
    let mut stmt = conn.prepare(&explain_sql).map_err(map_duck_err)?;
    let mut rows = stmt.query([]).map_err(map_duck_err)?;
    let mut buf = String::new();
    while let Some(row) = rows.next().map_err(map_duck_err)? {
        // DuckDB's EXPLAIN returns two columns: (explain_key, explain_value).
        // We only care about the textual physical plan.
        if let Ok(text) = row.get::<_, String>(1) {
            buf.push_str(&text);
            buf.push('\n');
        }
    }
    Ok(parse_largest_estimate(&buf))
}

/// Run `SELECT count(*) FROM (<sql>) t` and return the result.
pub fn actual_rows(conn: &Connection, sql: &str) -> Result<u64> {
    let count_sql = format!("SELECT count(*) FROM ({sql}) t");
    let n: i64 = conn
        .query_row(&count_sql, [], |row| row.get(0))
        .map_err(map_duck_err)?;
    Ok(n.max(0) as u64)
}

/// Capture an [`Observation`] for `sql`, using the provided template
/// hash and plan fingerprint identifiers. Both `est` and `actual` are
/// derived from `conn` using the helpers above.
pub fn capture_observation(
    conn: &Connection,
    sql: &str,
    template_hash: impl Into<String>,
    plan_fingerprint: impl Into<String>,
) -> Result<Observation> {
    let est = estimate_rows(conn, sql)?;
    let actual = actual_rows(conn, sql)?;
    Ok(Observation {
        template_hash: template_hash.into(),
        plan_fingerprint: plan_fingerprint.into(),
        est_rows: est,
        actual_rows: actual,
        latency_ms: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_largest_estimate_across_operators() {
        let explain = "\
            PROJECTION\n  Estimated Cardinality: 42\n\
            HASH_JOIN\n  Estimated Cardinality: 1000\n\
            SCAN\n  Estimated Cardinality: 500\n";
        assert_eq!(parse_largest_estimate(explain), 1000);
    }

    #[test]
    fn parse_returns_zero_when_no_estimate_present() {
        assert_eq!(parse_largest_estimate("no estimates here\n"), 0);
    }
}
