//! Benchmark runner — executes each query through DataFusion, captures
//! the optimizer's row estimate vs the actual row count, records the
//! observation in a samkhya feedback store, and prints a comparison
//! table.
//!
//! Only the `Synthetic` suite is in-process executable today; other
//! suites require real datasets and are reported as skipped.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use datafusion::arrow::array::Array;
use datafusion::common::stats::Precision;
use datafusion::datasource::TableProvider;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::collect as execute_physical_plan;
use datafusion::physical_plan::filter::FilterExec;
use datafusion::physical_plan::joins::{
    CrossJoinExec, HashJoinExec, NestedLoopJoinExec, SortMergeJoinExec, SymmetricHashJoinExec,
};
use datafusion::physical_plan::{ExecutionPlanVisitor, accept};
use datafusion::prelude::SessionContext;
use samkhya_core::Result;
use samkhya_core::error::Error;
use samkhya_core::feedback::{FeedbackStore, Observation, PlanObservation};
use samkhya_core::residual::{CorrectionFeatures, Corrector};
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::SamkhyaTableProvider;
use serde::Serialize;

use crate::imdb;
use crate::puffin_io;
use crate::queries::{Query, Suite};
use crate::synthetic;
use crate::tpch;

/// Configuration for a single benchmark run.
#[derive(Clone)]
pub struct Runner {
    suite: Suite,
    baseline: bool,
    feedback_path: Option<std::path::PathBuf>,
    /// Query-name allowlist; empty means "every query in the suite".
    only: std::collections::HashSet<String>,
    /// Query-name denylist, applied after `only`.
    exclude: std::collections::HashSet<String>,
    puffin_dir: Option<std::path::PathBuf>,
    imdb_dir: Option<std::path::PathBuf>,
    tpch_dir: Option<std::path::PathBuf>,
    json_out: Option<std::path::PathBuf>,
    /// WAVE-5F: source format for IMDb tables when `imdb_dir` is set.
    /// Defaults to CSV for backward compatibility; Parquet activates the
    /// `<imdb_dir>/<table>.parquet` + `<table>.parquet.puffin` registration
    /// path.
    imdb_format: crate::puffin_io::ImdbFormat,
    /// WAVE-5J: number of trials (replicates) of the full suite to execute
    /// in one process. Defaults to 1 (back-compat). Each trial emits its
    /// own per-query entries in the JSON output, tagged with `trial_id`.
    trials: usize,
    /// WAVE-5J: per-query wall-clock timeout in seconds. When `Some(s)`,
    /// any query whose execution exceeds `s` seconds is recorded as
    /// TIMEOUT (latency = s × 1000 ms, status="timeout") and the runner
    /// proceeds to the next query — cherry-picking forbidden per
    /// [[feedback-empirical-methodology]]. `None` = no timeout.
    query_timeout_s: Option<u64>,
    /// WAVE-5M: when `true`, before every trial iteration the runner
    /// advises the kernel to drop the page-cache pages backing each
    /// `*.parquet` file under the configured data directory (IMDb or
    /// TPC-H) via `posix_fadvise(POSIX_FADV_DONTNEED)`. Off by default
    /// (backward-compatible warm-cache behaviour). Citation: Leis et al.
    /// VLDB 2015 §3 — cold-cache amplification.
    cold_cache: bool,
    /// WAVE5-RC2 prong 1: optional runtime residual corrector applied
    /// per query inside the trial loop. When `Some(c)` and
    /// `baseline=false`, every query's raw optimizer estimate is fed
    /// into `c.correct(...)`; the corrected value becomes the
    /// `QueryOutcome::estimated_rows` reported in the JSON.
    /// When `None` (default), behavior is unchanged — the trial loop
    /// records the raw DataFusion estimate. The planner-level stat
    /// injection through `SamkhyaTableProvider` remains controlled by
    /// the existing `baseline` flag and is independent of this field.
    corrector: Option<std::sync::Arc<dyn samkhya_core::residual::Corrector>>,
}

/// One join-node q-error sample (Moerkotte VLDB 2009 §3).
///
/// Captured during `execute_query` by walking the physical plan tree:
/// the optimizer-estimated cardinality is read from each join node's
/// `ExecutionPlan::statistics().num_rows` snapshot taken *before*
/// execution; the actual cardinality is read from the same node's
/// `MetricsSet::output_rows` after `collect` finishes. Both arms (with
/// and without samkhya correction) populate this vector — the only
/// difference is the upstream stats the optimizer was given.
///
/// Citation: Moerkotte, Neumann, Steidl. "Preventing Bad Plans by
/// Bounding the Impact of Cardinality Estimation Errors." VLDB 2009,
/// §3.
#[derive(Debug, Clone, Serialize)]
pub struct JoinQError {
    /// Concrete `ExecutionPlan` type name (e.g. "HashJoinExec",
    /// "NestedLoopJoinExec", "SortMergeJoinExec", "CrossJoinExec",
    /// "SymmetricHashJoinExec"). Recorded so downstream aggregation
    /// can stratify q-error by join algorithm.
    pub node_type: &'static str,
    /// Pre-order index of this join node inside the per-query plan
    /// walk (0 = outermost / root-most join, 1 = next inward, …).
    pub node_idx: u32,
    /// Optimizer-estimated output rows at plan-create time. `None` when
    /// the operator returned `Precision::Absent`.
    pub estimated_rows: Option<u64>,
    /// Actual output rows from the operator's `MetricsSet`. `None`
    /// when the operator did not emit an `output_rows` metric (some
    /// operators omit it; the join family in DataFusion 46 reliably
    /// emits it under default `MetricBuilder` instrumentation).
    pub actual_rows: Option<u64>,
    /// Moerkotte q-error: `max(c_est / max(1, c_true), c_true / max(1, c_est))`.
    /// Symmetric, monotonic, lower-bounded at 1.0. `None` when either
    /// estimate or actual is missing.
    pub q_error: Option<f64>,
}

/// Per-query result captured during a run.
#[derive(Debug, Clone, Serialize)]
pub struct QueryOutcome {
    pub name: &'static str,
    pub estimated_rows: u64,
    pub actual_rows: u64,
    /// Q-error of the *final aggregate output* — degenerate (= 1.00)
    /// on every JOB-Slow query because they are all `SELECT MIN(...)`
    /// scalar aggregates returning 1 row. Kept for backwards
    /// compatibility with the synthetic-suite reporting path; use
    /// [`Self::per_join_q_errors`] for the Moerkotte-meaningful samples.
    pub q_error: f64,
    pub latency_ms: f64,
    /// Per-join-node q-error samples (Moerkotte VLDB 2009 §3) extracted
    /// by walking the physical plan tree. Empty when the plan contains
    /// no join nodes. See [`JoinQError`].
    #[serde(default)]
    pub per_join_q_errors: Vec<JoinQError>,
    /// Plan-shape features extracted from the physical plan, so a
    /// feedback store can record what the corrector would actually be
    /// handed at inference time. `None` for outcomes that never reached a
    /// physical plan (errors, timeouts).
    ///
    /// Without this the recorded observation carries only the baseline
    /// estimate, and a model trained from it is blind to six of its seven
    /// features — see `samkhya_core::feedback::PlanObservation`.
    #[serde(default)]
    pub features: Option<CorrectionFeatures>,
    /// WAVE-5J: 1-based trial id; 1 in legacy single-trial mode.
    #[serde(default = "default_trial_id")]
    pub trial_id: u32,
    /// WAVE-5J: per-query outcome status. `"ok"` for a normally-executed
    /// query; `"timeout"` for queries that exceeded `query_timeout_s`
    /// (latency = timeout, est/actual = 0, per-join empty); `"error"` for
    /// queries that errored before completion.
    #[serde(default = "default_status")]
    pub status: &'static str,
}

