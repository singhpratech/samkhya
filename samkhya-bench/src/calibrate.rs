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
//! LpBound ceiling baked into `GbtOptions`. Other `CorrectionFeatures`
//! slots stay at their `None` / 0 defaults — that's an honest reflection
//! of what the bench currently observes from the plan.

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
pub fn calibrate(suite: Suite, feedback_path: Option<&Path>) -> Result<()> {
    println!("=== phase 1: collect observations ===");
    let runner = match feedback_path {
        Some(p) => Runner::new(suite, false).with_feedback_path(p),
        None => Runner::new(suite, false),
    };
    runner.run()?;

    if !suite.is_executable() {
        // Nothing to train against — `runner.run()` already printed a
        // skip notice; bail out gracefully so the caller's exit code is
        // success rather than spurious failure.
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
            recollect_into(&store, suite)?;
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
fn recollect_into(store: &FeedbackStore, suite: Suite) -> Result<()> {
    use samkhya_core::feedback::Observation;

    // We can't share the in-memory store directly with Runner::run
    // because Runner owns its store. Instead, run the corrector-less
    // path via run_with_corrector + IdentityCorrector to capture
    // outcomes, then record them here.
    let runner = Runner::new(suite, false);
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
