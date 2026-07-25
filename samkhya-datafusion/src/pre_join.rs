//! Pre-join cardinality correction for DataFusion's physical optimizer.
//!
//! DataFusion 46 runs its built-in `join_selection` physical rule before
//! distribution and sorting enforcement. Appending a custom rule with
//! `SessionStateBuilder::with_physical_optimizer_rule` therefore runs too late:
//! join build-side and `PartitionMode` decisions have already been made.
//!
//! [`SamkhyaPreJoinRule`] corrects the statistics reported by each direct join
//! input, and [`install_pre_join_corrector`] inserts that rule immediately before
//! `join_selection` while preserving every other optimizer rule in the session.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use datafusion::common::stats::Precision;
use datafusion::common::tree_node::{Transformed, TransformedResult, TreeNode};
use datafusion::common::{DataFusionError, Result, Statistics};
use datafusion::execution::session_state::{SessionState, SessionStateBuilder};
use datafusion::physical_optimizer::PhysicalOptimizerRule;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::filter::FilterExec;
use datafusion::physical_plan::joins::{
    CrossJoinExec, HashJoinExec, NestedLoopJoinExec, SortMergeJoinExec, SymmetricHashJoinExec,
};
use samkhya_core::residual::{CorrectionFeatures, Corrector};

use crate::SamkhyaStatsExec;

const RULE_NAME: &str = "samkhya_pre_join_correction";
const JOIN_SELECTION_NAME: &str = "join_selection";

/// Sanity ceiling on any row count this rule publishes, ~1.1e12 rows.
///
/// DataFusion's join-cardinality estimator multiplies published row counts
/// together without checking for overflow, so handing it `u64::MAX` — which a
/// broken or adversarial corrector will happily propose — produces a wrapped,
/// meaningless number deep inside the planner. Capping what we publish keeps
/// the blast radius inside samkhya.
///
/// This is a sanity cap, not an overflow proof: two values at the cap still
/// overflow when multiplied. It exists to stop absurd inputs, not to make the
/// engine's arithmetic total. No real relation has 2^40 rows.
pub const SAFE_MAX_ROWS: u64 = 1 << 40;

/// Configuration for [`SamkhyaPreJoinRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PreJoinCorrectionOptions {
    /// Inclusive upper bound applied to corrector proposals.
    ///
    /// An explicit operator-supplied bound. It composes with the ceiling
    /// [`derive_ceiling`](Self::derive_ceiling) computes from the plan and
    /// with [`SAFE_MAX_ROWS`]: whichever is tightest wins. The default
    /// [`u64::MAX`] means "no explicit bound", which since 1.2.0 no longer
    /// means "no bound at all". With the safe default native floor, the
    /// native estimate wins if it is already greater than this ceiling.
    pub ceiling: u64,
    /// Permit a corrected estimate smaller than DataFusion's native estimate.
    ///
    /// The default is `false`, preserving the adapter's plan-memory-monotonic
    /// posture: underestimation must not make DataFusion choose a smaller hash
    /// build side than it would have chosen natively. Set this only when an
    /// operator has an independently validated rollout policy for downward
    /// corrections.
    pub allow_below_native: bool,
    /// Derive a per-input ceiling from the shape of the input subplan when
    /// the input is itself composite.
    ///
    /// A join can emit at most the product of its children's outputs, a
    /// union at most their sum, and a filter or projection at most its
    /// child's. Walking those relations gives a finite, provable ceiling for
    /// any composite input without needing the operator to supply one.
    ///
    /// Leaf inputs are deliberately left unconstrained: a scan's row count
    /// *is* the statistic under correction, so deriving a "ceiling" from it
    /// would simply forbid every upward correction.
    ///
    /// Defaults to `true`. Before 1.2.0 there was no derived ceiling and the
    /// default [`ceiling`](Self::ceiling) was [`u64::MAX`], which meant the
    /// bound guarantee was absent from the shipped DataFusion configuration
    /// unless an operator wired one up by hand.
    pub derive_ceiling: bool,
}

impl Default for PreJoinCorrectionOptions {
    fn default() -> Self {
        Self {
            ceiling: u64::MAX,
            allow_below_native: false,
            derive_ceiling: true,
        }
    }
}

impl PreJoinCorrectionOptions {
    /// Create options with an adapter-side correction ceiling.
    pub const fn with_ceiling(ceiling: u64) -> Self {
        Self {
            ceiling,
            allow_below_native: false,
            derive_ceiling: true,
        }
    }