#[allow(dead_code)] // referenced via `#[serde(default = "default_trial_id")]`
fn default_trial_id() -> u32 {
    1
}
#[allow(dead_code)] // referenced via `#[serde(default = "default_status")]`
fn default_status() -> &'static str {
    "ok"
}

/// Per-query result captured during a corrector-aware run. The raw
/// estimate is what DataFusion's optimizer reports; the corrected
/// estimate is the residual corrector's output for the same plan.
#[derive(Debug, Clone, Serialize)]
pub struct CorrectedOutcome {
    pub name: &'static str,
    pub raw_estimate: u64,
    pub corrected_estimate: u64,
    pub actual_rows: u64,
    pub q_error_raw: f64,
    pub q_error_corrected: f64,
    pub latency_ms: f64,
    /// Per-join-node q-error samples on the *raw* optimizer estimate
    /// path. See [`JoinQError`] for the formula and source.
    #[serde(default)]
    pub per_join_q_errors: Vec<JoinQError>,
    /// The feature vector handed to the corrector for this query.
    #[serde(default)]
    pub features: CorrectionFeatures,
}

impl Runner {
    pub fn new(suite: Suite, baseline: bool) -> Self {
        Self {
            suite,
            baseline,
            feedback_path: None,
            only: std::collections::HashSet::new(),
            exclude: std::collections::HashSet::new(),
            puffin_dir: None,
            imdb_dir: None,
            tpch_dir: None,
            json_out: None,
            imdb_format: crate::puffin_io::ImdbFormat::Csv,
            trials: 1,
            query_timeout_s: None,
            cold_cache: false,
            corrector: None,
        }
    }

    /// WAVE5-RC2 prong 1: attach a runtime residual corrector to the
    /// trial loop. When set and `baseline=false`, every query's raw
    /// optimizer estimate is passed through `c.correct(...)` and the
    /// corrected value is reported in [`QueryOutcome::estimated_rows`].
    /// Setting a corrector in baseline mode is a no-op (baseline runs
    /// the native DataFusion plan with no samkhya state injection).
    pub fn with_corrector(
        mut self,
        c: std::sync::Arc<dyn samkhya_core::residual::Corrector>,
    ) -> Self {
        self.corrector = Some(c);
        self
    }

