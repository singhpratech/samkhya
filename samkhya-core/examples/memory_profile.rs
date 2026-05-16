//! Memory profile harness — measures bincode-serialized byte sizes for
//! every samkhya-core sketch across 5 schema fixtures × 3 scale factors.
//!
//! Emits CSV to stdout. Consumed by `bench-results/11_memory_profile.md`.
//!
//! Schemas:
//!   - logs: 10 cols, 1 high-card, 1 timestamp
//!   - orders: 8 cols, 2 FKs, lognormal skew
//!   - users: 5 cols, low-card categoricals
//!   - events: 15 cols, mostly nullable
//!   - wide: 50 cols, mostly numeric
//!
//! Scale factor multiplies a base row count: 10_000 × {1, 10, 100}.
//!
//! Sizing assumptions:
//!   - bytes/row: rough sum of column physical widths (i64=8, f64=8, str=16
//!     mean, bool=1, ts=8, nullable column adds 1 bit ≈ 0 bytes amortized)
//!   - HLL precision p=12 per column (1.5% target relative error)
//!   - Bloom fp=1%, capacity ≈ distinct values per column
//!   - EquiDepth 64 buckets per numeric column
//!   - CorrelatedHist2D 16×16 cells per FK pair
//!   - CMS depth=5, width=1024 only on declared high-card columns
//!
//! 12 replicates per cell; we report total bytes (deterministic across
//! replicates given fixed precision configs, since sketch byte sizes
//! depend only on configuration — but we still replicate the actual
//! bincode pipeline to measure any framing variance).
//!
//! Run:
//! ```text
//! cargo run --release -p samkhya-core --example memory_profile
//! ```

use samkhya_core::sketches::{
    BloomFilter, CorrelatedHistogram2D, CountMinSketch, EquiDepthHistogram, HllSketch,
};

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    n_cols: usize,
    bytes_per_row: usize,
    high_card_cols: usize,
    numeric_cols: usize,   // gets EquiDepth
    fk_pairs: usize,       // gets CorrelatedHist2D
    distinct_per_col: u64, // typical distinct cardinality per col
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "logs",
        n_cols: 10,
        bytes_per_row: 96, // 1 ts + 1 i64 id + ~8 short strs avg 10b
        high_card_cols: 1,
        numeric_cols: 3,
        fk_pairs: 0,
        distinct_per_col: 10_000,
    },
    Fixture {
        name: "orders",
        n_cols: 8,
        bytes_per_row: 80,
        high_card_cols: 0,
        numeric_cols: 5,
        fk_pairs: 2,
        distinct_per_col: 5_000,
    },
    Fixture {
        name: "users",
        n_cols: 5,
        bytes_per_row: 64,
        high_card_cols: 0,
        numeric_cols: 1,
        fk_pairs: 0,
        distinct_per_col: 200,
    },
    Fixture {
        name: "events",
        n_cols: 15,
        bytes_per_row: 128,
        high_card_cols: 1,
        numeric_cols: 5,
        fk_pairs: 0,
        distinct_per_col: 2_000,
    },
    Fixture {
        name: "wide",
        n_cols: 50,
        bytes_per_row: 408, // 50 numeric cols ≈ 8 bytes each + ~8 overhead
        high_card_cols: 0,
        numeric_cols: 45,
        fk_pairs: 0,
        distinct_per_col: 1_000,
    },
];

const SCALES: &[u64] = &[1, 10, 100];
const BASE_ROWS: u64 = 10_000;
const REPLICATES: usize = 12;

// Per-column sketch configuration
const HLL_PRECISION: u8 = 12; // 4096 registers
const BLOOM_FP: f64 = 0.01;
const HIST_BUCKETS: usize = 64;
const CORR_BINS: usize = 16;

fn synthetic_distinct_values(n: u64, seed: u64) -> Vec<[u8; 8]> {
    let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    (0..n)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z = z ^ (z >> 31);
            z.to_le_bytes()
        })
        .collect()
}

fn build_hll(distinct: u64, seed: u64) -> usize {
    let mut h = HllSketch::new(HLL_PRECISION).unwrap();
    for v in synthetic_distinct_values(distinct.min(20_000), seed) {
        h.add(&v);
    }
    bincode::serialize(&h).unwrap().len()
}

fn build_bloom(distinct: u64, seed: u64) -> usize {
    let cap = distinct.max(1) as usize;
    let mut b = BloomFilter::new(cap, BLOOM_FP);
    for v in synthetic_distinct_values(distinct.min(20_000), seed) {
        b.insert(&v);
    }
    bincode::serialize(&b).unwrap().len()
}

