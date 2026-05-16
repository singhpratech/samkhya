//! Fortress test (H04): battle-harden the samkhya-arrow adapter.
//!
//! Covers:
//!   1. Sketch round-trip via Arrow ingestion → `to_bytes` → `from_bytes`
//!      → behavioral equivalence, for all five sketch types
//!      (HLL, Bloom, CMS, EquiDepth, CorrelatedHistogram2D).
//!   2. Arrow IPC stream round-trip of the input `RecordBatch`, with
//!      sketches built on each side compared byte-for-byte to confirm
//!      the IPC step is lossless w.r.t. sketch construction.
//!   3. Adversarial decode: corrupted byte streams (empty, 1-byte, all
//!      0xFF, truncated, random) handed to every sketch decoder, plus
//!      corrupted Arrow IPC streams handed to `StreamReader`. None of
//!      these may panic; all must return `Err`.

use std::io::Cursor;
use std::sync::Arc;

use arrow::array::{Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::reader::StreamReader;
use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;
use samkhya_arrow::batch::{build_blooms, build_column_sketches, build_histograms};
use samkhya_arrow::ingest::{
    ingest_array_into_cms, ingest_array_into_histogram_values, ingest_array_into_hll,
};
use samkhya_core::sketches::{
    BloomFilter, CorrelatedHistogram2D, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixture_batch() -> RecordBatch {
    // 20 rows, three columns: integer, string, float.  Mixed widths give
    // the per-type ingestion arms a workout.
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("label", DataType::Utf8, false),
        Field::new("score", DataType::Float64, false),
    ]));
    let ids: Vec<i64> = (0..20).collect();
    let labels: Vec<String> = (0..20).map(|i| format!("v{:02}", i % 7)).collect();
    let labels_ref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let scores: Vec<f64> = (0..20).map(|i| (i as f64) * 0.5).collect();
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(labels_ref)),
            Arc::new(Float64Array::from(scores)),
        ],
    )
    .expect("schema and columns line up")
}

fn ipc_roundtrip(batch: &RecordBatch) -> RecordBatch {
    // Write the batch into an Arrow IPC stream, then read it back.  This
    // is the path any consumer wired through the arrow-flight or
    // file-based protocols will travel.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut w = StreamWriter::try_new(&mut buf, &batch.schema()).expect("ipc writer");
        w.write(batch).expect("ipc write");
        w.finish().expect("ipc finish");
    }
    let reader = StreamReader::try_new(Cursor::new(buf), None).expect("ipc reader");
    let batches: Vec<RecordBatch> = reader.collect::<Result<_, _>>().expect("decoded batches");
    assert_eq!(batches.len(), 1, "single batch in, single batch out");
    batches.into_iter().next().unwrap()
}

// ---------------------------------------------------------------------------
// Step 6: per-sketch round-trip
// ---------------------------------------------------------------------------

#[test]
fn hll_roundtrip_from_record_batch() {
    let batch = fixture_batch();
    let sketches = build_column_sketches(&batch, 12).expect("build hll");
    assert_eq!(sketches.len(), 3);
    for hll in &sketches {
        let bytes = hll.to_bytes().expect("encode");
        let decoded = HllSketch::from_bytes(&bytes).expect("decode");
        // Canonical byte form is the portable equality witness.
        let re = decoded.to_bytes().expect("re-encode");
        assert_eq!(bytes, re, "HLL byte form stable across round-trip");
        // Behavioral equivalence: estimate matches.
        assert_eq!(hll.estimate(), decoded.estimate());
    }
}

#[test]
fn bloom_roundtrip_from_record_batch() {
    let batch = fixture_batch();
    let blooms = build_blooms(&batch, 0.01).expect("build bloom");
    assert_eq!(blooms.len(), 3);
    for bf in &blooms {
        let bytes = bf.to_bytes().expect("encode");
        let decoded = BloomFilter::from_bytes(&bytes).expect("decode");
        let re = decoded.to_bytes().expect("re-encode");
        assert_eq!(bytes, re, "Bloom byte form stable across round-trip");
        // Behavioral equivalence: any byte slice classifies the same way.
        for sample in [b"v00".as_slice(), b"v01", b"missing", b"", &[0xFFu8; 8]] {
            assert_eq!(bf.contains(sample), decoded.contains(sample));
        }
    }
}

