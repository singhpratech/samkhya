//! Bench-08 harness: solve-time latency for the four LpBound variants
//! (Product / Chain / Agm / LpJoin) across join sizes and graph topologies.
//!
//! Build with the `lp_solver` feature so `LpJoinBound` is available:
//!
//!   cargo run --release -p samkhya-core --example lpbound_latency \
//!       --features lp_solver
//!
//! Emits a CSV-like block on stdout that the parent bench-results doc
//! ingests verbatim. No external deps beyond samkhya-core itself + std.

use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

#[cfg(feature = "lp_solver")]
use samkhya_core::lpbound::LpJoinBound;
use samkhya_core::lpbound::{AgmBound, ChainBound, ProductBound, UpperBound};

thread_local! {
    static RAW_COLLECTOR: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

// Small deterministic PRNG (xorshift64*) — keeps the harness dep-free and
// makes the topology samples reproducible across reruns.
struct Xs(u64);
impl Xs {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn gen_range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next_u64() % (hi - lo).max(1)
    }
}

#[derive(Copy, Clone, Debug)]
enum Topology {
    Chain,
    Cycle,
    Star,
    Erdos,
}

fn topo_name(t: Topology) -> &'static str {
    match t {
        Topology::Chain => "chain",
        Topology::Cycle => "cycle",
        Topology::Star => "star",
        Topology::Erdos => "erdos",
    }
}

/// Build a join graph of `n` relations under the requested topology.
/// Returns (relations[], equality_predicates[], distinct_counts[]).
fn build_join(n: usize, t: Topology, rng: &mut Xs) -> (Vec<u64>, Vec<(usize, usize)>, Vec<u64>) {
    // Relation sizes drawn log-uniform in [10^3, 10^7]. Realistic JOB-ish
    // range; large enough that the LP coefficient `ln|R|` is non-trivial.
    let relations: Vec<u64> = (0..n)
        .map(|_| {
            let e = rng.gen_range(3, 8); // 10^3..10^7
            10u64.pow(e as u32) + rng.gen_range(0, 1_000)
        })
        .collect();

    // Distinct-count hints ~ relation size / 10 (typical key cardinality
    // for FK columns in OLAP schemas).
    let distinct: Vec<u64> = relations.iter().map(|&r| (r / 10).max(1)).collect();

    let mut preds: Vec<(usize, usize)> = Vec::new();
    match t {
        Topology::Chain => {
            for i in 0..n.saturating_sub(1) {
                preds.push((i, i + 1));
            }
        }
        Topology::Cycle => {
            for i in 0..n.saturating_sub(1) {
                preds.push((i, i + 1));
            }
            if n >= 3 {
                preds.push((n - 1, 0));
            }
        }
        Topology::Star => {
            for i in 1..n {
                preds.push((0, i));
            }
        }
        Topology::Erdos => {
            // Fixed-density Erdős–Rényi: p ≈ 2/n so the expected edge
            // count is O(n). Ensures the graph is non-trivial without
            // blowing up LP size.
            let p_num = 2u64;
            let p_den = n.max(2) as u64;
            for i in 0..n {
                for j in (i + 1)..n {
                    if rng.next_u64() % p_den < p_num {
                        preds.push((i, j));
                    }
                }
            }
            // Guarantee at least one edge so the bound isn't a trivial
            // product fallback.
            if preds.is_empty() && n >= 2 {
                preds.push((0, 1));
            }
        }
    }
    (relations, preds, distinct)
}

/// Time one ceiling-evaluation call. Uses a tight loop of `inner_reps`
/// iterations to amortise `Instant::now()` overhead at sub-microsecond
/// granularity, then reports the per-call time in nanoseconds.
fn time_one<F: FnMut() -> u64>(mut f: F, inner_reps: u32) -> u64 {
    // Warm: discard the first call so allocator / branch predictors are warm.
    let _ = black_box(f());
    let t0 = Instant::now();
    for _ in 0..inner_reps {
        black_box(f());
    }
    let dt = t0.elapsed();
    (dt.as_nanos() as u64) / (inner_reps as u64)
}

