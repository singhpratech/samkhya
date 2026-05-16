//! Calibrate subcommand — close the feedback loop end-to-end.
//!
//! The calibration loop is three phases:
//!
//! 1. Run the suite once in samkhya-corrected mode (rich stats wired
//!    through `SamkhyaTableProvider`) and record one observation per
//!    successfully executed query into a feedback store.
//! 2. Pull the recorded observations back out via `FeedbackStore::history`
//!    and train a `GbtCorrector` on them.
//! 3. Re-run the same suite, this time threading the trained corrector
//!    through `Runner::run_with_corrector`. Print a per-query before /
//!    after comparison plus a roll-up summary.
//!
//! The corrector is purely residual: it observes the raw DataFusion
//! row-count estimate and emits a corrected estimate clamped by the
//! LpBound ceiling baked into `GbtOptions`. At correction time the
//! bench now walks the physical plan via `runner::extract_features` and
//! populates `join_depth`, `predicate_count`, and outermost-join input
//! row / distinct counts on `CorrectionFeatures`. This gives the GBT
//! plan-shape signal even when DataFusion's baseline row estimate
//! collapses to zero (multi-join queries on `MemTable` sources).
//!
//! Caveat: the on-disk `FeedbackStore` only persists
//! `(template_hash, plan_fingerprint, est_rows, actual_rows, latency_ms)`
//! — see `Observation` in samkhya-core. Training therefore only sees
//! `baseline_estimate` (mapped from `est_rows`); the other feature
//! slots are zero-filled. Prediction, however, gets all features
//! populated. The training/prediction feature-space mismatch is a known
//! limitation: until `Observation` grows plan-shape columns the GBT
//! learns a one-variable function and applies it to a seven-variable
//! feature vector — leaf splits on the unseen features can only fire
//! by accident. Honesty over hype: this lets the corrector at least
//! emit a non-zero log-ratio for `baseline_estimate == 0` rows.

use std::path::Path;

use samkhya_core::Result;
use samkhya_core::feedback::FeedbackStore;
use samkhya_core::residual::gbt::{GbtCorrector, GbtOptions};

use crate::queries::Suite;
use crate::runner::Runner;