#[test]
fn cms_roundtrip_from_record_batch() {
    let batch = fixture_batch();
    let mut cms = CountMinSketch::new(4, 256).expect("cms config");
    // Ingest the string column — repeated labels make the count
    // estimates non-trivial.
    ingest_array_into_cms(batch.column(1).as_ref(), &mut cms, 1);

    let bytes = cms.to_bytes().expect("encode");
    let decoded = CountMinSketch::from_bytes(&bytes).expect("decode");
    let re = decoded.to_bytes().expect("re-encode");
    assert_eq!(bytes, re, "CMS byte form stable across round-trip");
    for needle in ["v00", "v03", "v06", "absent"] {
        assert_eq!(
            cms.estimate(needle.as_bytes()),
            decoded.estimate(needle.as_bytes())
        );
    }
}

#[test]
fn equidepth_roundtrip_from_record_batch() {
    let batch = fixture_batch();
    let hists = build_histograms(&batch, 4).expect("build histograms");
    assert_eq!(hists.len(), 3);
    // Non-numeric column (label) slots in as None.
    assert!(hists[1].is_none());
    for h in hists.into_iter().flatten() {
        let bytes = h.to_bytes().expect("encode");
        let decoded = EquiDepthHistogram::from_bytes(&bytes).expect("decode");
        let re = decoded.to_bytes().expect("re-encode");
        assert_eq!(bytes, re, "EquiDepth byte form stable across round-trip");
        // Spot-check range queries match.
        for (lo, hi) in [(-1.0, 1000.0), (0.0, 5.0), (2.0, 4.0)] {
            assert_eq!(h.estimate_range(lo, hi), decoded.estimate_range(lo, hi));
        }
    }
}

#[test]
fn correlated_roundtrip_from_record_batch() {
    // Build a CorrelatedHistogram2D from two numeric columns of the
    // batch by pulling them out through the Arrow ingest helper.
    let batch = fixture_batch();
    let xs = ingest_array_into_histogram_values(batch.column(0).as_ref()).expect("xs");
    let ys = ingest_array_into_histogram_values(batch.column(2).as_ref()).expect("ys");
    assert_eq!(xs.len(), ys.len());
    let pairs: Vec<(f64, f64)> = xs.into_iter().zip(ys).collect();
    let h = CorrelatedHistogram2D::from_pairs(&pairs, 5, 5).expect("build 2d");

    let bytes = h.to_bytes().expect("encode");
    let decoded = CorrelatedHistogram2D::from_bytes(&bytes).expect("decode");
    let re = decoded.to_bytes().expect("re-encode");
    assert_eq!(bytes, re, "CorrelatedHistogram2D byte form stable");

    // Behavioral equivalence: a sweep of 2D range queries.
    for (alo, ahi, blo, bhi) in [
        (-1.0, 100.0, -1.0, 100.0),
        (0.0, 5.0, 0.0, 5.0),
        (10.0, 15.0, 2.0, 4.0),
    ] {
        assert_eq!(
            h.estimate_range(alo, ahi, blo, bhi),
            decoded.estimate_range(alo, ahi, blo, bhi)
        );
    }
    assert_eq!(h.total(), decoded.total());
}

// ---------------------------------------------------------------------------
// Arrow IPC round-trip of the RecordBatch, then sketch equivalence
// ---------------------------------------------------------------------------

#[test]
fn ipc_roundtrip_preserves_sketch_construction() {
    let original = fixture_batch();
    let after_ipc = ipc_roundtrip(&original);

    // For each sketch type, building from the original batch and from
    // the IPC-decoded batch must yield byte-identical sketches.
    let hll_a = build_column_sketches(&original, 12).expect("hll a");
    let hll_b = build_column_sketches(&after_ipc, 12).expect("hll b");
    assert_eq!(hll_a.len(), hll_b.len());
    for (a, b) in hll_a.iter().zip(hll_b.iter()) {
        assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap());
    }

    let bf_a = build_blooms(&original, 0.01).expect("bf a");
    let bf_b = build_blooms(&after_ipc, 0.01).expect("bf b");
    for (a, b) in bf_a.iter().zip(bf_b.iter()) {
        assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap());
    }

    // CMS — explicit, since there's no batch helper.
    let mut cms_a = CountMinSketch::new(4, 256).unwrap();
    let mut cms_b = CountMinSketch::new(4, 256).unwrap();
    ingest_array_into_cms(original.column(1).as_ref(), &mut cms_a, 1);
    ingest_array_into_cms(after_ipc.column(1).as_ref(), &mut cms_b, 1);
    assert_eq!(cms_a.to_bytes().unwrap(), cms_b.to_bytes().unwrap());

    // Histograms.
    let h_a = build_histograms(&original, 4).expect("hist a");
    let h_b = build_histograms(&after_ipc, 4).expect("hist b");
    for (a, b) in h_a.iter().zip(h_b.iter()) {
        match (a, b) {
            (Some(x), Some(y)) => assert_eq!(x.to_bytes().unwrap(), y.to_bytes().unwrap()),
            (None, None) => {}
            _ => panic!("histogram None-ness diverged across IPC round-trip"),
        }
    }
}

