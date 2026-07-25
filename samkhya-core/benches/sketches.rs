//! Criterion microbenchmarks for samkhya-core sketches.
//!
//! Validates the sub-ms inference / sub-MB sketch budgets from samkhya.md §3
//! and guards against regressions on the HLL and Bloom hot paths.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use samkhya_core::sketches::{BloomFilter, HllSketch, Sketch};
use std::hint::black_box;

fn bench_hll_add_1k(c: &mut Criterion) {
    let mut group = c.benchmark_group("hll");
    group.throughput(Throughput::Elements(1_000));
    group.bench_function(BenchmarkId::from_parameter("hll_add_1k"), |b| {
        b.iter_with_setup(
            || HllSketch::new(14).unwrap(),
            |mut hll| {
                for i in 0u32..1_000 {
                    hll.add(&i.to_le_bytes());
                }
                black_box(hll);
            },
        );
    });
    group.finish();
}

fn bench_hll_add_100k(c: &mut Criterion) {
    let mut group = c.benchmark_group("hll");
    group.sample_size(20);
    group.throughput(Throughput::Elements(100_000));
    group.bench_function(BenchmarkId::from_parameter("hll_add_100k"), |b| {
        b.iter_with_setup(
            || HllSketch::new(14).unwrap(),
            |mut hll| {
                for i in 0u32..100_000 {
                    hll.add(&i.to_le_bytes());
                }
                black_box(hll);
            },
        );
    });
    group.finish();
}

fn bench_hll_estimate(c: &mut Criterion) {
    let mut hll = HllSketch::new(14).unwrap();
    for i in 0u32..100_000 {
        hll.add(&i.to_le_bytes());
    }
    c.bench_function("hll_estimate", |b| {
        b.iter(|| black_box(black_box(&hll).estimate()));
    });
}

fn bench_hll_merge(c: &mut Criterion) {
    let mut a_template = HllSketch::new(14).unwrap();
    for i in 0u32..50_000 {
        a_template.add(&i.to_le_bytes());
    }
    let mut b_loaded = HllSketch::new(14).unwrap();
    for i in 50_000u32..100_000 {
        b_loaded.add(&i.to_le_bytes());
    }

    c.bench_function("hll_merge", |bencher| {
        bencher.iter_with_setup(
            || a_template.clone(),
            |mut a| {
                a.merge(black_box(&b_loaded)).unwrap();
                black_box(a);
            },
        );
    });
}

fn bench_bloom_insert_10k(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom");
    group.throughput(Throughput::Elements(10_000));
    group.bench_function(BenchmarkId::from_parameter("bloom_insert_10k"), |b| {
        b.iter_with_setup(
            || BloomFilter::new(100_000, 0.01),
            |mut bf| {
                for i in 0u32..10_000 {
                    bf.insert(&i.to_le_bytes());
                }
                black_box(bf);
            },
        );
    });
    group.finish();
}

fn bench_bloom_contains_hit(c: &mut Criterion) {
    let mut bf = BloomFilter::new(100_000, 0.01);
    for i in 0u32..10_000 {
        bf.insert(&i.to_le_bytes());
    }
    // Item that is definitely present.
    let key = 1234u32.to_le_bytes();
    c.bench_function("bloom_contains_hit", |b| {
        b.iter(|| {
            let present = black_box(&bf).contains(black_box(&key));
            debug_assert!(present);
            black_box(present)
        });
    });
}

fn bench_bloom_contains_miss(c: &mut Criterion) {
    let mut bf = BloomFilter::new(100_000, 0.01);
    for i in 0u32..10_000 {
        bf.insert(&i.to_le_bytes());
    }
    // Item that is not in the filter (well outside the inserted range).
    let key = 9_999_999u32.to_le_bytes();
    c.bench_function("bloom_contains_miss", |b| {
        b.iter(|| {
            let present = black_box(&bf).contains(black_box(&key));
            black_box(present)
        });
    });
}

fn bench_hll_to_bytes(c: &mut Criterion) {
    let mut hll = HllSketch::new(14).unwrap();
    for i in 0u32..100_000 {
        hll.add(&i.to_le_bytes());
    }
    c.bench_function("hll_to_bytes", |b| {
        b.iter(|| {
            let bytes = black_box(&hll).to_bytes().unwrap();
            black_box(bytes)
        });
    });
}

fn bench_hll_from_bytes(c: &mut Criterion) {
    let mut hll = HllSketch::new(14).unwrap();
    for i in 0u32..100_000 {
        hll.add(&i.to_le_bytes());
    }
    let bytes = hll.to_bytes().unwrap();
    c.bench_function("hll_from_bytes", |b| {
        b.iter(|| {
            let decoded = HllSketch::from_bytes(black_box(&bytes)).unwrap();
            black_box(decoded)
        });
    });
}

criterion_group!(
    sketches,
    bench_hll_add_1k,
    bench_hll_add_100k,
    bench_hll_estimate,
    bench_hll_merge,
    bench_bloom_insert_10k,
    bench_bloom_contains_hit,
    bench_bloom_contains_miss,
    bench_hll_to_bytes,
    bench_hll_from_bytes,
);
criterion_main!(sketches);
