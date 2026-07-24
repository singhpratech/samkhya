//! Empirical tightness comparison of the LpBound family.
#![allow(deprecated)] // the legacy bound family is what this campaign measures
//!
//! Builds synthetic multi-way join graphs (path, star, cycle, clique) at
//! several arities and per-edge ℓ_p degree-sequence regimes, then evaluates
//! every bound in the LpBound family on each (topology x size x p) cell:
//!
//!   ProductBound >= ChainBound >= AgmBound >= LpJoinBound
//!
//! For each cell we also derive a *ground-truth lower bound* by materialising
//! a seeded synthetic instance and counting the actual join output rows.
//! Bounds whose ratio bound/ground_truth is closer to 1 are tighter.
//!
//! Run with:
//! ```text
//! cargo run --release --example lpbound_tightness -p samkhya-core \
//!     --features lp_solver
//! ```
//!
//! Output is a JSON object on stdout; the companion bench-results document
//! (`bench-results/07_lpbound_tightness.md`) interprets the numbers.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs::File;
use std::io::Write;

use samkhya_core::lpbound::{AgmBound, ChainBound, ProductBound, UpperBound};

#[cfg(feature = "lp_solver")]
use samkhya_core::lpbound::LpJoinBound;

// --- Tiny seeded RNG (splitmix64) so the benchmark is reproducible without
// pulling in `rand` as a dev-dependency. -------------------------------------

#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo).max(1)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// --- Topology builders -----------------------------------------------------

fn topology_path(n: usize) -> Vec<(usize, usize)> {
    (0..n.saturating_sub(1)).map(|i| (i, i + 1)).collect()
}

fn topology_star(n: usize) -> Vec<(usize, usize)> {
    // Hub = 0, spokes 1..n.
    (1..n).map(|i| (0, i)).collect()
}

fn topology_cycle(n: usize) -> Vec<(usize, usize)> {
    let mut e: Vec<(usize, usize)> = (0..n.saturating_sub(1)).map(|i| (i, i + 1)).collect();
    if n >= 3 {
        e.push((n - 1, 0));
    }
    e
}

fn topology_clique(n: usize) -> Vec<(usize, usize)> {
    let mut e = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            e.push((i, j));
        }
    }
    e
}

fn topology_for(name: &str, n: usize) -> Vec<(usize, usize)> {
    match name {
        "path" => topology_path(n),
        "star" => topology_star(n),
        "cycle" => topology_cycle(n),
        "clique" => topology_clique(n),
        _ => unreachable!(),
    }
}

// --- ℓ_p degree-sequence assignments ---------------------------------------
//
// Each relation r is parameterised by:
//   - a row count N_r (drawn from a per-topology window)
//   - a per-edge degree sequence sampled with a ℓ_p skew profile
//     - p = 1: uniform   (every key has degree ≈ N_r / D_r)
//     - p = 2: heavy-mid (Zipfian alpha ≈ 1.0)
//     - p = ∞: heavy-hit (a single hot key carries half the rows)
//
// We expose only the resulting per-relation `(row_count, distinct_count)`
// pair to the LpBound family — that is the data the bounds actually consume.

#[derive(Clone, Copy, Debug)]
enum PNorm {
    P1,
    P2,
    PInf,
}

impl PNorm {
    fn label(&self) -> &'static str {
        match self {
            PNorm::P1 => "p=1",
            PNorm::P2 => "p=2",
            PNorm::PInf => "p=inf",
        }
    }
    fn distinct_for(&self, n_rows: u64, rng: &mut Rng) -> u64 {
        let n = n_rows.max(1);
        match self {
            // Uniform: distinct ≈ rows / small_factor, so degree ≈ small_factor.
            PNorm::P1 => (n / rng.range(2, 6)).max(1),
            // Mid-skew: distinct ≈ sqrt(rows).
            PNorm::P2 => ((n as f64).sqrt() as u64).max(2),
            // Heavy-hitter: a handful of keys, large per-key degree.
            PNorm::PInf => rng.range(2, 16),
        }
    }
}

