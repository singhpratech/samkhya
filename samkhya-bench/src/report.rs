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
/// When `puffin_dir` is supplied, the samkhya pass loads its
/// `ColumnStats` overrides from Puffin sidecars in that directory.
/// The baseline pass ignores it (the baseline path never wraps tables).
///
/// Both runs use in-memory feedback stores; nothing is persisted.
pub fn compare(suite: Suite, puffin_dir: Option<&Path>) -> Result<()> {
    println!("=== baseline (raw MemTable) ===");
    Runner::new(suite, true).run()?;
    println!();
    println!("=== samkhya-wrapped (SamkhyaTableProvider) ===");
    let mut samkhya_runner = Runner::new(suite, false);
    if let Some(dir) = puffin_dir {
        samkhya_runner = samkhya_runner.with_puffin_dir(dir.to_path_buf());
    }
    samkhya_runner.run()?;
    Ok(())
}

/// Train placeholder — the actual GbtCorrector training lives behind the
/// `gbt` feature on samkhya-core and is exercised from the binary only
/// when that feature is enabled in a downstream build. The CLI surface
/// is exposed regardless so we can wire training in once the binary is
/// rebuilt against `samkhya-core --features gbt`.
/// Train a GBT residual corrector from a feedback store and persist it.
///
/// Trains only on observations that carry plan features
/// (`FeedbackStore::plan_history`). Rows recorded without them are skipped
/// rather than padded with zeros: a model fitted on a feature space the
/// adapter does not reproduce at inference time is blind to everything but
/// the baseline estimate, which is the defect this path exists to avoid.
///
/// The model is written to `out` so evaluation can run in a separate
/// process against a frozen model. That separation is what makes a
/// held-out measurement honest — a model frozen before the evaluation
/// queries ran cannot have seen them.
pub fn train(
    feedback_path: &Path,
    template: &str,
    out: &Path,
    options: samkhya_core::residual::gbt::GbtOptions,
) -> Result<()> {
    use samkhya_core::residual::gbt::GbtCorrector;

    let store = FeedbackStore::open(feedback_path)?;
    let trainable = store.plan_history(template)?;

    if trainable.is_empty() {
        let legacy = store.history(template)?;
        return Err(Error::Feedback(format!(
            "no trainable observations for template '{template}' in {}: found {} row(s), \
             none carrying plan features. Re-run the suite with --feedback against a \
             binary at 1.2.0 or later so features are recorded.",
            feedback_path.display(),
            legacy.len()
        )));
    }

    let usable = trainable
        .iter()
        .filter(|o| o.features.baseline_estimate > 0 && o.actual_rows > 0)
        .count();
    println!(
        "train: {} observation(s) for template '{}', {} usable (non-zero baseline and actual)",
        trainable.len(),
        template,
        usable
    );

    let corrector = GbtCorrector::train_on_plans(&trainable, options)?;
    corrector.save(out)?;
    println!(
        "train: fitted on {} row(s); model written to {}",
        corrector.training_rows(),
        out.display()
    );
    Ok(())
}
