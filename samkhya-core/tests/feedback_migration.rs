// SPDX-License-Identifier: Apache-2.0
//
// samkhya-core: the 1.2.0 plan-feature columns must be a migration, not a
// schema break.
//
// Sole author: Prateek Singh.
//
// A feedback store written by a 1.0/1.1 binary has no feature columns. Opening
// it with a current binary has to upgrade it in place, keep every row it
// already holds, and stay readable by the older binary — the columns are
// nullable precisely so that last property survives.

use samkhya_core::feedback::{FeedbackStore, PlanObservation};
use samkhya_core::residual::CorrectionFeatures;

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("samkhya_feedback_migration");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    std::fs::remove_file(&path).ok();
    path
}

/// Build a pre-1.2 store by hand: the v1 table, no feature columns.
fn write_legacy_store(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).expect("sqlite opens");
    conn.execute_batch(
        "CREATE TABLE observations (
             id              INTEGER PRIMARY KEY AUTOINCREMENT,
             template_hash   TEXT NOT NULL,
             plan_fingerprint TEXT NOT NULL,
             est_rows        INTEGER NOT NULL,
             actual_rows     INTEGER NOT NULL,
             latency_ms      REAL,
             recorded_at     TEXT NOT NULL DEFAULT (datetime('now'))
         );
         INSERT INTO observations (template_hash, plan_fingerprint, est_rows, actual_rows)
         VALUES ('old', 'p1', 10, 100);
         PRAGMA user_version = 1;",
    )
    .expect("legacy schema");
}

#[test]
fn an_older_store_migrates_in_place_and_keeps_its_rows() {
    let db = temp_db("legacy_schema.db");
    write_legacy_store(&db);

    let store = FeedbackStore::open(&db).expect("opens a pre-1.2 store");
    assert_eq!(store.count().expect("count"), 1);
    assert_eq!(store.history("old").expect("legacy read").len(), 1);
    // The legacy row carries no features, so it is not trainable.
    assert!(store.plan_history("old").expect("plan read").is_empty());

    // The migrated store accepts featured rows.
    store
        .record_plan(&PlanObservation {
            template_hash: "old".into(),
            plan_fingerprint: "p2".into(),
            features: CorrectionFeatures {
                baseline_estimate: 20,
                join_depth: 1,
                ..Default::default()
            },
            actual_rows: 200,
            latency_ms: None,
        })
        .expect("records after migration");
    assert_eq!(store.plan_history("old").expect("plan read").len(), 1);
    assert_eq!(store.count().expect("count"), 2);

    std::fs::remove_file(&db).ok();
}

/// Opening repeatedly must not re-add the columns or disturb the data.
#[test]
fn the_migration_is_idempotent() {
    let db = temp_db("idempotent.db");
    write_legacy_store(&db);

    for _ in 0..3 {
        let store = FeedbackStore::open(&db).expect("opens");
        assert_eq!(store.count().expect("count"), 1);
    }

    let conn = rusqlite::Connection::open(&db).expect("sqlite opens");
    let mut stmt = conn
        .prepare("PRAGMA table_info(observations)")
        .expect("pragma");
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    // Exactly one of each feature column, no duplicates.
    for expected in [
        "left_input_rows",
        "right_input_rows",
        "left_distinct",
        "right_distinct",
        "predicate_count",
        "join_depth",
    ] {
        assert_eq!(
            columns.iter().filter(|c| c.as_str() == expected).count(),
            1,
            "column {expected} should appear exactly once, got {columns:?}"
        );
    }

    std::fs::remove_file(&db).ok();
}

/// A fresh store gets the columns without any migration step.
#[test]
fn a_fresh_store_has_the_feature_columns() {
    let store = FeedbackStore::open_in_memory().expect("opens");
    store
        .record_plan(&PlanObservation {
            template_hash: "fresh".into(),
            plan_fingerprint: "p".into(),
            features: CorrectionFeatures {
                baseline_estimate: 1,
                join_depth: 2,
                predicate_count: 3,
                ..Default::default()
            },
            actual_rows: 5,
            latency_ms: None,
        })
        .expect("records");
    let history = store.plan_history("fresh").expect("reads");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].features.predicate_count, 3);
    assert_eq!(history[0].features.join_depth, 2);
}
