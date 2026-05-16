//! `samkhya stats <path>` — summarize a FeedbackStore SQLite file.

use std::path::Path;

use rusqlite::Connection;
use samkhya_core::feedback::Observation;
use samkhya_core::{Error, Result};

fn map_sqlite(e: rusqlite::Error) -> Error {
    Error::Feedback(e.to_string())
}

#[derive(Debug)]
struct TemplateRow {
    template_hash: String,
    n: u64,
    avg_q: f64,
    max_q: f64,
}

pub fn run(path: &Path) -> Result<()> {
    let conn = Connection::open(path).map_err(map_sqlite)?;

    // Total observations.
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    println!("== feedback store: {} ==", path.display());
    println!("total observations: {total}");

    // Distinct template hashes.
    let distinct: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT template_hash) FROM observations",
            [],
            |row| row.get(0),
        )
        .map_err(map_sqlite)?;
    println!("distinct templates:  {distinct}");

    if total == 0 {
        return Ok(());
    }

    // Latency percentiles — collect non-null latencies into memory and
    // compute p50 / p90 / p99 by sort + index. For an operator-facing
    // CLI this is fine; the alternative (window queries) needs SQLite
    // 3.25+ which we don't want to depend on.
    let mut latencies: Vec<f64> = {
        let mut stmt = conn
            .prepare(
                "SELECT latency_ms FROM observations WHERE latency_ms IS NOT NULL ORDER BY latency_ms ASC",
            )
            .map_err(map_sqlite)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, f64>(0))
            .map_err(map_sqlite)?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r.map_err(map_sqlite)?);
        }
        v
    };
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if latencies.is_empty() {
        println!("latency:             (no rows with latency_ms)");
    } else {
        let p = |q: f64| -> f64 {
            let idx = ((q * (latencies.len() as f64 - 1.0)).round() as usize)
                .min(latencies.len() - 1);
            latencies[idx]
        };
        println!(
            "latency ms:          p50={:.3}  p90={:.3}  p99={:.3}  max={:.3}",
            p(0.50),
            p(0.90),
            p(0.99),
            latencies.last().copied().unwrap_or(0.0),
        );
    }

    // Per-template aggregates. We pull (template, est, actual) rows and
    // fold q-error in Rust so we share the exact definition with
    // `Observation::q_error` (avoids subtle SQL-vs-Rust drift).
    let mut stmt = conn
        .prepare(
            "SELECT template_hash, est_rows, actual_rows FROM observations ORDER BY template_hash",
        )
        .map_err(map_sqlite)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(map_sqlite)?;

    let mut acc: Vec<TemplateRow> = Vec::new();
    let mut cur_hash: Option<String> = None;
    let mut cur_n: u64 = 0;
    let mut cur_sum: f64 = 0.0;
    let mut cur_max: f64 = 0.0;
    for r in rows {
        let (tpl, est, actual) = r.map_err(map_sqlite)?;
        let q = Observation {
            template_hash: tpl.clone(),
            plan_fingerprint: String::new(),
            est_rows: est,
            actual_rows: actual,
            latency_ms: None,
        }
        .q_error();

        match cur_hash.as_deref() {
            Some(h) if h == tpl => {
                cur_n += 1;
                if q.is_finite() {
                    cur_sum += q;
                    if q > cur_max {
                        cur_max = q;
                    }
                } else {
                    cur_max = f64::INFINITY;
                }
            }
            _ => {
                if let Some(h) = cur_hash.take() {
                    acc.push(TemplateRow {
                        template_hash: h,
                        n: cur_n,
                        avg_q: if cur_n > 0 { cur_sum / cur_n as f64 } else { 0.0 },
                        max_q: cur_max,
                    });
                }
                cur_hash = Some(tpl);
                cur_n = 1;
                cur_sum = if q.is_finite() { q } else { 0.0 };
                cur_max = q;
            }
        }
    }
    if let Some(h) = cur_hash {
        acc.push(TemplateRow {
            template_hash: h,
            n: cur_n,
            avg_q: if cur_n > 0 { cur_sum / cur_n as f64 } else { 0.0 },
            max_q: cur_max,
        });
    }

    println!();
    println!("per-template q-error:");
    println!("  {:<32}  {:>8}  {:>10}  {:>10}", "template_hash", "n", "avg_q", "max_q");
    for row in &acc {
        let avg = if row.avg_q.is_finite() {
            format!("{:.3}", row.avg_q)
        } else {
            "inf".to_string()
        };
        let max = if row.max_q.is_finite() {
            format!("{:.3}", row.max_q)
        } else {
            "inf".to_string()
        };
        println!(
            "  {:<32}  {:>8}  {:>10}  {:>10}",
            truncate(&row.template_hash, 32),
            row.n,
            avg,
            max
        );
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n.saturating_sub(3)])
    }
}
