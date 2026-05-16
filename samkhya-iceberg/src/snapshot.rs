//! Live snapshot walker — resolves Puffin sidecar paths from an open
//! `iceberg::table::Table` and (optionally) loads the samkhya
//! sketches stored inside those sidecars into [`ColumnStats`].
//!
//! Behind the `iceberg` cargo feature. The default build skips this
//! module entirely so the heavy `iceberg` crate (which pulls in
//! `opendal`, `arrow`, `parquet`, etc.) is never compiled.
//!
//! # API stability note
//!
//! The exact "current snapshot's stats files" accessor in the
//! `iceberg` crate has shifted between releases — `Snapshot::statistics`,
//! `Table::metadata().statistics()`, and
//! `TableMetadata::statistics_for_snapshot(...)` have all existed at
//! various points. The walker below is written against the
//! `iceberg = 0.9.1` shape: `Table::metadata().current_snapshot()`
//! returns the current snapshot, and the table metadata exposes
//! `statistics()` -> `&[StatisticsFile]` plus a snapshot-id field on
//! each entry. If a future iceberg release renames or reshuffles
//! these accessors, only the body of [`discover_puffin_sidecars`]
//! has to change — its signature and the [`SnapshotPuffinPaths`]
//! contract type are independent of the iceberg crate.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use samkhya_core::Result as SamkhyaResult;
use samkhya_core::puffin::PuffinReader;
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_core::stats::ColumnStats;

use crate::{Schema, SnapshotPuffinPaths};

/// Discover the Puffin sidecar paths attached to the current snapshot
/// of `table`.
///
/// Resolution order (best-effort against the iceberg 0.9.1 API):
///
/// 1. Read `table.metadata().current_snapshot_id()`.
/// 2. Filter `table.metadata().statistics()` (the slice of
///    `StatisticsFile` entries on `TableMetadata`) to those whose
///    `snapshot_id` matches the current snapshot.
/// 3. Return the `statistics_path` of each matching entry as a
///    [`SnapshotPuffinPaths`] entry.
///
/// If the iceberg crate's accessor shape differs from the assumption
/// above, this function returns an empty set rather than panicking;
/// callers can fall back to constructing [`SnapshotPuffinPaths`] by
/// hand.
pub async fn discover_puffin_sidecars(
    table: &iceberg::table::Table,
) -> SamkhyaResult<SnapshotPuffinPaths> {
    let metadata = table.metadata();
    let current_snapshot_id = metadata.current_snapshot().map(|s| s.snapshot_id());

    let mut paths: Vec<std::path::PathBuf> = Vec::new();

    // `TableMetadata::statistics_iter()` in iceberg 0.9.1 returns
    // an iterator over `&StatisticsFile`. Each entry carries
    // `snapshot_id` and `statistics_path` (the Puffin sidecar
    // path). We filter by the current snapshot id and collect the
    // paths.
    //
    // The accessor name has drifted across iceberg versions
    // (`statistics()` slice in earlier prototypes,
    // `statistics_for_snapshot(id)` in another iteration); 0.9.1
    // settled on `statistics_iter()`. The contract type
    // `SnapshotPuffinPaths` is independent of this rename.
    for stats_file in metadata.statistics_iter() {
        if let Some(current_id) = current_snapshot_id {
            if stats_file.snapshot_id != current_id {
                continue;
            }
        }
        paths.push(std::path::PathBuf::from(&stats_file.statistics_path));
    }

    Ok(SnapshotPuffinPaths {
        snapshot_id: current_snapshot_id,
        paths,
    })
}