/// Run the full collect-train-correct loop for `suite`.
///
/// If `feedback_path` is `Some`, observations are persisted to a SQLite
/// file at that path; otherwise an in-memory store is used and discarded
/// at the end of the run.
///
/// When `puffin_dir` is supplied, every Runner constructed in the loop
/// (phase 1 collect, phase 3 corrected re-run, and the in-memory
/// recollect helper) sources its `ColumnStats` overrides from Puffin
/// sidecars in that directory instead of the hardcoded distinct-count
/// table.
///
/// `imdb_dir` and `tpch_dir` are forwarded to every constructed
/// [`Runner`] so suites that require a real on-disk dataset
/// (JOB-Slow against `--imdb-dir`, TPC-H against `--tpch-dir`) can
/// participate in calibration just like the synthetic suite. They are
/// mutually exclusive at the CLI layer; this function accepts both so
/// it can stay generic across suites — the caller is expected to enforce
/// exclusivity.
pub fn calibrate(
    suite: Suite,
    feedback_path: Option<&Path>,
    puffin_dir: Option<&Path>,
    imdb_dir: Option<&Path>,
    tpch_dir: Option<&Path>,
) -> Result<()> {
    println!("=== phase 1: collect observations ===");
    let mut runner = Runner::new(suite, false);
    if let Some(p) = feedback_path {
        runner = runner.with_feedback_path(p);
    }
    if let Some(dir) = puffin_dir {
        runner = runner.with_puffin_dir(dir.to_path_buf());
    }
    if let Some(dir) = imdb_dir {
        runner = runner.with_imdb_dir(dir.to_path_buf());
    }
    if let Some(dir) = tpch_dir {
        runner = runner.with_tpch_dir(dir.to_path_buf());
    }
    runner.run()?;

    // The suite must be runnable end-to-end for calibration to have
    // observations to train on. `Synthetic` is unconditionally
    // executable; `JobSlowReal` requires `--imdb-dir`; `TpcH` requires
    // `--tpch-dir`. Anything else is a scaffolding suite that prints
    // a skip notice from `runner.run()` and exits cleanly here.
    let suite_runnable = suite.is_executable()
        || (suite.is_executable_with_imdb_dir() && imdb_dir.is_some())
        || (suite.is_executable_with_tpch_dir() && tpch_dir.is_some());
    if !suite_runnable {
        return Ok(());
    }

    println!();
    println!("=== phase 2: train residual corrector ===");
    let store = match feedback_path {
        Some(p) => FeedbackStore::open(p)?,
        None => {
            // The phase-1 runner used its own in-memory store, which has
            // already been dropped. Re-run silently against a shared
            // store so the corrector has data to train on.
            let store = FeedbackStore::open_in_memory()?;
            recollect_into(&store, suite, puffin_dir, imdb_dir, tpch_dir)?;
            store
        }
    };

    let template = format!("samkhya-bench-{}", suite.label());
    let history = store.history(&template)?;
    println!(
        "loaded {} observations for template '{}'",
        history.len(),
        template
    );
    if history.is_empty() {
        println!("no observations to train on; aborting calibration");
        return Ok(());
    }

    let corrector = GbtCorrector::train(&history, GbtOptions::default())?;
    println!("trained GbtCorrector (default GbtOptions)");

    println!();
    println!("=== phase 3: re-run with correction applied ===");
    let outcomes = runner.run_with_corrector(&corrector)?;

    println!(
        "{:<6} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "query", "raw_est", "corrected", "actual", "qerr_before", "qerr_after"
    );
    println!("{}", "-".repeat(72));
    let mut sum_before = 0.0f64;
    let mut sum_after = 0.0f64;
    let mut count_finite = 0u64;
    let mut count_improved = 0u64;
    for o in &outcomes {
        println!(
            "{:<6} {:>12} {:>12} {:>12} {:>12.2} {:>12.2}",
            o.name,
            o.raw_estimate,
            o.corrected_estimate,
            o.actual_rows,
            o.q_error_raw,
            o.q_error_corrected,
        );
        if o.q_error_raw.is_finite() && o.q_error_corrected.is_finite() {
            sum_before += o.q_error_raw;
            sum_after += o.q_error_corrected;
            count_finite += 1;
            if o.q_error_corrected < o.q_error_raw {
                count_improved += 1;
            }
        }
    }

    println!();
    if count_finite > 0 {
        let avg_before = sum_before / count_finite as f64;
        let avg_after = sum_after / count_finite as f64;
        println!(
            "avg q-error before: {:.2}, avg q-error after: {:.2}",
            avg_before, avg_after
        );
        println!("queries improved: {}/{}", count_improved, outcomes.len());
    } else {
        println!("no finite q-error samples to summarize");
    }
    Ok(())
}

/// Helper used only on the in-memory path: re-execute the suite to
/// repopulate a freshly-opened feedback store. We run the suite once
/// per phase already, so the cost is a single extra pass and keeps the
/// CLI surface honest (one call → one full calibration loop).
fn recollect_into(
    store: &FeedbackStore,
    suite: Suite,
    puffin_dir: Option<&Path>,
    imdb_dir: Option<&Path>,
    tpch_dir: Option<&Path>,
) -> Result<()> {
    use samkhya_core::feedback::Observation;

    // We can't share the in-memory store directly with Runner::run
    // because Runner owns its store. Instead, run the corrector-less
    // path via run_with_corrector + IdentityCorrector to capture
    // outcomes, then record them here.
    let mut runner = Runner::new(suite, false);
    if let Some(dir) = puffin_dir {
        runner = runner.with_puffin_dir(dir.to_path_buf());
    }
    if let Some(dir) = imdb_dir {
        runner = runner.with_imdb_dir(dir.to_path_buf());
    }
    if let Some(dir) = tpch_dir {
        runner = runner.with_tpch_dir(dir.to_path_buf());
    }
    let identity = samkhya_core::residual::IdentityCorrector;
    let outcomes = runner.run_with_corrector(&identity)?;
    let template = format!("samkhya-bench-{}", suite.label());
    for o in outcomes {
        store.record(&Observation {
            template_hash: template.clone(),
            plan_fingerprint: o.name.to_string(),
            est_rows: o.raw_estimate,
            actual_rows: o.actual_rows,
            latency_ms: Some(o.latency_ms),
        })?;
    }
    Ok(())
}
