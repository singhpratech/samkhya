//! Fortress tests for samkhya-iceberg's Puffin sidecar contract.
//!
//! Three axes of coverage:
//!
//! 1. Sketch round-trip: every samkhya sketch kind goes through a
//!    Puffin blob (write -> read) byte-identically.
//! 2. KIND-tag handling: blobs tagged with an unknown samkhya KIND
//!    (e.g. "samkhya.unknown-v9") are surfaced to callers without
//!    panic. The sidecar reader simply skips them (Iceberg's
//!    "readers ignore kinds they do not understand" contract), and
//!    typed `Sketch::from_bytes` rejects mismatched payloads with
//!    `Err`.
//! 3. Adversarial bytes: corrupted / random / truncated bytes hand
//!    to `PuffinReader::open` must return `Err`, never panic.

use std::collections::BTreeMap;
use std::io::Cursor;

use samkhya_core::puffin::{Blob, BlobMetadata, FooterPayload, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{
    BloomFilter, CorrelatedHistogram2D, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch,
};

/// Helper: write a Puffin blob with the given `kind` + `payload` and
/// return the resulting in-memory bytes plus the blob's reader-side
/// payload (after round-trip through `PuffinReader::read_blob`).
fn round_trip_blob(kind: &str, payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut writer = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
    writer
        .add_blob(Blob::new(kind, vec![1], payload))
        .expect("add_blob");
    let cursor = writer.finish().expect("finish");
    let bytes = cursor.into_inner();

    let mut reader = PuffinReader::open(Cursor::new(bytes.clone())).expect("open");
    let (idx, meta) = reader
        .find_blob(kind)
        .expect("blob with matching kind present");
    assert_eq!(meta.kind, kind);
    let read_back = reader.read_blob(idx).expect("read_blob");
    (bytes, read_back)
}

// ---------------------------------------------------------------------
// Axis 1: per-sketch round-trip — byte-identical payload preservation.
// ---------------------------------------------------------------------

#[test]
fn round_trip_hll_v1_byte_identical() {
    let mut sketch = HllSketch::new(12).unwrap();
    for i in 0..1_000u32 {
        sketch.add(&i.to_le_bytes());
    }
    let payload = sketch.to_bytes().unwrap();
    let (_, read_back) = round_trip_blob(HllSketch::KIND, &payload);
    assert_eq!(
        read_back, payload,
        "HLL payload must round-trip byte-identical"
    );
    let reconstructed = HllSketch::from_bytes(&read_back).expect("from_bytes");
    let err = (reconstructed.estimate() as f64 - 1_000.0).abs() / 1_000.0;
    assert!(err < 0.1, "HLL estimate off after round-trip: {err}");
}

#[test]
fn round_trip_bloom_v1_byte_identical() {
    let mut bf = BloomFilter::new(1_000, 0.01);
    for i in 0..500u32 {
        bf.insert(&i.to_le_bytes());
    }
    let payload = bf.to_bytes().unwrap();
    let (_, read_back) = round_trip_blob(BloomFilter::KIND, &payload);
    assert_eq!(
        read_back, payload,
        "Bloom payload must round-trip byte-identical"
    );
    let reconstructed = BloomFilter::from_bytes(&read_back).expect("from_bytes");
    for i in 0..500u32 {
        assert!(reconstructed.contains(&i.to_le_bytes()));
    }
}

#[test]
fn round_trip_cms_v1_byte_identical() {
    let mut cms = CountMinSketch::new(5, 1024).unwrap();
    for i in 0..200u32 {
        cms.add(&i.to_le_bytes(), 1);
    }
    let payload = cms.to_bytes().unwrap();
    let (_, read_back) = round_trip_blob(CountMinSketch::KIND, &payload);
    assert_eq!(
        read_back, payload,
        "CMS payload must round-trip byte-identical"
    );
    let reconstructed = CountMinSketch::from_bytes(&read_back).expect("from_bytes");
    // CMS never undercounts; estimate should be >= 1 for every inserted item.
    for i in 0..200u32 {
        assert!(reconstructed.estimate(&i.to_le_bytes()) >= 1);
    }
}

#[test]
fn round_trip_equidepth_v1_byte_identical() {
    let values: Vec<f64> = (0..1_000).map(|i| i as f64).collect();
    let hist = EquiDepthHistogram::from_values(&values, 16).unwrap();
    let payload = hist.to_bytes().unwrap();
    let (_, read_back) = round_trip_blob(EquiDepthHistogram::KIND, &payload);
    assert_eq!(
        read_back, payload,
        "EquiDepthHistogram payload must round-trip byte-identical"
    );
    let reconstructed = EquiDepthHistogram::from_bytes(&read_back).expect("from_bytes");
    assert_eq!(reconstructed.total(), 1_000);
}

#[test]
fn round_trip_correlated2d_v1_byte_identical() {
    let pairs: Vec<(f64, f64)> = (0..500)
        .map(|i| (i as f64, (i as f64) * 0.5 + 1.0))
        .collect();
    let h = CorrelatedHistogram2D::from_pairs(&pairs, 8, 8).unwrap();
    let payload = h.to_bytes().unwrap();
    let (_, read_back) = round_trip_blob(CorrelatedHistogram2D::KIND, &payload);
    assert_eq!(
        read_back, payload,
        "CorrelatedHistogram2D payload must round-trip byte-identical"
    );
    let reconstructed = CorrelatedHistogram2D::from_bytes(&read_back).expect("from_bytes");
    assert_eq!(reconstructed.total(), 500);
}

// ---------------------------------------------------------------------
// Axis 2: KIND-tag validation — unknown kinds, mismatched payloads.
// ---------------------------------------------------------------------

/// A blob written with a KIND tag samkhya does not recognize should
/// still produce a *valid* Puffin file (Iceberg's "readers ignore
/// unknown kinds" contract), and `find_blob` for samkhya's KINDs
/// must return None — never panic. The samkhya-iceberg sidecar
/// loader treats unknown kinds as silent skips, so the resulting
/// `ColumnStats` map must not surface that blob's bytes.
#[test]
fn unknown_kind_tag_is_silently_skipped_not_panicked() {
    let mut writer = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
    writer
        .add_blob(Blob::new("samkhya.unknown-v9", vec![3], b"opaque payload"))
        .unwrap();
    let cursor = writer.finish().unwrap();
    let bytes = cursor.into_inner();

    let reader = PuffinReader::open(Cursor::new(bytes)).expect("open succeeds with unknown kind");
    assert_eq!(reader.blobs().len(), 1);
    assert_eq!(reader.blobs()[0].kind, "samkhya.unknown-v9");

    // None of samkhya's registered KINDs should match.
    for kind in [
        HllSketch::KIND,
        BloomFilter::KIND,
        CountMinSketch::KIND,
        EquiDepthHistogram::KIND,
        CorrelatedHistogram2D::KIND,
    ] {
        assert!(
            reader.find_blob(kind).is_none(),
            "registered kind {kind} must not match opaque blob"
        );
    }
}

/// Feeding a sketch's `from_bytes` a payload that was serialized with
/// a *different* sketch kind must return `Err`, never panic. Each
/// sketch's payload format is bincode-encoded and distinct, so a
/// kind-mismatched payload will not deserialize cleanly.
#[test]
fn mismatched_kind_payload_errors_typed_from_bytes() {
    // Write an HLL payload but try to read it as every other kind.
    let mut hll = HllSketch::new(12).unwrap();
    for i in 0..100u32 {
        hll.add(&i.to_le_bytes());
    }
    let hll_payload = hll.to_bytes().unwrap();

    // BloomFilter::from_bytes on HLL bytes — must Err, not panic.
    let bloom_result = std::panic::catch_unwind(|| BloomFilter::from_bytes(&hll_payload));
    assert!(
        bloom_result.is_ok(),
        "BloomFilter::from_bytes must not panic on mismatched payload"
    );
    assert!(
        bloom_result.unwrap().is_err(),
        "BloomFilter::from_bytes must Err on HLL payload"
    );

    // EquiDepthHistogram::from_bytes on HLL bytes.
    let hist_result = std::panic::catch_unwind(|| EquiDepthHistogram::from_bytes(&hll_payload));
    assert!(
        hist_result.is_ok(),
        "EquiDepthHistogram::from_bytes must not panic on mismatched payload"
    );
    assert!(
        hist_result.unwrap().is_err(),
        "EquiDepthHistogram::from_bytes must Err on HLL payload"
    );

    // CorrelatedHistogram2D::from_bytes on HLL bytes.
    let c2d_result = std::panic::catch_unwind(|| CorrelatedHistogram2D::from_bytes(&hll_payload));
    assert!(
        c2d_result.is_ok(),
        "CorrelatedHistogram2D::from_bytes must not panic on mismatched payload"
    );
    assert!(
        c2d_result.unwrap().is_err(),
        "CorrelatedHistogram2D::from_bytes must Err on HLL payload"
    );
}

// ---------------------------------------------------------------------
// Axis 3: Adversarial bytes — `PuffinReader::open` must never panic.
// ---------------------------------------------------------------------

#[test]
fn adversarial_empty_blob_errors_not_panics() {
    let result = std::panic::catch_unwind(|| PuffinReader::open(Cursor::new(Vec::<u8>::new())));
    assert!(result.is_ok(), "0-byte input must not panic");
    assert!(result.unwrap().is_err(), "0-byte input must Err");
}

#[test]
fn adversarial_single_byte_errors_not_panics() {
    let result = std::panic::catch_unwind(|| PuffinReader::open(Cursor::new(vec![0x42u8])));
    assert!(result.is_ok(), "1-byte input must not panic");
    assert!(result.unwrap().is_err(), "1-byte input must Err");
}

#[test]
fn adversarial_1kb_random_errors_not_panics() {
    // Deterministic xorshift PRNG — same bytes across runs so a
    // regression is easy to bisect. Seeded with a chosen constant.
    let mut state: u64 = 0xdead_beef_cafe_f00d;
    let mut bytes = Vec::with_capacity(1024);
    for _ in 0..1024 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        bytes.push((state & 0xff) as u8);
    }
    let result = std::panic::catch_unwind(|| PuffinReader::open(Cursor::new(bytes)));
    assert!(result.is_ok(), "1KB random input must not panic");
    assert!(result.unwrap().is_err(), "1KB random input must Err");
}