// --- Ground-truth join cardinality (materialised) --------------------------
//
// For each relation we generate a multiset of key values whose support size
// equals the chosen distinct count and whose per-key frequencies follow the
// ℓ_p profile. We then count the cardinality of the multi-way equi-join on
// the join graph by hash-merging: pick a root relation, propagate join keys
// outward, and product-aggregate the per-key frequencies. This is exact for
// the tree topologies (path, star) and yields a strict lower bound for the
// cyclic topologies (cycle, clique) — sufficient for "is the bound tight?".

#[derive(Clone)]
struct Relation {
    /// Frequency map: join-key value -> rows in this relation carrying it.
    /// (Single-attribute schema; multi-attribute joins are modelled by
    /// repeating the relation per shared attribute.)
    freq: HashMap<u64, u64>,
}

impl Relation {
    fn rows(&self) -> u64 {
        self.freq.values().copied().sum()
    }
}

fn synth_relation(n_rows: u64, distinct: u64, p: PNorm, rng: &mut Rng) -> Relation {
    let distinct = distinct.max(1).min(n_rows.max(1));
    let mut freq: HashMap<u64, u64> = HashMap::with_capacity(distinct as usize);

    // Assign each row to a key according to ℓ_p profile.
    match p {
        PNorm::P1 => {
            // Uniform.
            for _ in 0..n_rows {
                let k = rng.range(0, distinct);
                *freq.entry(k).or_insert(0) += 1;
            }
        }
        PNorm::P2 => {
            // Zipf-ish: pick key with weight ~ 1/(rank+1).
            // Materialise weights once; sample by inverse-CDF.
            let weights: Vec<f64> = (0..distinct).map(|r| 1.0 / (r as f64 + 1.0)).collect();
            let total: f64 = weights.iter().sum();
            let mut cdf = Vec::with_capacity(distinct as usize);
            let mut s = 0.0;
            for w in &weights {
                s += w / total;
                cdf.push(s);
            }
            for _ in 0..n_rows {
                let u = rng.unit();
                let k = cdf.iter().position(|&c| c >= u).unwrap_or(0) as u64;
                *freq.entry(k).or_insert(0) += 1;
            }
        }
        PNorm::PInf => {
            // Heavy-hitter: 60% of rows on key 0, rest spread uniformly.
            for _ in 0..n_rows {
                let k = if rng.unit() < 0.6 {
                    0
                } else {
                    rng.range(1, distinct.max(2))
                };
                *freq.entry(k).or_insert(0) += 1;
            }
        }
    }
    Relation { freq }
}

/// Exact join cardinality for tree topologies; for cyclic topologies this
/// returns the cardinality assuming all edges share one attribute — which is
/// a strict lower bound (the *worst-case* output, not the upper bound),
/// since adding more shared-attribute constraints can only shrink the
/// result. Either way the value is a *true cardinality realisable by some
/// instance*, so every upper bound must dominate it.
fn join_cardinality(relations: &[Relation], edges: &[(usize, usize)]) -> u128 {
    if relations.is_empty() {
        return 0;
    }
    if edges.is_empty() {
        return relations
            .iter()
            .fold(1u128, |a, r| a.saturating_mul(r.rows() as u128));
    }

    // Single shared attribute across all edges: the cardinality is
    //   sum_k prod_r freq_r[k]
    // taken over keys present in every relation that has at least one
    // incident edge. Singletons (no incident edge) multiply the result by
    // their full row count.
    let n = relations.len();
    let mut has_edge = vec![false; n];
    for &(i, j) in edges {
        has_edge[i] = true;
        has_edge[j] = true;
    }

    let key_union: HashSet<u64> = (0..n)
        .filter(|&r| has_edge[r])
        .flat_map(|r| relations[r].freq.keys().copied())
        .collect();

    let mut joined: u128 = 0;
    for k in &key_union {
        let mut prod: u128 = 1;
        let mut viable = true;
        for r in 0..n {
            if !has_edge[r] {
                continue;
            }
            let f = relations[r].freq.get(k).copied().unwrap_or(0) as u128;
            if f == 0 {
                viable = false;
                break;
            }
            prod = prod.saturating_mul(f);
        }
        if viable {
            joined = joined.saturating_add(prod);
        }
    }
    for r in 0..n {
        if !has_edge[r] {
            joined = joined.saturating_mul(relations[r].rows() as u128);
        }
    }
    joined.max(1)
}

