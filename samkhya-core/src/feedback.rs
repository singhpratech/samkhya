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

/// Plan-shape feature columns, added in 1.2.0.
///
/// Every column is nullable, so a store written by an older binary reads
/// back unchanged and rows recorded through [`FeedbackStore::record`]
/// simply leave them `NULL`. That keeps the addition a migration rather
/// than a schema break, which is why `SCHEMA_USER_VERSION` does not move.
///
/// They exist because a corrector trained without them is trained on a
/// different feature space than the one it sees at inference time — see
/// [`PlanObservation`].
const FEATURE_COLUMNS: &[(&str, &str)] = &[
    ("left_input_rows", "INTEGER"),
    ("right_input_rows", "INTEGER"),
    ("left_distinct", "INTEGER"),
    ("right_distinct", "INTEGER"),
    ("predicate_count", "INTEGER"),
    ("join_depth", "INTEGER"),
];

/// Schema version stamped into SQLite's `PRAGMA user_version`.
///
/// Bumped only when the on-disk schema changes in a backwards-incompatible
/// way. Stores written by an older binary (with `user_version = 0`,
/// i.e. unset) are silently upgraded by writing the current value;
/// stores written by a newer binary (with a strictly larger version)
/// are rejected so we never silently truncate forward-versioned data.
/// See `documents/SECURITY-REVIEW-2026-05-17.md` item L3.
const SCHEMA_USER_VERSION: i32 = 1;

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
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::feedback::Observation;
    ///
    /// // 10× underestimate: est=10, actual=100 → q-error = 10.
    /// let obs = Observation {
    ///     template_hash: "t".into(),
    ///     plan_fingerprint: "p".into(),
    ///     est_rows: 10,
    ///     actual_rows: 100,
    ///     latency_ms: None,
    /// };
    /// assert!((obs.q_error() - 10.0).abs() < 1e-9);
    /// ```
    pub fn q_error(&self) -> f64 {
        if self.est_rows == 0 || self.actual_rows == 0 {
            return f64::INFINITY;
        }
        let r = self.actual_rows as f64 / self.est_rows as f64;
        if r >= 1.0 { r } else { 1.0 / r }
    }
}

/// An observation captured together with the plan-shape features the
/// corrector will be handed at inference time.
///
/// # Why this exists alongside [`Observation`]
///
/// [`Observation`] records only `est_rows` and `actual_rows`. Training from
/// it forces the trainer to synthesise a feature vector with
/// `baseline_estimate` set and the other six slots zeroed — while at
/// inference time an adapter fills all seven. A tree model never splits on
/// a feature that was constant during training, so six of the seven
/// features are dead weight and the corrector is effectively
/// one-dimensional. That is a silent train/serve skew, not a crash, which
/// is why it survived so long.
///
/// `PlanObservation` closes it by recording what the corrector will
/// actually see. Prefer it for anything that will be trained on.
///
/// # Examples
///
/// ```
/// use samkhya_core::feedback::PlanObservation;
/// use samkhya_core::residual::CorrectionFeatures;
///
/// let obs = PlanObservation {
///     template_hash: "q7".into(),
///     plan_fingerprint: "hash-join#1".into(),
///     features: CorrectionFeatures { baseline_estimate: 10, ..Default::default() },
///     actual_rows: 100,
///     latency_ms: None,
/// };
/// // 10x under-estimate.
/// assert!((obs.q_error() - 10.0).abs() < 1e-9);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanObservation {
    pub template_hash: String,
    pub plan_fingerprint: String,
    /// The feature vector the corrector saw, including `baseline_estimate`.
    pub features: crate::residual::CorrectionFeatures,
    pub actual_rows: u64,
    pub latency_ms: Option<f64>,
}

impl PlanObservation {
    /// Multiplicative q-error against the baseline estimate. `f64::INFINITY`
    /// when either side is zero, matching [`Observation::q_error`].
    pub fn q_error(&self) -> f64 {
        if self.features.baseline_estimate == 0 || self.actual_rows == 0 {
            return f64::INFINITY;
        }
        let r = self.actual_rows as f64 / self.features.baseline_estimate as f64;
        if r >= 1.0 { r } else { 1.0 / r }
    }

    /// Reduce to the legacy shape, discarding the plan features.
    pub fn to_observation(&self) -> Observation {
        Observation {
            template_hash: self.template_hash.clone(),
            plan_fingerprint: self.plan_fingerprint.clone(),
            est_rows: self.features.baseline_estimate,
            actual_rows: self.actual_rows,
            latency_ms: self.latency_ms,
        }
    }
}

/// SQLite-backed feedback store.
pub struct FeedbackStore {
    conn: Connection,
}

