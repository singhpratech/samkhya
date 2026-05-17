//! Bloom filter false-positive-rate validation sweep.
//!
//! Sweeps configured FPR ∈ {0.001, 0.005, 0.01, 0.05} × capacity ∈
//! {10^4, 10^5, 10^6, 10^7} and measures empirical FPR over 10^6 negative
//! queries, with ≥30 trials per cell.
//!
//! Run with:
//! ```text
//! cargo run --release -p samkhya-core --example bloom_fpr_sweep
//! ```
//!
//! Emits a TSV stream on stdout (one row per cell) plus a summary block.
//! Uses splitmix64 for deterministic, seedable key generation — no `rand`
//! dependency.

use std::env;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

use samkhya_core::sketches::BloomFilter;

/// splitmix64 — deterministic, fast, statistically excellent.
#[inline(always)]
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Insert `capacity` distinct 64-bit keys drawn from `insert_seed`, then
/// query `n_queries` keys drawn from `query_seed` whose namespace is
/// guaranteed disjoint from the insert set (we tag the high bit and high
/// byte differently). Returns (empirical_fpr, insert_ns_per_op,
/// query_ns_per_op).
fn run_trial(
    capacity: usize,
    fp_rate: f64,
    n_queries: usize,
    insert_seed: u64,
    query_seed: u64,
) -> (f64, f64, f64) {
    let mut bf = BloomFilter::new(capacity, fp_rate);

    // Insert phase: keys tagged with high byte 0x00 (low half of u64 space).
    let mut s = insert_seed;
    let t0 = Instant::now();
    for _ in 0..capacity {
        let raw = splitmix64(&mut s);
        // Force high bit clear so insert / query namespaces never collide.
        let key = raw & 0x7FFF_FFFF_FFFF_FFFF;
        bf.insert(&key.to_le_bytes());
    }
    let insert_elapsed = t0.elapsed();

    // Query phase: keys tagged with high bit set (top half of u64 space).
    // This guarantees disjointness from the insert namespace, so every
    // positive return is a true false positive.
    let mut s = query_seed;
    let mut fps: u64 = 0;
    let t0 = Instant::now();
    for _ in 0..n_queries {
        let raw = splitmix64(&mut s);
        let key = raw | 0x8000_0000_0000_0000;
        if bf.contains(&key.to_le_bytes()) {
            fps += 1;
        }
    }
    let query_elapsed = t0.elapsed();

    let empirical_fpr = fps as f64 / n_queries as f64;
    let insert_ns = insert_elapsed.as_nanos() as f64 / capacity as f64;
    let query_ns = query_elapsed.as_nanos() as f64 / n_queries as f64;
    (empirical_fpr, insert_ns, query_ns)
}