// --- Driver ----------------------------------------------------------------

#[derive(Default)]
struct Stats {
    samples: u64,
    sum_ratio_product: f64,
    sum_ratio_chain: f64,
    sum_ratio_agm: f64,
    sum_ratio_lp: f64,
    ratios_product: Vec<f64>,
    ratios_chain: Vec<f64>,
    ratios_lp: Vec<f64>,
    ratios_agm: Vec<f64>,
    ordering_ok: u64, // ProductBound >= ChainBound >= AgmBound >= LpJoinBound
    lp_le_agm: u64,
    /// Trials where a bound landed strictly below the materialised true
    /// cardinality. A sound ceiling can never do this; the count exists so
    /// that when one does, the campaign says so instead of averaging it
    /// away. Through v1.1 the ratios were clamped with `.max(1.0)`, which
    /// reported an unsound bound as a perfectly tight one.
    violations_product: u64,
    violations_chain: u64,
    violations_agm: u64,
    violations_lp: u64,
    /// Trials where the true cardinality exceeds `u64::MAX`, so every
    /// bound saturates and a bound-vs-truth comparison says nothing about
    /// soundness. Excluded from the violation counts and reported
    /// separately rather than silently folded in.
    saturated: u64,
}

fn median(xs: &mut [f64]) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = xs.len();
    if n % 2 == 0 {
        0.5 * (xs[n / 2 - 1] + xs[n / 2])
    } else {
        xs[n / 2]
    }
}