    /// Explicitly opt into or out of estimates below DataFusion's native value.
    pub const fn with_allow_below_native(mut self, allow: bool) -> Self {
        self.allow_below_native = allow;
        self
    }

    /// Turn the subplan-derived ceiling on or off.
    pub const fn with_derive_ceiling(mut self, derive: bool) -> Self {
        self.derive_ceiling = derive;
        self
    }
}

/// Point-in-time diagnostics from a [`SamkhyaPreJoinRule`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PreJoinCorrectionMetrics {
    /// Inputs with a usable native row estimate handed to the corrector.
    pub attempts: usize,
    /// Inputs whose reported statistics changed.
    pub applied: usize,
    /// `Ok(None)` results, which retain the native statistics.
    pub abstained: usize,
    /// Corrector errors safely converted to native-statistics fallback.
    pub errors: usize,
    /// Proposals reduced by the adapter-side ceiling.
    pub clamped: usize,
    /// Proposals raised to DataFusion's native estimate by the safety floor.
    pub floored: usize,
}

/// Physical optimizer rule that corrects direct join-input statistics.
///
/// The rule is deliberately fail-open: `Ok(None)` and `Err(_)` from the
/// configured [`Corrector`] both retain DataFusion's native statistics. A
/// successful proposal is marked [`Precision::Inexact`]. By default the
/// published value is `max(native, min(proposal, ceiling))`; the native floor
/// can only be disabled with the explicit
/// [`PreJoinCorrectionOptions::allow_below_native`] opt-in.
///
/// Since 1.2.0 the published value is bounded even with no operator
/// configuration: [`PreJoinCorrectionOptions::derive_ceiling`] bounds composite
/// inputs by their own plan shape, and [`SAFE_MAX_ROWS`] caps everything.
///
/// When byte-size statistics exist they are scaled by the row-count ratio
/// because DataFusion's `JoinSelection` prefers bytes over rows when both sides
/// publish them.
pub struct SamkhyaPreJoinRule {
    corrector: Arc<dyn Corrector>,
    options: PreJoinCorrectionOptions,
    attempts: AtomicUsize,
    applied: AtomicUsize,
    abstained: AtomicUsize,
    errors: AtomicUsize,
    clamped: AtomicUsize,
    floored: AtomicUsize,
}

impl fmt::Debug for SamkhyaPreJoinRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SamkhyaPreJoinRule")
            .field("corrector", &self.corrector.name())
            .field("options", &self.options)
            .field("metrics", &self.metrics())
            .finish()
    }
}

impl SamkhyaPreJoinRule {
    /// Construct a pre-join rule around any samkhya corrector.
    pub fn new(corrector: Arc<dyn Corrector>, options: PreJoinCorrectionOptions) -> Self {
        Self {
            corrector,
            options,
            attempts: AtomicUsize::new(0),
            applied: AtomicUsize::new(0),
            abstained: AtomicUsize::new(0),
            errors: AtomicUsize::new(0),
            clamped: AtomicUsize::new(0),
            floored: AtomicUsize::new(0),
        }
    }

    /// Return the configured corrector.
    pub fn corrector(&self) -> &Arc<dyn Corrector> {
        &self.corrector
    }

    /// Return this rule's immutable options.
    pub const fn options(&self) -> PreJoinCorrectionOptions {
        self.options
    }

    /// Snapshot the rule's process-local diagnostic counters.
    pub fn metrics(&self) -> PreJoinCorrectionMetrics {
        PreJoinCorrectionMetrics {
            attempts: self.attempts.load(Ordering::Relaxed),
            applied: self.applied.load(Ordering::Relaxed),
            abstained: self.abstained.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            clamped: self.clamped.load(Ordering::Relaxed),
            floored: self.floored.load(Ordering::Relaxed),
        }
    }

    fn correct_join_inputs(
        &self,
        plan: Arc<dyn ExecutionPlan>,
    ) -> Result<Transformed<Arc<dyn ExecutionPlan>>> {
        if !is_join(plan.as_ref()) {
            return Ok(Transformed::no(plan));
        }

        let children: Vec<Arc<dyn ExecutionPlan>> = plan.children().into_iter().cloned().collect();
        if children.len() != 2 {
            return Ok(Transformed::no(plan));
        }

        let left_stats = children[0].statistics()?;
        let right_stats = children[1].statistics()?;
        let pair = JoinInputFeatures::new(&left_stats, &right_stats);

        let (left, left_changed) = self.correct_input(Arc::clone(&children[0]), left_stats, &pair);
        let (right, right_changed) =
            self.correct_input(Arc::clone(&children[1]), right_stats, &pair);

        if !left_changed && !right_changed {
            return Ok(Transformed::no(plan));
        }

        plan.with_new_children(vec![left, right])
            .map(Transformed::yes)
    }

