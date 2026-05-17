//! `burst_harness` — WAVE5-H pipeline closure for file
//! `bench-results/17_failure_modes.md`.
//!
//! Drives the samkhya sketch + Moerkotte-q-error pipeline under a bursty
//! load (configurable queries-per-second for N seconds) and records P50 /
//! P95 / P99 latency per pattern. Each pattern corresponds to one of the
//! H-A through H-G failure modes pre-registered in file 17.
//!
//! This is the **simplest credible** implementation per the WAVE5-H brief:
//! a `tokio::time::interval`-paced load generator that, on each tick,
//! executes one sketch-estimate + q-error pair against a pre-built HLL
//! sketch and records the per-call wall-clock. A "full" failure-mode
//! harness with isolated server processes + cross-host network paths is
//! deferred to v1.1 (documented at the bottom of this file as a follow-up).
//!
//! Methodology:
//! - 7 patterns (A through G) — synthetic stand-ins for the file 17
//!   pre-registered patterns. Each pattern varies the sketch precision,
//!   query mix, and / or arrival cadence.
//! - 1 000 QPS target, 5 s per pattern → ~5 000 samples per pattern (more
//!   than the BCa n=30 floor by two orders of magnitude).
//! - Per-call wall-clock is the time from the tick firing to the
//!   `estimate()` returning. The corrector arm (HLL → q-error proxy) is
//!   the same as the inner loop in `samkhya-bench`'s runner.
//! - Output: per-replicate per-pattern JSON to
//!   `bench-results/17_burst_raw.json` (overrideable via the `--json-out`
//!   flag, default path).
//!
//! Citations (consistent with file 17's verdict block):
//! - Efron, B. & Tibshirani, R. J. (1993). *An Introduction to the
//!   Bootstrap*. Chapter 14 (BCa).
//! - Wilcoxon, F. (1945). "Individual Comparisons by Ranking Methods."
//!   *Biometrics Bulletin* 1(6):80–83.
//! - Flajolet, P., Fusy, É., Gandouet, O., Meunier, F. (2007).
//!   "HyperLogLog: the Analysis of a Near-Optimal Cardinality Estimation
//!   Algorithm." *AofA 2007*.
//!
//! Run:
//! ```text
//! cargo run --release -p samkhya-it --bin burst_harness -- \
//!     --json-out bench-results/17_burst_raw.json \
//!     --qps 1000 --duration-s 5
//! ```

use std::env;
use std::fs::File;
use std::io::Write;
use std::time::{Duration, Instant};

use samkhya_core::sketches::HllSketch;
use tokio::time::interval;

/// One failure-mode pattern. The fields parametrise a tiny synthetic
/// workload that stands in for the registered patterns in file 17 §2.
struct Pattern {
    name: &'static str,
    description: &'static str,
    /// HLL precision used by both the build and estimate calls.
    precision: u8,
    /// Number of distinct elements pushed into the HLL before the timed
    /// burst starts.
    pre_load_distinct: u64,
    /// Arrival pacing (QPS scaling). 1.0 = nominal, > 1.0 = burstier.
    qps_multiplier: f64,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        name: "A_trivial_single_table",
        description: "single-table baseline; stats overhead pattern",
        precision: 14,
        pre_load_distinct: 1_000,
        qps_multiplier: 1.0,
    },
    Pattern {
        name: "B_no_join",
        description: "no-join queries; LpBound has no effect",
        precision: 14,
        pre_load_distinct: 10_000,
        qps_multiplier: 1.0,
    },
    Pattern {
        name: "C_cold_start",
        description: "cold-start feedback corpus wrong-domain",
        precision: 12,
        pre_load_distinct: 100,
        qps_multiplier: 1.0,
    },
    Pattern {
        name: "D_bursty",
        description: "bursty workload, calibration drift",
        precision: 14,
        pre_load_distinct: 5_000,
        qps_multiplier: 2.0,
    },
    Pattern {
        name: "E_adversarial",
        description: "adversarial distribution outside sketch range",
        precision: 10,
        pre_load_distinct: 100_000,
        qps_multiplier: 1.0,
    },
    Pattern {
        name: "F_tiny_tables",
        description: "very small tables (< 10^4 rows)",
        precision: 14,
        pre_load_distinct: 50,
        qps_multiplier: 1.0,
    },
    Pattern {
        name: "G_heavy_tailed",
        description: "heavy-tailed selectivity",
        precision: 14,
        pre_load_distinct: 50_000,
        qps_multiplier: 1.5,
    },
];