fn build_equidepth(n: u64, seed: u64) -> usize {
    let n = n.min(20_000) as usize;
    let mut state = seed;
    let values: Vec<f64> = (0..n)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            (state as f64) / (u64::MAX as f64) * 1000.0
        })
        .collect();
    let h = EquiDepthHistogram::from_values(&values, HIST_BUCKETS).unwrap();
    bincode::serialize(&h).unwrap().len()
}

fn build_corr2d(n: u64, seed: u64) -> usize {
    let n = n.min(20_000) as usize;
    let mut state = seed;
    let pairs: Vec<(f64, f64)> = (0..n)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let a = (state as f64) / (u64::MAX as f64) * 1000.0;
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let b = (state as f64) / (u64::MAX as f64) * 1000.0;
            (a, b)
        })
        .collect();
    let h = CorrelatedHistogram2D::from_pairs(&pairs, CORR_BINS, CORR_BINS).unwrap();
    bincode::serialize(&h).unwrap().len()
}

fn build_cms(distinct: u64, seed: u64) -> usize {
    let mut c = CountMinSketch::with_defaults();
    for v in synthetic_distinct_values(distinct.min(20_000), seed) {
        c.add(&v, 1);
    }
    bincode::serialize(&c).unwrap().len()
}

/// 95% bootstrap CI on the mean of a small sample (10–20 obs).
fn bootstrap_mean_ci(samples: &[f64], iters: usize, seed: u64) -> (f64, f64, f64) {
    let n = samples.len();
    let mean: f64 = samples.iter().sum::<f64>() / (n as f64);
    let mut state = seed;
    let mut means = Vec::with_capacity(iters);
    for _ in 0..iters {
        let mut acc = 0.0;
        for _ in 0..n {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let idx = (state as usize) % n;
            acc += samples[idx];
        }
        means.push(acc / (n as f64));
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lo = means[(iters as f64 * 0.025) as usize];
    let hi = means[(iters as f64 * 0.975) as usize];
    (mean, lo, hi)
}

fn main() {
    println!(
        "fixture,scale,rows,bytes_per_row,raw_bytes,hll_total,bloom_total,equidepth_total,\
         cms_total,corr2d_total,samkhya_total_mean,samkhya_total_lo,samkhya_total_hi,pct_of_raw"
    );

    for fx in FIXTURES {
        for &sf in SCALES {
            let rows = BASE_ROWS * sf;
            let raw_bytes = rows * (fx.bytes_per_row as u64);
            // distinct count typically grows sublinearly; cap at distinct_per_col * sqrt(scale)
            let distinct = ((fx.distinct_per_col as f64) * (sf as f64).sqrt()) as u64;

            let mut totals: Vec<f64> = Vec::with_capacity(REPLICATES);
            let mut hll_total = 0;
            let mut bloom_total = 0;
            let mut equidepth_total = 0;
            let mut cms_total = 0;
            let mut corr2d_total = 0;

            for rep in 0..REPLICATES {
                let seed = 0xA5A5_5A5A_DEAD_BEEFu64.wrapping_add(rep as u64);
                let mut total = 0usize;

                // HLL on every column
                let mut h = 0;
                for c in 0..fx.n_cols {
                    h += build_hll(distinct, seed.wrapping_add(c as u64));
                }
                total += h;

                // Bloom on every column
                let mut b = 0;
                for c in 0..fx.n_cols {
                    b += build_bloom(distinct, seed.wrapping_add(0x1000 + c as u64));
                }
                total += b;

                // EquiDepth on numeric columns
                let mut e = 0;
                for c in 0..fx.numeric_cols {
                    e += build_equidepth(rows.min(20_000), seed.wrapping_add(0x2000 + c as u64));
                }
                total += e;

                // CMS only on declared high-card columns
                let mut m = 0;
                for c in 0..fx.high_card_cols {
                    m += build_cms(distinct, seed.wrapping_add(0x3000 + c as u64));
                }
                total += m;

                // CorrelatedHist2D on FK pairs
                let mut cr = 0;
                for c in 0..fx.fk_pairs {
                    cr += build_corr2d(rows.min(20_000), seed.wrapping_add(0x4000 + c as u64));
                }
                total += cr;

                totals.push(total as f64);
                if rep == 0 {
                    hll_total = h;
                    bloom_total = b;
                    equidepth_total = e;
                    cms_total = m;
                    corr2d_total = cr;
                }
            }

            let (mean, lo, hi) = bootstrap_mean_ci(&totals, 2000, 0xCAFE);
            let pct = mean / (raw_bytes as f64) * 100.0;

            println!(
                "{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{:.1},{:.6}",
                fx.name,
                sf,
                rows,
                fx.bytes_per_row,
                raw_bytes,
                hll_total,
                bloom_total,
                equidepth_total,
                cms_total,
                corr2d_total,
                mean,
                lo,
                hi,
                pct
            );
        }
    }
}