/// Bootstrap 95% CI for the mean of `xs` using `n_boot` resamples.
fn bootstrap_ci_mean(xs: &[f64], n_boot: usize, boot_seed: u64) -> (f64, f64) {
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let n = xs.len();
    let mut means = Vec::with_capacity(n_boot);
    let mut s = boot_seed;
    for _ in 0..n_boot {
        let mut sum = 0.0;
        for _ in 0..n {
            let r = splitmix64(&mut s);
            let idx = (r as usize) % n;
            sum += xs[idx];
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lo = means[(n_boot as f64 * 0.025) as usize];
    let hi = means[(n_boot as f64 * 0.975).min(n_boot as f64 - 1.0) as usize];
    (lo, hi)
}

fn main() {
    // Pre-registered grid.
    let fp_targets = [0.001_f64, 0.005, 0.01, 0.05];
    let capacities = [10_000usize, 100_000, 1_000_000, 10_000_000];
    let n_queries: usize = 1_000_000;
    let n_trials: usize = 30;
    let safety_margin: f64 = 1.10; // empirical ≤ target × 1.10

    println!(
        "# bloom_fpr_sweep  fp_targets={:?}  capacities={:?}  n_queries={}  n_trials={}  safety_margin={}",
        fp_targets, capacities, n_queries, n_trials, safety_margin
    );
    println!(
        "cell\tfp_target\tcapacity\tnum_bits\tnum_hashes\tbits_per_elem\tempirical_mean\tci95_lo\tci95_hi\tinsert_ns\tquery_ns\tinsert_mops\tquery_mops\tpass"
    );

    let mut pass_count = 0usize;
    let mut total_cells = 0usize;

    let raw_path = env::var("SAMKHYA_RAW_OUT").ok();
    let mut raw_cells: Vec<String> = Vec::new();

    for &fp in &fp_targets {
        for &cap in &capacities {
            total_cells += 1;
            // Probe the configured geometry once.
            let probe = BloomFilter::new(cap, fp);
            let num_bits = probe.num_bits();
            let num_hashes = probe.num_hashes();
            let bits_per_elem = num_bits as f64 / cap as f64;

            let mut empirical = Vec::with_capacity(n_trials);
            let mut insert_ns_acc = 0.0;
            let mut query_ns_acc = 0.0;

            for t in 0..n_trials {
                let insert_seed = 0xA5A5_0000_0000_0000_u64
                    ^ ((cap as u64).wrapping_mul(0x9E37_79B9))
                    ^ ((fp.to_bits()).wrapping_mul(0xDEAD_BEEF))
                    ^ (t as u64).wrapping_mul(0xC0FF_EE01);
                let query_seed = insert_seed ^ 0x5A5A_F0F0_3C3C_C3C3;
                let (e, ins, qry) = run_trial(cap, fp, n_queries, insert_seed, query_seed);
                empirical.push(e);
                insert_ns_acc += ins;
                query_ns_acc += qry;
            }

            let mean = empirical.iter().sum::<f64>() / n_trials as f64;
            let (lo, hi) = bootstrap_ci_mean(&empirical, 2000, 0xBEEF);
            let insert_ns = insert_ns_acc / n_trials as f64;
            let query_ns = query_ns_acc / n_trials as f64;
            let insert_mops = 1000.0 / insert_ns;
            let query_mops = 1000.0 / query_ns;
            let pass = mean <= fp * safety_margin;
            if pass {
                pass_count += 1;
            }
            println!(
                "C{:02}\t{}\t{}\t{}\t{}\t{:.4}\t{:.6}\t{:.6}\t{:.6}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{}",
                total_cells,
                fp,
                cap,
                num_bits,
                num_hashes,
                bits_per_elem,
                mean,
                lo,
                hi,
                insert_ns,
                query_ns,
                insert_mops,
                query_mops,
                if pass { "PASS" } else { "FAIL" }
            );

            if raw_path.is_some() {
                let emp_vec = empirical
                    .iter()
                    .map(|v| format!("{v:.10}"))
                    .collect::<Vec<_>>()
                    .join(",");
                raw_cells.push(format!(
                    "{{\"fp_target\":{fp},\"capacity\":{cap},\"num_bits\":{num_bits},\"num_hashes\":{num_hashes},\"trials\":{n_trials},\"empirical_fpr\":[{emp_vec}]}}"
                ));
            }
        }
    }

    if let Some(path) = raw_path {
        let body = format!(
            "{{\"benchmark\":\"bloom_fpr_sweep\",\"n_queries_per_trial\":{n_queries},\"seed_scheme\":\"0xA5A5...^cap^fp^trial\",\"cells\":[{}]}}",
            raw_cells.join(",")
        );
        let mut f = File::create(&path).expect("create raw output file");
        f.write_all(body.as_bytes()).expect("write raw output");
        eprintln!("# raw per-trial vectors written to {path}");
    }

    eprintln!(
        "summary: {} / {} cells PASS at safety_margin = {}",
        pass_count, total_cells, safety_margin
    );
}
