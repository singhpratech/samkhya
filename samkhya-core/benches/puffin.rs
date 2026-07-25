//! Criterion microbenchmarks for the Puffin sidecar reader/writer.
//!
//! These exercise the footer encode/decode and seek/read paths that gate
//! sub-ms sidecar access in samkhya.md §3.

use std::io::Cursor;

use criterion::{Criterion, criterion_group, criterion_main};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use std::hint::black_box;

/// Deterministic pseudo-random 1KB blob payloads (no rand dep needed).
fn make_blobs(n: usize, size: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            let seed = (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
            let mut buf = vec![0u8; size];
            let mut state = seed;
            for byte in buf.iter_mut() {
                // Linear-congruential mix; good enough for benchmark payloads.
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                *byte = (state >> 56) as u8;
            }
            buf
        })
        .collect()
}

fn write_puffin(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
    for (i, p) in payloads.iter().enumerate() {
        writer
            .add_blob(Blob::new("samkhya.bench-v1", vec![i as i32], p))
            .unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn bench_puffin_write_10_blobs(c: &mut Criterion) {
    let payloads = make_blobs(10, 1024);
    c.bench_function("puffin_write_10_blobs", |b| {
        b.iter_with_setup(
            || payloads.clone(),
            |payloads| {
                let mut writer = PuffinWriter::new(Cursor::new(Vec::with_capacity(16 * 1024)));
                for (i, p) in payloads.iter().enumerate() {
                    writer
                        .add_blob(Blob::new("samkhya.bench-v1", vec![i as i32], p))
                        .unwrap();
                }
                let out = writer.finish().unwrap().into_inner();
                black_box(out);
            },
        );
    });
}

fn bench_puffin_open(c: &mut Criterion) {
    let payloads = make_blobs(10, 1024);
    let bytes = write_puffin(&payloads);
    c.bench_function("puffin_open", |b| {
        b.iter(|| {
            let reader = PuffinReader::open(Cursor::new(black_box(bytes.as_slice()))).unwrap();
            debug_assert_eq!(reader.blobs().len(), 10);
            black_box(reader);
        });
    });
}

fn bench_puffin_read_blob(c: &mut Criterion) {
    let payloads = make_blobs(10, 1024);
    let bytes = write_puffin(&payloads);
    c.bench_function("puffin_read_blob", |b| {
        b.iter(|| {
            let mut reader = PuffinReader::open(Cursor::new(black_box(bytes.as_slice()))).unwrap();
            let blob = reader.read_blob(0).unwrap();
            black_box(blob);
        });
    });
}

criterion_group!(
    puffin,
    bench_puffin_write_10_blobs,
    bench_puffin_open,
    bench_puffin_read_blob,
);
criterion_main!(puffin);
