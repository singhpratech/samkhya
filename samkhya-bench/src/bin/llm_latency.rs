//! llm_latency — MEASURED end-to-end latency for the `llm_http`
//! LLM-pluggable corrector backend
//! (`samkhya-core::residual::llm::LlmHttpCorrector`).
//!
//! Pre-registered hypotheses (see `bench-results/19_llm_corrector.md` §1):
//! - **H1-A**: Dummy-backend (transport-floor) P95 < 5 ms on loopback.
//! - **H1-B**: Anthropic Claude / OpenAI end-to-end P95 < 2 s at B=8
//!   (pending API key approval — measured only when keys are present).
//! - **H1-D**: Local Ollama backend honors the default wire contract
//!   (pending ollama availability — measured only when reachable).
//!
//! Wire contract (matches `samkhya-core/src/residual.rs::llm`):
//!
//! ```text
//! POST /infer  Content-Type: application/json
//! { "features": [<f64>, ...], "baseline_estimate": <u64> }
//! → 200 OK
//! { "estimate": <u64> }
//! ```
//!
//! Output: a single JSON object written to ``--json-out`` carrying one
//! record per batch size, each with the raw per-trial latency vector
//! (microseconds, u64). The output schema mirrors `tabpfn_latency`'s
//! exactly (`samkhya.bench.llm_latency.v1` vs
//! `samkhya.bench.tabpfn_latency.v1`) so the same downstream aggregator
//! (`bench-results/scripts/bootstrap_ci.py`) computes BCa CIs without
//! modification.
//!
//! Citations:
//! - Efron & Tibshirani 1993, *An Introduction to the Bootstrap*,
//!   Chapter 14 (BCa) — applied downstream to these per-trial vectors.
//! - Wilcoxon 1945, "Individual Comparisons by Ranking Methods",
//!   *Biometrics Bulletin* 1(6):80–83 — applied downstream to paired
//!   q-error deltas in file 19 §5.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Serialize;

const FEATURE_LEN: usize = 7;

#[derive(Parser, Debug)]
#[command(
    name = "llm_latency",
    about = "Measure HTTP round-trip latency to an LLM-pluggable corrector inference server"
)]
struct Args {
    /// Comma-separated list of batch sizes (one /infer call per batch).
    #[arg(long, default_value = "1,4,8,16,32")]
    batch_sizes: String,

    /// Sequence length L: retained for API parity with `tabpfn_latency`.
    /// LLM backends do not consume this directly — the wire payload is
    /// `(features, baseline_estimate)` only — but the value is recorded
    /// in the output for reproducibility.
    #[arg(long, default_value_t = 128)]
    seq_len: usize,

    /// Per-batch trials (after warm-up).
    #[arg(long, default_value_t = 30)]
    trials: usize,

    /// Warm-up trials per batch (discarded).
    #[arg(long, default_value_t = 5)]
    warmup: usize,

    /// Inference server base URL.
    #[arg(long, default_value = "http://127.0.0.1:8766/infer")]
    url: String,

    /// Per-request HTTP timeout (ms). LLMs are 2–3 orders of magnitude
    /// slower than TabPFN; default 60 s here, matching the
    /// `LlmHttpCorrector::MAX_TIMEOUT_MS` ceiling for diagnostic runs.
    /// The production corrector defaults to 2 s.
    #[arg(long, default_value_t = 60_000u64)]
    timeout_ms: u64,

    /// Output JSON path.
    #[arg(long)]
    json_out: PathBuf,

    /// RNG seed for feature generation (default 42 per pre-registration).
    #[arg(long, default_value_t = 42u64)]
    seed: u64,

    /// LLM backend identifier (recorded in the output JSON only — the
    /// server-side backend is selected by `SAMKHYA_LLM_BACKEND` on the
    /// server process, not by this flag). Pass-through for the
    /// `run-llm-bench.sh` driver.
    #[arg(long, default_value = "dummy")]
    llm_backend: String,

    /// Convenience: when set, override `--url` to the dummy backend's
    /// default port (8766) and tag the run as a transport-floor probe.
    /// Equivalent to `--llm-backend dummy --url http://127.0.0.1:8766/infer`
    /// but explicit so the driver script can pass `--mock` and have the
    /// binary do the right thing.
    #[arg(long, default_value_t = false)]
    mock: bool,
}

#[derive(Serialize)]
struct InferRequest<'a> {
    features: &'a [f64],
    baseline_estimate: u64,
}

#[derive(Serialize)]
struct PerBatch {
    batch_size: usize,
    seq_len: usize,
    n_warmup: usize,
    n_trials: usize,
    latency_us: Vec<u64>,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    min_ms: f64,
    max_ms: f64,
    successful_trials: usize,
    failed_trials: usize,
}