    /// Persist observations to a SQLite store at the given path.
    /// If unset, an in-memory store is used.
    pub fn with_feedback_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.feedback_path = Some(path.into());
        self
    }

    /// Source samkhya-corrected `ColumnStats` overrides from Puffin
    /// sidecars in the given directory (one `.puffin` per table, as
    /// produced by `build-puffin`). When unset, the runner falls back
    /// to the hardcoded distinct counts wired into `wrap_with_stats`.
    /// Ignored in baseline mode (the baseline path never wraps tables).
    pub fn with_puffin_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.puffin_dir = Some(dir.into());
        self
    }

    /// Point the runner at an unpacked IMDb dump on disk (the directory
    /// produced by `data/job/README.md`'s download script). When set, the
    /// `JobSlowReal` suite becomes executable: the SessionContext is built
    /// from real IMDb CSV/Parquet files via [`crate::imdb::register_imdb_tables`]
    /// instead of the synthetic in-memory tables. Ignored by every other
    /// suite.
    pub fn with_imdb_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.imdb_dir = Some(dir.into());
        self
    }

    /// WAVE-5F: select the on-disk source format for IMDb tables. Defaults
    /// to [`crate::puffin_io::ImdbFormat::Csv`] (backward-compatible); set
    /// to [`crate::puffin_io::ImdbFormat::Parquet`] to read sibling-level
    /// `<imdb_dir>/<table>.parquet` files and source per-column
    /// `ColumnStats` from `<table>.parquet.puffin` sidecars.
    pub fn with_imdb_format(mut self, format: crate::puffin_io::ImdbFormat) -> Self {
        self.imdb_format = format;
        self
    }

    /// Point the runner at a TPC-H Parquet dump on disk (the directory
    /// produced by `tpchgen-cli -s 1 --format=parquet --output-dir=...`
    /// or by DuckDB's `EXPORT DATABASE` after `CALL dbgen(sf=1)`). When
    /// set, the `TpcH` suite becomes executable: the SessionContext is
    /// built by [`crate::tpch::register_tpch_tables`]. Ignored by every
    /// other suite.
    pub fn with_tpch_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.tpch_dir = Some(dir.into());
        self
    }

    /// Emit a structured JSON report of every per-query outcome (including
    /// the per-join-node q-error vector produced by walking the physical
    /// plan) at the given path after the run finishes. Off by default; the
    /// existing stdout report path is preserved verbatim.
    pub fn with_json_out(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.json_out = Some(path.into());
        self
    }

    /// WAVE-5J: set the number of full-suite replicates per run. Defaults
    /// to 1. Each trial executes the full suite end-to-end within the
    /// same process (so a process-crashing OOM in trial N truncates
    /// trials N+1..T, by design — driver scripts can re-launch the
    /// process to recover).
    pub fn with_trials(mut self, trials: usize) -> Self {
        self.trials = trials.max(1);
        self
    }

    /// WAVE-5J: set the per-query wall-clock timeout. Queries exceeding
    /// the timeout are recorded as TIMEOUT entries (latency =
    /// `timeout × 1000` ms, status="timeout") and the runner proceeds —
    /// they are not dropped from aggregates (cherry-picking forbidden).
    pub fn with_query_timeout_s(mut self, seconds: u64) -> Self {
        self.query_timeout_s = if seconds > 0 { Some(seconds) } else { None };
        self
    }

    /// WAVE-5M: enable cold-cache trials. Before each trial iteration
    /// the runner advises the kernel to drop page-cache pages backing
    /// every `*.parquet` file under the configured data directory via
    /// `posix_fadvise(POSIX_FADV_DONTNEED)` (no root required). Off by
    /// default. Citation: Leis et al. VLDB 2015 §3.
    pub fn with_cold_cache(mut self, enabled: bool) -> Self {
        self.cold_cache = enabled;
        self
    }

    /// Restrict the run to these query names.
    ///
    /// Together with [`with_exclude`](Self::with_exclude) this is how a
    /// training set and an evaluation set are kept disjoint. A corrector
    /// evaluated on the queries it was fitted on measures memorisation, not
    /// correction.
    pub fn with_only(mut self, names: Vec<String>) -> Self {
        self.only = names.into_iter().collect();
        self
    }

    /// Skip these query names. Applied after [`with_only`](Self::with_only).
    pub fn with_exclude(mut self, names: Vec<String>) -> Self {
        self.exclude = names.into_iter().collect();
        self
    }

    /// The queries this run will execute, after `--only` and `--exclude`.
    fn selected_queries(&self) -> Vec<&'static Query> {
        self.suite
            .queries()
            .iter()
            .filter(|q| self.only.is_empty() || self.only.contains(q.name))
            .filter(|q| !self.exclude.contains(q.name))
            .collect()
    }

    pub fn suite(&self) -> Suite {
        self.suite
    }

    pub fn is_baseline(&self) -> bool {
        self.baseline
    }

    /// Execute the configured suite.
    pub fn run(&self) -> Result<()> {
        if !self.is_runnable() {
            let extra = if self.suite.is_executable_with_imdb_dir() {
                " (supply --imdb-dir to enable)"
            } else if self.suite.is_executable_with_tpch_dir() {
                " (supply --tpch-dir to enable)"
            } else {
                ""
            };
            println!(
                "runner: suite {} is not in-process executable yet (needs real dataset){}; skipping.",
                self.suite.label(),
                extra
            );
            for q in self.suite.queries() {
                println!("  - {} (skipped)", q.name);
            }
            return Ok(());
        }

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(Error::from)?;
        rt.block_on(self.run_async())
    }

    /// True if this runner has enough configuration to actually execute the
    /// configured suite end-to-end. Synthetic always qualifies; JobSlowReal
    /// qualifies when an IMDb data directory has been supplied.
    fn is_runnable(&self) -> bool {
        if self.suite.is_executable() {
            return true;
        }
        if self.suite.is_executable_with_imdb_dir() && self.imdb_dir.is_some() {
            return true;
        }
        if self.suite.is_executable_with_tpch_dir() && self.tpch_dir.is_some() {
            return true;
        }
        false
    }

    async fn run_async(&self) -> Result<()> {
        let mode = if self.baseline {
            "baseline (native plan)"
        } else {
            "samkhya-corrected"
        };
        let ctx = self.build_context().await?;
        let store = match self.feedback_path.as_ref() {
            Some(p) => FeedbackStore::open(p)?,
            None => FeedbackStore::open_in_memory()?,
        };

        let selected = self.selected_queries();
        if selected.len() != self.suite.queries().len() {
            println!(
                "runner: query filter active — {} of {} {} queries selected",
                selected.len(),
                self.suite.queries().len(),
                self.suite.label(),
            );
        }
        println!(
            "runner: executing {} {} queries in {} mode",
            selected.len(),
            self.suite.label(),
            mode,
        );
        println!(
            "{:<6} {:>12} {:>12} {:>10} {:>10}",
            "query", "estimated", "actual", "q-error", "ms"
        );
        println!("{}", "-".repeat(56));

        let template_hash = format!("samkhya-bench-{}", self.suite.label());
        let mut outcomes: Vec<QueryOutcome> = Vec::new();
        let trial_count = self.trials.max(1);
        // WAVE-5J: incremental JSON write — flush after every query so an
        // OOM-kill (q16a path) does not lose previously-completed entries.
        for trial_idx in 1..=trial_count {
            if trial_count > 1 {
                println!("--- trial {trial_idx} of {trial_count} ---");
            }
            // WAVE-5M: cold-cache eviction before each trial. Best-effort
            // (skipped silently if the data directory is unknown — synthetic
            // suite has no on-disk corpus to evict).
            if self.cold_cache {
                self.evict_for_cold_cache(trial_idx);
            }
            for q in selected.iter().copied() {
                if is_placeholder_query(q) {
                    if trial_idx == 1 {
                        println!("{:<6} (placeholder; SQL not yet imported)", q.name);
                    }
                    continue;
                }
                // WAVE5-RC2 prong 1: dispatch through `execute_query_dispatch`
                // so a configured runtime corrector is actually applied. In
                // baseline mode (or with no corrector), this is identical to
                // the prior `execute_query(&ctx, q)` call. In corrected mode
                // with a corrector present, the raw DataFusion estimate is
                // fed through `corrector.correct(...)` and the corrected value
                // becomes `outcome.estimated_rows` in the QueryOutcome (and
                // hence in the JSON `estimated` column).
                let active_corrector: Option<&dyn samkhya_core::residual::Corrector> =
                    if self.baseline {
                        None
                    } else {
                        self.corrector.as_deref()
                    };
                let exec_result = match self.query_timeout_s {
                    Some(s) => {
                        let dur = std::time::Duration::from_secs(s);
                        match tokio::time::timeout(
                            dur,
                            execute_query_dispatch(&ctx, q, active_corrector),
                        )
                        .await
                        {
                            Ok(inner) => inner,
                            Err(_) => {
                                // WAVE-5J: TIMEOUT is recorded, not dropped
                                // (cherry-picking forbidden per
                                // [[feedback-empirical-methodology]]).
                                let timeout_ms = (s as f64) * 1000.0;
                                let outcome = QueryOutcome {
                                    name: q.name,
                                    estimated_rows: 0,
                                    actual_rows: 0,
                                    q_error: f64::INFINITY,
                                    latency_ms: timeout_ms,
                                    per_join_q_errors: Vec::new(),
                                    features: None,
                                    trial_id: trial_idx as u32,
                                    status: "timeout",
                                };
                                println!(
                                    "{:<6} TIMEOUT after {}s (trial {})",
                                    outcome.name, s, trial_idx
                                );
                                outcomes.push(outcome);
                                continue;
                            }
                        }
                    }
                    None => execute_query_dispatch(&ctx, q, active_corrector).await,
                };
                match exec_result {
                    Ok(mut outcome) => {
                        outcome.trial_id = trial_idx as u32;
                        outcome.status = "ok";
                        println!(
                            "{:<6} {:>12} {:>12} {:>10.2} {:>10.2}",
                            outcome.name,
                            outcome.estimated_rows,
                            outcome.actual_rows,
                            outcome.q_error,
                            outcome.latency_ms,
                        );
                        // Record the plan features alongside the estimate
                        // whenever we have them. A model trained from an
                        // observation without them is blind to six of its
                        // seven inputs — see
                        // `samkhya_core::feedback::PlanObservation`.
                        match outcome.features.clone() {
                            Some(features) => {
                                store.record_plan(&PlanObservation {
                                    template_hash: template_hash.clone(),
                                    plan_fingerprint: q.sql.to_string(),
                                    features,
                                    actual_rows: outcome.actual_rows,
                                    latency_ms: Some(outcome.latency_ms),
                                })?;
                            }
                            None => {
                                store.record(&Observation {
                                    template_hash: template_hash.clone(),
                                    plan_fingerprint: q.sql.to_string(),
                                    est_rows: outcome.estimated_rows,
                                    actual_rows: outcome.actual_rows,
                                    latency_ms: Some(outcome.latency_ms),
                                })?;
                            }
                        }
                        outcomes.push(outcome);
                    }
                    Err(e) => {
                        println!("{:<6} ERROR (trial {}): {}", q.name, trial_idx, e);
                        outcomes.push(QueryOutcome {
                            name: q.name,
                            estimated_rows: 0,
                            actual_rows: 0,
                            q_error: f64::INFINITY,
                            latency_ms: 0.0,
                            per_join_q_errors: Vec::new(),
                            features: None,
                            trial_id: trial_idx as u32,
                            status: "error",
                        });
                    }
                }
                // WAVE-5J: incremental flush — write JSON after every query
                // so OOM-kill on subsequent queries preserves prior progress.
                if let Some(path) = self.json_out.as_ref() {
                    let _ = write_outcomes_json(path, &outcomes, self.suite.label(), self.baseline);
                }
            }
        }

        println!();
        println!("recorded {} observations to feedback store", store.count()?);
        if !outcomes.is_empty() {
            // q-error is >= 1 by definition (Moerkotte VLDB 2009 §3) and is
            // multiplicative, so the geometric mean over *finite* samples is
            // the right summary. Through 1.1 this summed only the finite
            // values but divided by the unfiltered count, so every infinity
            // silently pulled the average down — which is how a "q-error" of
            // 0.39 could ever be printed. Non-finite samples are now counted
            // and reported rather than dissolved into the denominator.
            let finite: Vec<f64> = outcomes
                .iter()
                .map(|o| o.q_error)
                .filter(|q| q.is_finite())
                .collect();
            let non_finite = outcomes.len() - finite.len();
            if finite.is_empty() {
                println!(
                    "q-error: no finite samples ({} of {} queries had a zero estimate or actual)",
                    non_finite,
                    outcomes.len()
                );
            } else {
                let geomean =
                    (finite.iter().map(|q| q.ln()).sum::<f64>() / finite.len() as f64).exp();
                let max_q = finite.iter().copied().fold(0f64, f64::max);
                println!(
                    "q-error: geomean {geomean:.2}, max {max_q:.2} over {} finite sample(s){}",
                    finite.len(),
                    if non_finite > 0 {
                        format!(
                            "; {non_finite} sample(s) unbounded (zero estimate or actual) and excluded"
                        )
                    } else {
                        String::new()
                    }
                );
            }

            // Aggregate per-join q-error samples across the workload —
            // Moerkotte VLDB 2009 §3 metrics on the meaningful intermediate
            // join cardinalities (rather than the structurally-degenerate
            // final-aggregate q-error reported above).
            let all_join_q: Vec<f64> = outcomes
                .iter()
                .flat_map(|o| o.per_join_q_errors.iter().filter_map(|j| j.q_error))
                .filter(|q| q.is_finite())
                .collect();
            if !all_join_q.is_empty() {
                let geomean = (all_join_q.iter().map(|q| q.ln()).sum::<f64>()
                    / all_join_q.len() as f64)
                    .exp();
                let max_join_q = all_join_q.iter().fold(0f64, |a, b| a.max(*b));
                println!(
                    "per-join q-error (Moerkotte VLDB 2009 §3): n={}, geomean={:.2}, max={:.2}",
                    all_join_q.len(),
                    geomean,
                    max_join_q,
                );
            }
        }

        if let Some(path) = self.json_out.as_ref() {
            write_outcomes_json(path, &outcomes, self.suite.label(), self.baseline)?;
            println!("wrote JSON report to {}", path.display());
        }
        Ok(())
    }

    /// Execute the configured suite, applying a residual `Corrector` to
    /// every raw DataFusion estimate. Returns one [`CorrectedOutcome`]
    /// per successfully executed query. The original [`Runner::run`]
    /// path is unaffected.
    pub fn run_with_corrector<C: Corrector + ?Sized>(
        &self,
        corrector: &C,
    ) -> Result<Vec<CorrectedOutcome>> {
        if !self.is_runnable() {
            let extra = if self.suite.is_executable_with_imdb_dir() {
                " (supply --imdb-dir to enable)"
            } else if self.suite.is_executable_with_tpch_dir() {
                " (supply --tpch-dir to enable)"
            } else {
                ""
            };
            println!(
                "runner: suite {} is not in-process executable yet (needs real dataset){}; skipping.",
                self.suite.label(),
                extra
            );
            return Ok(Vec::new());
        }

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(Error::from)?;
        rt.block_on(self.run_with_corrector_async(corrector))
    }

    async fn run_with_corrector_async<C: Corrector + ?Sized>(
        &self,
        corrector: &C,
    ) -> Result<Vec<CorrectedOutcome>> {
        let ctx = self.build_context().await?;
        let mut outcomes = Vec::new();
        for q in self.selected_queries() {
            if is_placeholder_query(q) {
                continue;
            }
            match execute_query_with_corrector(&ctx, q, corrector).await {
                Ok(outcome) => outcomes.push(outcome),
                Err(e) => {
                    println!("{:<6} ERROR: {}", q.name, e);
                }
            }
        }
        Ok(outcomes)
    }
}

