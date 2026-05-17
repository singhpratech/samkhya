//! Histogram accuracy harness for bench-results/06_histogram_accuracy.md.
//!
//! Sweeps `EquiDepthHistogram` and `CorrelatedHistogram2D` across
//!   - distributions: uniform, gaussian, zipfian, lognormal, bimodal
//!   - row counts:     10^4, 10^5, 10^6
//!   - bucket counts:  32, 64, 128, 256
//!   - 2D correlations: rho ∈ {0.0, 0.3, 0.7, 0.95}
//!
//! For each configuration we issue 1000 random range queries, compute
//! relative error vs. ground truth, and aggregate P50/P95/P99/max.
//! 30 trials per cell with distinct seeds; 95% bootstrap CI on P95.
//!
//! Run with:
//!   cargo run -p samkhya-core --release --example histogram_accuracy
//!
//! Output is stdout-only; the markdown in bench-results/ summarises a
//! reference run on the v0.4.0 release hardware.

use std::env;
use std::fs::File;
use std::io::Write;

use samkhya_core::sketches::{CorrelatedHistogram2D, EquiDepthHistogram};

// ---------------------------------------------------------------------------
// Deterministic PRNG — xoshiro256** (Blackman & Vigna, 2018). Pure-Rust
// and seedable so every cell of the sweep is reproducible without pulling
// in an external `rand` dependency.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Rng {
    s: [u64; 4],
}

impl Rng {
    fn seeded(seed: u64) -> Self {
        // splitmix64 to spread one u64 into four state words
        let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
        let mut next = || {
            z = z.wrapping_add(0x9E3779B97F4A7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
            x ^ (x >> 31)
        };
        Self {
            s: [next(), next(), next(), next()],
        }
    }

    fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform `[0, 1)` double.
    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }

    /// Standard normal via Box–Muller.
    fn normal(&mut self) -> f64 {
        let mut u1 = self.uniform();
        if u1 < 1e-300 {
            u1 = 1e-300;
        }
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

// ---------------------------------------------------------------------------
// Synthetic populations
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Dist {
    Uniform,
    Gaussian,
    Zipfian,
    Lognormal,
    Bimodal,
}

impl Dist {
    fn name(self) -> &'static str {
        match self {
            Dist::Uniform => "uniform",
            Dist::Gaussian => "gaussian",
            Dist::Zipfian => "zipfian",
            Dist::Lognormal => "lognormal",
            Dist::Bimodal => "bimodal",
        }
    }

    fn sample(self, rng: &mut Rng) -> f64 {
        match self {
            Dist::Uniform => rng.uniform(),
            Dist::Gaussian => rng.normal(),
            Dist::Zipfian => {
                // Discrete zipfian over 1..=1000 with skew s=1.07 via
                // inverse CDF on the harmonic. Approximation that produces
                // a heavy-tailed integer-valued column for histogramming.
                let u = rng.uniform().max(1e-12);
                let s = 1.07f64;
                // Continuous Zipf inverse: x = (1 - u)^(-1/(s-1))
                let x = (1.0 - u).powf(-1.0 / (s - 1.0));
                x.min(1.0e6)
            }
            Dist::Lognormal => (rng.normal() * 0.7).exp(),
            Dist::Bimodal => {
                if rng.uniform() < 0.5 {
                    rng.normal() * 0.3 - 2.0
                } else {
                    rng.normal() * 0.3 + 2.0
                }
            }
        }
    }
}

fn generate(d: Dist, n: usize, rng: &mut Rng) -> Vec<f64> {
    (0..n).map(|_| d.sample(rng)).collect()
}

// ---------------------------------------------------------------------------
// Statistics helpers
// ---------------------------------------------------------------------------

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// 95% bootstrap CI on the P95 of `samples` using `b` resamples.
fn bootstrap_p95_ci(samples: &[f64], rng: &mut Rng, b: usize) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let n = samples.len();
    let mut p95s: Vec<f64> = Vec::with_capacity(b);
    for _ in 0..b {
        let mut resample: Vec<f64> = (0..n)
            .map(|_| samples[(rng.next_u64() as usize) % n])
            .collect();
        resample.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        p95s.push(percentile(&resample, 0.95));
    }
    p95s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (percentile(&p95s, 0.025), percentile(&p95s, 0.975))
}

// ---------------------------------------------------------------------------
// Ground truth + query workload
// ---------------------------------------------------------------------------

fn true_range_count(values: &[f64], lo: f64, hi: f64) -> u64 {
    values.iter().filter(|&&v| v >= lo && v <= hi).count() as u64
}