/// Same as `time_one` but treats each call as cold: drops black_box'd
/// volatile state in between to defeat trivial result-caching.
fn time_one_cold<F: FnMut() -> u64>(mut f: F) -> u64 {
    let t0 = Instant::now();
    let r = black_box(f());
    let dt = t0.elapsed();
    let _ = black_box(r);
    dt.as_nanos() as u64
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn run_cell(name: &str, sizes: &[usize], topologies: &[Topology], reps_per_cell: usize) {
    // Print header once per bound.
    println!("# bound = {name}");
    println!(
        "# cols: bound,topology,join_size,n_samples,p50_ns,p95_ns,p99_ns,p50_warm_ns,p50_cold_ns,inner_loop_iters"
    );
    for &t in topologies {
        for &n in sizes {
            // Adaptive inner-loop count: fast bounds (Product/Chain/Agm)
            // are tens of ns/call, so we wrap them in a 4k inner loop to
            // get a stable timing per outer sample. The LP solver is in
            // the µs..ms regime, so inner = 1 is fine.
            let inner = match name {
                "LpJoin" => 1u32,
                _ => 4096u32,
            };
            let mut samples_warm: Vec<u64> = Vec::with_capacity(reps_per_cell);
            let mut samples_cold: Vec<u64> = Vec::with_capacity(reps_per_cell);
            // Seed per (topology, size) pair for reproducibility.
            let seed = 0x9E37_79B9_7F4A_7C15u64
                ^ (n as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
                ^ (topo_name(t).as_ptr() as u64);
            let mut rng = Xs::new(seed);
            for _ in 0..reps_per_cell {
                let (rels, preds, distinct) = build_join(n, t, &mut rng);
                // Build the bound object once; the per-call cost we want
                // is the `ceiling()` invocation, not construction.
                let warm: u64 = match name {
                    "Product" => time_one(|| ProductBound.ceiling(&rels, &preds), inner),
                    "Chain" => {
                        let cb = ChainBound::new(distinct.clone());
                        time_one(|| cb.ceiling(&rels, &preds), inner)
                    }
                    "Agm" => time_one(|| AgmBound.ceiling(&rels, &preds), inner),
                    #[cfg(feature = "lp_solver")]
                    "LpJoin" => {
                        let lp = LpJoinBound::new();
                        time_one(|| lp.ceiling(&rels, &preds), inner)
                    }
                    _ => 0,
                };
                let cold: u64 = match name {
                    "Product" => time_one_cold(|| ProductBound.ceiling(&rels, &preds)),
                    "Chain" => {
                        let cb = ChainBound::new(distinct.clone());
                        time_one_cold(|| cb.ceiling(&rels, &preds))
                    }
                    "Agm" => time_one_cold(|| AgmBound.ceiling(&rels, &preds)),
                    #[cfg(feature = "lp_solver")]
                    "LpJoin" => {
                        let lp = LpJoinBound::new();
                        time_one_cold(|| lp.ceiling(&rels, &preds))
                    }
                    _ => 0,
                };
                samples_warm.push(warm);
                samples_cold.push(cold);
            }
            // Snapshot raw vectors BEFORE sorting (for stable per-trial export).
            RAW_COLLECTOR.with(|c| {
                let warm_str = samples_warm
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let cold_str = samples_cold
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                c.borrow_mut().push(format!(
                    "{{\"bound\":\"{name}\",\"topology\":\"{}\",\"join_size\":{n},\"trials\":{},\"inner_loop\":{inner},\"warm_ns\":[{warm_str}],\"cold_ns\":[{cold_str}]}}",
                    topo_name(t),
                    samples_warm.len(),
                ));
            });

            samples_warm.sort_unstable();
            samples_cold.sort_unstable();
            let p50 = percentile(&samples_warm, 0.50);
            let p95 = percentile(&samples_warm, 0.95);
            let p99 = percentile(&samples_warm, 0.99);
            let p50_cold = percentile(&samples_cold, 0.50);
            println!(
                "{},{},{},{},{},{},{},{},{},{}",
                name,
                topo_name(t),
                n,
                samples_warm.len(),
                p50,
                p95,
                p99,
                p50,
                p50_cold,
                inner
            );
        }
    }
    println!();
}

#[cfg(feature = "lp_solver")]
fn run_lp_iter_analysis(sizes: &[usize], topologies: &[Topology]) {
    // The microlp backend doesn't expose iteration counts; we use the
    // wall-time delta between the fastest single-relation LP (≈ baseline
    // setup + one pivot) and the larger-component LP as a proxy. This
    // surfaces the simplex-pivot scaling without needing solver internals.
    println!("# lp_iteration_proxy (LpJoin only)");
    println!("# cols: topology,join_size,p50_setup_ns,p50_solve_ns,proxy_iters");
    let mut rng = Xs::new(0xCAFEBABE);
    let lp = LpJoinBound::new();
    // Baseline: 2-relation single-edge LP. Treat as ≈1 simplex pivot.
    let mut baseline_samples = Vec::with_capacity(64);
    for _ in 0..64 {
        let (r, p, _) = build_join(2, Topology::Chain, &mut rng);
        baseline_samples.push(time_one_cold(|| lp.ceiling(&r, &p)));
    }
    baseline_samples.sort_unstable();
    let baseline = percentile(&baseline_samples, 0.50).max(1);
    for &t in topologies {
        for &n in sizes {
            let mut samples = Vec::with_capacity(32);
            let mut rng2 = Xs::new(0x1234_5678_9ABC_DEF0u64 ^ n as u64);
            for _ in 0..32 {
                let (r, p, _) = build_join(n, t, &mut rng2);
                samples.push(time_one_cold(|| lp.ceiling(&r, &p)));
            }
            samples.sort_unstable();
            let med = percentile(&samples, 0.50);
            let solve = med.saturating_sub(baseline);
            let proxy_iters = ((solve as f64) / (baseline as f64)).max(1.0);
            println!(
                "{},{},{},{},{:.2}",
                topo_name(t),
                n,
                baseline,
                solve,
                proxy_iters
            );
        }
    }
    println!();
}

fn main() {
    // Touch CARGO_PKG_VERSION so any future regression report can grep
    // the binary's provenance from stdout.
    println!(
        "# samkhya-core lpbound_latency v{}",
        env!("CARGO_PKG_VERSION")
    );
    println!("# inner-loop amortised timing; per-call ns; 30 outer reps × {{warm, cold}}");

    let sizes = [2usize, 3, 5, 7, 10, 15];
    let topologies = [
        Topology::Chain,
        Topology::Cycle,
        Topology::Star,
        Topology::Erdos,
    ];
    let reps = 30;

    run_cell("Product", &sizes, &topologies, reps);
    run_cell("Chain", &sizes, &topologies, reps);
    run_cell("Agm", &sizes, &topologies, reps);

    #[cfg(feature = "lp_solver")]
    {
        run_cell("LpJoin", &sizes, &topologies, reps);
        run_lp_iter_analysis(&sizes, &topologies);
    }
    #[cfg(not(feature = "lp_solver"))]
    println!("# LpJoin skipped: rebuild with --features lp_solver");

    if let Ok(path) = env::var("SAMKHYA_RAW_OUT") {
        let raw = RAW_COLLECTOR.with(|c| c.borrow().join(","));
        let body = format!("{{\"benchmark\":\"lpbound_latency\",\"cells\":[{}]}}", raw);
        let mut f = File::create(&path).expect("create raw output file");
        f.write_all(body.as_bytes()).expect("write raw output");
        eprintln!("# raw per-trial vectors written to {path}");
    }
}