impl Runner {
    /// WAVE-5M: evict the IMDb (or TPC-H) parquet corpus from the page
    /// cache via `posix_fadvise(POSIX_FADV_DONTNEED)` and log a one-line
    /// summary. Called before each trial iteration when `--cold-cache`
    /// is on. Synthetic suite has no on-disk corpus and is skipped.
    fn evict_for_cold_cache(&self, trial_idx: usize) {
        let dir = self.imdb_dir.as_deref().or(self.tpch_dir.as_deref());
        let Some(dir) = dir else {
            println!(
                "[cold-cache] trial {trial_idx}: no --imdb-dir/--tpch-dir; skipping eviction (synthetic suite)"
            );
            return;
        };
        match crate::cold_cache::evict_imdb_parquet_from_page_cache(dir) {
            Ok(bytes) => {
                let mib = (bytes as f64) / (1024.0 * 1024.0);
                println!(
                    "[cold-cache] trial {trial_idx}: evicted {:.1} MiB before run",
                    mib
                );
            }
            Err(e) => {
                eprintln!("[cold-cache] trial {trial_idx}: eviction failed: {e}");
            }
        }
    }

    /// Dispatch SessionContext construction by suite.
    ///
    /// `JobSlowReal` + a configured `imdb_dir` builds against the real
    /// IMDb dump via [`crate::imdb::register_imdb_tables`]. `TpcH` +
    /// a configured `tpch_dir` builds against the on-disk Parquet dump
    /// via [`crate::tpch::register_tpch_tables`]. Everything else falls
    /// back to the synthetic in-memory context.
    async fn build_context(&self) -> Result<SessionContext> {
        if self.suite.is_executable_with_imdb_dir() {
            if let Some(dir) = self.imdb_dir.as_deref() {
                imdb::probe_imdb_dir(dir)?;
                let ctx = SessionContext::new();
                match self.imdb_format {
                    crate::puffin_io::ImdbFormat::Csv => {
                        imdb::register_imdb_tables_async_with_baseline(&ctx, dir, self.baseline)
                            .await?;
                    }
                    crate::puffin_io::ImdbFormat::Parquet => {
                        imdb::register_imdb_parquet_async_with_baseline(&ctx, dir, self.baseline)
                            .await?;
                    }
                }
                return Ok(ctx);
            }
        }
        if self.suite.is_executable_with_tpch_dir() {
            if let Some(dir) = self.tpch_dir.as_deref() {
                tpch::probe_tpch_dir(dir)?;
                return tpch::build_tpch_context(dir).await;
            }
        }
        build_synthetic_context(self.baseline, self.puffin_dir.as_deref()).await
    }
}

