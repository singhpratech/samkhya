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
use datafusion::datasource::TableProvider;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::SessionContext;
use samkhya_core::Result;
use samkhya_core::error::Error;
use samkhya_core::feedback::{FeedbackStore, Observation};
use samkhya_core::stats::ColumnStats;
use samkhya_datafusion::SamkhyaTableProvider;

use crate::queries::{Query, Suite};
use crate::synthetic;

/// Configuration for a single benchmark run.
#[derive(Debug, Clone)]
pub struct Runner {
    suite: Suite,
    baseline: bool,
    feedback_path: Option<std::path::PathBuf>,
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

impl Runner {
    pub fn new(suite: Suite, baseline: bool) -> Self {
        Self {
            suite,
            baseline,
            feedback_path: None,
        }
    }

    /// Persist observations to a SQLite store at the given path.
    /// If unset, an in-memory store is used.
    pub fn with_feedback_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.feedback_path = Some(path.into());
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
        if !self.suite.is_executable() {
            println!(
                "runner: suite {} is not in-process executable yet (needs real dataset); skipping.",
                self.suite.label()
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

    async fn run_async(&self) -> Result<()> {
        let mode = if self.baseline {
            "baseline (native plan)"
        } else {
            "samkhya-corrected"
        };
        let ctx = build_synthetic_context(self.baseline).await?;
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
}

async fn build_synthetic_context(baseline: bool) -> Result<SessionContext> {
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
    } else {
        ctx.register_table(
            "customers",
            wrap_with_samkhya(customers, synthetic::N_CUSTOMERS),
        )
        .map_err(df_err)?;
        ctx.register_table(
            "products",
            wrap_with_samkhya(products, synthetic::N_PRODUCTS),
        )
        .map_err(df_err)?;
        ctx.register_table("orders", wrap_with_samkhya(orders, synthetic::N_ORDERS))
            .map_err(df_err)?;
        ctx.register_table(
            "order_items",
            wrap_with_samkhya(order_items, synthetic::N_ORDER_ITEMS),
        )
        .map_err(df_err)?;
    }
    Ok(ctx)
}

/// Wrap a MemTable with a [`SamkhyaTableProvider`] that exposes
/// ground-truth row counts as `ColumnStats`. For the synthetic schema
/// we know the cardinality exactly; in production this would be sourced
/// from Puffin sidecars or the feedback store.
fn wrap_with_samkhya<T: TableProvider + 'static>(
    inner: Arc<T>,
    row_count: usize,
) -> Arc<dyn TableProvider> {
    let schema = inner.schema();
    let mut wrapper = SamkhyaTableProvider::new(inner);
    for col_idx in 0..schema.fields().len() {
        wrapper =
            wrapper.with_column_stats(col_idx, ColumnStats::new().with_row_count(row_count as u64));
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
            datafusion::common::stats::Precision::Exact(n)
            | datafusion::common::stats::Precision::Inexact(n) => Some(n as u64),
            datafusion::common::stats::Precision::Absent => None,
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
