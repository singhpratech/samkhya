use std::sync::Arc;

use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::common::config::ConfigOptions;
use datafusion::common::stats::Precision;
use datafusion::common::{ColumnStatistics, JoinType, Statistics};
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::physical_expr::expressions::Column;
use datafusion::physical_optimizer::PhysicalOptimizerRule;
use datafusion::physical_optimizer::join_selection::JoinSelection;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::empty::EmptyExec;
use datafusion::physical_plan::joins::{HashJoinExec, PartitionMode};
use datafusion::physical_planner::DefaultPhysicalPlanner;
use samkhya_core::residual::{CorrectionFeatures, Corrector};
use samkhya_core::{Error, Result as SamkhyaResult};
use samkhya_datafusion::{
    PreJoinCorrectionOptions, SamkhyaPreJoinRule, SamkhyaStatsExec, install_pre_join_corrector,
};

struct InvertSides;

impl Corrector for InvertSides {
    fn correct(&self, features: &CorrectionFeatures) -> SamkhyaResult<Option<u64>> {
        Ok(Some(match features.baseline_estimate {
            10 => 1_000,
            100 => 1,
            other => other,
        }))
    }

    fn name(&self) -> &'static str {
        "invert-sides"
    }
}

struct RaiseAll;

impl Corrector for RaiseAll {
    fn correct(&self, features: &CorrectionFeatures) -> SamkhyaResult<Option<u64>> {
        Ok(Some(features.baseline_estimate.saturating_add(10_000)))
    }

    fn name(&self) -> &'static str {
        "raise-all"
    }
}

struct MaxCorrector;

impl Corrector for MaxCorrector {
    fn correct(&self, _features: &CorrectionFeatures) -> SamkhyaResult<Option<u64>> {
        Ok(Some(u64::MAX))
    }

    fn name(&self) -> &'static str {
        "max"
    }
}

struct AbstainCorrector;

impl Corrector for AbstainCorrector {
    fn correct(&self, _features: &CorrectionFeatures) -> SamkhyaResult<Option<u64>> {
        Ok(None)
    }

    fn name(&self) -> &'static str {
        "abstain"
    }
}

struct ErrorCorrector;

impl Corrector for ErrorCorrector {
    fn correct(&self, _features: &CorrectionFeatures) -> SamkhyaResult<Option<u64>> {
        Err(Error::Feedback("model unavailable".to_owned()))
    }

    fn name(&self) -> &'static str {
        "error"
    }
}

fn input(name: &str, rows: usize, bytes: usize) -> Arc<dyn ExecutionPlan> {
    let schema = Arc::new(Schema::new(vec![Field::new(name, DataType::Int64, false)]));
    let input: Arc<dyn ExecutionPlan> = Arc::new(EmptyExec::new(Arc::clone(&schema)));
    let stats = Statistics {
        num_rows: Precision::Exact(rows),
        total_byte_size: Precision::Exact(bytes),
        column_statistics: vec![ColumnStatistics {
            distinct_count: Precision::Inexact(rows),
            ..ColumnStatistics::new_unknown()
        }],
    };
    Arc::new(SamkhyaStatsExec::new(input, stats))
}

fn hash_join() -> Arc<dyn ExecutionPlan> {
    let left = input("left_k", 10, 100);
    let right = input("right_k", 100, 1_000);
    let on = vec![(
        Arc::new(Column::new("left_k", 0)) as _,
        Arc::new(Column::new("right_k", 0)) as _,
    )];
    Arc::new(
        HashJoinExec::try_new(
            left,
            right,
            on,
            None,
            &JoinType::Inner,
            None,
            PartitionMode::Auto,
            false,
        )
        .expect("valid hash join"),
    )
}

fn find_hash_join(plan: &dyn ExecutionPlan) -> Option<&HashJoinExec> {
    if let Some(join) = plan.as_any().downcast_ref::<HashJoinExec>() {
        return Some(join);
    }
    plan.children()
        .into_iter()
        .find_map(|child| find_hash_join(child.as_ref()))
}

fn rows(plan: &Arc<dyn ExecutionPlan>) -> usize {
    match plan.statistics().expect("statistics").num_rows {
        Precision::Exact(value) | Precision::Inexact(value) => value,
        Precision::Absent => panic!("row statistics unexpectedly absent"),
    }
}

