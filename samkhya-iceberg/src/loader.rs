//! Strict local Puffin loading shared by feature and no-feature builds.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use samkhya_core::portable::{PortableSketchBlob, PortableStatsSnapshot, is_supported_column_kind};
use samkhya_core::puffin::{
    PuffinReader, SAMKHYA_SCHEMA_VERSION_PROPERTY, validate_samkhya_schema_version,
};
use samkhya_core::stats::ColumnStats;
use samkhya_core::{Error, Result};

use crate::{Schema, SnapshotPuffinPaths};

/// Strictly load all supported samkhya blobs from local Puffin sidecars.
///
/// Unknown kinds are skipped without reading their payload. Any I/O error,
/// malformed known payload, ambiguous duplicate, or snapshot mismatch is
/// returned to the caller; no partial statistics are published.
pub fn load_portable_stats(paths: &SnapshotPuffinPaths) -> Result<PortableStatsSnapshot> {
    let mut blobs = Vec::new();
    for sidecar in &paths.paths {
        let file = File::open(local_path(sidecar)?)?;
        let mut reader = PuffinReader::open(file)?;
        validate_samkhya_schema_version(
            reader
                .footer()
                .properties
                .get(SAMKHYA_SCHEMA_VERSION_PROPERTY)
                .map(String::as_str),
        )?;
        let metadata = reader.blobs().to_vec();
        for (index, meta) in metadata.into_iter().enumerate() {
            if !is_supported_column_kind(&meta.kind) {
                continue;
            }
            validate_snapshot_metadata(
                paths.snapshot_id,
                meta.snapshot_id,
                meta.sequence_number,
                &meta.kind,
            )?;
            let payload = reader.read_blob_decompressed(index)?;
            let mut blob = PortableSketchBlob::new(meta.kind, meta.fields, payload)
                .with_properties(meta.properties);
            if let (Some(snapshot_id), Some(sequence_number)) =
                (meta.snapshot_id, meta.sequence_number)
            {
                blob = blob.with_snapshot_metadata(snapshot_id, sequence_number);
            }
            blobs.push(blob);
        }
    }

    let snapshot = PortableStatsSnapshot::new(paths.snapshot_id, blobs);
    snapshot.validate()?;
    Ok(snapshot)
}

/// Strictly load and project local sidecars to schema-validated scalar stats.
pub fn try_column_stats_from_paths(
    paths: &SnapshotPuffinPaths,
    schema: &Schema,
) -> Result<HashMap<usize, ColumnStats>> {
    let snapshot = load_portable_stats(paths)?;
    column_stats_from_snapshot(&snapshot, schema)
}

/// Project a decoded snapshot to `ColumnStats` keyed by Iceberg field id.
///
/// This does not map field ids to engine column ordinals. Adapters must use
/// [`Schema::position_of`](crate::Schema::position_of) or an explicit binding.
pub fn column_stats_from_snapshot(
    snapshot: &PortableStatsSnapshot,
    schema: &Schema,
) -> Result<HashMap<usize, ColumnStats>> {
    validate_schema(snapshot, schema)?;
    let mut out = HashMap::with_capacity(schema.fields().len());
    for field in schema.fields() {
        let stats = snapshot
            .decode_column(field.field_id)?
            .map_or_else(ColumnStats::new, |decoded| decoded.column_stats().clone());
        out.insert(field.field_id as usize, stats);
    }
    Ok(out)
}

/// Source-compatible, fail-open projection retained for v1 callers.
///
/// New deployment paths should prefer [`try_column_stats_from_paths`] so a
/// corrupt or mismatched sidecar cannot be mistaken for empty statistics.
pub fn column_stats_from_paths(
    paths: &SnapshotPuffinPaths,
    schema: &Schema,
) -> HashMap<usize, ColumnStats> {
    try_column_stats_from_paths(paths, schema).unwrap_or_else(|_| empty_stats(schema))
}

fn validate_schema(snapshot: &PortableStatsSnapshot, schema: &Schema) -> Result<()> {
    let declared: HashSet<i32> = schema.fields().iter().map(|field| field.field_id).collect();
    if let Some(invalid) = declared.iter().find(|field_id| **field_id <= 0) {
        return Err(Error::InvalidPuffin(format!(
            "Iceberg schema field ids must be positive; got {invalid}"
        )));
    }
    for blob in snapshot.blobs() {
        if !is_supported_column_kind(blob.kind()) {
            continue;
        }
        let [field_id] = blob.fields() else {
            return Err(Error::InvalidPuffin(format!(
                "{} requires exactly one schema field",
                blob.kind()
            )));
        };
        if !declared.contains(field_id) {
            return Err(Error::InvalidPuffin(format!(
                "{} references field {field_id}, which is absent from the current schema",
                blob.kind()
            )));
        }
    }
    Ok(())
}

fn validate_snapshot_metadata(
    expected: Option<i64>,
    actual: Option<i64>,
    sequence: Option<i64>,
    kind: &str,
) -> Result<()> {
    if actual.is_some() != sequence.is_some() {
        return Err(Error::InvalidPuffin(format!(
            "{kind} must carry both snapshot-id and sequence-number or neither"
        )));
    }
    if let (Some(expected), Some(actual)) = (expected, actual) {
        if actual != -1 && actual != expected {
            return Err(Error::InvalidPuffin(format!(
                "{kind} snapshot-id {actual} does not match discovered snapshot {expected}"
            )));
        }
    }
    Ok(())
}

fn local_path(path: &Path) -> Result<PathBuf> {
    let raw = path
        .to_str()
        .ok_or_else(|| Error::InvalidPuffin("sidecar path is not valid UTF-8".to_owned()))?;
    Ok(PathBuf::from(raw.strip_prefix("file://").unwrap_or(raw)))
}

fn empty_stats(schema: &Schema) -> HashMap<usize, ColumnStats> {
    schema
        .fields()
        .iter()
        .filter(|field| field.field_id > 0)
        .map(|field| (field.field_id as usize, ColumnStats::new()))
        .collect()
}