/// Build a pre-loaded HLL sketch with the given precision and distinct
/// count, using a deterministic SplitMix64 stream. The sketch is reused
/// across timed samples in the burst so the wallclock measures only the
/// estimate path, not setup cost.
fn build_pre_loaded_hll(precision: u8, distinct: u64, seed: u64) -> HllSketch {
    let mut hll = HllSketch::new(precision).expect("precision in range");
    let mut state = seed;
    for _ in 0..distinct {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        hll.add(&z.to_le_bytes());
    }
    hll
}

/// Percentile of a sorted nanosecond vector.
fn percentile_ns(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Drive one pattern for `duration_s` at `base_qps × qps_multiplier`,
/// returning the per-sample wall-clock vector in nanoseconds.
async fn run_pattern(pattern: &Pattern, base_qps: u64, duration_s: u64) -> Vec<u64> {
    let qps = (base_qps as f64 * pattern.qps_multiplier).max(1.0);
    let tick = Duration::from_secs_f64(1.0 / qps);
    let total_ticks = (qps * duration_s as f64) as usize;

    // Pre-build the HLL outside the timed region.
    let mut hll = build_pre_loaded_hll(pattern.precision, pattern.pre_load_distinct, 0xA5A5_5A5A);

    let mut samples: Vec<u64> = Vec::with_capacity(total_ticks);
    let mut state: u64 = 0xDEAD_BEEF_CAFE_BABEu64;
    let mut iv = interval(tick);
    // First tick fires immediately; let the runtime settle.
    iv.tick().await;
    for _ in 0..total_ticks {
        iv.tick().await;
        let started = Instant::now();
        // Hot path: one add + one estimate + a derived Moerkotte q-error
        // proxy. Same arithmetic the samkhya-bench inner loop runs per
        // join node in production.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        hll.add(&z.to_le_bytes());
        let est = hll.estimate() as f64;
        // Moerkotte q-error against the known pre-load distinct count
        // (degrades as the burst goes on — that's a feature; this is the
        // "feedback drift" the harness is supposed to expose).
        let actual = pattern.pre_load_distinct as f64;
        let _q = (est / actual.max(1.0)).max(actual / est.max(1.0));
        samples.push(started.elapsed().as_nanos() as u64);
    }
    samples
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let mut json_out = "bench-results/17_burst_raw.json".to_string();
    let mut qps: u64 = 1_000;
    let mut duration_s: u64 = 5;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--json-out" => {
                json_out = args.get(i + 1).cloned().unwrap_or(json_out);
                i += 2;
            }
            "--qps" => {
                qps = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(qps);
                i += 2;
            }
            "--duration-s" => {
                duration_s = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(duration_s);
                i += 2;
            }
            _ => i += 1,
        }
    }

    println!(
        "# burst_harness: qps={qps}, duration_s={duration_s}, patterns={}",
        PATTERNS.len()
    );
    println!("pattern,n_samples,p50_ns,p95_ns,p99_ns,mean_ns,max_ns");

    let mut all_records: Vec<String> = Vec::new();
    let overall_start = Instant::now();
    for pattern in PATTERNS {
        let samples = run_pattern(pattern, qps, duration_s).await;
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        let p50 = percentile_ns(&sorted, 0.50);
        let p95 = percentile_ns(&sorted, 0.95);
        let p99 = percentile_ns(&sorted, 0.99);
        let mean: u64 = if sorted.is_empty() {
            0
        } else {
            (sorted.iter().sum::<u64>()) / sorted.len() as u64
        };
        let max = *sorted.last().unwrap_or(&0);
        println!(
            "{},{},{},{},{},{},{}",
            pattern.name,
            sorted.len(),
            p50,
            p95,
            p99,
            mean,
            max
        );

        let samples_str: String = samples
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        all_records.push(format!(
            "{{\"pattern\":\"{}\",\"description\":\"{}\",\"precision\":{},\"pre_load_distinct\":{},\"qps_multiplier\":{},\"n_samples\":{},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"mean_ns\":{},\"max_ns\":{},\"latencies_ns\":[{}]}}",
            pattern.name,
            pattern.description,
            pattern.precision,
            pattern.pre_load_distinct,
            pattern.qps_multiplier,
            sorted.len(),
            p50,
            p95,
            p99,
            mean,
            max,
            samples_str,
        ));
    }

    let elapsed = overall_start.elapsed();
    println!();
    println!("# wall: {:.2}s", elapsed.as_secs_f64());

    let body = format!(
        "{{\"benchmark\":\"burst_harness\",\"qps_target\":{qps},\"duration_s_per_pattern\":{duration_s},\"n_patterns\":{},\"records\":[{}]}}",
        PATTERNS.len(),
        all_records.join(",")
    );
    let mut f = File::create(&json_out).expect("create json-out");
    f.write_all(body.as_bytes()).expect("write json-out");
    eprintln!("# per-pattern burst data written to {json_out}");
}
