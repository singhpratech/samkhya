//! Stress benches for the v0.9.0 hardening tier.
//!
//! These three benches push the hot paths an order of magnitude past the
//! microbenchmarks in `sketches.rs` and `puffin.rs`. They aren't designed
//! to track sub-microsecond regressions — Criterion's adaptive sample count
//! drops to the minimum on each — they exist to confirm samkhya holds
//! together when the inputs scale up:
//!
//!   * `hll_million_inserts`     — 1M items into a p=14 HLL. Reports total
//!                                  wall time per million; the per-insert
//!                                  cost has its own bench in `sketches.rs`.
//!   * `puffin_thousand_blobs`   — write 1k blobs of 1KB each, then read
//!                                  the file back through PuffinReader.
//!                                  Exercises the JSON-footer scaling path.
//!   * `feedback_ten_thousand_observations` — bulk-insert 10k observations
//!                                  into an in-memory FeedbackStore. The
//!                                  SQLite path's amortized insert cost is
//!                                  the load-bearing number for the v0.6.0
//!                                  JOB-Slow run (113 queries × N runs).
//!
//! Run with:
//!
//!   cargo bench -p samkhya-core --bench stress
//!
//! These do not gate CI — the criterion `cargo bench --no-run` compile
//! check is the only thing the v0.9.0 CI cares about. Real numbers come
//! from a local run on the release hardware.

use std::io::Cursor;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use samkhya_core::feedback::{FeedbackStore, Observation};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::HllSketch;

// ---------------------------------------------------------------------------
// hll_million_inserts
// ---------------------------------------------------------------------------

fn bench_hll_million_inserts(c: &mut Criterion) {
    let mut group = c.benchmark_group("stress");
    // 1M inserts per iteration; clamp the sample count so the bench finishes
    // in <60s rather than the criterion default ~30 samples.
    group.sample_size(10);
    group.bench_function("hll_million_inserts", |b| {
        b.iter_with_setup(
            || HllSketch::new(14).expect("p=14 is in range [4,18]"),
            |mut hll| {
                for i in 0u32..1_000_000 {
                    hll.add(&i.to_le_bytes());
                }
                // Force the estimate out so the optimizer can't elide the
                // register updates.
                black_box(hll.estimate());
                black_box(hll);
            },
        );
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// puffin_thousand_blobs
// ---------------------------------------------------------------------------

/// Deterministic 1KB payload generator. Reused across iterations so the
/// bench measures the writer/reader, not the payload-generation cost.
fn make_payloads(n: usize, size: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            // Cheap LCG so the payloads are non-zero and non-uniform without
            // bringing in a `rand` dependency.
            let mut state = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let mut buf = vec![0u8; size];
            for byte in buf.iter_mut() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                *byte = (state >> 56) as u8;
            }
            buf
        })
        .collect()
}

fn bench_puffin_thousand_blobs(c: &mut Criterion) {
    let mut group = c.benchmark_group("stress");
    group.sample_size(10);
    // Pre-build payloads once outside the timed region.
    let payloads = make_payloads(1_000, 1_024);
    group.bench_function("puffin_thousand_blobs", |b| {
        b.iter(|| {
            // Write 1000 blobs.
            let mut writer = PuffinWriter::new(Cursor::new(Vec::with_capacity(2 * 1024 * 1024)));
            for (i, p) in payloads.iter().enumerate() {
                writer
                    .add_blob(Blob::new("samkhya.stress-v1", vec![i as i32], p))
                    .expect("writer never errors on plain in-memory cursor");
            }
            let bytes = writer
                .finish()
                .expect("finish never errors on plain in-memory cursor")
                .into_inner();

            // Read them back through PuffinReader.
            let mut reader = PuffinReader::open(Cursor::new(bytes.as_slice()))
                .expect("self-written file always parses");
            // Touch every blob so the read-side seek/payload path is exercised.
            for idx in 0..reader.blobs().len() {
                let blob = reader
                    .read_blob(idx)
                    .expect("self-written offsets are always valid");
                black_box(blob);
            }
            black_box(reader);
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// feedback_ten_thousand_observations
// ---------------------------------------------------------------------------

fn bench_feedback_ten_thousand_observations(c: &mut Criterion) {
    let mut group = c.benchmark_group("stress");
    group.sample_size(10);
    group.bench_function("feedback_ten_thousand_observations", |b| {
        b.iter_with_setup(
            || FeedbackStore::open_in_memory().expect("in-memory SQLite always opens"),
            |store| {
                for i in 0u64..10_000 {
                    let obs = Observation {
                        template_hash: "stress-template".to_string(),
                        plan_fingerprint: format!("plan-{i}"),
                        est_rows: i.saturating_add(1),
                        actual_rows: i.saturating_add(2),
                        latency_ms: Some((i as f64) * 0.01),
                    };
                    store.record(&obs).expect("in-memory insert never fails");
                }
                let count = store
                    .count()
                    .expect("count over the same in-memory store never fails");
                black_box(count);
            },
        );
    });
    group.finish();
}

criterion_group!(
    stress,
    bench_hll_million_inserts,
    bench_puffin_thousand_blobs,
    bench_feedback_ten_thousand_observations,
);
criterion_main!(stress);