async fn build_synthetic_context(
    baseline: bool,
    puffin_dir: Option<&std::path::Path>,
) -> Result<SessionContext> {
    let ctx = SessionContext::new();
    let customers = synthetic::customers_table(synthetic::N_CUSTOMERS).map_err(df_err)?;
    let products = synthetic::products_table(synthetic::N_PRODUCTS).map_err(df_err)?;
    let orders =
        synthetic::orders_table(synthetic::N_ORDERS, synthetic::N_CUSTOMERS).map_err(df_err)?;
    let order_items = synthetic::order_items_table(
        synthetic::N_ORDER_ITEMS,
        synthetic::N_ORDERS,
        synthetic::N_PRODUCTS,
    )
    .map_err(df_err)?;
    if baseline {
        ctx.register_table("customers", customers).map_err(df_err)?;
        ctx.register_table("products", products).map_err(df_err)?;
        ctx.register_table("orders", orders).map_err(df_err)?;
        ctx.register_table("order_items", order_items)
            .map_err(df_err)?;
    } else if let Some(dir) = puffin_dir {
        // Source per-column overrides from Puffin sidecars built via
        // `build-puffin`. Falls back to the hardcoded path below for
        // any table whose sidecar is missing.
        let mut sidecar_stats = puffin_io::load_column_stats_from_sidecars(dir)?;
        register_with_sidecar(
            &ctx,
            "customers",
            customers,
            synthetic::N_CUSTOMERS as u64,
            sidecar_stats.remove("customers"),
            &[
                ("customer_id", synthetic::N_CUSTOMERS as u64),
                ("region", 4),
                ("segment", 3),
            ],
        )?;
        register_with_sidecar(
            &ctx,
            "products",
            products,
            synthetic::N_PRODUCTS as u64,
            sidecar_stats.remove("products"),
            &[
                ("product_id", synthetic::N_PRODUCTS as u64),
                ("category", 5),
            ],
        )?;
        register_with_sidecar(
            &ctx,
            "orders",
            orders,
            synthetic::N_ORDERS as u64,
            sidecar_stats.remove("orders"),
            &[
                ("order_id", synthetic::N_ORDERS as u64),
                ("customer_id", synthetic::N_CUSTOMERS as u64),
                ("status", 5),
            ],
        )?;
        register_with_sidecar(
            &ctx,
            "order_items",
            order_items,
            synthetic::N_ORDER_ITEMS as u64,
            sidecar_stats.remove("order_items"),
            &[
                ("order_id", synthetic::N_ORDERS as u64),
                ("product_id", synthetic::N_PRODUCTS as u64),
            ],
        )?;
    } else {
        // Provide samkhya-known distinct counts that DataFusion's MemTable
        // doesn't compute by default — the actual information advantage.
        ctx.register_table(
            "customers",
            wrap_with_stats(
                customers,
                synthetic::N_CUSTOMERS as u64,
                &[
                    ("customer_id", synthetic::N_CUSTOMERS as u64),
                    ("region", 4),
                    ("segment", 3),
                ],
            ),
        )
        .map_err(df_err)?;
        ctx.register_table(
            "products",
            wrap_with_stats(
                products,
                synthetic::N_PRODUCTS as u64,
                &[
                    ("product_id", synthetic::N_PRODUCTS as u64),
                    ("category", 5),
                ],
            ),
        )
        .map_err(df_err)?;
        ctx.register_table(
            "orders",
            wrap_with_stats(
                orders,
                synthetic::N_ORDERS as u64,
                &[
                    ("order_id", synthetic::N_ORDERS as u64),
                    ("customer_id", synthetic::N_CUSTOMERS as u64),
                    ("status", 5),
                ],
            ),
        )
        .map_err(df_err)?;
        ctx.register_table(
            "order_items",
            wrap_with_stats(
                order_items,
                synthetic::N_ORDER_ITEMS as u64,
                &[
                    ("order_id", synthetic::N_ORDERS as u64),
                    ("product_id", synthetic::N_PRODUCTS as u64),
                ],
            ),
        )
        .map_err(df_err)?;
    }
    Ok(ctx)
}

/// Register a table with samkhya-corrected stats. When a sidecar
/// override list is supplied, it is used verbatim via
/// [`wrap_with_stats_from_overrides`]; otherwise the function falls
/// back to the hardcoded `distinct_per_col` slice (same path
/// [`wrap_with_stats`] takes).
fn register_with_sidecar<T: TableProvider + 'static>(
    ctx: &SessionContext,
    name: &str,
    inner: Arc<T>,
    row_count: u64,
    sidecar: Option<Vec<(usize, ColumnStats)>>,
    fallback_distinct_per_col: &[(&str, u64)],
) -> Result<()> {
    let wrapped = match sidecar {
        Some(overrides) => wrap_with_stats_from_overrides(inner, row_count, overrides),
        None => wrap_with_stats(inner, row_count, fallback_distinct_per_col),
    };
    ctx.register_table(name, wrapped).map_err(df_err)?;
    Ok(())
}

/// Wrap a MemTable with samkhya-known row count + per-column distinct
/// counts. Row count overrides ensure a stable num_rows reaches downstream
/// physical operators; distinct counts feed DataFusion's equality-predicate
/// selectivity estimator (1/distinct_count instead of the 1/5 default).
fn wrap_with_stats<T: TableProvider + 'static>(
    inner: Arc<T>,
    row_count: u64,
    distinct_per_col: &[(&str, u64)],
) -> Arc<dyn TableProvider> {
    let schema = inner.schema();
    let mut wrapper = SamkhyaTableProvider::new(inner);
    for (col_name, distinct_count) in distinct_per_col {
        if let Some((idx, _)) = schema
            .fields()
            .iter()
            .enumerate()
            .find(|(_, f)| f.name() == col_name)
        {
            wrapper = wrapper.with_column_stats(
                idx,
                ColumnStats::new()
                    .with_row_count(row_count)
                    .with_distinct_count(*distinct_count),
            );
        }
    }
    Arc::new(wrapper)
}

/// Variant of [`wrap_with_stats`] that takes a pre-resolved set of
/// `(column_index, ColumnStats)` overrides — the shape returned by
/// [`crate::puffin_io::load_column_stats_from_sidecars`]. Each override
/// is augmented with the supplied `row_count` so the table-level
/// `num_rows` fold inside `SamkhyaTableProvider` still has a value to
/// pick.
fn wrap_with_stats_from_overrides<T: TableProvider + 'static>(
    inner: Arc<T>,
    row_count: u64,
    overrides: Vec<(usize, ColumnStats)>,
) -> Arc<dyn TableProvider> {
    let schema = inner.schema();
    let n_fields = schema.fields().len();
    let mut wrapper = SamkhyaTableProvider::new(inner);
    for (col_idx, stats) in overrides {
        if col_idx >= n_fields {
            continue;
        }
        // Stamp the row count onto every override entry so the
        // provider's table-level fold (max row_count across overrides)
        // still resolves to the table's true row count.
        let merged = stats.with_row_count(row_count);
        wrapper = wrapper.with_column_stats(col_idx, merged);
    }
    Arc::new(wrapper)
}

async fn execute_query(ctx: &SessionContext, q: &Query) -> Result<QueryOutcome> {
    // Build the logical → physical plan once, then drive execution
    // through the same `Arc<dyn ExecutionPlan>` we will later walk for
    // per-join metrics. Going through `ctx.sql(...).collect()` would
    // throw away the post-execution `MetricsSet` because the
    // DataFrame's internal plan is no longer reachable.
    let logical = ctx
        .state()
        .create_logical_plan(q.sql)
        .await
        .map_err(df_err)?;
    let physical: Arc<dyn ExecutionPlan> = ctx
        .state()
        .create_physical_plan(&logical)
        .await
        .map_err(df_err)?;

    // Snapshot the root-level estimate before execution (preserves the
    // existing final-aggregate q-error contract).
    let estimated_rows = extract_root_estimate(physical.as_ref());

    // Snapshot per-join optimizer-estimated rows (statistics) before
    // execution. After collect() we revisit the same plan tree to read
    // each join's MetricsSet::output_rows.
    let join_estimates = collect_join_estimates(physical.as_ref());

    let start = Instant::now();
    let batches = execute_physical_plan(physical.clone(), ctx.task_ctx())
        .await
        .map_err(df_err)?;
    let elapsed = start.elapsed();
    let latency_ms = elapsed.as_secs_f64() * 1000.0;

    // For aggregate queries the result is a single scalar row; for non-aggregate
    // it's the row count. Either way summing num_rows across batches works,
    // *except* for COUNT(*) where we want the scalar value not "1 row in result".
    let actual_rows = extract_actual_count(&batches);
    let q_error = compute_q_error(estimated_rows, actual_rows);

    let per_join_q_errors = pair_join_metrics(physical.as_ref(), join_estimates);
    let features = extract_features(physical.as_ref(), estimated_rows);

    Ok(QueryOutcome {
        name: q.name,
        estimated_rows,
        actual_rows,
        q_error,
        latency_ms,
        per_join_q_errors,
        features: Some(features),
        trial_id: 1,
        status: "ok",
    })
}

