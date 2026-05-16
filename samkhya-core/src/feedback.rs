//! Feedback recorder — captures `(plan, estimate, actual)` triples to a
//! SQLite sidecar so the residual correction model can learn from real
//! engine behavior. Inspired by Bao / AutoSteer.
//!
//! The store is deliberately minimal: one process, one connection, one
//! table per concern. The schema is forward-compatible — new optional
//! columns can be added with `ALTER TABLE` migrations later.

use std::path::Path;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS observations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    template_hash   TEXT NOT NULL,
    plan_fingerprint TEXT NOT NULL,
    est_rows        INTEGER NOT NULL,
    actual_rows     INTEGER NOT NULL,
    latency_ms      REAL,
    recorded_at     TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_obs_template ON observations(template_hash);
CREATE INDEX IF NOT EXISTS idx_obs_plan ON observations(plan_fingerprint);
"#;

/// A single observation captured at query end.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    pub template_hash: String,
    pub plan_fingerprint: String,
    pub est_rows: u64,
    pub actual_rows: u64,
    pub latency_ms: Option<f64>,
}

impl Observation {
    /// Multiplicative q-error: `max(actual/est, est/actual)`. Returns `f64::INFINITY` if either is 0.
    pub fn q_error(&self) -> f64 {
        if self.est_rows == 0 || self.actual_rows == 0 {
            return f64::INFINITY;
        }
        let r = self.actual_rows as f64 / self.est_rows as f64;
        if r >= 1.0 { r } else { 1.0 / r }
    }
}

/// SQLite-backed feedback store.
pub struct FeedbackStore {
    conn: Connection,
}

impl FeedbackStore {
    /// Open or create a store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(map_sqlite)?;
        conn.execute_batch(SCHEMA_V1).map_err(map_sqlite)?;
        Ok(Self { conn })
    }

    /// Open an in-memory store (test / ephemeral).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(map_sqlite)?;
        conn.execute_batch(SCHEMA_V1).map_err(map_sqlite)?;
        Ok(Self { conn })
    }

    /// Record an observation.
    pub fn record(&self, obs: &Observation) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO observations (template_hash, plan_fingerprint, est_rows, actual_rows, latency_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    obs.template_hash,
                    obs.plan_fingerprint,
                    obs.est_rows as i64,
                    obs.actual_rows as i64,
                    obs.latency_ms,
                ],
            )
            .map_err(map_sqlite)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Return all observations for a given query template, oldest first.
    pub fn history(&self, template_hash: &str) -> Result<Vec<Observation>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT template_hash, plan_fingerprint, est_rows, actual_rows, latency_ms
                 FROM observations WHERE template_hash = ?1 ORDER BY id ASC",
            )
            .map_err(map_sqlite)?;
        let rows = stmt
            .query_map(params![template_hash], |row| {
                Ok(Observation {
                    template_hash: row.get(0)?,
                    plan_fingerprint: row.get(1)?,
                    est_rows: row.get::<_, i64>(2)? as u64,
                    actual_rows: row.get::<_, i64>(3)? as u64,
                    latency_ms: row.get(4)?,
                })
            })
            .map_err(map_sqlite)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(map_sqlite)
    }

    /// Number of observations stored.
    pub fn count(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM observations", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|n| n as u64)
            .map_err(map_sqlite)
    }
}

fn map_sqlite(e: rusqlite::Error) -> Error {
    Error::Feedback(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(template: &str, est: u64, actual: u64) -> Observation {
        Observation {
            template_hash: template.into(),
            plan_fingerprint: "plan-abc".into(),
            est_rows: est,
            actual_rows: actual,
            latency_ms: Some(42.0),
        }
    }

    #[test]
    fn record_and_count() {
        let store = FeedbackStore::open_in_memory().unwrap();
        assert_eq!(store.count().unwrap(), 0);
        store.record(&sample("t1", 100, 110)).unwrap();
        store.record(&sample("t1", 100, 90)).unwrap();
        store.record(&sample("t2", 50, 200)).unwrap();
        assert_eq!(store.count().unwrap(), 3);
    }

    #[test]
    fn history_filters_by_template() {
        let store = FeedbackStore::open_in_memory().unwrap();
        store.record(&sample("t1", 100, 110)).unwrap();
        store.record(&sample("t2", 50, 200)).unwrap();
        store.record(&sample("t1", 100, 90)).unwrap();
        let t1 = store.history("t1").unwrap();
        assert_eq!(t1.len(), 2);
        assert!(t1.iter().all(|o| o.template_hash == "t1"));
    }

    #[test]
    fn q_error_computes_correctly() {
        let obs_over = sample("t1", 10, 100); // 10× underestimate
        assert!((obs_over.q_error() - 10.0).abs() < 1e-9);
        let obs_under = sample("t1", 100, 10); // 10× overestimate
        assert!((obs_under.q_error() - 10.0).abs() < 1e-9);
        let obs_exact = sample("t1", 100, 100);
        assert!((obs_exact.q_error() - 1.0).abs() < 1e-9);
        let obs_zero = sample("t1", 0, 100);
        assert!(obs_zero.q_error().is_infinite());
    }

    #[test]
    fn persists_to_disk() {
        let tmp = std::env::temp_dir().join(format!("samkhya-test-{}.db", std::process::id()));
        // ensure clean start
        let _ = std::fs::remove_file(&tmp);
        {
            let store = FeedbackStore::open(&tmp).unwrap();
            store.record(&sample("t1", 1, 2)).unwrap();
        }
        let store2 = FeedbackStore::open(&tmp).unwrap();
        assert_eq!(store2.count().unwrap(), 1);
        std::fs::remove_file(&tmp).ok();
    }
}