fn main() {
    let topologies = ["path", "star", "cycle", "clique"];
    let sizes = [3usize, 5, 7];
    let p_norms = [PNorm::P1, PNorm::P2, PNorm::PInf];
    let trials_per_cell = 30u64;

    println!("{{");
    println!("  \"benchmark\": \"lpbound_tightness\",");
    println!("  \"date\": \"2026-05-16\",");
    println!("  \"trials_per_cell\": {trials_per_cell},");
    println!("  \"cells\": [");

    let mut first_cell = true;
    let mut global_ordering_ok = 0u64;
    let mut global_total = 0u64;
    let mut global_violations = 0u64;
    let mut global_saturated = 0u64;
    let mut star5_lp_improvement_skew: Vec<f64> = Vec::new();

    let raw_path = env::var("SAMKHYA_RAW_OUT").ok();
    let mut raw_cells: Vec<String> = Vec::new();

    for &topo in &topologies {
        for &n in &sizes {
            for &p in &p_norms {
                let mut stats = Stats::default();
                for trial in 0..trials_per_cell {
                    let seed = mix_seed(topo, n, p, trial);
                    let mut rng = Rng::new(seed);

                    // Per-relation row counts in [200, 5_000].
                    let rows: Vec<u64> = (0..n).map(|_| rng.range(200, 5_000)).collect();
                    let distinct: Vec<u64> = rows
                        .iter()
                        .map(|&nr| p.distinct_for(nr, &mut rng))
                        .collect();
                    let edges = topology_for(topo, n);

                    // Materialise the synthetic relations to get a true
                    // (lower-bound) cardinality the upper bounds must dominate.
                    let rels: Vec<Relation> = rows
                        .iter()
                        .zip(distinct.iter())
                        .map(|(&nr, &d)| synth_relation(nr, d, p, &mut rng))
                        .collect();
                    let truth = join_cardinality(&rels, &edges).max(1) as f64;

                    let prod_b = ProductBound.ceiling(&rows, &edges) as f64;
                    let agm_b = AgmBound.ceiling(&rows, &edges) as f64;
                    let chain_b = ChainBound::new(distinct.clone()).ceiling(&rows, &edges) as f64;

                    #[cfg(feature = "lp_solver")]
                    let lp_b = LpJoinBound::with_distinct_counts(distinct.clone())
                        .ceiling_with_distinct(&rows, &edges) as f64;
                    #[cfg(not(feature = "lp_solver"))]
                    let lp_b = agm_b; // sentinel — not part of the comparison

                    // Soundness first: a ceiling below the materialised
                    // truth is a defect, not a tightness result. Count it
                    // before any ratio is taken.
                    //
                    // Bounds are `u64` and saturate; the materialised truth
                    // is computed in `u128`. When the truth exceeds
                    // `u64::MAX` every bound is pinned at the saturation
                    // value and lands "below" it for arithmetic reasons that
                    // have nothing to do with soundness. Those trials are
                    // counted separately, never as violations.
                    let saturated = truth > u64::MAX as f64;
                    if saturated {
                        stats.saturated += 1;
                    } else {
                        if prod_b < truth - 0.5 {
                            stats.violations_product += 1;
                        }
                        if chain_b < truth - 0.5 {
                            stats.violations_chain += 1;
                        }
                        if agm_b < truth - 0.5 {
                            stats.violations_agm += 1;
                        }
                        if lp_b < truth - 0.5 {
                            stats.violations_lp += 1;
                        }
                    }

                    // Ratios bound / truth. Reported unclamped: a value
                    // below 1.0 means the ceiling was breached, and that
                    // has to remain visible in the receipts.
                    let r_prod = prod_b / truth;
                    let r_chain = chain_b / truth;
                    let r_agm = agm_b / truth;
                    let r_lp = lp_b / truth;

                    stats.samples += 1;
                    stats.sum_ratio_product += r_prod;
                    stats.sum_ratio_chain += r_chain;
                    stats.sum_ratio_agm += r_agm;
                    stats.sum_ratio_lp += r_lp;
                    stats.ratios_product.push(r_prod);
                    stats.ratios_chain.push(r_chain);
                    stats.ratios_lp.push(r_lp);
                    stats.ratios_agm.push(r_agm);

                    // Ordering check.
                    // For cyclic topologies, ChainBound's simple-divide model
                    // can fall *below* AgmBound (since it amortises across
                    // many predicates) — that violates the documented
                    // scaffolding ordering in degenerate distinct-count
                    // regimes, so we record the actual count and report it.
                    let ord =
                        prod_b >= chain_b - 0.5 && chain_b >= agm_b - 0.5 && agm_b >= lp_b - 0.5;
                    if ord {
                        stats.ordering_ok += 1;
                    }
                    if lp_b <= agm_b + 0.5 {
                        stats.lp_le_agm += 1;
                    }

                    // Only count a tightness improvement when the tighter
                    // bound is actually sound on this instance. Crediting a
                    // breached ceiling as an improvement is how the v1.1
                    // campaign produced its headline ratio.
                    if topo == "star"
                        && n == 5
                        && matches!(p, PNorm::P2 | PNorm::PInf)
                        && lp_b > 0.0
                        && !saturated
                        && lp_b >= truth - 0.5
                        && agm_b >= truth - 0.5
                    {
                        let improvement = (agm_b / lp_b).max(1.0);
                        star5_lp_improvement_skew.push(improvement);
                    }
                }

                global_total += stats.samples;
                global_ordering_ok += stats.ordering_ok;
                global_saturated += stats.saturated;
                global_violations += stats.violations_product
                    + stats.violations_chain
                    + stats.violations_agm
                    + stats.violations_lp;

                let median_lp_over_agm = {
                    let mut xs: Vec<f64> = stats
                        .ratios_agm
                        .iter()
                        .zip(stats.ratios_lp.iter())
                        .filter(|(a, l)| **a >= 1.0 && **l >= 1.0)
                        .map(|(a, l)| (a / l).max(1.0))
                        .collect();
                    median(&mut xs)
                };

                if !first_cell {
                    println!(",");
                }
                first_cell = false;
                print!(
                    "    {{\"topology\": \"{topo}\", \"size\": {n}, \"p\": \"{p_label}\", \
                     \"samples\": {samples}, \
                     \"mean_ratio_product\": {mp:.3}, \
                     \"mean_ratio_chain\": {mc:.3}, \
                     \"mean_ratio_agm\": {ma:.3}, \
                     \"mean_ratio_lp\": {ml:.3}, \
                     \"median_lp_vs_agm_speedup\": {mlsp:.3}, \
                     \"ordering_holds_pct\": {opct:.1}, \
                     \"lp_le_agm_pct\": {lpct:.1}, \
                     \"violations_product\": {vp}, \
                     \"violations_chain\": {vc}, \
                     \"violations_agm\": {va}, \
                     \"violations_lp\": {vl}, \
                     \"saturated_trials\": {sat}}}",
                    topo = topo,
                    n = n,
                    p_label = p.label(),
                    samples = stats.samples,
                    mp = stats.sum_ratio_product / stats.samples as f64,
                    mc = stats.sum_ratio_chain / stats.samples as f64,
                    ma = stats.sum_ratio_agm / stats.samples as f64,
                    ml = stats.sum_ratio_lp / stats.samples as f64,
                    mlsp = median_lp_over_agm,
                    opct = 100.0 * stats.ordering_ok as f64 / stats.samples as f64,
                    lpct = 100.0 * stats.lp_le_agm as f64 / stats.samples as f64,
                    vp = stats.violations_product,
                    vc = stats.violations_chain,
                    va = stats.violations_agm,
                    vl = stats.violations_lp,
                    sat = stats.saturated,
                );

                if raw_path.is_some() {
                    let fmt = |xs: &[f64]| {
                        xs.iter()
                            .map(|v| format!("{v:.6}"))
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    raw_cells.push(format!(
                        "{{\"topology\":\"{topo}\",\"size\":{n},\"p\":\"{}\",\"trials\":{},\"ratios_product\":[{}],\"ratios_chain\":[{}],\"ratios_agm\":[{}],\"ratios_lp\":[{}]}}",
                        p.label(),
                        stats.samples,
                        fmt(&stats.ratios_product),
                        fmt(&stats.ratios_chain),
                        fmt(&stats.ratios_agm),
                        fmt(&stats.ratios_lp),
                    ));
                }
            }
        }
    }

    println!();
    println!("  ],");
    println!(
        "  \"global_ordering_pct\": {:.2},",
        100.0 * global_ordering_ok as f64 / global_total.max(1) as f64
    );
    let median_star5_skew = {
        let mut xs = star5_lp_improvement_skew.clone();
        median(&mut xs)
    };
    println!("  \"hypothesis_star5_skew_median_lp_vs_agm\": {median_star5_skew:.3},");
    println!("  \"soundness_violations_total\": {global_violations},");
    println!("  \"saturated_trials_total\": {global_saturated},");
    println!("  \"feature_lp_solver\": {}", cfg!(feature = "lp_solver"));
    println!("}}");

    if let Some(path) = raw_path {
        let body = format!(
            "{{\"benchmark\":\"lpbound_tightness\",\"trials_per_cell\":{trials_per_cell},\"feature_lp_solver\":{},\"cells\":[{}]}}",
            cfg!(feature = "lp_solver"),
            raw_cells.join(",")
        );
        let mut f = File::create(&path).expect("create raw output file");
        f.write_all(body.as_bytes()).expect("write raw output");
        eprintln!("# raw per-trial vectors written to {path}");
    }

    // Suppress unused-import warning when lp_solver is off.
    let _ = BTreeMap::<(), ()>::new();
}

/// Deterministic seed derived from cell coordinates.
fn mix_seed(topo: &str, n: usize, p: PNorm, trial: u64) -> u64 {
    let mut h: u64 = 0xDEAD_BEEF_CAFE_F00D;
    for b in topo.as_bytes() {
        h = h.wrapping_mul(0x100000001B3).wrapping_add(*b as u64);
    }
    h = h.wrapping_mul(0x100000001B3).wrapping_add(n as u64);
    h = h.wrapping_mul(0x100000001B3).wrapping_add(match p {
        PNorm::P1 => 1,
        PNorm::P2 => 2,
        PNorm::PInf => 3,
    });
    h = h.wrapping_mul(0x100000001B3).wrapping_add(trial);
    h
}