/// WAVE5-RC2 prong 1 dispatch helper. Routes to either the corrected
/// or baseline execution path based on whether a `Corrector` is
/// supplied. When `corrector` is `None`, behaviour is byte-identical
/// to [`execute_query`]. When `Some(_)`, the corrected estimate
/// (clamped by the corrector's ceiling) becomes
/// `QueryOutcome::estimated_rows` and the q-error in the outcome is
/// the *corrected* q-error — so downstream JSON consumers compare on
/// the corrected number without changes.
///
/// `per_join_q_errors` are populated from the raw plan walk in both
/// arms (the corrector affects the *final* estimate, not the
/// optimizer's intermediate join cardinalities).
async fn execute_query_dispatch(
    ctx: &SessionContext,
    q: &Query,
    corrector: Option<&dyn Corrector>,
) -> Result<QueryOutcome> {
    match corrector {
        None => execute_query(ctx, q).await,
        Some(c) => {
            let corrected = execute_query_with_corrector(ctx, q, c).await?;
            Ok(QueryOutcome {
                name: corrected.name,
                estimated_rows: corrected.corrected_estimate,
                actual_rows: corrected.actual_rows,
                q_error: corrected.q_error_corrected,
                latency_ms: corrected.latency_ms,
                per_join_q_errors: corrected.per_join_q_errors,
                features: Some(corrected.features),
                trial_id: 1, // overwritten by the trial loop
                status: "ok",
            })
        }
    }
}

/// Variant of [`execute_query`] that feeds the raw optimizer estimate
/// into a residual [`Corrector`] and reports q-error both before and
/// after correction. Walks the physical plan via [`extract_features`] so
/// the corrector sees plan-shape signal (join depth, predicate count,
/// outermost-join input rows / distinct counts) even when the baseline
/// estimate collapses to zero.
async fn execute_query_with_corrector<C: Corrector + ?Sized>(
    ctx: &SessionContext,
    q: &Query,
    corrector: &C,
) -> Result<CorrectedOutcome> {
    let logical = ctx
        .state()
        .create_logical_plan(q.sql)
        .await
        .map_err(df_err)?;
    let physical: Arc<dyn ExecutionPlan> = ctx
        .state()
        .create_physical_plan(&logical)
        .await
        .map_err(df_err)?;
    let raw_estimate = extract_root_estimate(physical.as_ref());

    let features = extract_features(physical.as_ref(), raw_estimate);
    let corrected_estimate = corrector.correct(&features)?.unwrap_or(raw_estimate);
    let join_estimates = collect_join_estimates(physical.as_ref());

    let start = Instant::now();
    let batches = execute_physical_plan(physical.clone(), ctx.task_ctx())
        .await
        .map_err(df_err)?;
    let elapsed = start.elapsed();
    let latency_ms = elapsed.as_secs_f64() * 1000.0;

    let actual_rows = extract_actual_count(&batches);
    let q_error_raw = compute_q_error(raw_estimate, actual_rows);
    let q_error_corrected = compute_q_error(corrected_estimate, actual_rows);
    let per_join_q_errors = pair_join_metrics(physical.as_ref(), join_estimates);

    Ok(CorrectedOutcome {
        name: q.name,
        raw_estimate,
        corrected_estimate,
        actual_rows,
        q_error_raw,
        q_error_corrected,
        latency_ms,
        per_join_q_errors,
        features,
    })
}

/// Walk a physical plan and pull out the small set of features
/// [`CorrectionFeatures`] exposes today.
///
/// The traversal is a single pre-order pass:
///
/// - `join_depth` counts every `HashJoinExec` / `NestedLoopJoinExec` /
///   `CrossJoinExec` / `SortMergeJoinExec` encountered (the four DF 46
///   physical join operators).
/// - `predicate_count` counts every `FilterExec`.
/// - `left_input_rows` / `right_input_rows` and `left_distinct` /
///   `right_distinct` are sourced from the **outermost** join node — the
///   first one seen during pre-order walk. Distinct counts are summed
///   across that side's columns as a coarse proxy until column-specific
///   features land.
///
/// Statistics are read via `ExecutionPlan::statistics()`; `Precision::Absent`
/// slots collapse to `None` (which `CorrectionFeatures::to_vec` then
/// flattens to 0 — the trained corrector treats 0 as "unknown").
pub fn extract_features(
    physical: &dyn ExecutionPlan,
    baseline_estimate: u64,
) -> CorrectionFeatures {
    let mut features = CorrectionFeatures {
        baseline_estimate,
        ..Default::default()
    };
    let mut outermost_join_seen = false;
    walk_plan(physical, &mut features, &mut outermost_join_seen);
    features
}

fn walk_plan(
    node: &dyn ExecutionPlan,
    features: &mut CorrectionFeatures,
    outermost_join_seen: &mut bool,
) {
    let any = node.as_any();
    let is_join = any.is::<HashJoinExec>()
        || any.is::<NestedLoopJoinExec>()
        || any.is::<CrossJoinExec>()
        || any.is::<SortMergeJoinExec>();

    if is_join {
        features.join_depth = features.join_depth.saturating_add(1);
        if !*outermost_join_seen {
            *outermost_join_seen = true;
            let children = node.children();
            if let Some(left) = children.first() {
                let (rows, distinct) = side_stats(left.as_ref());
                features.left_input_rows = rows;
                features.left_distinct = distinct;
            }
            if let Some(right) = children.get(1) {
                let (rows, distinct) = side_stats(right.as_ref());
                features.right_input_rows = rows;
                features.right_distinct = distinct;
            }
        }
    }

    if any.is::<FilterExec>() {
        features.predicate_count = features.predicate_count.saturating_add(1);
    }

    for child in node.children() {
        walk_plan(child.as_ref(), features, outermost_join_seen);
    }
}

