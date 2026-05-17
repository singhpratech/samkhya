//! HLL precision sweep: empirical relative error vs theoretical 1.04/sqrt(2^p)
//! bound across p ∈ {8, 10, 12, 14, 16} and n ∈ {1e3, 1e4, 1e5, 1e6, 1e7}.
//!
//! - Deterministic SplitMix64 PRNG (no external deps).
//! - 30 trials per (p, n) cell; each trial uses a distinct seed; we stream
//!   `n` u64 values into a fresh `HllSketch` and record the relative error.
//! - Reports mean error, 95% bootstrap CI on the mean, max error, and the
//!   fraction of trials within the theoretical 1.04/sqrt(2^p) bound.
//! - Emits CSV to stdout for table assembly.
//!
//! Run:
//! ```text
//! cargo run --release -p samkhya-core --example hll_precision_sweep
//! ```

use std::env;
use std::fs::File;
use std::io::Write;

use samkhya_core::sketches::HllSketch;

/// Pure-Rust SplitMix64 — deterministic, dependency-free, well-distributed
/// (good enough for "synthetic distinct u64 stream"; not crypto).
#[derive(Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// One trial: stream `n` PRNG-generated u64 values into a fresh HLL with
/// the given precision. Returns the *signed* relative error.
fn run_trial(p: u8, n: u64, seed: u64) -> f64 {
    let mut hll = HllSketch::new(p).expect("precision in range");
    let mut rng = SplitMix64::new(seed);
    for _ in 0..n {
        let v = rng.next_u64();
        hll.add(&v.to_le_bytes());
    }
    let est = hll.estimate() as f64;
    (est - n as f64) / n as f64
}

/// 95% bootstrap CI on the mean using percentile method with 2000 resamples.
fn bootstrap_ci_mean(samples: &[f64], n_boot: usize, seed: u64) -> (f64, f64) {
    let n = samples.len();
    let mut means = Vec::with_capacity(n_boot);
    let mut rng = SplitMix64::new(seed);
    for _ in 0..n_boot {
        let mut sum = 0.0;
        for _ in 0..n {
            let idx = (rng.next_u64() as usize) % n;
            sum += samples[idx];
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lo = means[(0.025 * n_boot as f64) as usize];
    let hi = means[(0.975 * n_boot as f64) as usize];
    (lo, hi)
}

fn theoretical_bound(p: u8) -> f64 {
    let m = (1u64 << p) as f64;
    1.04 / m.sqrt()
}

fn main() {
    let precisions: [u8; 5] = [8, 10, 12, 14, 16];
    let cardinalities: [u64; 5] = [1_000, 10_000, 100_000, 1_000_000, 10_000_000];
    let trials: usize = 30;

    // Per-trial persistence path via env var; deterministic schema.
    let raw_path = env::var("SAMKHYA_RAW_OUT").ok();
    let mut raw_cells: Vec<String> = Vec::new();

    println!("# HLL precision sweep — raw CSV");
    println!(
        "p,n,trials,mean_abs_relerr,mean_signed_relerr,ci95_lo_abs,ci95_hi_abs,max_abs_relerr,frac_within_bound,theoretical_bound"
    );

    for &p in &precisions {
        let bound = theoretical_bound(p);
        for &n in &cardinalities {
            // Skip the p=8, n=1e7 corner? No — keep it; we want to *see*
            // the high-cardinality saturation behavior at low precision.
            let mut abs_errs = Vec::with_capacity(trials);
            let mut signed_errs = Vec::with_capacity(trials);
            let mut within = 0usize;
            for t in 0..trials {
                // Distinct seed per (p, n, t): mix all three into 64 bits.
                let seed = 0xC0FF_EE00_0000_0000
                    ^ ((p as u64) << 48)
                    ^ ((n.trailing_zeros() as u64) << 32)
                    ^ (t as u64);
                let rel = run_trial(p, n, seed);
                let abs = rel.abs();
                if abs <= bound {
                    within += 1;
                }
                abs_errs.push(abs);
                signed_errs.push(rel);
            }
            let mean_abs = abs_errs.iter().sum::<f64>() / trials as f64;
            let mean_signed = signed_errs.iter().sum::<f64>() / trials as f64;
            let (lo, hi) = bootstrap_ci_mean(&abs_errs, 2000, 0xBEEF_0000 ^ p as u64 ^ n);
            let max_abs = abs_errs.iter().cloned().fold(0.0_f64, f64::max);
            let frac = within as f64 / trials as f64;
            println!(
                "{p},{n},{trials},{mean_abs:.6},{mean_signed:.6},{lo:.6},{hi:.6},{max_abs:.6},{frac:.3},{bound:.6}"
            );

            if raw_path.is_some() {
                let abs_vec = abs_errs
                    .iter()
                    .map(|v| format!("{v:.10}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let signed_vec = signed_errs
                    .iter()
                    .map(|v| format!("{v:.10}"))
                    .collect::<Vec<_>>()
                    .join(",");
                raw_cells.push(format!(
                    "{{\"p\":{p},\"n\":{n},\"trials\":{trials},\"bound\":{bound:.10},\"abs_errs\":[{abs_vec}],\"signed_errs\":[{signed_vec}]}}"
                ));
            }
        }
    }

    if let Some(path) = raw_path {
        let body = format!(
            "{{\"benchmark\":\"hll_precision_sweep\",\"seed_scheme\":\"0xC0FFEE00_0000_0000 ^ (p<<48) ^ (log2n<<32) ^ trial\",\"cells\":[{}]}}",
            raw_cells.join(",")
        );
        let mut f = File::create(&path).expect("create raw output file");
        f.write_all(body.as_bytes()).expect("write raw output");
        eprintln!("# raw per-trial vectors written to {path}");
    }
}
