//! Cross-module integration test for samkhya-core.
//!
//! Exercises a realistic pipeline that touches sketches, Puffin I/O,
//! ColumnStats, the feedback recorder, and LpBound clamping — in one go.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};

use samkhya_core::ColumnStats;
use samkhya_core::feedback::{FeedbackStore, Observation};
use samkhya_core::lpbound::{AgmBound, UpperBound, clamp_estimate, saturating_clamp};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{HllSketch, Sketch};
use tempfile::NamedTempFile;

#[test]
fn full_pipeline_round_trip() {
    // ---- 1. Build a deterministic dataset ---------------------------------
    let total_rows = 10_000u64;
    let distinct_target = 5_000u64;
    let ids: Vec<u64> = (0..total_rows).map(|i| i % distinct_target).collect();
    let true_distinct = ids.iter().copied().collect::<HashSet<u64>>().len() as u64;
    assert_eq!(true_distinct, distinct_target);

    // ---- 2. Build the HLL sketch ------------------------------------------
    let mut hll = HllSketch::new(14).expect("valid precision");
    for id in &ids {
        hll.add(&id.to_le_bytes());
    }
    let hll_bytes = hll.to_bytes().expect("hll serialize");

    // ---- 3. Persist to a Puffin file --------------------------------------
    let puffin_tmp = NamedTempFile::new().expect("tempfile for puffin");
    let puffin_path = puffin_tmp.path().to_path_buf();
    {
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&puffin_path)
            .expect("open puffin for write");
        let mut writer = PuffinWriter::new(file);
        writer
            .add_blob(Blob::new(HllSketch::KIND, vec![1], &hll_bytes))
            .expect("add hll blob");
        writer.finish().expect("finish puffin");
    }

    // ---- 4. Reopen and recover --------------------------------------------
    let mut reader = PuffinReader::open(File::open(&puffin_path).expect("reopen puffin"))
        .expect("parse puffin footer");
    let (idx, meta) = reader.find_blob(HllSketch::KIND).expect("hll blob present");
    assert_eq!(meta.fields, vec![1]);
    let recovered_bytes = reader.read_blob(idx).expect("read hll blob");
    let recovered = HllSketch::from_bytes(&recovered_bytes).expect("decode hll");
    let estimate = recovered.estimate();

    // HLL estimate should be within ~5% of the ground truth at p=14.
    let rel_err = (estimate as f64 - true_distinct as f64).abs() / true_distinct as f64;
    assert!(
        rel_err < 0.05,
        "HLL estimate {estimate} off by {rel_err} (truth={true_distinct})"
    );

    // ---- 5. Derive an LpBound ceiling and build ColumnStats ---------------
    let ceiling = AgmBound.ceiling(&[total_rows, total_rows], &[(0, 1)]);
    let stats = ColumnStats::new()
        .with_row_count(total_rows)
        .with_distinct_count(estimate)
        .with_upper_bound(ceiling);
    assert_eq!(stats.row_count, Some(total_rows));
    assert_eq!(stats.distinct_count, Some(estimate));
    assert_eq!(stats.upper_bound_rows, Some(ceiling));

    // The recovered estimate must respect the ceiling.
    let clamped = clamp_estimate(estimate as f64, ceiling).expect("estimate fits under ceiling");
    assert_eq!(clamped, estimate);

    // An obviously oversized correction should saturate, not blow up.
    let oversized = saturating_clamp((ceiling as f64) * 10.0, ceiling);
    assert_eq!(oversized, ceiling);

    // ---- 6. Record + replay a feedback observation ------------------------
    let sqlite_tmp = NamedTempFile::new().expect("tempfile for sqlite");
    let sqlite_path = sqlite_tmp.path().to_path_buf();
    let store = FeedbackStore::open(&sqlite_path).expect("open feedback store");
    let obs = Observation {
        template_hash: "tpl-int-test".into(),
        plan_fingerprint: "plan-hll-hash-join".into(),
        est_rows: estimate,
        actual_rows: true_distinct,
        latency_ms: Some(12.5),
    };
    let row_id = store.record(&obs).expect("record observation");
    assert!(row_id > 0);
    assert_eq!(store.count().expect("count"), 1);

    let history = store.history("tpl-int-test").expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0], obs);

    // q-error is bounded by the HLL's relative error envelope; 1.10 is
    // comfortably loose for p=14 on 5_000 distinct items.
    let q = history[0].q_error();
    assert!(q < 1.10, "q-error {q} too large for p=14 HLL");
}