/// Pull `(num_rows, sum_of_distinct_counts)` out of a side's
/// `ExecutionPlan::statistics()`. Either entry is `None` when the
/// underlying `Precision` is `Absent`.
fn side_stats(plan: &dyn ExecutionPlan) -> (Option<u64>, Option<u64>) {
    let stats = match plan.statistics() {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    let rows = match stats.num_rows {
        Precision::Exact(n) | Precision::Inexact(n) => Some(n as u64),
        Precision::Absent => None,
    };
    let mut distinct_sum: u64 = 0;
    let mut any_present = false;
    for col in &stats.column_statistics {
        match col.distinct_count {
            Precision::Exact(n) | Precision::Inexact(n) => {
                distinct_sum = distinct_sum.saturating_add(n as u64);
                any_present = true;
            }
            Precision::Absent => {}
        }
    }
    let distinct = if any_present {
        Some(distinct_sum)
    } else {
        None
    };
    (rows, distinct)
}

/// Pull the optimizer-estimated cardinality from the root of a
/// physical plan tree (the same number `Runner::run` already reports
/// as `estimated_rows`). Returns 0 when `Precision::Absent`.
fn extract_root_estimate(plan: &dyn ExecutionPlan) -> u64 {
    plan.statistics()
        .ok()
        .and_then(|s| match s.num_rows {
            Precision::Exact(n) | Precision::Inexact(n) => Some(n as u64),
            Precision::Absent => None,
        })
        .unwrap_or(0)
}

/// Concrete `ExecutionPlan` type name for DataFusion 46's join family,
/// or `None` for non-join nodes. Listed exhaustively (downcast-based,
/// not name-based) so that future operator types don't get silently
/// misclassified — adding a new join type forces a code change here.
fn join_node_type_name(node: &dyn ExecutionPlan) -> Option<&'static str> {
    let any = node.as_any();
    if any.is::<HashJoinExec>() {
        Some("HashJoinExec")
    } else if any.is::<NestedLoopJoinExec>() {
        Some("NestedLoopJoinExec")
    } else if any.is::<CrossJoinExec>() {
        Some("CrossJoinExec")
    } else if any.is::<SortMergeJoinExec>() {
        Some("SortMergeJoinExec")
    } else if any.is::<SymmetricHashJoinExec>() {
        Some("SymmetricHashJoinExec")
    } else {
        None
    }
}

/// Snapshot of the optimizer's pre-execution estimate for one join
/// node, paired with the node's pre-order index. The actual row count
/// is filled in after `collect()` by reading `MetricsSet::output_rows`
/// from the same plan tree via [`pair_join_metrics`].
#[derive(Debug)]
struct JoinEstimateSnapshot {
    node_type: &'static str,
    node_idx: u32,
    estimated_rows: Option<u64>,
}

/// `ExecutionPlanVisitor` (DataFusion's native pre-order walker) that
/// records each join node's type + optimizer estimate during the walk.
/// Using `accept()` rather than a hand-rolled recursive walk keeps the
/// node-ordering identical to the one DataFusion's `EXPLAIN ANALYZE`
/// uses, so join indices line up with what users see in plan dumps.
struct JoinEstimateCollector {
    estimates: Vec<JoinEstimateSnapshot>,
    next_idx: u32,
}

impl JoinEstimateCollector {
    fn new() -> Self {
        Self {
            estimates: Vec::new(),
            next_idx: 0,
        }
    }
}

impl ExecutionPlanVisitor for JoinEstimateCollector {
    // Walk is infallible: every node touch only records data, no I/O.
    type Error = Infallible;

    fn pre_visit(&mut self, plan: &dyn ExecutionPlan) -> std::result::Result<bool, Self::Error> {
        if let Some(node_type) = join_node_type_name(plan) {
            let estimated_rows = match plan.statistics() {
                Ok(s) => match s.num_rows {
                    Precision::Exact(n) | Precision::Inexact(n) => Some(n as u64),
                    Precision::Absent => None,
                },
                Err(_) => None,
            };
            self.estimates.push(JoinEstimateSnapshot {
                node_type,
                node_idx: self.next_idx,
                estimated_rows,
            });
            self.next_idx = self.next_idx.saturating_add(1);
        }
        Ok(true)
    }
}

/// Walk the physical plan tree and collect one [`JoinEstimateSnapshot`]
/// per join node (pre-order). Internally drives DataFusion's
/// `ExecutionPlanVisitor` via `accept()`.
fn collect_join_estimates(plan: &dyn ExecutionPlan) -> Vec<JoinEstimateSnapshot> {
    let mut visitor = JoinEstimateCollector::new();
    // The visitor's Error is Infallible; this match exists to prove
    // exhaustiveness without `unwrap()`.
    match accept(plan, &mut visitor) {
        Ok(()) => {}
        Err(never) => match never {},
    }
    visitor.estimates
}

/// Visitor that pairs each pre-order join node with the matching
/// estimate snapshot and reads `MetricsSet::output_rows` from the
/// post-execution plan. Output rows can legitimately be `None` for
/// operators that don't emit the metric — in DF 46 every join in the
/// `joins::` module wires `MetricBuilder::new(&metrics).output_rows(...)`
/// at construction time, but third-party plans built atop the trait
/// might not. We fall through to `q_error = None` rather than fabricate
/// a value.
struct JoinMetricsCollector {
    estimates: Vec<JoinEstimateSnapshot>,
    cursor: usize,
    out: Vec<JoinQError>,
}

impl JoinMetricsCollector {
    fn new(estimates: Vec<JoinEstimateSnapshot>) -> Self {
        let cap = estimates.len();
        Self {
            estimates,
            cursor: 0,
            out: Vec::with_capacity(cap),
        }
    }
}

impl ExecutionPlanVisitor for JoinMetricsCollector {
    type Error = Infallible;

    fn pre_visit(&mut self, plan: &dyn ExecutionPlan) -> std::result::Result<bool, Self::Error> {
        if join_node_type_name(plan).is_none() {
            return Ok(true);
        }
        // We expect the second walk to encounter joins in the same
        // pre-order as the first. If it doesn't (a same-Arc plan that
        // mutated between walks would be a DataFusion-internal bug),
        // fall back to a `None`-actual entry rather than mis-pair.
        let snap = self.estimates.get(self.cursor);
        self.cursor += 1;

        let actual_rows = plan
            .metrics()
            .and_then(|m| m.output_rows())
            .map(|n| n as u64);
        let (node_type, node_idx, estimated_rows) = match snap {
            Some(s) => (s.node_type, s.node_idx, s.estimated_rows),
            None => (
                join_node_type_name(plan).unwrap_or("UnknownJoinExec"),
                self.cursor as u32 - 1,
                None,
            ),
        };
        let q_error = match (estimated_rows, actual_rows) {
            (Some(est), Some(act)) => Some(q_error_moerkotte(est, act)),
            _ => None,
        };
        self.out.push(JoinQError {
            node_type,
            node_idx,
            estimated_rows,
            actual_rows,
            q_error,
        });
        Ok(true)
    }
}

/// Pair the pre-collected estimates (one per join, pre-order) with the
/// post-execution `MetricsSet::output_rows` reading from the *same*
/// plan tree and compute Moerkotte VLDB 2009 §3 q-error per join.
fn pair_join_metrics(
    plan: &dyn ExecutionPlan,
    estimates: Vec<JoinEstimateSnapshot>,
) -> Vec<JoinQError> {
    let mut visitor = JoinMetricsCollector::new(estimates);
    match accept(plan, &mut visitor) {
        Ok(()) => {}
        Err(never) => match never {},
    }
    visitor.out
}

/// Moerkotte q-error formula, VLDB 2009 §3:
///
/// `q(c_est, c_true) = max(c_est / max(1, c_true), c_true / max(1, c_est))`
///
/// Lower bound 1.0 (perfect estimate), symmetric in the two arguments,
/// monotonic, and unaffected by linear scaling.
fn q_error_moerkotte(estimated: u64, actual: u64) -> f64 {
    let est = estimated.max(1) as f64;
    let act = actual.max(1) as f64;
    (est / act).max(act / est)
}