    fn correct_input(
        &self,
        input: Arc<dyn ExecutionPlan>,
        mut stats: Statistics,
        pair: &JoinInputFeatures,
    ) -> (Arc<dyn ExecutionPlan>, bool) {
        let Some(native_rows) = precision_value(&stats.num_rows) else {
            return (input, false);
        };
        let native_rows_u64 = native_rows as u64;
        self.attempts.fetch_add(1, Ordering::Relaxed);

        let features = CorrectionFeatures {
            baseline_estimate: native_rows_u64,
            left_input_rows: pair.left_rows,
            right_input_rows: pair.right_rows,
            left_distinct: pair.left_distinct,
            right_distinct: pair.right_distinct,
            predicate_count: count_filters(input.as_ref()),
            join_depth: join_depth(input.as_ref()),
        };

        let proposed = match self.corrector.correct(&features) {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.abstained.fetch_add(1, Ordering::Relaxed);
                return (input, false);
            }
            Err(_) => {
                // Optimizer-time model failures must not fail the query. The
                // native statistics are already present and are the safe
                // fallback contract of Corrector::correct.
                self.errors.fetch_add(1, Ordering::Relaxed);
                return (input, false);
            }
        };

        let derived = if self.options.derive_ceiling {
            derive_input_ceiling(input.as_ref())
        } else {
            None
        };
        let effective_ceiling = match derived {
            Some(value) => self.options.ceiling.min(value),
            None => self.options.ceiling,
        };

        let ceiling_clamped = proposed.min(effective_ceiling).min(SAFE_MAX_ROWS);
        if ceiling_clamped != proposed {
            self.clamped.fetch_add(1, Ordering::Relaxed);
        }
        // Keep the native row count as the safe floor unless the operator
        // explicitly opts into downward corrections. The floor intentionally
        // wins over a conflicting adapter ceiling: preserving the native
        // planner's memory-sizing posture is safer than publishing a smaller
        // value merely to satisfy a misconfigured global ceiling.
        let bounded = if !self.options.allow_below_native && ceiling_clamped < native_rows_u64 {
            self.floored.fetch_add(1, Ordering::Relaxed);
            native_rows_u64
        } else {
            ceiling_clamped
        };
        let corrected_rows = usize::try_from(bounded).unwrap_or(usize::MAX);
        if corrected_rows == native_rows {
            return (input, false);
        }

        stats.num_rows = Precision::Inexact(corrected_rows);
        stats.total_byte_size = scale_byte_size(stats.total_byte_size, native_rows, corrected_rows);
        self.applied.fetch_add(1, Ordering::Relaxed);

        (
            Arc::new(SamkhyaStatsExec::new(input, stats)) as Arc<dyn ExecutionPlan>,
            true,
        )
    }
}

impl PhysicalOptimizerRule for SamkhyaPreJoinRule {
    fn optimize(
        &self,
        plan: Arc<dyn ExecutionPlan>,
        _config: &datafusion::common::config::ConfigOptions,
    ) -> Result<Arc<dyn ExecutionPlan>> {
        plan.transform_up(|plan| self.correct_join_inputs(plan))
            .data()
    }

    fn name(&self) -> &str {
        RULE_NAME
    }

    fn schema_check(&self) -> bool {
        true
    }
}

/// Install a corrector immediately before DataFusion's built-in
/// `join_selection` physical optimizer rule.
///
/// Existing logical/physical optimizer rules and session configuration are
/// preserved. Installation is idempotent by rule name: reinstalling replaces
/// the previous samkhya pre-join rule instead of applying corrections twice.
/// An error is returned when the session has no `join_selection` rule, because
/// appending silently would recreate the too-late ordering this API prevents.
pub fn install_pre_join_corrector(
    state: SessionState,
    rule: Arc<SamkhyaPreJoinRule>,
) -> Result<SessionState> {
    let mut rules = state.physical_optimizers().to_vec();
    rules.retain(|candidate| candidate.name() != RULE_NAME);
    let join_selection_index = rules
        .iter()
        .position(|candidate| candidate.name() == JOIN_SELECTION_NAME)
        .ok_or_else(|| {
            DataFusionError::Plan(
                "cannot install samkhya pre-join correction: DataFusion session has no \
                 join_selection physical optimizer rule"
                    .to_owned(),
            )
        })?;
    rules.insert(join_selection_index, rule);

    Ok(SessionStateBuilder::new_from_existing(state)
        .with_physical_optimizer_rules(rules)
        .build())
}