fn random_range(rng: &mut Rng, vmin: f64, vmax: f64) -> (f64, f64) {
    let a = vmin + rng.uniform() * (vmax - vmin);
    let b = vmin + rng.uniform() * (vmax - vmin);
    if a <= b { (a, b) } else { (b, a) }
}

fn rel_err(est: u64, truth: u64) -> f64 {
    let t = truth.max(1) as f64;
    (est as i64 - truth as i64).abs() as f64 / t
}

// ---------------------------------------------------------------------------
// 1D sweep
// ---------------------------------------------------------------------------

struct CellSummary {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    p95_ci_lo: f64,
    p95_ci_hi: f64,
    p95_per_trial: Vec<f64>,
}

fn run_equidepth_cell(
    dist: Dist,
    n: usize,
    buckets: usize,
    trials: usize,
    queries: usize,
) -> CellSummary {
    let mut p95_per_trial: Vec<f64> = Vec::with_capacity(trials);
    let mut all_errs: Vec<f64> = Vec::with_capacity(trials * queries);

    for trial in 0..trials {
        let seed =
            (dist as u64) * 1_000_003 + (n as u64) * 97 + (buckets as u64) * 31 + trial as u64;
        let mut rng = Rng::seeded(seed);
        let pop = generate(dist, n, &mut rng);
        let (vmin, vmax) = pop
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        let hist = EquiDepthHistogram::from_values(&pop, buckets).expect("buckets > 0");

        let mut errs: Vec<f64> = Vec::with_capacity(queries);
        for _ in 0..queries {
            let (lo, hi) = random_range(&mut rng, vmin, vmax);
            let est = hist.estimate_range(lo, hi);
            let truth = true_range_count(&pop, lo, hi);
            errs.push(rel_err(est, truth));
        }
        errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        p95_per_trial.push(percentile(&errs, 0.95));
        all_errs.extend_from_slice(&errs);
    }

    all_errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut ci_rng = Rng::seeded(0xDEADBEEF_u64.wrapping_add(buckets as u64));
    let (lo, hi) = bootstrap_p95_ci(&p95_per_trial, &mut ci_rng, 1000);

    CellSummary {
        p50: percentile(&all_errs, 0.50),
        p95: percentile(&all_errs, 0.95),
        p99: percentile(&all_errs, 0.99),
        max: *all_errs.last().unwrap_or(&0.0),
        p95_ci_lo: lo,
        p95_ci_hi: hi,
        p95_per_trial,
    }
}

// ---------------------------------------------------------------------------
// 2D sweep
// ---------------------------------------------------------------------------

fn correlated_pairs(rho: f64, n: usize, rng: &mut Rng) -> Vec<(f64, f64)> {
    // Two standard normals coupled via Cholesky:
    //   a = z1, b = rho*z1 + sqrt(1-rho^2)*z2
    let k = (1.0 - rho * rho).max(0.0).sqrt();
    (0..n)
        .map(|_| {
            let z1 = rng.normal();
            let z2 = rng.normal();
            (z1, rho * z1 + k * z2)
        })
        .collect()
}

struct TwoDSummary {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    p95_ci_lo: f64,
    p95_ci_hi: f64,
    indep_p95: f64,
    p95_per_trial: Vec<f64>,
}

fn run_correlated_cell(
    rho: f64,
    n: usize,
    bins: usize,
    trials: usize,
    queries: usize,
) -> TwoDSummary {
    let mut p95_per_trial: Vec<f64> = Vec::with_capacity(trials);
    let mut all_errs: Vec<f64> = Vec::with_capacity(trials * queries);
    let mut indep_errs: Vec<f64> = Vec::with_capacity(trials * queries);

    for trial in 0..trials {
        let seed = ((rho * 1000.0) as u64) * 1_000_003
            + (n as u64) * 97
            + (bins as u64) * 31
            + trial as u64;
        let mut rng = Rng::seeded(seed);
        let pairs = correlated_pairs(rho, n, &mut rng);
        let xs: Vec<f64> = pairs.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = pairs.iter().map(|p| p.1).collect();
        let (xmin, xmax) = xs
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        let (ymin, ymax) = ys
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });

        let h2d = CorrelatedHistogram2D::from_pairs(&pairs, bins, bins).expect("bins > 0");
        let hx = EquiDepthHistogram::from_values(&xs, bins).expect("bins > 0");
        let hy = EquiDepthHistogram::from_values(&ys, bins).expect("bins > 0");
        let total = pairs.len() as f64;

        let mut errs: Vec<f64> = Vec::with_capacity(queries);
        for _ in 0..queries {
            let (ax, bx) = random_range(&mut rng, xmin, xmax);
            let (ay, by) = random_range(&mut rng, ymin, ymax);
            let truth = pairs
                .iter()
                .filter(|&&(x, y)| x >= ax && x <= bx && y >= ay && y <= by)
                .count() as u64;

            let est2d = h2d.estimate_range(ax, bx, ay, by);
            errs.push(rel_err(est2d, truth));

            // 1D-product (independence) baseline:
            //   est = total * P(A) * P(B)
            let nx = hx.estimate_range(ax, bx) as f64;
            let ny = hy.estimate_range(ay, by) as f64;
            let est_indep = if total > 0.0 {
                (nx * ny / total).round() as u64
            } else {
                0
            };
            indep_errs.push(rel_err(est_indep, truth));
        }
        errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        p95_per_trial.push(percentile(&errs, 0.95));
        all_errs.extend_from_slice(&errs);
    }

    all_errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    indep_errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut ci_rng = Rng::seeded(0xC0FFEE_u64.wrapping_add(bins as u64));
    let (lo, hi) = bootstrap_p95_ci(&p95_per_trial, &mut ci_rng, 1000);

    TwoDSummary {
        p50: percentile(&all_errs, 0.50),
        p95: percentile(&all_errs, 0.95),
        p99: percentile(&all_errs, 0.99),
        max: *all_errs.last().unwrap_or(&0.0),
        p95_ci_lo: lo,
        p95_ci_hi: hi,
        indep_p95: percentile(&indep_errs, 0.95),
        p95_per_trial,
    }
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

