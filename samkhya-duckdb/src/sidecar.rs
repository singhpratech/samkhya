//! Client-side consumption of portable Puffin statistics.
//!
//! Iceberg discovery and Puffin I/O produce a
//! [`PortableStatsSnapshot`]. This module projects one Iceberg field ID from
//! that engine-neutral snapshot into the canonical statistics and sketches a
//! DuckDB embedding can inspect. It deliberately does not mutate DuckDB's
//! catalog or optimizer; native cardinality injection requires a separate,
//! stable DuckDB extension hook.

use samkhya_core::Result;
use samkhya_core::portable::{DecodedColumnStats, PortableStatsSnapshot};
use samkhya_core::sketches::{EquiDepthHistogram, HllSketch};
use samkhya_core::stats::ColumnStats;

/// A client-side DuckDB view of the portable statistics for one Iceberg field.
///
/// The scalar [`ColumnStats`] and typed sketches all come from the same
/// validated snapshot entry. Keeping the sketches available lets callers use
/// HLL membership-independent estimates and histogram range estimates without
/// reconstructing them from planner scalars.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PortableColumnStats {
    decoded: DecodedColumnStats,
}

impl PortableColumnStats {
    /// Iceberg field ID used to select this column.
    pub const fn iceberg_field_id(&self) -> i32 {
        self.decoded.field_id()
    }

    /// Canonical, engine-neutral scalar statistics.
    ///
    /// An HLL contributes `distinct_count`. A histogram remains available via
    /// [`Self::histogram`] but does not imply a full table row count.
    pub const fn column_stats(&self) -> &ColumnStats {
        self.decoded.column_stats()
    }

    /// Decoded HLL, when the snapshot carried the v1 HLL kind.
    pub const fn hll(&self) -> Option<&HllSketch> {
        self.decoded.hll()
    }

    /// Decoded equi-depth histogram, when present.
    pub const fn histogram(&self) -> Option<&EquiDepthHistogram> {
        self.decoded.histogram()
    }
}

/// Decode portable statistics for one Iceberg field ID.
///
/// Returns `Ok(None)` when the field has no supported HLL or histogram.
/// Malformed known payloads, duplicate known kinds, invalid field IDs, and
/// other snapshot validation failures return `Err`; callers must not mistake
/// corrupt statistics for an empty column.
///
/// This function is always available. It does not require the `bundled`
/// feature because it performs no DuckDB FFI or SQL execution.
pub fn decode_portable_column(
    snapshot: &PortableStatsSnapshot,
    iceberg_field_id: i32,
) -> Result<Option<PortableColumnStats>> {
    snapshot
        .decode_column(iceberg_field_id)
        .map(|decoded| decoded.map(|decoded| PortableColumnStats { decoded }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use samkhya_core::portable::PortableSketchBlob;
    use samkhya_core::sketches::Sketch;

    const FIELD_ID: i32 = 17;

    fn fixture() -> (PortableStatsSnapshot, Vec<u8>, Vec<u8>, u64) {
        let mut hll = HllSketch::new(12).expect("valid HLL precision");
        for value in 0..512_u32 {
            hll.add(value.to_string().as_bytes());
        }
        let expected_distinct = hll.estimate();
        let histogram = EquiDepthHistogram::from_values(
            &(0..200).map(|value| value as f64).collect::<Vec<_>>(),
            10,
        )
        .expect("valid histogram");
        let hll_bytes = hll.to_bytes().expect("serialize HLL");
        let histogram_bytes = histogram.to_bytes().expect("serialize histogram");
        let snapshot = PortableStatsSnapshot::new(
            Some(42),
            vec![
                PortableSketchBlob::new(HllSketch::KIND, vec![FIELD_ID], hll_bytes.clone()),
                PortableSketchBlob::new(
                    EquiDepthHistogram::KIND,
                    vec![FIELD_ID],
                    histogram_bytes.clone(),
                ),
                PortableSketchBlob::new(
                    "vendor.future-statistics-v1",
                    vec![FIELD_ID],
                    b"opaque".to_vec(),
                ),
            ],
        );
        (snapshot, hll_bytes, histogram_bytes, expected_distinct)
    }

    #[test]
    fn decodes_canonical_stats_and_preserves_typed_sketches() {
        let (snapshot, hll_bytes, histogram_bytes, expected_distinct) = fixture();
        let decoded = decode_portable_column(&snapshot, FIELD_ID)
            .expect("valid snapshot")
            .expect("supported column");

        assert_eq!(decoded.iceberg_field_id(), FIELD_ID);
        assert_eq!(
            decoded.column_stats().distinct_count,
            Some(expected_distinct)
        );
        assert_eq!(
            decoded.hll().expect("HLL").to_bytes().expect("HLL bytes"),
            hll_bytes
        );
        let histogram = decoded.histogram().expect("histogram");
        assert_eq!(
            histogram.to_bytes().expect("histogram bytes"),
            histogram_bytes
        );
        assert_eq!(histogram.total(), 200);
        assert!(histogram.estimate_range(0.0, 99.0) > 0);
    }

    #[test]
    fn histogram_only_column_keeps_scalar_stats_conservative() {
        let histogram =
            EquiDepthHistogram::from_values(&[1.0, 2.0, 3.0, 4.0], 2).expect("valid histogram");
        let snapshot = PortableStatsSnapshot::new(
            None,
            vec![PortableSketchBlob::new(
                EquiDepthHistogram::KIND,
                vec![FIELD_ID],
                histogram.to_bytes().expect("serialize histogram"),
            )],
        );

        let decoded = decode_portable_column(&snapshot, FIELD_ID)
            .expect("valid snapshot")
            .expect("histogram column");
        assert!(decoded.hll().is_none());
        assert!(decoded.histogram().is_some());
        assert_eq!(decoded.column_stats().row_count, None);
        assert_eq!(decoded.column_stats().distinct_count, None);
    }

    #[test]
    fn unknown_kind_abstains_without_losing_snapshot_bytes() {
        let snapshot = PortableStatsSnapshot::new(
            None,
            vec![PortableSketchBlob::new(
                "samkhya.hll-v2",
                vec![FIELD_ID],
                b"future payload".to_vec(),
            )],
        );

        assert!(
            decode_portable_column(&snapshot, FIELD_ID)
                .expect("unknown kinds are valid")
                .is_none()
        );
        assert_eq!(snapshot.blobs()[0].payload(), b"future payload");
    }

    #[test]
    fn corrupt_known_payload_fails_closed() {
        let snapshot = PortableStatsSnapshot::new(
            None,
            vec![PortableSketchBlob::new(
                HllSketch::KIND,
                vec![FIELD_ID],
                b"corrupt".to_vec(),
            )],
        );

        assert!(decode_portable_column(&snapshot, FIELD_ID).is_err());
    }

    #[test]
    fn invalid_field_id_is_rejected() {
        let (snapshot, _, _, _) = fixture();
        assert!(decode_portable_column(&snapshot, -1).is_err());
    }
}
