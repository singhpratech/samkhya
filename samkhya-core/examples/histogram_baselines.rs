#![allow(clippy::needless_range_loop)]
//! Canonical histogram baselines for bench-results/06_histogram_accuracy.md.
//!
//! Adds MaxDiff(V,A) (Poosala SIGMOD 1996, "Improved Histograms for
//! Selectivity Estimation of Range Predicates") and V-Optimal (Jagadish
//! VLDB 1998, "Optimal Histograms with Quality Guarantees") alongside the
//! equi-depth numbers already in `histogram_accuracy.rs`.
//!
//! Sweep: dist x n x B=128, 30 trials, 1000 queries/trial. The xoshiro256**
//! seed function is bit-identical to histogram_accuracy.rs so populations
//! line up cell-for-cell.
//!
//! Run: cargo run -p samkhya-core --release --example histogram_baselines

#[derive(Clone)]
struct Rng {
    s: [u64; 4],
}

impl Rng {
    fn seeded(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
        let mut nxt = || {
            z = z.wrapping_add(0x9E3779B97F4A7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
            x ^ (x >> 31)
        };
        Self {
            s: [nxt(), nxt(), nxt(), nxt()],
        }
    }
    fn next_u64(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }
    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
    fn normal(&mut self) -> f64 {
        let mut u1 = self.uniform();
        if u1 < 1e-300 {
            u1 = 1e-300;
        }
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

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
    fn sample(self, r: &mut Rng) -> f64 {
        match self {
            Dist::Uniform => r.uniform(),
            Dist::Gaussian => r.normal(),
            Dist::Zipfian => {
                let u = r.uniform().max(1e-12);
                let s = 1.07f64;
                (1.0 - u).powf(-1.0 / (s - 1.0)).min(1.0e6)
            }
            Dist::Lognormal => (r.normal() * 0.7).exp(),
            Dist::Bimodal => {
                if r.uniform() < 0.5 {
                    r.normal() * 0.3 - 2.0
                } else {
                    r.normal() * 0.3 + 2.0
                }
            }
        }
    }
}

fn generate(d: Dist, n: usize, r: &mut Rng) -> Vec<f64> {
    (0..n).map(|_| d.sample(r)).collect()
}

#[derive(Clone)]
struct BucketHist {
    boundaries: Vec<f64>,
    counts: Vec<u64>,
}

impl BucketHist {
    fn estimate_range(&self, lo: f64, hi: f64) -> u64 {
        if lo > hi || self.counts.is_empty() {
            return 0;
        }
        let mut e = 0.0f64;
        for i in 0..self.counts.len() {
            let (bl, bh, c) = (
                self.boundaries[i],
                self.boundaries[i + 1],
                self.counts[i] as f64,
            );
            if bh < lo || bl > hi {
                continue;
            }
            if bl >= lo && bh <= hi {
                e += c;
                continue;
            }
            let bw = (bh - bl).max(f64::EPSILON);
            let ow = (bh.min(hi) - bl.max(lo)).max(0.0);
            e += c * (ow / bw);
        }
        e.max(0.0) as u64
    }
}

// MaxDiff(V,A) — Poosala SIGMOD 1996. Sort by attribute, reduce to (val,
// freq) over distinct values, score gaps by |df| * dv, cut at the b - 1
// largest. Canonical V=frequency, A=area variant.
fn build_maxdiff(values: &[f64], buckets: usize) -> BucketHist {
    if values.is_empty() || buckets == 0 {
        return BucketHist {
            boundaries: vec![0.0, 0.0],
            counts: vec![0],
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut vals: Vec<f64> = Vec::new();
    let mut freqs: Vec<u64> = Vec::new();
    for &v in &sorted {
        if let Some(&last) = vals.last() {
            if (v - last).abs() < f64::EPSILON * last.abs().max(1.0) {
                *freqs.last_mut().unwrap() += 1;
                continue;
            }
        }
        vals.push(v);
        freqs.push(1);
    }
    let d = vals.len();
    if d <= buckets {
        let mut boundaries = Vec::with_capacity(d + 1);
        boundaries.push(vals[0]);
        for i in 0..d {
            boundaries.push(vals[i]);
        }
        return BucketHist {
            boundaries,
            counts: freqs,
        };
    }
    let mut diffs: Vec<(f64, usize)> = (0..d - 1)
        .map(|i| {
            let fd = (freqs[i + 1] as f64 - freqs[i] as f64).abs();
            let ad = vals[i + 1] - vals[i];
            (fd * ad, i)
        })
        .collect();
    diffs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut cuts: Vec<usize> = diffs.iter().take(buckets - 1).map(|&(_, i)| i).collect();
    cuts.sort_unstable();
    let mut boundaries = Vec::with_capacity(buckets + 1);
    let mut counts: Vec<u64> = Vec::with_capacity(buckets);
    boundaries.push(vals[0]);
    let mut start = 0usize;
    for &cut in &cuts {
        let end = cut;
        counts.push(freqs[start..=end].iter().sum());
        boundaries.push(vals[end]);
        start = end + 1;
    }
    counts.push(freqs[start..d].iter().sum());
    boundaries.push(vals[d - 1]);
    BucketHist { boundaries, counts }
}

// V-Optimal — Jagadish VLDB 1998. Minimises sum-squared error of bucket
// mean frequency. Direct DP over 10^6 distinct values is O(b*n^2) ≈ 10^14;
// we anchor at k = 500 equi-depth quantiles before the DP — the standard
// practical reduction. With k=500, b=128 the inner loop is ~3.2e7 ops.
const VOPT_ANCHORS: usize = 500;

fn build_voptimal(values: &[f64], buckets: usize) -> BucketHist {
    if values.is_empty() || buckets == 0 {
        return BucketHist {
            boundaries: vec![0.0, 0.0],
            counts: vec![0],
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let b = buckets.min(n);
    let k = VOPT_ANCHORS.min(n);
    let mut offsets: Vec<usize> = (0..=k).map(|i| (i * n) / k).collect();
    offsets[k] = n;
    let mut av: Vec<f64> = Vec::with_capacity(k);
    let mut ac: Vec<u64> = Vec::with_capacity(k);
    for i in 0..k {
        av.push(sorted[offsets[i]]);
        ac.push((offsets[i + 1] - offsets[i]) as u64);
    }
    let mut pf = vec![0.0f64; k + 1];
    let mut pf2 = vec![0.0f64; k + 1];
    for i in 0..k {
        let f = ac[i] as f64;
        pf[i + 1] = pf[i] + f;
        pf2[i + 1] = pf2[i] + f * f;
    }
    let sse = |i: usize, j: usize| -> f64 {
        let c = (j - i) as f64;
        let s = pf[j] - pf[i];
        (pf2[j] - pf2[i] - s * s / c).max(0.0)
    };
    let bb = b.min(k);
    let inf = f64::INFINITY;
    let mut dp = vec![vec![inf; k + 1]; bb + 1];
    let mut bt = vec![vec![0usize; k + 1]; bb + 1];
    dp[0][0] = 0.0;
    for nb in 1..=bb {
        for j in nb..=k {
            let lo_i = nb - 1;
            let mut best = inf;
            let mut bi = lo_i;
            for i in lo_i..j {
                let v = dp[nb - 1][i] + sse(i, j);
                if v < best {
                    best = v;
                    bi = i;
                }
            }
            dp[nb][j] = best;
            bt[nb][j] = bi;
        }
    }
    let mut cuts: Vec<usize> = Vec::with_capacity(bb + 1);
    let mut j = k;
    cuts.push(j);
    for nb in (1..=bb).rev() {
        let i = bt[nb][j];
        cuts.push(i);
        j = i;
    }
    cuts.reverse();
    let mut boundaries = Vec::with_capacity(bb + 1);
    let mut counts = Vec::with_capacity(bb);
    boundaries.push(av[0]);
    for bi in 0..bb {
        let lo = cuts[bi];
        let hi = cuts[bi + 1];
        counts.push(ac[lo..hi].iter().sum());
        let right = if hi < k { av[hi] } else { sorted[n - 1] };
        boundaries.push(right);
    }
    BucketHist { boundaries, counts }
}

fn percentile(s: &[f64], p: f64) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let idx = ((s.len() as f64 - 1.0) * p).round() as usize;
    s[idx.min(s.len() - 1)]
}

fn true_range_count(v: &[f64], lo: f64, hi: f64) -> u64 {
    v.iter().filter(|&&x| x >= lo && x <= hi).count() as u64
}

fn random_range(r: &mut Rng, vmin: f64, vmax: f64) -> (f64, f64) {
    let a = vmin + r.uniform() * (vmax - vmin);
    let b = vmin + r.uniform() * (vmax - vmin);
    if a <= b { (a, b) } else { (b, a) }
}

fn rel_err(est: u64, truth: u64) -> f64 {
    (est as i64 - truth as i64).abs() as f64 / truth.max(1) as f64
}

#[derive(Clone, Copy)]
enum Baseline {
    MaxDiff,
    VOptimal,
}
impl Baseline {
    fn name(self) -> &'static str {
        match self {
            Baseline::MaxDiff => "maxdiff",
            Baseline::VOptimal => "voptimal",
        }
    }
    fn build(self, v: &[f64], b: usize) -> BucketHist {
        match self {
            Baseline::MaxDiff => build_maxdiff(v, b),
            Baseline::VOptimal => build_voptimal(v, b),
        }
    }
}

struct Cell {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

fn run_cell(bl: Baseline, d: Dist, n: usize, b: usize, trials: usize, q: usize) -> Cell {
    let mut errs: Vec<f64> = Vec::with_capacity(trials * q);
    for t in 0..trials {
        // Seed identical to histogram_accuracy.rs.
        let seed = (d as u64) * 1_000_003 + (n as u64) * 97 + (b as u64) * 31 + t as u64;
        let mut rng = Rng::seeded(seed);
        let pop = generate(d, n, &mut rng);
        let (vmin, vmax) = pop
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        let hist = bl.build(&pop, b);
        for _ in 0..q {
            let (lo, hi) = random_range(&mut rng, vmin, vmax);
            errs.push(rel_err(
                hist.estimate_range(lo, hi),
                true_range_count(&pop, lo, hi),
            ));
        }
    }
    errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Cell {
        p50: percentile(&errs, 0.50),
        p95: percentile(&errs, 0.95),
        p99: percentile(&errs, 0.99),
        max: *errs.last().unwrap_or(&0.0),
    }
}

fn main() {
    let trials = 30;
    let q = 1000;
    let dists = [
        Dist::Uniform,
        Dist::Gaussian,
        Dist::Zipfian,
        Dist::Lognormal,
        Dist::Bimodal,
    ];
    let ns = [10_000usize, 100_000, 1_000_000];
    let b = 128usize;
    let baselines = [Baseline::MaxDiff, Baseline::VOptimal];
    println!("# Canonical histogram baselines (MaxDiff + V-Optimal) @ B=128");
    println!("baseline,dist,n,buckets,p50,p95,p99,max");
    for &bl in &baselines {
        for &d in &dists {
            for &n in &ns {
                let s = run_cell(bl, d, n, b, trials, q);
                println!(
                    "{},{},{},{},{:.4},{:.4},{:.4},{:.4}",
                    bl.name(),
                    d.name(),
                    n,
                    b,
                    s.p50,
                    s.p95,
                    s.p99,
                    s.max
                );
            }
        }
    }
}