impl FeedbackStore {
    /// Open or create a store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_ref = path.as_ref();
        let conn = Connection::open(path_ref).map_err(map_sqlite)?;
        conn.execute_batch(SCHEMA_V1).map_err(map_sqlite)?;
        check_or_stamp_schema_version(&conn)?;
        add_feature_columns(&conn)?;
        // SECURITY-REVIEW-2026-05-17.md (M2): the feedback store records
        // plan fingerprints which may carry schema details or filter
        // values. Tighten the file mode to 0o600 (owner-only) so a
        // shared-system reader cannot snapshot the store. Best-effort:
        // a failure here (e.g., the file does not exist because we are
        // running against an in-memory or VFS-special path) is logged
        // but not promoted to an error — the store is still usable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) =
                std::fs::set_permissions(path_ref, std::fs::Permissions::from_mode(0o600))
            {
                log::debug!(
                    "feedback store: could not tighten perms on {}: {}",
                    path_ref.display(),
                    err
                );
            }
        }
        Ok(Self { conn })
    }

    /// Open an in-memory store (test / ephemeral).
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::feedback::FeedbackStore;
    ///
    /// let store = FeedbackStore::open_in_memory().unwrap();
    /// assert_eq!(store.count().unwrap(), 0);
    /// ```
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(map_sqlite)?;
        conn.execute_batch(SCHEMA_V1).map_err(map_sqlite)?;
        check_or_stamp_schema_version(&conn)?;
        add_feature_columns(&conn)?;
        Ok(Self { conn })
    }

    /// Record an observation *with* the plan-shape features the corrector
    /// will see at inference time.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::feedback::{FeedbackStore, PlanObservation};
    /// use samkhya_core::residual::CorrectionFeatures;
    ///
    /// let store = FeedbackStore::open_in_memory().unwrap();
    /// let obs = PlanObservation {
    ///     template_hash: "job-slow".into(),
    ///     plan_fingerprint: "hash-join#7".into(),
    ///     features: CorrectionFeatures {
    ///         baseline_estimate: 1_000,
    ///         left_input_rows: Some(500),
    ///         right_input_rows: Some(2_000),
    ///         predicate_count: 2,
    ///         join_depth: 3,
    ///         ..Default::default()
    ///     },
    ///     actual_rows: 9_500,
    ///     latency_ms: Some(12.5),
    /// };
    /// store.record_plan(&obs).unwrap();
    ///
    /// let history = store.plan_history("job-slow").unwrap();
    /// assert_eq!(history.len(), 1);
    /// assert_eq!(history[0].features.join_depth, 3);
    /// ```
    pub fn record_plan(&self, obs: &PlanObservation) -> Result<i64> {
        let f = &obs.features;
        self.conn
            .execute(
                "INSERT INTO observations (template_hash, plan_fingerprint, est_rows, actual_rows, \
                 latency_ms, left_input_rows, right_input_rows, left_distinct, right_distinct, \
                 predicate_count, join_depth) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    obs.template_hash,
                    obs.plan_fingerprint,
                    f.baseline_estimate as i64,
                    obs.actual_rows as i64,
                    obs.latency_ms,
                    f.left_input_rows.map(|v| v as i64),
                    f.right_input_rows.map(|v| v as i64),
                    f.left_distinct.map(|v| v as i64),
                    f.right_distinct.map(|v| v as i64),
                    i64::from(f.predicate_count),
                    i64::from(f.join_depth),
                ],
            )
            .map_err(map_sqlite)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Return every observation for `template_hash` that carries plan
    /// features, oldest first.
    ///
    /// Rows recorded through [`record`](Self::record) have no features and
    /// are skipped: training on them would silently reintroduce the
    /// feature-space mismatch this type exists to prevent. The filter is
    /// `predicate_count IS NOT NULL`, which only
    /// [`record_plan`](Self::record_plan) sets.
    pub fn plan_history(&self, template_hash: &str) -> Result<Vec<PlanObservation>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT template_hash, plan_fingerprint, est_rows, actual_rows, latency_ms, \
                 left_input_rows, right_input_rows, left_distinct, right_distinct, \
                 predicate_count, join_depth \
                 FROM observations \
                 WHERE template_hash = ?1 AND predicate_count IS NOT NULL \
                 ORDER BY id ASC",
            )
            .map_err(map_sqlite)?;
        let rows = stmt
            .query_map(params![template_hash], |row| {
                let opt_u64 = |v: Option<i64>| v.map(|n| n as u64);
                Ok(PlanObservation {
                    template_hash: row.get(0)?,
                    plan_fingerprint: row.get(1)?,
                    features: crate::residual::CorrectionFeatures {
                        baseline_estimate: row.get::<_, i64>(2)? as u64,
                        left_input_rows: opt_u64(row.get(5)?),
                        right_input_rows: opt_u64(row.get(6)?),
                        left_distinct: opt_u64(row.get(7)?),
                        right_distinct: opt_u64(row.get(8)?),
                        predicate_count: row.get::<_, i64>(9)? as u32,
                        join_depth: row.get::<_, i64>(10)? as u32,
                    },
                    actual_rows: row.get::<_, i64>(3)? as u64,
                    latency_ms: row.get(4)?,
                })
            })
            .map_err(map_sqlite)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(map_sqlite)
    }

    /// Record an observation.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::feedback::{FeedbackStore, Observation};
    ///
    /// let store = FeedbackStore::open_in_memory().unwrap();
    /// let obs = Observation {
    ///     template_hash: "tpch-q1".into(),
    ///     plan_fingerprint: "hash-join#42".into(),
    ///     est_rows: 1000,
    ///     actual_rows: 950,
    ///     latency_ms: Some(12.5),
    /// };
    /// let id = store.record(&obs).unwrap();
    /// assert!(id > 0);
    /// assert_eq!(store.count().unwrap(), 1);
    /// ```
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