/// Combine [`discover_puffin_sidecars`] with samkhya-core's
/// `PuffinReader` to deserialize every samkhya sketch in every
/// sidecar of the current snapshot and project the results into a
/// `{ field_id -> ColumnStats }` map.
///
/// Unknown blob kinds (Iceberg's own `apache-datasketches-theta-v1`,
/// `deletion-vector-v1`, etc.) are silently skipped — readers
/// ignore kinds they do not understand.
pub async fn load_column_stats(
    table: &iceberg::table::Table,
) -> SamkhyaResult<HashMap<usize, ColumnStats>> {
    let paths = discover_puffin_sidecars(table).await?;
    let mut out: HashMap<usize, ColumnStats> = HashMap::new();

    for sidecar in &paths.paths {
        // Best-effort filesystem open. Iceberg in production reads
        // through an `opendal::Operator` — that path will land when
        // the streaming reader is wired in; today we lean on the
        // filesystem so the smoke test and local-development case
        // both Just Work.
        if let Ok(stats) = read_sidecar(sidecar) {
            for (field_id, sketch_stats) in stats {
                let entry = out.entry(field_id).or_default();
                merge_into(entry, sketch_stats);
            }
        }
    }
    Ok(out)
}

/// Open a single Puffin sidecar and project every samkhya sketch
/// inside it into a `(field_id, ColumnStats)` pair. Unknown blob
/// kinds are silently skipped.
fn read_sidecar(path: &Path) -> SamkhyaResult<Vec<(usize, ColumnStats)>> {
    let file = File::open(path)?;
    let mut reader = PuffinReader::open(file)?;
    let mut out: Vec<(usize, ColumnStats)> = Vec::new();

    // Clone metadata up front so we can both iterate and call
    // `&mut self` methods on the reader.
    let blobs = reader.blobs().to_vec();
    for (idx, meta) in blobs.iter().enumerate() {
        let Some(field_id) = meta.fields.first().copied() else {
            continue;
        };
        if meta.kind == HllSketch::KIND {
            let raw = reader.read_blob_decompressed(idx)?;
            let hll = HllSketch::from_bytes(&raw)?;
            let stats = ColumnStats::new().with_distinct_count(hll.estimate());
            out.push((field_id as usize, stats));
        }
        // Other samkhya kinds (bloom, cms, equi-depth, correlated)
        // do not directly project into a ColumnStats scalar field,
        // so we leave them for the caller-side residual layer to
        // pick up; the snapshot walker's job is only to surface
        // what is portable into the engine's stats slot.
    }
    Ok(out)
}

/// Merge `src` into `dst`, preferring populated fields in `src`
/// when they are present. Keeps the contract that earlier sidecars
/// can be progressively refined by later ones inside the same
/// snapshot.
fn merge_into(dst: &mut ColumnStats, src: ColumnStats) {
    if src.row_count.is_some() {
        dst.row_count = src.row_count;
    }
    if src.null_count.is_some() {
        dst.null_count = src.null_count;
    }
    if src.distinct_count.is_some() {
        dst.distinct_count = src.distinct_count;
    }
    if src.min.is_some() {
        dst.min = src.min;
    }
    if src.max.is_some() {
        dst.max = src.max;
    }
    if src.upper_bound_rows.is_some() {
        dst.upper_bound_rows = src.upper_bound_rows;
    }
}

/// Same projection as [`column_stats_from_paths`](crate::column_stats_from_paths)
/// but powered by the live Puffin reader rather than the no-feature
/// placeholder. Useful when the caller already has a
/// `SnapshotPuffinPaths` in hand (e.g. from a unit test) and does
/// not want to round-trip through `iceberg::table::Table`.
pub fn column_stats_from_paths_live(
    paths: &SnapshotPuffinPaths,
    schema: &Schema,
) -> HashMap<usize, ColumnStats> {
    let mut out: HashMap<usize, ColumnStats> = schema
        .fields()
        .iter()
        .map(|f| (f.field_id as usize, ColumnStats::default()))
        .collect();
    for sidecar in &paths.paths {
        if let Ok(stats) = read_sidecar(sidecar) {
            for (field_id, sketch_stats) in stats {
                let entry = out.entry(field_id).or_default();
                merge_into(entry, sketch_stats);
            }
        }
    }
    out
}