#[derive(Serialize)]
struct Run {
    schema: &'static str,
    url: String,
    llm_backend: String,
    mock: bool,
    seq_len: usize,
    seed: u64,
    timeout_ms: u64,
    trials_per_batch: usize,
    warmup_per_batch: usize,
    batches: Vec<PerBatch>,
    notes: &'static str,
}

fn percentile_sorted(xs: &[u64], p: f64) -> f64 {
    let n = xs.len();
    if n == 0 {
        return f64::NAN;
    }
    if n == 1 {
        return xs[0] as f64;
    }
    let pos = p * ((n - 1) as f64);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        xs[lo] as f64
    } else {
        let frac = pos - lo as f64;
        xs[lo] as f64 * (1.0 - frac) + xs[hi] as f64 * frac
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();
    if args.mock {
        // Pin the URL to the dummy backend's default port and tag the
        // backend label so the output JSON records "this was the
        // transport floor" unambiguously.
        args.url = "http://127.0.0.1:8766/infer".into();
        args.llm_backend = "dummy".into();
    }

    let batch_sizes: Vec<usize> = args
        .batch_sizes
        .split(',')
        .map(|s| s.trim().parse::<usize>().expect("batch size must be usize"))
        .collect();

    let timeout = Duration::from_millis(args.timeout_ms);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();

    let mut rng = StdRng::seed_from_u64(args.seed);
    let mut all_batches: Vec<PerBatch> = Vec::with_capacity(batch_sizes.len());

    for &b in &batch_sizes {
        let mut features: Vec<f64> = Vec::with_capacity(b * FEATURE_LEN);
        for _ in 0..(b * FEATURE_LEN) {
            // Match the tabpfn_latency feature distribution: uniform in
            // [-3, 3] approximates a Gaussian z-score without pulling
            // rand_distr. Determinism comes from StdRng+seed.
            let u1: f64 = rng.gen_range(-3.0..3.0);
            features.push(u1);
        }
        let payload = InferRequest {
            features: &features,
            baseline_estimate: 1_000_000,
        };

        // Warm-up
        let mut warm_fail = 0usize;
        for _ in 0..args.warmup {
            match agent.post(&args.url).send_json(&payload) {
                Ok(resp) => {
                    let _ = resp.into_string();
                }
                Err(_) => warm_fail += 1,
            }
        }

        // Timed trials.
        let mut us: Vec<u64> = Vec::with_capacity(args.trials);
        let mut ok = 0usize;
        let mut fail = 0usize;
        for _ in 0..args.trials {
            let t0 = Instant::now();
            match agent.post(&args.url).send_json(&payload) {
                Ok(resp) => {
                    let _ = resp.into_string();
                    let elapsed = t0.elapsed();
                    us.push(elapsed.as_micros() as u64);
                    ok += 1;
                }
                Err(_) => {
                    fail += 1;
                }
            }
        }
        let mut sorted = us.clone();
        sorted.sort_unstable();
        let p50 = percentile_sorted(&sorted, 0.50) / 1_000.0;
        let p95 = percentile_sorted(&sorted, 0.95) / 1_000.0;
        let p99 = percentile_sorted(&sorted, 0.99) / 1_000.0;
        let minv = sorted.first().copied().unwrap_or(0) as f64 / 1_000.0;
        let maxv = sorted.last().copied().unwrap_or(0) as f64 / 1_000.0;

        eprintln!(
            "B={:>3}  ok={}  fail={}  warm_fail={}  P50={:>7.2} ms  P95={:>7.2} ms  P99={:>7.2} ms",
            b, ok, fail, warm_fail, p50, p95, p99
        );

        all_batches.push(PerBatch {
            batch_size: b,
            seq_len: args.seq_len,
            n_warmup: args.warmup,
            n_trials: args.trials,
            latency_us: us,
            p50_ms: p50,
            p95_ms: p95,
            p99_ms: p99,
            min_ms: minv,
            max_ms: maxv,
            successful_trials: ok,
            failed_trials: fail,
        });
    }

    let run = Run {
        schema: "samkhya.bench.llm_latency.v1",
        url: args.url.clone(),
        llm_backend: args.llm_backend.clone(),
        mock: args.mock,
        seq_len: args.seq_len,
        seed: args.seed,
        timeout_ms: args.timeout_ms,
        trials_per_batch: args.trials,
        warmup_per_batch: args.warmup,
        batches: all_batches,
        notes: "Per-trial latencies in microseconds (u64). \
                Citations: Efron-Tibshirani 1993 ch.14 BCa; Wilcoxon 1945 \
                signed-rank for paired backend comparisons.",
    };
    if let Some(parent) = args.json_out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(&args.json_out)?;
    f.write_all(serde_json::to_string_pretty(&run)?.as_bytes())?;
    eprintln!("wrote {}", args.json_out.display());
    Ok(())
}