#[derive(Debug, Clone, Copy)]
struct JoinInputFeatures {
    left_rows: Option<u64>,
    right_rows: Option<u64>,
    left_distinct: Option<u64>,
    right_distinct: Option<u64>,
}

impl JoinInputFeatures {
    fn new(left: &Statistics, right: &Statistics) -> Self {
        Self {
            left_rows: precision_value(&left.num_rows).map(|value| value as u64),
            right_rows: precision_value(&right.num_rows).map(|value| value as u64),
            left_distinct: max_distinct(left),
            right_distinct: max_distinct(right),
        }
    }
}

/// Provable ceiling on how many rows a composite input can emit, derived
/// from the statistics its own children publish.
///
/// Only two shapes are claimed, because only two are guaranteed for every
/// node DataFusion may hand us:
///
/// * a join emits at most the product of its children's row counts;
/// * a filter emits at most its child's row count.
///
/// Everything else returns `None`. That deliberately includes a plain
/// recursive walk of the plan: a node is free to publish statistics that
/// differ from its child's — [`SamkhyaStatsExec`] does exactly that, and so
/// does any source that reports at its own level — so "the child bounds the
/// parent" is not a safe general rule to lean on for a *provable* ceiling.
/// Leaves return `None` too, because a scan's row count is precisely the
/// statistic under correction and treating it as a ceiling would forbid
/// every upward correction.
///
/// Arithmetic saturates rather than wrapping, and a result that saturates
/// reports `None`: an unbounded answer is worthless as a ceiling, and
/// dressing it up as one would be worse than admitting there is none.
fn derive_input_ceiling(plan: &dyn ExecutionPlan) -> Option<u64> {
    let children = plan.children();

    let child_rows = |child: &dyn ExecutionPlan| -> Option<u64> {
        precision_value(&child.statistics().ok()?.num_rows).map(|value| value as u64)
    };

    let ceiling = if is_join(plan) && !children.is_empty() {
        let mut product = 1u64;
        for child in &children {
            product = product.saturating_mul(child_rows(child.as_ref())?);
        }
        product
    } else if plan.as_any().is::<FilterExec>() && children.len() == 1 {
        child_rows(children[0].as_ref())?
    } else {
        return None;
    };

    (ceiling < u64::MAX).then_some(ceiling)
}

fn is_join(plan: &dyn ExecutionPlan) -> bool {
    let any = plan.as_any();
    any.is::<HashJoinExec>()
        || any.is::<CrossJoinExec>()
        || any.is::<NestedLoopJoinExec>()
        || any.is::<SymmetricHashJoinExec>()
        || any.is::<SortMergeJoinExec>()
}

fn precision_value(value: &Precision<usize>) -> Option<usize> {
    match value {
        Precision::Exact(value) | Precision::Inexact(value) => Some(*value),
        Precision::Absent => None,
    }
}

fn max_distinct(stats: &Statistics) -> Option<u64> {
    stats
        .column_statistics
        .iter()
        .filter_map(|column| precision_value(&column.distinct_count))
        .max()
        .map(|value| value as u64)
}

fn count_filters(plan: &dyn ExecutionPlan) -> u32 {
    let here = u32::from(plan.as_any().is::<FilterExec>());
    plan.children().into_iter().fold(here, |count, child| {
        count.saturating_add(count_filters(child.as_ref()))
    })
}

fn join_depth(plan: &dyn ExecutionPlan) -> u32 {
    let child_depth = plan
        .children()
        .into_iter()
        .map(|child| join_depth(child.as_ref()))
        .max()
        .unwrap_or(0);
    if is_join(plan) {
        child_depth.saturating_add(1)
    } else {
        child_depth
    }
}

fn scale_byte_size(
    byte_size: Precision<usize>,
    native_rows: usize,
    corrected_rows: usize,
) -> Precision<usize> {
    let precision = match byte_size {
        Precision::Exact(value) | Precision::Inexact(value) => value,
        Precision::Absent => return Precision::Absent,
    };

    // There is no meaningful average row width when the native row count is
    // zero. Dropping bytes forces JoinSelection to use corrected num_rows.
    if native_rows == 0 {
        return Precision::Absent;
    }

    let scaled = (precision as u128)
        .saturating_mul(corrected_rows as u128)
        .div_ceil(native_rows as u128)
        .min(usize::MAX as u128) as usize;
    Precision::Inexact(scaled)
}