fn main() {
    let trials = 30;
    let queries = 1000;
    let dists = [
        Dist::Uniform,
        Dist::Gaussian,
        Dist::Zipfian,
        Dist::Lognormal,
        Dist::Bimodal,
    ];
    let ns = [10_000usize, 100_000, 1_000_000];
    let buckets_set = [32usize, 64, 128, 256];

    let raw_path = env::var("SAMKHYA_RAW_OUT").ok();
    let mut raw_1d: Vec<String> = Vec::new();
    let mut raw_2d: Vec<String> = Vec::new();

    println!("# EquiDepthHistogram 1D");
    println!("dist,n,buckets,p50,p95,p99,max,p95_ci_lo,p95_ci_hi");
    for &dist in &dists {
        for &n in &ns {
            for &b in &buckets_set {
                let s = run_equidepth_cell(dist, n, b, trials, queries);
                println!(
                    "{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}",
                    dist.name(),
                    n,
                    b,
                    s.p50,
                    s.p95,
                    s.p99,
                    s.max,
                    s.p95_ci_lo,
                    s.p95_ci_hi,
                );
                if raw_path.is_some() {
                    let p95_vec = s
                        .p95_per_trial
                        .iter()
                        .map(|v| format!("{v:.10}"))
                        .collect::<Vec<_>>()
                        .join(",");
                    raw_1d.push(format!(
                        "{{\"dist\":\"{}\",\"n\":{n},\"buckets\":{b},\"trials\":{trials},\"queries_per_trial\":{queries},\"p95_per_trial\":[{p95_vec}]}}",
                        dist.name()
                    ));
                }
            }
        }
    }

    println!();
    println!("# CorrelatedHistogram2D");
    println!("rho,n,bins,p50,p95,p99,max,p95_ci_lo,p95_ci_hi,indep_p95");
    let rhos = [0.0f64, 0.3, 0.7, 0.95];
    let ns2 = [10_000usize, 100_000];
    let bins_set = [16usize, 32, 64];
    for &rho in &rhos {
        for &n in &ns2 {
            for &b in &bins_set {
                let s = run_correlated_cell(rho, n, b, trials, queries);
                println!(
                    "{:.2},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}",
                    rho, n, b, s.p50, s.p95, s.p99, s.max, s.p95_ci_lo, s.p95_ci_hi, s.indep_p95,
                );
                if raw_path.is_some() {
                    let p95_vec = s
                        .p95_per_trial
                        .iter()
                        .map(|v| format!("{v:.10}"))
                        .collect::<Vec<_>>()
                        .join(",");
                    raw_2d.push(format!(
                        "{{\"rho\":{rho},\"n\":{n},\"bins\":{b},\"trials\":{trials},\"queries_per_trial\":{queries},\"p95_per_trial\":[{p95_vec}]}}"
                    ));
                }
            }
        }
    }

    if let Some(path) = raw_path {
        let body = format!(
            "{{\"benchmark\":\"histogram_accuracy\",\"seed_scheme\":\"f(dist,n,buckets,trial) splitmix64\",\"equidepth_1d\":[{}],\"correlated_2d\":[{}]}}",
            raw_1d.join(","),
            raw_2d.join(",")
        );
        let mut f = File::create(&path).expect("create raw output file");
        f.write_all(body.as_bytes()).expect("write raw output");
        eprintln!("# raw per-trial vectors written to {path}");
    }
}
