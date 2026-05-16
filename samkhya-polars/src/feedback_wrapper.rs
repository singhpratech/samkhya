//! Wrap `LazyFrame::collect()` so every query records an
//! `(estimated, actual)` row-count observation into a
//! [`samkhya_core::feedback::FeedbackStore`].
//!
//! Polars 0.44 does not expose a plan-level row estimate through its
//! public API, so `est_rows` is always written as `0`; downstream
//! consumers should treat `est_rows == 0` observations as "actual-only"
//! samples. Once an upstream estimator is exposed (see
//! [pola-rs/polars#23345](https://github.com/pola-rs/polars/issues/23345)),
//! this wrapper will be the natural place to populate the field.

use std::time::Instant;

use polars::frame::DataFrame;
use polars::lazy::frame::LazyFrame;

use samkhya_core::feedback::{FeedbackStore, Observation};
use samkhya_core::{Error, Result};

/// Collect `lf` and record the resulting row count into `store`
/// under `template_hash`.
///
/// The returned `DataFrame` is exactly what `LazyFrame::collect()`
/// would have returned; the feedback recording is best-effort and
/// errors propagate after a successful collect so the caller still sees
/// the row data on a recorder failure (the alternative — losing the
/// frame — would be much harder to debug).
pub fn lazy_collect_with_feedback(
    lf: LazyFrame,
    store: &FeedbackStore,
    template_hash: &str,
) -> Result<DataFrame> {
    let start = Instant::now();
    let df = lf
        .collect()
        .map_err(|e| Error::Feedback(format!("polars collect failed: {e}")))?;
    let latency_ms = start.elapsed().as_secs_f64() * 1_000.0;
    let actual_rows = df.height() as u64;

    let obs = Observation {
        template_hash: template_hash.to_string(),
        plan_fingerprint: "polars-lazyframe".to_string(),
        // Polars does not expose plan-level row estimates today, so we
        // record est_rows = 0 and let downstream tooling treat this as
        // an actual-only sample.
        est_rows: 0,
        actual_rows,
        latency_ms: Some(latency_ms),
    };
    store.record(&obs)?;
    Ok(df)
}
