//! samkhya-iceberg — bridge between Apache Iceberg snapshots and samkhya's
//! Puffin sidecars.
//!
//! # Integration model
//!
//! Iceberg already has a Puffin sidecar concept: every snapshot's
//! manifest carries a `statistics` (or `partition-statistics`) entry
//! that lists Puffin files written alongside the table. Each Puffin
//! file holds typed blobs identified by a `KIND` tag — Iceberg's own
//! blob kinds (`apache-datasketches-theta-v1`, `deletion-vector-v1`,
//! …) live in the same physical file as samkhya's blob kinds
//! (`samkhya.hll-v1`, `samkhya.bloom-v1`, …). Readers ignore kinds
//! they do not understand.
//!
//! samkhya already knows how to write Puffin files
//! ([`samkhya_core::puffin`]) and how to bundle the deserialized
//! sketches into [`ColumnStats`];
//! what it has been missing is the *snapshot-aware* link that says
//! "for this current table snapshot, here are the sidecar paths
//! samkhya should look at". That is the job of this crate.
//!
//! # Crate shape
//!
//! - The always-on surface (no cargo features) exposes
//!   [`SnapshotPuffinPaths`] (a list of sidecar paths discovered from
//!   a snapshot manifest) and the always-on
//!   [`column_stats_from_paths`] placeholder. Downstream consumers
//!   can take a dependency on this crate as a *contract type*
//!   without ever pulling the heavy `iceberg` dependency tree.
//! - The optional `snapshot` module — gated behind the `iceberg`
//!   feature — contains the actual snapshot-walking logic that
//!   resolves [`SnapshotPuffinPaths`] from an open
//!   `iceberg::table::Table` and the loader that combines that
//!   walk with [`samkhya_core::puffin::PuffinReader`] to produce
//!   [`ColumnStats`].
//!
//! # Enabling the live Iceberg walker
//!
//! ```toml
//! [dependencies]
//! samkhya-iceberg = { version = "0.0.1", features = ["iceberg"] }
//! ```
//!
//! Without the feature, you can still construct
//! [`SnapshotPuffinPaths`] manually from any source (a test harness,
//! a Puffin-only pipeline that does not own an Iceberg table) and
//! hand it to [`column_stats_from_paths`].
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::HashMap;
use std::path::PathBuf;

use samkhya_core::stats::ColumnStats;

/// List of Puffin sidecar paths attached to a single Iceberg snapshot.
///
/// Returned by the snapshot walker in `crate::snapshot` (available with
/// the `iceberg` cargo feature) and also constructible by hand for tests
/// and Puffin-only callers that do not own an Iceberg table.
///
/// Always available — does *not* require the `iceberg` cargo feature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotPuffinPaths {
    /// Iceberg snapshot id this set was resolved against, or `None`
    /// when the list was assembled outside of a snapshot context
    /// (e.g. in a unit test).
    pub snapshot_id: Option<i64>,
    /// Filesystem-style paths (or URIs — Iceberg stores them as
    /// strings, so we preserve that) pointing at Puffin sidecars.
    pub paths: Vec<PathBuf>,
}

impl SnapshotPuffinPaths {
    /// Construct an empty set, useful as a builder seed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a set from a list of string paths, recording an
    /// optional snapshot id.
    pub fn from_strings<I, S>(snapshot_id: Option<i64>, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<PathBuf>,
    {
        Self {
            snapshot_id,
            paths: paths.into_iter().map(Into::into).collect(),
        }
    }

    /// Number of sidecar paths in the set.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// `true` when the set carries no sidecar paths.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Lightweight schema view used by [`column_stats_from_paths`].
///
/// The full Iceberg schema is rich (nested fields, identifier
/// transforms, partition specs) — for the purposes of mapping
/// samkhya Puffin blobs onto `ColumnStats`, we only need the
/// `(field_id, name)` projection. Keeping a local type here lets
/// the no-feature build still expose a meaningful API surface.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    fields: Vec<SchemaField>,
}

/// One column entry in [`Schema`].
#[derive(Debug, Clone)]
pub struct SchemaField {
    /// Iceberg field id (corresponds to `BlobMetadata::fields[0]` in
    /// samkhya Puffin blobs).
    pub field_id: i32,
    /// Human-readable column name.
    pub name: String,
}

impl Schema {
    /// Construct a schema from an ordered list of `(field_id, name)`
    /// pairs.
    pub fn from_fields<I, S>(fields: I) -> Self
    where
        I: IntoIterator<Item = (i32, S)>,
        S: Into<String>,
    {
        Self {
            fields: fields
                .into_iter()
                .map(|(field_id, name)| SchemaField {
                    field_id,
                    name: name.into(),
                })
                .collect(),
        }
    }

    /// All fields, in declaration order.
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }
}

/// Placeholder that returns an empty `ColumnStats` map for every
/// schema column. Once the snapshot-walking loader in
/// `crate::snapshot` (the `iceberg`-feature-gated module) lands, this
/// function will defer to it; for now it satisfies the contract type
/// so downstream code can call it from the no-feature build without
/// conditionally compiling.
///
/// The key is the Iceberg field id (matches `BlobMetadata::fields`
/// in samkhya Puffin blobs); the value is the assembled
/// [`ColumnStats`] for that field.
pub fn column_stats_from_paths(
    _paths: &SnapshotPuffinPaths,
    schema: &Schema,
) -> HashMap<usize, ColumnStats> {
    // The real walker lives behind the `iceberg` feature in
    // `crate::snapshot::load_column_stats`. Outside that feature
    // we still hand back a well-typed (empty) map so callers can
    // unconditionally depend on this function.
    schema
        .fields()
        .iter()
        .map(|f| (f.field_id as usize, ColumnStats::default()))
        .collect()
}

#[cfg(feature = "iceberg")]
pub mod snapshot;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_paths_round_trip() {
        let paths = SnapshotPuffinPaths::from_strings(Some(42), ["/tmp/a.puffin", "/tmp/b.puffin"]);
        assert_eq!(paths.snapshot_id, Some(42));
        assert_eq!(paths.len(), 2);
        assert!(!paths.is_empty());
    }

    #[test]
    fn empty_snapshot_paths() {
        let paths = SnapshotPuffinPaths::new();
        assert!(paths.is_empty());
        assert_eq!(paths.snapshot_id, None);
    }

    #[test]
    fn column_stats_placeholder_keys_by_field_id() {
        let schema = Schema::from_fields([(7, "a"), (11, "b")]);
        let paths = SnapshotPuffinPaths::new();
        let stats = column_stats_from_paths(&paths, &schema);
        assert_eq!(stats.len(), 2);
        assert!(stats.contains_key(&7));
        assert!(stats.contains_key(&11));
    }
}
