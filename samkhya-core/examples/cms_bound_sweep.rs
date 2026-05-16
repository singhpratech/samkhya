//! Count-Min-Sketch theoretical bound verification.
//!
//! Sweeps (epsilon, delta) parameters x stream length N, draws keys from a
//! Zipfian(s=1.1, vocab=10_000) distribution, builds a CMS sized per the
//! classical guarantee
//!
//!     width  >= ceil(e / epsilon)
//!     depth  >= ceil(ln(1/delta))
//!
//! and reports the empirical fraction of queries whose estimate exceeds
//! `true + epsilon * N`. The classical bound says that fraction should be
//! <= delta (per query).
//!
//! Pre-registered hypothesis: empirical bound-exceedance <= delta * 1.2
//! across all cells, averaged over >= 30 trials per cell.
//!
//! Run with:
//! ```text
//! cargo run -p samkhya-core --release --example cms_bound_sweep
//! ```
//!
//! No external RNG / distribution crates: the example carries its own
//! splitmix64 PRNG and Zipfian inverse-CDF sampler so the benchmark stays
//! reproducible across toolchains.

use std::time::Instant;

use samkhya_core::sketches::CountMinSketch;

// --- splitmix64 PRNG ------------------------------------------------------

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

    fn next_f64(&mut self) -> f64 {
        // 53-bit mantissa, uniform in [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// --- Zipfian inverse-CDF sampler -----------------------------------------

struct Zipfian {
    cdf: Vec<f64>, // cdf[i] = P(X <= i+1)
}

impl Zipfian {
    fn new(vocab: usize, s: f64) -> Self {
        let weights: Vec<f64> = (1..=vocab).map(|k| 1.0 / (k as f64).powf(s)).collect();
        let z: f64 = weights.iter().sum();
        let mut cdf = Vec::with_capacity(vocab);
        let mut acc = 0.0;
        for w in &weights {
            acc += w / z;
            cdf.push(acc);
        }
        // Numerical safety: clamp final entry to exactly 1.0.
        if let Some(last) = cdf.last_mut() {
            *last = 1.0;
        }
        Self { cdf }
    }

    /// Sample a 1-indexed rank in 1..=vocab via binary search on the CDF.
    fn sample(&self, rng: &mut SplitMix64) -> u32 {
        let u = rng.next_f64();
        // partition_point returns first index where cdf[i] > u (after
        // wrapping with `<= u`-style predicate).
        let idx = self.cdf.partition_point(|&c| c <= u);
        (idx as u32) + 1
    }
}

// --- CMS sizing -----------------------------------------------------------

/// Classical CMS sizing: width = ceil(e / epsilon), depth = ceil(ln(1/delta)).
fn size_cms(epsilon: f64, delta: f64) -> (u32, u32) {
    let width = (std::f64::consts::E / epsilon).ceil() as u32;
    let depth = (1.0_f64 / delta).ln().ceil() as u32;
    (depth.max(1), width.max(1))
}

// --- Per-trial measurement ------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct TrialStats {
    max_over: u32,
    p95_over: u32,
    frac_exceeding: f64,
    #[allow(dead_code)]
    keys_probed: usize,
}

fn run_trial(epsilon: f64, delta: f64, n: u64, vocab: usize, zipf_s: f64, seed: u64) -> TrialStats {
    let (depth, width) = size_cms(epsilon, delta);
    let mut cms = CountMinSketch::new(depth, width).expect("valid CMS dims");

    let mut rng = SplitMix64::new(seed);
    let zipf = Zipfian::new(vocab, zipf_s);

    let mut truth = vec![0u32; vocab];

    for _ in 0..n {
        let k = zipf.sample(&mut rng);
        truth[(k - 1) as usize] = truth[(k - 1) as usize].saturating_add(1);
        cms.add(&k.to_le_bytes(), 1);
    }

    let bound = (epsilon * n as f64).ceil() as u32;
    let mut overs: Vec<u32> = Vec::with_capacity(vocab);
    let mut exceed = 0u64;
    let mut probed = 0u64;
    for (i, &t) in truth.iter().enumerate() {
        if t == 0 {
            // Keys that never appeared inflate the per-query count but the
            // bound still applies; include them so we count "all distinct
            // queries posed against the structure".
        }
        let key = ((i as u32) + 1).to_le_bytes();
        let est = cms.estimate(&key);
        let over = est.saturating_sub(t);
        overs.push(over);
        if over > bound {
            exceed += 1;
        }
        probed += 1;
    }
    overs.sort_unstable();
    let p95_idx = ((overs.len() as f64) * 0.95).floor() as usize;
    let p95 = *overs.get(p95_idx.min(overs.len() - 1)).unwrap_or(&0);
    let max = *overs.last().unwrap_or(&0);

    TrialStats {
        max_over: max,
        p95_over: p95,
        frac_exceeding: exceed as f64 / probed as f64,
        keys_probed: probed as usize,
    }
}