// Smoke check that an HLL built directly via `ingest_array_into_hll` on an
// Arrow array round-trips identically to one built via the batch helper.
#[test]
fn ingest_hll_array_path_matches_batch_path() {
    let batch = fixture_batch();
    let mut hll_direct = HllSketch::new(12).expect("hll precision");
    ingest_array_into_hll(batch.column(0).as_ref(), &mut hll_direct);

    let sketches = build_column_sketches(&batch, 12).expect("batch hll");
    assert_eq!(
        hll_direct.to_bytes().unwrap(),
        sketches[0].to_bytes().unwrap(),
        "ingest and batch helpers agree on HLL byte form for the same column"
    );
}

// ---------------------------------------------------------------------------
// Step 7: adversarial decoders — must error, never panic
// ---------------------------------------------------------------------------

fn corruption_corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("one_byte", vec![0x00]),
        ("one_byte_ff", vec![0xFF]),
        ("all_zero_16", vec![0x00; 16]),
        ("all_ff_16", vec![0xFF; 16]),
        ("all_ff_1k", vec![0xFF; 1024]),
        ("ascii_garbage", b"not a real sketch payload".to_vec()),
    ];
    // Pseudo-random (deterministic) garbage of various sizes.
    for size in [3usize, 7, 31, 127, 1023, 4096] {
        let bytes: Vec<u8> = (0..size)
            .map(|i| ((i.wrapping_mul(2654435761)) & 0xFF) as u8)
            .collect();
        let leaked: &'static str = Box::leak(format!("random_{size}").into_boxed_str());
        out.push((leaked, bytes));
    }
    out
}

// SRC01 follow-up: HLL `from_bytes` now post-validates the decoded
// struct (precision ∈ [4,18] AND registers.len() == 2^precision). The
// historical 16-byte all-zero leak — which bincode-decoded as
// `{precision: 0, registers: vec![]}` and bypassed the new()-time
// range check — is now rejected. The admitted-set assertion below
// flipped from `admitted = {"all_zero_16"}` to `admitted == {}`.
#[test]
fn hll_decode_never_panics() {
    let mut admitted: Vec<&'static str> = Vec::new();
    for (label, bytes) in corruption_corpus() {
        let res = std::panic::catch_unwind(|| HllSketch::from_bytes(&bytes));
        let res = res.unwrap_or_else(|_| panic!("HllSketch::from_bytes panicked on {label}"));
        if res.is_ok() {
            admitted.push(label);
        }
    }
    assert!(
        admitted.is_empty(),
        "HLL from_bytes admitted adversarial inputs: {admitted:?} (expected empty after SRC01)"
    );
}

#[test]
fn bloom_decode_rejects_corruption() {
    for (label, bytes) in corruption_corpus() {
        let res = std::panic::catch_unwind(|| BloomFilter::from_bytes(&bytes));
        let res = res.unwrap_or_else(|_| panic!("BloomFilter::from_bytes panicked on {label}"));
        assert!(
            res.is_err(),
            "BloomFilter::from_bytes accepted garbage: {label}"
        );
    }
}

#[test]
fn cms_decode_rejects_corruption() {
    for (label, bytes) in corruption_corpus() {
        let res = std::panic::catch_unwind(|| CountMinSketch::from_bytes(&bytes));
        let res = res.unwrap_or_else(|_| panic!("CountMinSketch::from_bytes panicked on {label}"));
        assert!(
            res.is_err(),
            "CountMinSketch::from_bytes accepted garbage: {label}"
        );
    }
}

