//! Benchmark runner — executes each query through DataFusion, captures
//! the optimizer's row estimate vs the actual row count, records the
//! observation in a samkhya feedback store, and prints a comparison
//! table.
//!
//! Only the `Synthetic` suite is in-process executable today; other
//! suites require real datasets and are reported as skipped.

use std::sync::Arc;
use std::time::Instant;

use datafusion::arrow::array::Array;
use datafusion::common::stats::Precision;
use datafusion::datasource::TableProvider;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::filter::FilterExec;
use datafusion::physical_plan::joins::{
    CrossJoinExec, HashJoinExec, NestedLoopJoinExec, SortMergeJoinExec,
};
use datafusion::prelude::SessionContext;
use samkhya_core::Result;
use samkhya_core::error::Error;
use samkhya_core::feedback::{FeedbackStore, Observation};
use samkhya_core::residual::{CorrectionFeatures, Corrector};
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::SamkhyaTableProvider;

use crate::imdb;
use crate::puffin_io;
use crate::queries::{Query, Suite};
use crate::synthetic;

/// Configuration for a single benchmark run.
#[derive(Debug, Clone)]
pub struct Runner {
    suite: Suite,
    baseline: bool,
    feedback_path: Option<std::path::PathBuf>,
    puffin_dir: Option<std::path::PathBuf>,
    imdb_dir: Option<std::path::PathBuf>,
}

/// Per-query result captured during a run.
#[derive(Debug, Clone)]
pub struct QueryOutcome {
    pub name: &'static str,
    pub estimated_rows: u64,
    pub actual_rows: u64,
    pub q_error: f64,
    pub latency_ms: f64,
}

/// Per-query result captured during a corrector-aware run. The raw
/// estimate is what DataFusion's optimizer reports; the corrected
/// estimate is the residual corrector's output for the same plan.
#[derive(Debug, Clone)]
pub struct CorrectedOutcome {
    pub name: &'static str,
    pub raw_estimate: u64,
    pub corrected_estimate: u64,
    pub actual_rows: u64,
    pub q_error_raw: f64,
    pub q_error_corrected: f64,
    pub latency_ms: f64,
}

impl Runner {
    pub fn new(suite: Suite, baseline: bool) -> Self {
        Self {
            suite,
            baseline,
            feedback_path: None,
            puffin_dir: None,
            imdb_dir: None,
        }
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

        println!(
            "runner: executing {} {} queries in {} mode",
            self.suite.queries().len(),
            self.suite.label(),
            mode,
        );
        println!(
            "{:<6} {:>12} {:>12} {:>10} {:>10}",
            "query", "estimated", "actual", "q-error", "ms"
        );
        println!("{}", "-".repeat(56));

        let template_hash = format!("samkhya-bench-{}", self.suite.label());
        let mut outcomes = Vec::new();
        for q in self.suite.queries() {
            if is_placeholder_query(q) {
                println!("{:<6} (placeholder; SQL not yet imported)", q.name);
                continue;
            }
            match execute_query(&ctx, q).await {
                Ok(outcome) => {
                    println!(
                        "{:<6} {:>12} {:>12} {:>10.2} {:>10.2}",
                        outcome.name,
                        outcome.estimated_rows,
                        outcome.actual_rows,
                        outcome.q_error,
                        outcome.latency_ms,
                    );
                    let obs = Observation {
                        template_hash: template_hash.clone(),
                        plan_fingerprint: q.sql.to_string(),
                        est_rows: outcome.estimated_rows,
                        actual_rows: outcome.actual_rows,
                        latency_ms: Some(outcome.latency_ms),
                    };
                    store.record(&obs)?;
                    outcomes.push(outcome);
                }
                Err(e) => {
                    println!("{:<6} ERROR: {}", q.name, e);
                }
            }
        }

        println!();
        println!("recorded {} observations to feedback store", store.count()?);
        if !outcomes.is_empty() {
            let avg_q: f64 = outcomes
                .iter()
                .map(|o| o.q_error)
                .filter(|q| q.is_finite())
                .sum::<f64>()
                / outcomes.len() as f64;
            let max_q = outcomes
                .iter()
                .map(|o| o.q_error)
                .filter(|q| q.is_finite())
                .fold(0f64, f64::max);
            println!("avg q-error: {avg_q:.2}, max q-error: {max_q:.2}");
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
            println!(
                "runner: suite {} is not in-process executable yet (needs real dataset); skipping.",
                self.suite.label()
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
        for q in self.suite.queries() {
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
    /// Dispatch SessionContext construction by suite.
    ///
    /// `JobSlowReal` + a configured `imdb_dir` builds against the real
    /// IMDb dump via [`crate::imdb::register_imdb_tables`]. Everything else
    /// falls back to the synthetic in-memory context.
    async fn build_context(&self) -> Result<SessionContext> {
        if self.suite.is_executable_with_imdb_dir() {
            if let Some(dir) = self.imdb_dir.as_deref() {
                imdb::probe_imdb_dir(dir)?;
                let ctx = SessionContext::new();
                imdb::register_imdb_tables(&ctx, dir)?;
                return Ok(ctx);
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
    // Build the logical → physical plan to extract the optimizer's
    // estimated cardinality.
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
    let estimated_rows = physical
        .statistics()
        .ok()
        .and_then(|s| match s.num_rows {
            Precision::Exact(n) | Precision::Inexact(n) => Some(n as u64),
            Precision::Absent => None,
        })
        .unwrap_or(0);

    let start = Instant::now();
    let df = ctx.sql(q.sql).await.map_err(df_err)?;
    let batches = df.collect().await.map_err(df_err)?;
    let elapsed = start.elapsed();
    let latency_ms = elapsed.as_secs_f64() * 1000.0;

    // For aggregate queries the result is a single scalar row; for non-aggregate
    // it's the row count. Either way summing num_rows across batches works,
    // *except* for COUNT(*) where we want the scalar value not "1 row in result".
    let actual_rows = extract_actual_count(&batches);

    let q_error = compute_q_error(estimated_rows, actual_rows);
    Ok(QueryOutcome {
        name: q.name,
        estimated_rows,
        actual_rows,
        q_error,
        latency_ms,
    })
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
    let raw_estimate = physical
        .statistics()
        .ok()
        .and_then(|s| match s.num_rows {
            Precision::Exact(n) | Precision::Inexact(n) => Some(n as u64),
            Precision::Absent => None,
        })
        .unwrap_or(0);

    let features = extract_features(physical.as_ref(), raw_estimate);
    let corrected_estimate = corrector.correct(&features)?.unwrap_or(raw_estimate);

    let start = Instant::now();
    let df = ctx.sql(q.sql).await.map_err(df_err)?;
    let batches = df.collect().await.map_err(df_err)?;
    let elapsed = start.elapsed();
    let latency_ms = elapsed.as_secs_f64() * 1000.0;

    let actual_rows = extract_actual_count(&batches);
    let q_error_raw = compute_q_error(raw_estimate, actual_rows);
    let q_error_corrected = compute_q_error(corrected_estimate, actual_rows);

    Ok(CorrectedOutcome {
        name: q.name,
        raw_estimate,
        corrected_estimate,
        actual_rows,
        q_error_raw,
        q_error_corrected,
        latency_ms,
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