/// Add the 1.2.0 plan-feature columns to an existing `observations` table.
///
/// Idempotent: each column is added only when absent, so opening a store
/// repeatedly is free and opening one written by an older binary upgrades
/// it in place. Every column is nullable, so nothing already recorded
/// becomes invalid and an older binary can still read the file.
fn add_feature_columns(conn: &Connection) -> Result<()> {
    let mut existing = std::collections::HashSet::new();
    {
        let mut stmt = conn
            .prepare("PRAGMA table_info(observations)")
            .map_err(map_sqlite)?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(map_sqlite)?;
        for name in names {
            existing.insert(name.map_err(map_sqlite)?);
        }
    }
    for (column, ty) in FEATURE_COLUMNS {
        if existing.contains(*column) {
            continue;
        }
        conn.execute_batch(&format!(
            "ALTER TABLE observations ADD COLUMN {column} {ty}"
        ))
        .map_err(map_sqlite)?;
    }
    Ok(())
}

fn map_sqlite(e: rusqlite::Error) -> Error {
    Error::Feedback(e.to_string())
}

/// Read the SQLite `user_version` PRAGMA and either stamp it (if unset)
/// or reject the store (if it carries a strictly larger version).
///
/// See `documents/SECURITY-REVIEW-2026-05-17.md` item L3: a previously
/// malicious or simply newer-schema `.db` opened by an older samkhya
/// would silently mismatch row shape on read; the new PRAGMA check
/// makes that visible.
fn check_or_stamp_schema_version(conn: &Connection) -> Result<()> {
    let on_disk: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if on_disk == 0 {
        // Fresh / pre-versioning store. Stamp the current version so
        // future opens see a match. Using `execute_batch` because
        // `PRAGMA user_version = N` is not a parameterised statement
        // (SQLite refuses bind params on PRAGMA writes).
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_USER_VERSION}"))
            .map_err(map_sqlite)?;
        return Ok(());
    }
    if on_disk > SCHEMA_USER_VERSION {
        return Err(Error::Feedback(format!(
            "feedback store schema version {on_disk} is newer than this build supports \
             ({SCHEMA_USER_VERSION}); refuse to open to avoid data truncation"
        )));
    }
    if on_disk < SCHEMA_USER_VERSION {
        // Older but compatible. No migrations yet (we are on v1), so
        // just bump the marker. Future versions will run migration
        // SQL here before bumping.
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_USER_VERSION}"))
            .map_err(map_sqlite)?;
    }
    Ok(())
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
    fn schema_version_stamped_on_fresh_store() {
        let store = FeedbackStore::open_in_memory().unwrap();
        let v: i32 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_USER_VERSION);
    }

    #[test]
    fn refuses_forward_versioned_store() {
        // Open once to stamp the schema, then manually bump the
        // user_version past what this binary supports and re-open.
        let tmp = std::env::temp_dir().join(format!(
            "samkhya-feedback-forward-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        {
            let store = FeedbackStore::open(&tmp).unwrap();
            store
                .conn
                .execute_batch(&format!(
                    "PRAGMA user_version = {}",
                    SCHEMA_USER_VERSION + 99
                ))
                .unwrap();
        }
        match FeedbackStore::open(&tmp) {
            Ok(_) => panic!("expected forward-version rejection, got Ok"),
            Err(Error::Feedback(msg)) => assert!(
                msg.contains("newer than this build"),
                "expected forward-version rejection, got: {msg}"
            ),
            Err(other) => panic!("expected Error::Feedback, got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
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
