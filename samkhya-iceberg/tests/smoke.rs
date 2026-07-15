//! Smoke test: build a fake `SnapshotPuffinPaths` with one sidecar
//! path pointing at a tempfile, write a samkhya Puffin into the
//! tempfile, and assert that the path-walking logic returns the
//! expected entry plus the deserialized HLL distinct count.

#![cfg(feature = "iceberg")]

use std::io::Cursor;

use samkhya_core::puffin::{Blob, PuffinWriter};
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_iceberg::snapshot::column_stats_from_paths_live;
use samkhya_iceberg::{Schema, SnapshotPuffinPaths};

/// End-to-end against a real on-disk Puffin sidecar (no Iceberg
/// catalog required). Exercises the path-walking + samkhya blob
/// projection without depending on an Iceberg test fixture.
#[test]
fn snapshot_paths_walk_returns_expected_entry() {
    // Build a real samkhya HLL sketch and serialize it into a
    // Puffin payload in-memory.
    let mut hll = HllSketch::new(12).unwrap();
    for i in 0..1_000u32 {
        hll.add(&i.to_le_bytes());
    }
    let payload = hll.to_bytes().unwrap();

    let mut writer = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
    writer
        .add_blob(Blob::new(HllSketch::KIND, vec![7], &payload))
        .unwrap();
    let cursor = writer.finish().unwrap();
    let puffin_bytes = cursor.into_inner();

    // Write the Puffin bytes to a tempfile and point a fake
    // `SnapshotPuffinPaths` at it.
    let dir = tempfile::tempdir().unwrap();
    let sidecar = dir.path().join("snapshot-0.puffin");
    std::fs::write(&sidecar, &puffin_bytes).unwrap();

    let paths = SnapshotPuffinPaths::from_strings(Some(123), [sidecar.clone()]);
    assert_eq!(paths.snapshot_id, Some(123));
    assert_eq!(paths.len(), 1);
    assert_eq!(paths.paths[0], sidecar);

    // Project through the live walker — schema declares field id 7
    // (matching the blob's `fields[0]`).
    let schema = Schema::from_fields([(7, "user_id")]);
    let stats = column_stats_from_paths_live(&paths, &schema);

    let entry = stats.get(&7).expect("field id 7 present");
    let distinct = entry.distinct_count.expect("HLL distinct count set");
    let err = (distinct as f64 - 1_000.0).abs() / 1_000.0;
    assert!(
        err < 0.1,
        "HLL distinct estimate off by {err} (got {distinct})"
    );
}

// The real `iceberg::table::Table` path, including stale/current statistics
// files and FileIO-backed loading, is the cross-engine release gate in
// `samkhya-it/tests/puffin_cross_engine.rs`.
