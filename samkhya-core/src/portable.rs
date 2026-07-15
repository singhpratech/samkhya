//! Engine-neutral, decoded view of versioned sketch payloads.
//!
//! Puffin discovers and transports opaque bytes; this module is the shared
//! handoff that lets every engine decode those bytes with identical rules.
//! Unknown blob kinds remain available in the raw bundle and are ignored by
//! typed projection, while malformed payloads for known kinds fail closed.

use std::collections::{BTreeMap, HashSet};

use crate::sketches::{EquiDepthHistogram, HllSketch, Sketch};
use crate::{ColumnStats, Error, Result};

/// One decompressed Puffin blob with its portability metadata intact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PortableSketchBlob {
    kind: String,
    fields: Vec<i32>,
    snapshot_id: Option<i64>,
    sequence_number: Option<i64>,
    payload: Vec<u8>,
    properties: BTreeMap<String, String>,
}

impl PortableSketchBlob {
    /// Construct a blob when snapshot metadata is unavailable.
    pub fn new(kind: impl Into<String>, fields: Vec<i32>, payload: Vec<u8>) -> Self {
        Self {
            kind: kind.into(),
            fields,
            snapshot_id: None,
            sequence_number: None,
            payload,
            properties: BTreeMap::new(),
        }
    }

    /// Attach the snapshot identity carried by Puffin blob metadata.
    pub fn with_snapshot_metadata(mut self, snapshot_id: i64, sequence_number: i64) -> Self {
        self.snapshot_id = Some(snapshot_id);
        self.sequence_number = Some(sequence_number);
        self
    }

    /// Attach arbitrary Puffin blob properties.
    pub fn with_properties(mut self, properties: BTreeMap<String, String>) -> Self {
        self.properties = properties;
        self
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn fields(&self) -> &[i32] {
        &self.fields
    }

    pub const fn snapshot_id(&self) -> Option<i64> {
        self.snapshot_id
    }

    pub const fn sequence_number(&self) -> Option<i64> {
        self.sequence_number
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn properties(&self) -> &BTreeMap<String, String> {
        &self.properties
    }
}

/// Decompressed sketch blobs associated with one table snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PortableStatsSnapshot {
    snapshot_id: Option<i64>,
    blobs: Vec<PortableSketchBlob>,
}

impl PortableStatsSnapshot {
    pub fn new(snapshot_id: Option<i64>, blobs: Vec<PortableSketchBlob>) -> Self {
        Self { snapshot_id, blobs }
    }

    pub const fn snapshot_id(&self) -> Option<i64> {
        self.snapshot_id
    }

    pub fn blobs(&self) -> &[PortableSketchBlob] {
        &self.blobs
    }

    /// Return every raw blob whose field list includes `field_id`.
    pub fn blobs_for_field(&self, field_id: i32) -> impl Iterator<Item = &PortableSketchBlob> {
        self.blobs
            .iter()
            .filter(move |blob| blob.fields.contains(&field_id))
    }

    /// Validate known single-column kinds and decode their payloads.
    ///
    /// Unknown kinds are intentionally skipped according to the Puffin reader
    /// contract. Duplicate `(field, kind)` entries are rejected as ambiguous.
    pub fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for blob in &self.blobs {
            if !is_supported_column_kind(blob.kind()) {
                continue;
            }
            let [field_id] = blob.fields() else {
                return Err(Error::InvalidPuffin(format!(
                    "{} requires exactly one field id; got {:?}",
                    blob.kind(),
                    blob.fields()
                )));
            };
            if *field_id < 0 {
                return Err(Error::InvalidPuffin(format!(
                    "{} carries negative field id {field_id}",
                    blob.kind()
                )));
            }
            if !seen.insert((*field_id, blob.kind().to_owned())) {
                return Err(Error::InvalidPuffin(format!(
                    "duplicate {} blob for field {field_id}",
                    blob.kind()
                )));
            }
            decode_known(blob)?;
        }
        Ok(())
    }

    /// Decode the supported sketches for one Iceberg field id.
    ///
    /// `Ok(None)` means no supported HLL or equi-depth histogram is present.
    pub fn decode_column(&self, field_id: i32) -> Result<Option<DecodedColumnStats>> {
        if field_id < 0 {
            return Err(Error::InvalidPuffin(format!(
                "field id must be non-negative; got {field_id}"
            )));
        }
        self.validate()?;

        let mut hll = None;
        let mut histogram = None;
        for blob in self.blobs.iter().filter(|blob| blob.fields() == [field_id]) {
            match blob.kind() {
                HllSketch::KIND => hll = Some(HllSketch::from_bytes(blob.payload())?),
                EquiDepthHistogram::KIND => {
                    histogram = Some(EquiDepthHistogram::from_bytes(blob.payload())?)
                }
                _ => {}
            }
        }

        if hll.is_none() && histogram.is_none() {
            return Ok(None);
        }
        let stats = hll.as_ref().map_or_else(ColumnStats::new, |sketch| {
            ColumnStats::new().with_distinct_count(sketch.estimate())
        });
        Ok(Some(DecodedColumnStats {
            field_id,
            stats,
            hll,
            histogram,
        }))
    }
}