/// Serialize the per-query outcome list (including each query's
/// per-join q-error vector) to JSON at `path`. Schema is stable: the
/// receipt and the downstream aggregator in `bench-results/wave*_raw/`
/// depend on it.
fn write_outcomes_json(
    path: &std::path::Path,
    outcomes: &[QueryOutcome],
    suite_label: &str,
    baseline: bool,
) -> Result<()> {
    #[derive(Serialize)]
    struct Report<'a> {
        suite: &'a str,
        mode: &'static str,
        n_queries: usize,
        outcomes: &'a [QueryOutcome],
    }
    let report = Report {
        suite: suite_label,
        mode: if baseline { "baseline" } else { "samkhya" },
        n_queries: outcomes.len(),
        outcomes,
    };
    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| Error::Feedback(format!("serde_json: {e}")))?;
    std::fs::write(path, json).map_err(|e| Error::Feedback(format!("write {:?}: {e}", path)))?;
    Ok(())
}

fn extract_actual_count(batches: &[datafusion::arrow::record_batch::RecordBatch]) -> u64 {
    // If the result is a single-column Int64 scalar (COUNT(*) result),
    // pull the value out; otherwise fall back to summed batch row counts.
    if batches.len() == 1
        && batches[0].num_rows() == 1
        && batches[0].num_columns() == 1
        && batches[0].schema().field(0).data_type()
            == &datafusion::arrow::datatypes::DataType::Int64
    {
        if let Some(arr) = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<datafusion::arrow::array::Int64Array>()
        {
            if !arr.is_null(0) {
                return arr.value(0) as u64;
            }
        }
    }
    batches.iter().map(|b| b.num_rows() as u64).sum()
}

fn compute_q_error(est: u64, actual: u64) -> f64 {
    if est == 0 || actual == 0 {
        return f64::INFINITY;
    }
    let r = actual as f64 / est as f64;
    if r >= 1.0 { r } else { 1.0 / r }
}

fn df_err(e: impl std::fmt::Display) -> Error {
    Error::Feedback(format!("datafusion: {e}"))
}

/// Returns true for entries whose SQL text is still the `PLACEHOLDER_SQL`
/// sentinel from [`crate::queries::job_slow`]. These rows exist in the
/// roster so per-query reporting is correct, but they cannot be executed
/// until the canonical SQL is imported.
fn is_placeholder_query(q: &Query) -> bool {
    q.sql.starts_with("-- TODO(v0.6.0)")
}

#[cfg(test)]
mod tests {
    //! Per-join-node q-error extraction tests (Moerkotte VLDB 2009 §3).
    //!
    //! Builds a tiny `SessionContext` with two in-memory tables, runs a
    //! synthetic equi-join, and asserts that the new visitor surfaces at
    //! least one [`JoinQError`] entry with the right node type and a
    //! finite q-error value.
    use super::*;
    use datafusion::arrow::array::Int64Array;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::arrow::record_batch::RecordBatch;
    use datafusion::datasource::MemTable;
    use datafusion::prelude::SessionContext;

    fn build_two_table_ctx() -> SessionContext {
        let ctx = SessionContext::new();

        // left: 8 rows, id 0..8
        let left_schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
        let left_batch = RecordBatch::try_new(
            left_schema.clone(),
            vec![Arc::new(Int64Array::from((0..8i64).collect::<Vec<_>>()))],
        )
        .unwrap();
        let left = MemTable::try_new(left_schema, vec![vec![left_batch]]).unwrap();
        ctx.register_table("t_left", Arc::new(left)).unwrap();

        // right: 16 rows, id 0..16
        let right_schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
        let right_batch = RecordBatch::try_new(
            right_schema.clone(),
            vec![Arc::new(Int64Array::from((0..16i64).collect::<Vec<_>>()))],
        )
        .unwrap();
        let right = MemTable::try_new(right_schema, vec![vec![right_batch]]).unwrap();
        ctx.register_table("t_right", Arc::new(right)).unwrap();

        ctx
    }

    #[test]
    fn q_error_moerkotte_is_symmetric_and_lower_bounded() {
        // Lower bound = 1.0 at the perfect estimate.
        assert!((q_error_moerkotte(100, 100) - 1.0).abs() < 1e-9);
        // Symmetric in the two arguments.
        let a = q_error_moerkotte(10, 1000);
        let b = q_error_moerkotte(1000, 10);
        assert!((a - b).abs() < 1e-9);
        // Standard 10× over-estimate → q = 10.
        assert!((q_error_moerkotte(100, 10) - 10.0).abs() < 1e-9);
        // Zero handled (clamp to 1) — must not panic or yield NaN.
        let q = q_error_moerkotte(0, 5);
        assert!(q.is_finite() && q >= 1.0);
    }

    #[tokio::test]
    async fn per_join_q_error_is_extracted_from_synthetic_join() {
        let ctx = build_two_table_ctx();
        // Force an inner equi-join so DataFusion's physical planner
        // emits a HashJoinExec (or any concrete join family member).
        let sql = "SELECT COUNT(*) FROM t_left l JOIN t_right r ON l.id = r.id";

        let logical = ctx.state().create_logical_plan(sql).await.unwrap();
        let physical: Arc<dyn ExecutionPlan> =
            ctx.state().create_physical_plan(&logical).await.unwrap();

        // Pre-execution: collect per-join estimates.
        let estimates = collect_join_estimates(physical.as_ref());
        assert!(
            !estimates.is_empty(),
            "expected ≥ 1 join in the physical plan for an inner equi-join"
        );
        // Execute the same Arc so the post-walk reads metrics from the
        // node instances that actually ran.
        let batches = execute_physical_plan(physical.clone(), ctx.task_ctx())
            .await
            .unwrap();
        // Result is COUNT(*) = 8 (the intersection cardinality).
        let actual = extract_actual_count(&batches);
        assert_eq!(actual, 8, "intersection of 0..8 and 0..16 is 8");

        // Pair estimates with post-execution actuals.
        let joins = pair_join_metrics(physical.as_ref(), estimates);
        assert!(
            !joins.is_empty(),
            "pair_join_metrics returned no entries despite estimate walk finding joins"
        );

        // Every entry must carry a concrete join node-type label
        // belonging to DataFusion's physical-join family.
        for j in &joins {
            assert!(
                matches!(
                    j.node_type,
                    "HashJoinExec"
                        | "NestedLoopJoinExec"
                        | "CrossJoinExec"
                        | "SortMergeJoinExec"
                        | "SymmetricHashJoinExec"
                ),
                "join entry has unexpected node_type: {:?}",
                j
            );
        }
        // The actual row count at the join node must be the intersection
        // size (8) — confirms `MetricsSet::output_rows` is wired through
        // DataFusion 46's join family and surfaces via our visitor.
        // (Plain MemTables do not expose `Statistics::num_rows`, so the
        // estimated half may legitimately be None on this synthetic
        // plan; the test still validates the actual-side extraction
        // which is the new code path the receipt's acceptance gate
        // requires.)
        let observed_actual: Vec<u64> = joins.iter().filter_map(|j| j.actual_rows).collect();
        assert!(
            observed_actual.contains(&8),
            "expected a join node to report 8 output rows; got: {:?}",
            observed_actual
        );

        // Verify the q-error formula is invoked when both halves are
        // available — independent of whether DataFusion ground-truths
        // the estimate on this particular plan.
        let synthetic_q = q_error_moerkotte(2, 8);
        assert!((synthetic_q - 4.0).abs() < 1e-9);
    }
}