#[test]
fn corrected_stats_change_join_selection_build_side() {
    let config = ConfigOptions::new();
    let join_selection = JoinSelection::new();

    let baseline = join_selection
        .optimize(hash_join(), &config)
        .expect("baseline join selection");
    let baseline_join = find_hash_join(baseline.as_ref()).expect("hash join remains");
    assert_eq!(baseline_join.left().schema().field(0).name(), "left_k");

    let rule = SamkhyaPreJoinRule::new(Arc::new(InvertSides), PreJoinCorrectionOptions::default());
    let corrected = rule
        .optimize(hash_join(), &config)
        .expect("pre-join correction");
    let optimized = join_selection
        .optimize(corrected, &config)
        .expect("corrected join selection");
    let optimized_join = find_hash_join(optimized.as_ref()).expect("hash join remains");

    // DataFusion builds its hash table from the left side. The corrected
    // left estimate grows to 1000 while the proposed 100 -> 1 correction on
    // the right is safely floored at its native 100, so the built-in rule
    // still swaps without allowing a below-native estimate.
    assert_eq!(optimized_join.left().schema().field(0).name(), "right_k");
    assert_eq!(rule.metrics().applied, 1);
    assert_eq!(rule.metrics().floored, 1);
}

#[test]
fn installed_rule_changes_plan_through_datafusion_default_optimizer() {
    let state = SessionStateBuilder::new().with_default_features().build();
    let rule = Arc::new(SamkhyaPreJoinRule::new(
        Arc::new(InvertSides),
        PreJoinCorrectionOptions::default(),
    ));
    let state = install_pre_join_corrector(state, Arc::clone(&rule)).expect("install");

    let optimized = DefaultPhysicalPlanner::default()
        .optimize_physical_plan(hash_join(), &state, |_, _| {})
        .expect("DataFusion default physical optimizer");
    let join = find_hash_join(optimized.as_ref()).expect("hash join remains");

    assert_eq!(join.left().schema().field(0).name(), "right_k");
    assert_eq!(rule.metrics().applied, 1);
    assert_eq!(rule.metrics().floored, 1);
}

#[test]
fn corrected_stats_change_partition_mode() {
    let mut config = ConfigOptions::new();
    config.optimizer.hash_join_single_partition_threshold = 500;
    config.optimizer.hash_join_single_partition_threshold_rows = 50;
    let join_selection = JoinSelection::new();

    let baseline = join_selection
        .optimize(hash_join(), &config)
        .expect("baseline join selection");
    assert_eq!(
        find_hash_join(baseline.as_ref())
            .expect("hash join")
            .partition_mode(),
        &PartitionMode::CollectLeft
    );

    let rule = SamkhyaPreJoinRule::new(Arc::new(RaiseAll), PreJoinCorrectionOptions::default());
    let corrected = rule
        .optimize(hash_join(), &config)
        .expect("pre-join correction");
    let optimized = join_selection
        .optimize(corrected, &config)
        .expect("corrected join selection");
    assert_eq!(
        find_hash_join(optimized.as_ref())
            .expect("hash join")
            .partition_mode(),
        &PartitionMode::Partitioned
    );
}

#[test]
fn adapter_ceiling_clamps_rows_and_bytes() {
    let rule = SamkhyaPreJoinRule::new(
        Arc::new(MaxCorrector),
        PreJoinCorrectionOptions::with_ceiling(200),
    );
    let corrected = rule
        .optimize(hash_join(), &ConfigOptions::new())
        .expect("pre-join correction");
    let join = find_hash_join(corrected.as_ref()).expect("hash join");

    assert_eq!(rows(join.left()), 200);
    assert_eq!(rows(join.right()), 200);
    assert_eq!(rule.metrics().clamped, 2);
    assert_eq!(rule.metrics().applied, 2);

    // Native average row width was ten bytes for each input; scaling the
    // byte statistic keeps JoinSelection's preferred size metric coherent.
    assert_eq!(
        join.left().statistics().unwrap().total_byte_size,
        Precision::Inexact(2_000)
    );
    assert_eq!(
        join.right().statistics().unwrap().total_byte_size,
        Precision::Inexact(2_000)
    );
}