// --- Bootstrap CI ---------------------------------------------------------

fn bootstrap_ci_mean(samples: &[f64], iters: usize, seed: u64) -> (f64, f64, f64) {
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let mut rng = SplitMix64::new(seed);
    let mut means = Vec::with_capacity(iters);
    let n = samples.len();
    for _ in 0..iters {
        let mut s = 0.0;
        for _ in 0..n {
            let idx = (rng.next_u64() as usize) % n;
            s += samples[idx];
        }
        means.push(s / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lo = means[(iters as f64 * 0.025) as usize];
    let hi = means[((iters as f64 * 0.975) as usize).min(iters - 1)];
    (mean, lo, hi)
}

// --- Main sweep -----------------------------------------------------------

fn main() {
    let cells: &[(f64, f64)] = &[(0.01, 0.01), (0.001, 0.01), (0.0001, 0.001)];
    let ns: &[u64] = &[100_000, 1_000_000, 10_000_000];
    let vocab = 10_000usize;
    let zipf_s = 1.1f64;
    let trials = 30usize;

    println!("# CMS bound verification sweep");
    println!("# Zipfian(s={zipf_s}, vocab={vocab}), trials={trials}");
    println!(
        "# columns: epsilon, delta, N, depth, width, mem_bytes, \
         mean_max_over, mean_p95_over, mean_frac_exceed, ci95_lo, ci95_hi, \
         delta_x_1.2, status, wall_s"
    );

    for &(eps, del) in cells {
        let (depth, width) = size_cms(eps, del);
        let mem = 4u64 * depth as u64 * width as u64;
        for &n in ns {
            let t0 = Instant::now();
            let mut maxes = Vec::with_capacity(trials);
            let mut p95s = Vec::with_capacity(trials);
            let mut fracs = Vec::with_capacity(trials);
            for t in 0..trials {
                let seed = 0xC315_5EED
                    ^ (eps.to_bits() ^ del.to_bits())
                    ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    ^ (t as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                let s = run_trial(eps, del, n, vocab, zipf_s, seed);
                maxes.push(s.max_over as f64);
                p95s.push(s.p95_over as f64);
                fracs.push(s.frac_exceeding);
            }
            let (max_mean, max_lo, max_hi) = bootstrap_ci_mean(&maxes, 1000, 0xA1A1);
            let (p95_mean, _p95_lo, _p95_hi) = bootstrap_ci_mean(&p95s, 1000, 0xB2B2);
            let (frac_mean, frac_lo, frac_hi) = bootstrap_ci_mean(&fracs, 1000, 0xC3C3);
            let bound = del * 1.2;
            let status = if frac_hi <= bound {
                "PASS"
            } else if frac_mean <= bound {
                "MARGIN"
            } else {
                "FAIL"
            };
            let wall = t0.elapsed().as_secs_f64();
            println!(
                "{eps:.5}, {del:.5}, {n:>10}, {depth:>3}, {width:>7}, {mem:>10}, \
                 {max_mean:>10.2} [{max_lo:.2},{max_hi:.2}], \
                 {p95_mean:>8.2}, \
                 {frac_mean:.6} [{frac_lo:.6},{frac_hi:.6}], \
                 {bound:.6}, {status}, {wall:.2}s"
            );
        }
    }
}
