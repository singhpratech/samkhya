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
//!   [`SnapshotPuffinPaths`], strict local Puffin loading, and a
//!   compatibility projection to [`ColumnStats`]. Downstream consumers can
//!   load local sidecars without pulling the heavy `iceberg` dependency tree.
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
//! samkhya-iceberg = { version = "1.1", features = ["iceberg"] }
//! ```
//!
//! Without the feature, you can still construct
//! [`SnapshotPuffinPaths`] manually from any source (a test harness,
//! a Puffin-only pipeline that does not own an Iceberg table) and
//! hand it to [`column_stats_from_paths`].
#![deny(rustdoc::broken_intra_doc_links)]

use std::path::PathBuf;

mod loader;

pub use loader::{
    column_stats_from_paths, column_stats_from_snapshot, load_portable_stats,
    try_column_stats_from_paths,
};

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    fields: Vec<SchemaField>,
}

/// One column entry in [`Schema`].
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// Position of `field_id` in this schema's declared order.
    pub fn position_of(&self, field_id: i32) -> Option<usize> {
        self.fields
            .iter()
            .position(|field| field.field_id == field_id)
    }
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
    fn column_stats_compatibility_keys_by_field_id() {
        let schema = Schema::from_fields([(7, "a"), (11, "b")]);
        let paths = SnapshotPuffinPaths::new();
        let stats = column_stats_from_paths(&paths, &schema);
        assert_eq!(stats.len(), 2);
        assert!(stats.contains_key(&7));
        assert!(stats.contains_key(&11));
    }

    #[test]
    fn schema_position_is_explicit_not_field_id_cast() {
        let schema = Schema::from_fields([(17, "target"), (23, "other")]);
        assert_eq!(schema.position_of(17), Some(0));
        assert_eq!(schema.position_of(23), Some(1));
        assert_eq!(schema.position_of(0), None);
    }
}
