//! Snapshot-aware Puffin discovery and loading through Iceberg's `FileIO`.
//!
//! This module is available with the `iceberg` feature. It keeps field IDs
//! intact and uses the table's configured storage implementation, so local,
//! object-store, and catalog-backed tables follow the same decoding path.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use iceberg::puffin::PuffinReader as IcebergPuffinReader;
use samkhya_core::portable::{PortableSketchBlob, PortableStatsSnapshot, is_supported_column_kind};
use samkhya_core::puffin::{SAMKHYA_SCHEMA_VERSION_PROPERTY, validate_samkhya_schema_version};
use samkhya_core::stats::ColumnStats;
use samkhya_core::{Error, Result as SamkhyaResult};

use crate::{Schema, SnapshotPuffinPaths, column_stats_from_snapshot};

/// Discover sidecars attached to the table's current snapshot.
///
/// Files for stale snapshots are excluded. A table without a current snapshot
/// returns an empty, snapshot-less set. Paths are sorted and deduplicated so
/// consumers see deterministic input regardless of metadata iteration order.
pub async fn discover_puffin_sidecars(
    table: &iceberg::table::Table,
) -> SamkhyaResult<SnapshotPuffinPaths> {
    let Some(current_snapshot) = table.metadata().current_snapshot() else {
        return Ok(SnapshotPuffinPaths::new());
    };
    let snapshot_id = current_snapshot.snapshot_id();
    let mut paths: Vec<PathBuf> = table
        .metadata()
        .statistics_iter()
        .filter(|statistics| statistics.snapshot_id == snapshot_id)
        .map(|statistics| PathBuf::from(&statistics.statistics_path))
        .collect();
    paths.sort();
    paths.dedup();

    Ok(SnapshotPuffinPaths {
        snapshot_id: Some(snapshot_id),
        paths,
    })
}

/// Load supported samkhya blobs for the current snapshot through Iceberg.
///
/// Unknown Puffin blob kinds are skipped without fetching their payloads.
/// Known corrupt payloads, duplicate sketches, schema-independent metadata
/// conflicts, and snapshot/sequence mismatches fail closed.
pub async fn load_portable_stats_from_table(
    table: &iceberg::table::Table,
) -> SamkhyaResult<PortableStatsSnapshot> {
    let sidecars = discover_puffin_sidecars(table).await?;
    let current_sequence_number = table
        .metadata()
        .current_snapshot()
        .map(|snapshot| snapshot.sequence_number());
    let mut blobs = Vec::new();

    for sidecar in &sidecars.paths {
        let location = sidecar.to_str().ok_or_else(|| {
            Error::InvalidPuffin("Iceberg statistics path is not valid UTF-8".to_owned())
        })?;
        let input = table.file_io().new_input(location).map_err(iceberg_error)?;
        let reader = IcebergPuffinReader::new(input);
        let file_metadata = reader.file_metadata().await.map_err(iceberg_error)?.clone();
        validate_samkhya_schema_version(
            file_metadata
                .properties()
                .get(SAMKHYA_SCHEMA_VERSION_PROPERTY)
                .map(String::as_str),
        )?;

        for metadata in file_metadata.blobs() {
            if !is_supported_column_kind(metadata.blob_type()) {
                continue;
            }
            validate_snapshot_id(
                sidecars.snapshot_id,
                metadata.snapshot_id(),
                metadata.blob_type(),
            )?;
            validate_sequence_number(
                current_sequence_number,
                metadata.sequence_number(),
                metadata.blob_type(),
            )?;
            let blob = reader.blob(metadata).await.map_err(iceberg_error)?;
            let properties: BTreeMap<String, String> = metadata
                .properties()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            blobs.push(
                PortableSketchBlob::new(
                    metadata.blob_type(),
                    metadata.fields().to_vec(),
                    blob.data().to_vec(),
                )
                .with_snapshot_metadata(metadata.snapshot_id(), metadata.sequence_number())
                .with_properties(properties),
            );
        }
    }

    let snapshot = PortableStatsSnapshot::new(sidecars.snapshot_id, blobs);
    snapshot.validate()?;
    Ok(snapshot)
}

/// Load and schema-check the scalar planner statistics for the current table.
///
/// The returned map is keyed by Iceberg field ID, not engine column position.
pub async fn load_column_stats(
    table: &iceberg::table::Table,
) -> SamkhyaResult<HashMap<usize, ColumnStats>> {
    let snapshot = load_portable_stats_from_table(table).await?;
    let schema = Schema::from_fields(
        table
            .current_schema_ref()
            .as_struct()
            .fields()
            .iter()
            .map(|field| (field.id, field.name.clone())),
    );
    column_stats_from_snapshot(&snapshot, &schema)
}

/// Compatibility projection for callers that already resolved local paths.
///
/// This preserves the v1 fail-open behavior. Deployment code should prefer
/// [`crate::try_column_stats_from_paths`] or
/// [`load_portable_stats_from_table`] for explicit errors.
pub fn column_stats_from_paths_live(
    paths: &SnapshotPuffinPaths,
    schema: &Schema,
) -> HashMap<usize, ColumnStats> {
    crate::column_stats_from_paths(paths, schema)
}

fn validate_snapshot_id(expected: Option<i64>, actual: i64, kind: &str) -> SamkhyaResult<()> {
    if let Some(expected) = expected {
        // -1 is the Puffin sentinel for an unavailable snapshot identity.
        if actual != -1 && actual != expected {
            return Err(Error::InvalidPuffin(format!(
                "{kind} snapshot-id {actual} does not match current snapshot {expected}"
            )));
        }
    }
    Ok(())
}

fn validate_sequence_number(expected: Option<i64>, actual: i64, kind: &str) -> SamkhyaResult<()> {
    if let Some(expected) = expected {
        // -1 is samkhya's legacy sentinel for unavailable snapshot metadata.
        if actual != -1 && actual != expected {
            return Err(Error::InvalidPuffin(format!(
                "{kind} sequence-number {actual} does not match current snapshot sequence {expected}"
            )));
        }
    }
    Ok(())
}

fn iceberg_error(error: iceberg::Error) -> Error {
    Error::InvalidPuffin(format!("Iceberg Puffin access failed: {error}"))
}
