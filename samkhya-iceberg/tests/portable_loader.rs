use std::fs::File;
use std::path::Path;

use samkhya_core::puffin::{Blob, PuffinWriter};
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_iceberg::{
    Schema, SnapshotPuffinPaths, load_portable_stats, try_column_stats_from_paths,
};
use tempfile::TempDir;

const SNAPSHOT_ID: i64 = 42;
const FIELD_ID: i32 = 17;

fn hll_payload() -> Vec<u8> {
    let mut hll = HllSketch::new(10).unwrap();
    for value in 0..128_u32 {
        hll.add(value.to_string().as_bytes());
    }
    hll.to_bytes().unwrap()
}

fn write_blob(path: &Path, kind: &str, field_id: i32, payload: &[u8]) {
    let mut writer = PuffinWriter::new(File::create(path).unwrap());
    writer
        .add_blob_for_snapshot(Blob::new(kind, vec![field_id], payload), SNAPSHOT_ID, 7)
        .unwrap();
    writer.finish().unwrap().sync_all().unwrap();
}

#[test]
fn strict_projection_is_keyed_by_iceberg_field_id() {
    let temp = TempDir::new().unwrap();
    let sidecar = temp.path().join("stats.puffin");
    write_blob(&sidecar, HllSketch::KIND, FIELD_ID, &hll_payload());
    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID), [&sidecar]);
    let schema = Schema::from_fields([(FIELD_ID, "value")]);

    let stats = try_column_stats_from_paths(&paths, &schema).unwrap();
    assert_eq!(stats.len(), 1);
    assert!(stats[&(FIELD_ID as usize)].distinct_count.is_some());
    assert!(!stats.contains_key(&0));
}

#[test]
fn strict_loader_rejects_snapshot_mismatch() {
    let temp = TempDir::new().unwrap();
    let sidecar = temp.path().join("stats.puffin");
    write_blob(&sidecar, HllSketch::KIND, FIELD_ID, &hll_payload());
    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID + 1), [&sidecar]);
    assert!(load_portable_stats(&paths).is_err());
}

#[test]
fn strict_projection_rejects_schema_mismatch() {
    let temp = TempDir::new().unwrap();
    let sidecar = temp.path().join("stats.puffin");
    write_blob(&sidecar, HllSketch::KIND, FIELD_ID, &hll_payload());
    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID), [&sidecar]);
    let wrong_schema = Schema::from_fields([(FIELD_ID + 1, "other")]);
    assert!(try_column_stats_from_paths(&paths, &wrong_schema).is_err());
}

#[test]
fn corrupt_known_payload_fails_closed() {
    let temp = TempDir::new().unwrap();
    let sidecar = temp.path().join("stats.puffin");
    write_blob(&sidecar, HllSketch::KIND, FIELD_ID, b"corrupt");
    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID), [&sidecar]);
    assert!(load_portable_stats(&paths).is_err());
}

#[test]
fn unknown_kind_is_skipped_without_decoding() {
    let temp = TempDir::new().unwrap();
    let sidecar = temp.path().join("stats.puffin");
    write_blob(
        &sidecar,
        "vendor.future-statistics-v1",
        FIELD_ID,
        b"not a samkhya payload",
    );
    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID), [&sidecar]);
    assert!(load_portable_stats(&paths).unwrap().blobs().is_empty());
}

#[test]
fn duplicate_known_kind_across_sidecars_is_rejected() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first.puffin");
    let second = temp.path().join("second.puffin");
    let payload = hll_payload();
    write_blob(&first, HllSketch::KIND, FIELD_ID, &payload);
    write_blob(&second, HllSketch::KIND, FIELD_ID, &payload);
    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID), [&first, &second]);
    assert!(load_portable_stats(&paths).is_err());
}

#[test]
fn explicit_future_schema_version_is_rejected() {
    let temp = TempDir::new().unwrap();
    let sidecar = temp.path().join("future.puffin");
    let payload = hll_payload();
    let mut writer = PuffinWriter::new(File::create(&sidecar).unwrap())
        .with_file_property("samkhya.schema-version", "2");
    writer
        .add_blob_for_snapshot(
            Blob::new(HllSketch::KIND, vec![FIELD_ID], &payload),
            SNAPSHOT_ID,
            7,
        )
        .unwrap();
    writer.finish().unwrap().sync_all().unwrap();

    let paths = SnapshotPuffinPaths::from_strings(Some(SNAPSHOT_ID), [&sidecar]);
    assert!(load_portable_stats(&paths).is_err());
}