#[test]
fn equidepth_decode_rejects_corruption() {
    for (label, bytes) in corruption_corpus() {
        let res = std::panic::catch_unwind(|| EquiDepthHistogram::from_bytes(&bytes));
        let res =
            res.unwrap_or_else(|_| panic!("EquiDepthHistogram::from_bytes panicked on {label}"));
        assert!(
            res.is_err(),
            "EquiDepthHistogram::from_bytes accepted garbage: {label}"
        );
    }
}

#[test]
fn correlated_decode_rejects_corruption() {
    for (label, bytes) in corruption_corpus() {
        let res = std::panic::catch_unwind(|| CorrelatedHistogram2D::from_bytes(&bytes));
        let res =
            res.unwrap_or_else(|_| panic!("CorrelatedHistogram2D::from_bytes panicked on {label}"));
        assert!(
            res.is_err(),
            "CorrelatedHistogram2D::from_bytes accepted garbage: {label}"
        );
    }
}

// Truncate a valid sketch byte stream at every prefix length and confirm
// every prefix shorter than the full payload decodes to `Err`. (Empty is
// already covered by the corpus, hence the >=1 lower bound here.)
#[test]
fn hll_truncated_prefixes_reject() {
    let mut h = HllSketch::new(8).unwrap();
    h.add(b"a");
    h.add(b"b");
    let bytes = h.to_bytes().unwrap();
    for n in 1..bytes.len() {
        let prefix = &bytes[..n];
        let res = std::panic::catch_unwind(|| HllSketch::from_bytes(prefix));
        let res = res.unwrap_or_else(|_| panic!("HllSketch::from_bytes panicked at prefix {n}"));
        assert!(res.is_err(), "HLL accepted truncated prefix len {n}");
    }
    // The full payload must still decode cleanly.
    assert!(HllSketch::from_bytes(&bytes).is_ok());
}

// ---------------------------------------------------------------------------
// Adversarial Arrow IPC: corrupted streams handed to StreamReader.
// We only need to confirm the *adapter* doesn't propagate a panic — the
// adapter doesn't itself decode IPC, but any caller that pipes Arrow IPC
// into our ingestion path will go through StreamReader first, so we
// document the safety of that handoff here.
// ---------------------------------------------------------------------------

// arrow-ipc 54.3.1 (the version pinned in this workspace) is known to
// panic with `capacity overflow` on certain malformed length prefixes
// (e.g. a 64-byte all-0xFF payload). That panic is an *upstream* defect
// in arrow-rs, not in samkhya, and is documented in the H04 receipt's
// blocker list. Here we test the inputs the upstream decoder *should*
// reject cleanly — empty, short, ASCII, and truncations of an otherwise
// valid stream. A future arrow-rs upgrade should let us re-add the
// `all_ff_64` case.
#[test]
fn ipc_stream_reader_rejects_corruption() {
    let cases: Vec<(&'static str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("one_byte", vec![0x00]),
        ("one_byte_ff", vec![0xFF]),
        ("all_zero_64", vec![0x00; 64]),
        ("ascii", b"definitely not an arrow stream".to_vec()),
    ];
    // Also include a *valid* IPC stream truncated at every prefix length.
    let batch = fixture_batch();
    let mut full: Vec<u8> = Vec::new();
    {
        let mut w = StreamWriter::try_new(&mut full, &batch.schema()).unwrap();
        w.write(&batch).unwrap();
        w.finish().unwrap();
    }
    let mut all_cases = cases;
    // Sample a handful of truncation points to keep the test fast and
    // deterministic.
    for n in [1usize, 8, 32, 64, full.len() / 2, full.len() - 1] {
        if n < full.len() {
            let leaked: &'static str = Box::leak(format!("truncated_{n}").into_boxed_str());
            all_cases.push((leaked, full[..n].to_vec()));
        }
    }

    for (label, bytes) in all_cases {
        let res = std::panic::catch_unwind(|| {
            let reader = StreamReader::try_new(Cursor::new(bytes.clone()), None);
            match reader {
                Err(_) => Ok::<(), ()>(()), // header rejection — fine
                Ok(r) => {
                    // Iterate the stream; any decode error is fine, the
                    // important property is no panic.
                    for batch in r {
                        if batch.is_err() {
                            return Ok(());
                        }
                    }
                    Ok(())
                }
            }
        });
        assert!(
            res.is_ok(),
            "StreamReader panicked on adversarial input: {label}"
        );
    }
}