#[test]
fn native_floor_wins_over_conflicting_ceiling() {
    let rule = SamkhyaPreJoinRule::new(
        Arc::new(MaxCorrector),
        PreJoinCorrectionOptions::with_ceiling(50),
    );
    let corrected = rule
        .optimize(hash_join(), &ConfigOptions::new())
        .expect("pre-join correction");
    let join = find_hash_join(corrected.as_ref()).expect("hash join");

    assert_eq!(rows(join.left()), 50);
    assert_eq!(rows(join.right()), 100);
    assert_eq!(rule.metrics().clamped, 2);
    assert_eq!(rule.metrics().floored, 1);
    assert_eq!(rule.metrics().applied, 1);
}

#[test]
fn below_native_estimates_require_explicit_opt_in() {
    let rule = SamkhyaPreJoinRule::new(
        Arc::new(InvertSides),
        PreJoinCorrectionOptions::default().with_allow_below_native(true),
    );
    let corrected = rule
        .optimize(hash_join(), &ConfigOptions::new())
        .expect("pre-join correction");
    let join = find_hash_join(corrected.as_ref()).expect("hash join");

    assert_eq!((rows(join.left()), rows(join.right())), (1_000, 1));
    assert_eq!(rule.metrics().floored, 0);
    assert_eq!(rule.metrics().applied, 2);
}

#[test]
fn abstention_and_error_fall_back_without_failing_planning() {
    let abstaining = SamkhyaPreJoinRule::new(
        Arc::new(AbstainCorrector),
        PreJoinCorrectionOptions::default(),
    );
    let abstained = abstaining
        .optimize(hash_join(), &ConfigOptions::new())
        .expect("abstention must not fail planning");
    let join = find_hash_join(abstained.as_ref()).expect("hash join");
    assert_eq!((rows(join.left()), rows(join.right())), (10, 100));
    assert_eq!(abstaining.metrics().abstained, 2);
    assert_eq!(abstaining.metrics().applied, 0);

    let failing = SamkhyaPreJoinRule::new(
        Arc::new(ErrorCorrector),
        PreJoinCorrectionOptions::default(),
    );
    let fallback = failing
        .optimize(hash_join(), &ConfigOptions::new())
        .expect("corrector error must not fail planning");
    let join = find_hash_join(fallback.as_ref()).expect("hash join");
    assert_eq!((rows(join.left()), rows(join.right())), (10, 100));
    assert_eq!(failing.metrics().errors, 2);
    assert_eq!(failing.metrics().applied, 0);
}

#[test]
fn installer_places_rule_immediately_before_join_selection_and_is_idempotent() {
    let state = SessionStateBuilder::new().with_default_features().build();
    let rule = Arc::new(SamkhyaPreJoinRule::new(
        Arc::new(AbstainCorrector),
        PreJoinCorrectionOptions::default(),
    ));
    let state = install_pre_join_corrector(state, Arc::clone(&rule)).expect("install");
    let state = install_pre_join_corrector(state, rule).expect("reinstall");
    let names: Vec<&str> = state
        .physical_optimizers()
        .iter()
        .map(|optimizer| optimizer.name())
        .collect();

    let pre_join = names
        .iter()
        .position(|name| *name == "samkhya_pre_join_correction")
        .expect("pre-join rule present");
    let join_selection = names
        .iter()
        .position(|name| *name == "join_selection")
        .expect("join selection present");
    assert_eq!(pre_join + 1, join_selection);
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == "samkhya_pre_join_correction")
            .count(),
        1
    );
}

#[test]
fn installer_rejects_sessions_without_join_selection() {
    let state = SessionStateBuilder::new()
        .with_default_features()
        .with_physical_optimizer_rules(vec![])
        .build();
    let rule = Arc::new(SamkhyaPreJoinRule::new(
        Arc::new(AbstainCorrector),
        PreJoinCorrectionOptions::default(),
    ));
    let error = install_pre_join_corrector(state, rule).expect_err("missing join selection");
    assert!(error.to_string().contains("no join_selection"));
}