/// Typed adapter view of the supported sketches for one column.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DecodedColumnStats {
    field_id: i32,
    stats: ColumnStats,
    hll: Option<HllSketch>,
    histogram: Option<EquiDepthHistogram>,
}

impl DecodedColumnStats {
    pub const fn field_id(&self) -> i32 {
        self.field_id
    }

    /// Canonical scalar statistics suitable for native planner adapters.
    ///
    /// The HLL contributes `distinct_count`. A histogram remains available
    /// through [`Self::histogram`] but does not imply a full table row count.
    pub const fn column_stats(&self) -> &ColumnStats {
        &self.stats
    }

    pub const fn hll(&self) -> Option<&HllSketch> {
        self.hll.as_ref()
    }

    pub const fn histogram(&self) -> Option<&EquiDepthHistogram> {
        self.histogram.as_ref()
    }
}

/// Whether `kind` has a typed, single-column projection in this release.
pub fn is_supported_column_kind(kind: &str) -> bool {
    matches!(kind, HllSketch::KIND | EquiDepthHistogram::KIND)
}

fn decode_known(blob: &PortableSketchBlob) -> Result<()> {
    match blob.kind() {
        HllSketch::KIND => HllSketch::from_bytes(blob.payload()).map(|_| ()),
        EquiDepthHistogram::KIND => EquiDepthHistogram::from_bytes(blob.payload()).map(|_| ()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PortableStatsSnapshot, Vec<u8>, Vec<u8>) {
        let mut hll = HllSketch::new(10).unwrap();
        for value in 0..256_u32 {
            hll.add(&value.to_le_bytes());
        }
        let histogram = EquiDepthHistogram::from_values(
            &(0..100).map(|value| value as f64).collect::<Vec<_>>(),
            10,
        )
        .unwrap();
        let hll_bytes = hll.to_bytes().unwrap();
        let histogram_bytes = histogram.to_bytes().unwrap();
        let snapshot = PortableStatsSnapshot::new(
            Some(42),
            vec![
                PortableSketchBlob::new(HllSketch::KIND, vec![17], hll_bytes.clone()),
                PortableSketchBlob::new(
                    EquiDepthHistogram::KIND,
                    vec![17],
                    histogram_bytes.clone(),
                ),
                PortableSketchBlob::new("vendor.future-v1", vec![17], b"opaque".to_vec()),
            ],
        );
        (snapshot, hll_bytes, histogram_bytes)
    }

    #[test]
    fn decodes_supported_kinds_and_preserves_raw_payloads() {
        let (snapshot, hll_bytes, histogram_bytes) = fixture();
        snapshot.validate().unwrap();
        let decoded = snapshot.decode_column(17).unwrap().unwrap();

        assert_eq!(decoded.field_id(), 17);
        assert!(decoded.column_stats().distinct_count.is_some());
        assert_eq!(decoded.histogram().unwrap().total(), 100);
        assert_eq!(decoded.histogram().unwrap().estimate_range(0.0, 49.0), 50);
        assert_eq!(
            snapshot
                .blobs()
                .iter()
                .find(|blob| blob.kind() == HllSketch::KIND)
                .unwrap()
                .payload(),
            hll_bytes
        );
        assert_eq!(
            snapshot
                .blobs()
                .iter()
                .find(|blob| blob.kind() == EquiDepthHistogram::KIND)
                .unwrap()
                .payload(),
            histogram_bytes
        );
    }

    #[test]
    fn unknown_kind_is_preserved_but_not_projected() {
        let snapshot = PortableStatsSnapshot::new(
            None,
            vec![PortableSketchBlob::new(
                "samkhya.hll-v2",
                vec![17],
                b"future".to_vec(),
            )],
        );
        assert!(snapshot.decode_column(17).unwrap().is_none());
        assert_eq!(snapshot.blobs()[0].payload(), b"future");
    }

    #[test]
    fn corrupt_known_payload_fails_closed() {
        let snapshot = PortableStatsSnapshot::new(
            None,
            vec![PortableSketchBlob::new(
                HllSketch::KIND,
                vec![17],
                b"corrupt".to_vec(),
            )],
        );
        assert!(snapshot.validate().is_err());
        assert!(snapshot.decode_column(17).is_err());
    }

    #[test]
    fn duplicate_known_kind_is_rejected() {
        let (mut snapshot, _, _) = fixture();
        let duplicate = snapshot
            .blobs()
            .iter()
            .find(|blob| blob.kind() == HllSketch::KIND)
            .unwrap()
            .clone();
        snapshot.blobs.push(duplicate);
        assert!(snapshot.validate().is_err());
    }
}