#[test]
fn adversarial_valid_envelope_garbage_footer_errors_not_panics() {
    // Mimic a "valid"-looking Puffin envelope: head magic + random
    // payload bytes + head magic + a payload length that points at
    // total garbage + flags + trailing magic. The footer JSON
    // decode must fail.
    let magic = b"PFA1";
    let garbage_payload: Vec<u8> = (0..256).map(|i| (i ^ 0xa5) as u8).collect();
    let bogus_footer_json: Vec<u8> = (0..64).map(|i| (i ^ 0x5a) as u8).collect();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&garbage_payload);
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&bogus_footer_json);
    bytes.extend_from_slice(&(bogus_footer_json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(magic);

    let result = std::panic::catch_unwind(|| PuffinReader::open(Cursor::new(bytes)));
    assert!(
        result.is_ok(),
        "Valid envelope wrapping garbage footer must not panic"
    );
    assert!(
        result.unwrap().is_err(),
        "Valid envelope wrapping garbage footer must Err"
    );
}

/// Defensive: a *valid* Puffin envelope (proper footer JSON) but
/// with a `BlobMetadata::offset` / `length` pointing outside the
/// file must error when the caller tries to actually read the
/// blob — not panic. Construct it by hand so we exercise the
/// reader's bounds check.
#[test]
fn adversarial_blob_offset_out_of_bounds_errors_not_panics() {
    // Build a synthetic Puffin file with a footer that claims a
    // blob payload at an offset past EOF.
    let head = b"PFA1";
    let footer_payload = FooterPayload {
        blobs: vec![BlobMetadata {
            kind: HllSketch::KIND.to_string(),
            fields: vec![0],
            snapshot_id: None,
            sequence_number: None,
            offset: 1_000_000, // way past EOF
            length: 4_096,
            compression_codec: None,
            properties: BTreeMap::new(),
        }],
        properties: BTreeMap::new(),
    };
    let json = serde_json::to_vec(&footer_payload).unwrap();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(head);
    bytes.extend_from_slice(head); // footer-head magic
    bytes.extend_from_slice(&json);
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(head); // trailing magic

    let result = std::panic::catch_unwind(|| {
        let mut reader = PuffinReader::open(Cursor::new(bytes))?;
        reader.read_blob(0)
    });
    assert!(
        result.is_ok(),
        "Out-of-bounds blob offset must not panic the reader"
    );
    assert!(
        result.unwrap().is_err(),
        "Out-of-bounds blob offset must surface as Err"
    );
}

// ---------------------------------------------------------------------
// Cross-check: the no-feature `column_stats_from_paths` placeholder
// must not panic on a nonsense path and must hand back default
// `ColumnStats` for declared schema fields.
// ---------------------------------------------------------------------

#[test]
fn column_stats_from_paths_robust_to_missing_sidecar() {
    use samkhya_iceberg::{Schema, SnapshotPuffinPaths, column_stats_from_paths};
    let schema = Schema::from_fields([(1, "a"), (2, "b")]);
    let paths = SnapshotPuffinPaths::from_strings(Some(7), ["/definitely/does/not/exist.puffin"]);
    let stats = column_stats_from_paths(&paths, &schema);
    assert_eq!(stats.len(), 2);
    assert!(stats.contains_key(&1));
    assert!(stats.contains_key(&2));
}

#[cfg(feature = "iceberg")]
#[test]
fn live_walker_robust_to_missing_sidecar() {
    use samkhya_iceberg::snapshot::column_stats_from_paths_live;
    use samkhya_iceberg::{Schema, SnapshotPuffinPaths};
    let schema = Schema::from_fields([(1, "a"), (2, "b")]);
    let paths = SnapshotPuffinPaths::from_strings(Some(7), ["/definitely/does/not/exist.puffin"]);
    let stats = column_stats_from_paths_live(&paths, &schema);
    // Missing sidecar -> sidecar read is best-effort, so the schema
    // defaults are preserved and no panic surfaces.
    assert_eq!(stats.len(), 2);
}
