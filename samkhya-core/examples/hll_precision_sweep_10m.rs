//! HLL precision sweep at the **10M-row anchor scale** required by the
//! file-01 (`01_cpu_baseline_multithread.md`) anchor-cell pre-registration.
//!
//! WAVE5G observation: the existing `hll_precision_sweep.rs` already runs at
//! 10^7 across p ∈ {8, 10, 12, 14, 16}, but its raw-JSON sidecar mingles the
//! 10M cells with the 10K / 100K / 1M / 1e7 cells of the precision matrix and
//! the file-01 markdown specifies the anchor as a *single* high-precision
//! (p=14) cell at n=10^7 — the same precision the single-thread `t=1` anchor
//! row in §4.1 of file 01 uses.
//!
//! This example emits a **dedicated** per-trial RSE vector for the
//! (p=14, n=10^7) anchor cell to `bench-results/01_hll_precision_raw.json`
//! (path overrideable via `SAMKHYA_RAW_OUT`), which the BCa post-processor
//! at `bench-results/scripts/bootstrap_ci.py --method bca` ingests. The
//! sidecar schema is intentionally a JSON array of f64 values so the script
//! can be called as `bootstrap_ci.py --input <path> --statistic mean
//! --method bca --n-resamples 10000 --seed 42`.
//!
//! Methodology (per WAVE5G + EMP01):
//! - 30 trials per cell, distinct SplitMix64 seed per (p, n, t).
//! - First seed tried — no seed search; bootstrap RNG is independent.
//! - Reports both the per-trial signed relative error AND the per-trial
//!   *relative standard error* magnitude RSE_i = |est_i - n| / n
//!   (Flajolet 2007's standard accuracy metric — `1.04 / sqrt(2^p)` for the
//!   classical HLL bound).
//!
//! Citations:
//! - Flajolet, P., Fusy, É., Gandouet, O., Meunier, F. (2007).
//!   "HyperLogLog: the Analysis of a Near-Optimal Cardinality Estimation
//!   Algorithm." *AofA 2007*.
//! - Efron, B. & Tibshirani, R. J. (1993). *An Introduction to the
//!   Bootstrap*. Chapter 14 (BCa).
//!
//! Run:
//! ```text
//! cargo run --release -p samkhya-core --example hll_precision_sweep_10m
//! ```
//!
//! With sidecar persistence:
//! ```text
//! SAMKHYA_RAW_OUT=bench-results/01_hll_precision_raw.json \
//!     cargo run --release -p samkhya-core --example hll_precision_sweep_10m
//! ```

use std::env;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

use samkhya_core::sketches::HllSketch;

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

/// Run one trial: stream `n` PRNG-generated u64 values into a fresh HLL
/// with the given precision. Returns (signed_relerr, abs_relerr).
fn run_trial(p: u8, n: u64, seed: u64) -> (f64, f64) {
    let mut hll = HllSketch::new(p).expect("precision in range");
    let mut rng = SplitMix64::new(seed);
    for _ in 0..n {
        let v = rng.next_u64();
        hll.add(&v.to_le_bytes());
    }
    let est = hll.estimate() as f64;
    let signed = (est - n as f64) / n as f64;
    (signed, signed.abs())
}

fn theoretical_bound(p: u8) -> f64 {
    let m = (1u64 << p) as f64;
    1.04 / m.sqrt()
}

fn main() {
    // Pre-registered anchor cell from file 01.
    const P_ANCHOR: u8 = 14;
    const N_ANCHOR: u64 = 10_000_000;
    const TRIALS: usize = 30;

    let raw_path = env::var("SAMKHYA_RAW_OUT")
        .unwrap_or_else(|_| "bench-results/01_hll_precision_raw.json".to_string());

    let mut signed_errs = Vec::with_capacity(TRIALS);
    let mut abs_errs = Vec::with_capacity(TRIALS);

    println!("# HLL 10M anchor sweep — per-trial RSE");
    println!("p,n,trial,seed,signed_relerr,abs_relerr");

    let bound = theoretical_bound(P_ANCHOR);
    let start = Instant::now();
    for t in 0..TRIALS {
        // Deterministic seed schedule (matches WAVE5G hll_precision_sweep.rs
        // convention so cross-checking is possible).
        let seed = 0xC0FF_EE00_0000_0000
            ^ ((P_ANCHOR as u64) << 48)
            ^ ((N_ANCHOR.trailing_zeros() as u64) << 32)
            ^ (t as u64);
        let (signed, abs) = run_trial(P_ANCHOR, N_ANCHOR, seed);
        signed_errs.push(signed);
        abs_errs.push(abs);
        println!(
            "{},{},{},{},{:.10},{:.10}",
            P_ANCHOR, N_ANCHOR, t, seed, signed, abs
        );
    }
    let elapsed = start.elapsed();

    let mean_abs = abs_errs.iter().sum::<f64>() / TRIALS as f64;
    let mean_signed = signed_errs.iter().sum::<f64>() / TRIALS as f64;
    let max_abs = abs_errs.iter().cloned().fold(0.0_f64, f64::max);
    let min_abs = abs_errs.iter().cloned().fold(f64::INFINITY, f64::min);
    let within: usize = abs_errs.iter().filter(|&&e| e <= bound).count();

    println!();
    println!("# Summary (p={P_ANCHOR}, n={N_ANCHOR}, trials={TRIALS})");
    println!("# theoretical bound (Flajolet 2007): {bound:.6}");
    println!("# mean abs RSE   : {mean_abs:.6}");
    println!("# mean signed err: {mean_signed:+.6}");
    println!("# min  abs RSE   : {min_abs:.6}");
    println!("# max  abs RSE   : {max_abs:.6}");
    println!(
        "# frac within bd : {}/{} = {:.3}",
        within,
        TRIALS,
        within as f64 / TRIALS as f64
    );
    println!("# wall time      : {:.2}s", elapsed.as_secs_f64());

    // Emit per-trial JSON sidecar as a plain f64 array (matches the
    // stdin/--input contract of bench-results/scripts/bootstrap_ci.py).
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
    let body = format!(
        "{{\"benchmark\":\"hll_precision_sweep_10m\",\"seed_scheme\":\"0xC0FFEE00_0000_0000 ^ (p<<48) ^ (log2n<<32) ^ trial\",\"p\":{P_ANCHOR},\"n\":{N_ANCHOR},\"trials\":{TRIALS},\"bound_flajolet_2007\":{bound:.10},\"abs_relerr\":[{abs_vec}],\"signed_relerr\":[{signed_vec}]}}"
    );
    let mut f = File::create(&raw_path).expect("create raw output file");
    f.write_all(body.as_bytes()).expect("write raw output");
    eprintln!("# raw per-trial RSE vectors written to {raw_path}");
}
