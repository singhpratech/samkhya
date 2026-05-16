//! Report subcommand — summarize a feedback store and (optionally)
//! train a residual corrector from its history.

use std::path::Path;

use samkhya_core::feedback::FeedbackStore;
use samkhya_core::{Error, Result};

use crate::queries::Suite;
use crate::runner::Runner;

/// Print a per-template summary of all observations in the store.
pub fn summarize(path: &Path) -> Result<()> {
    let store = FeedbackStore::open(path)?;
    let total = store.count()?;
    println!(
        "feedback store: {} ({} observations)",
        path.display(),
        total
    );
    if total == 0 {
        return Ok(());
    }
    // We don't currently track distinct templates in a single query, so we
    // fold over a few well-known suite names. Future versions of the store
    // should expose `SELECT DISTINCT template_hash` directly.
    for template in [
        "samkhya-bench-synthetic",
        "samkhya-bench-job-slow",
        "samkhya-bench-tpc-h",
        "samkhya-bench-stats-ceb",
    ] {
        let history = store.history(template)?;
        if history.is_empty() {
            continue;
        }
        println!();
        println!("template: {} ({} observations)", template, history.len());
        println!(
            "{:<6} {:>12} {:>12} {:>10} {:>10}",
            "#", "est_rows", "actual_rows", "q-error", "ms"
        );
        println!("{}", "-".repeat(56));
        for (i, obs) in history.iter().enumerate() {
            println!(
                "{:<6} {:>12} {:>12} {:>10.2} {:>10.2}",
                i,
                obs.est_rows,
                obs.actual_rows,
                obs.q_error(),
                obs.latency_ms.unwrap_or(0.0),
            );
        }
        let finite_q: Vec<f64> = history
            .iter()
            .map(|o| o.q_error())
            .filter(|q| q.is_finite())
            .collect();
        if !finite_q.is_empty() {
            let avg = finite_q.iter().sum::<f64>() / finite_q.len() as f64;
            let max = finite_q.iter().fold(0f64, |acc, &q| acc.max(q));
            println!("avg q-error: {avg:.2}, max q-error: {max:.2}");
        }
    }
    Ok(())
}

/// Run a suite twice (baseline + samkhya) and print a side-by-side
/// comparison of each query's estimate, actual rows, and q-error.
///
/// Both runs use in-memory feedback stores; nothing is persisted.
pub fn compare(suite: Suite) -> Result<()> {
    println!("=== baseline (raw MemTable) ===");
    Runner::new(suite, true).run()?;
    println!();
    println!("=== samkhya-wrapped (SamkhyaTableProvider) ===");
    Runner::new(suite, false).run()?;
    Ok(())
}

/// Train placeholder — the actual GbtCorrector training lives behind the
/// `gbt` feature on samkhya-core and is exercised from the binary only
/// when that feature is enabled in a downstream build. The CLI surface
/// is exposed regardless so we can wire training in once the binary is
/// rebuilt against `samkhya-core --features gbt`.
pub fn train_stub(feedback_path: &Path, template: &str) -> Result<()> {
    let store = FeedbackStore::open(feedback_path)?;
    let history = store.history(template)?;
    if history.is_empty() {
        return Err(Error::Feedback(format!(
            "no observations found for template {template} in {}",
            feedback_path.display()
        )));
    }
    println!(
        "train: would train a residual corrector on {} observations for template '{}'",
        history.len(),
        template
    );
    println!(
        "note: enable samkhya-core's `gbt` feature and link a custom binary to run actual training"
    );
    Ok(())
}
