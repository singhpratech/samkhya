//! End-to-end smoke test: runner builds the synthetic context, executes
//! every query in the suite through DataFusion, and lands observations
//! in the feedback store.

use samkhya_bench::queries::Suite;
use samkhya_bench::runner::Runner;

#[test]
fn baseline_run_executes_all_synthetic_queries() {
    let runner = Runner::new(Suite::Synthetic, true);
    let res = runner.run();
    assert!(res.is_ok(), "baseline run errored: {:?}", res);
}

#[test]
fn samkhya_wrapped_run_executes_all_synthetic_queries() {
    let runner = Runner::new(Suite::Synthetic, false);
    let res = runner.run();
    assert!(res.is_ok(), "samkhya-wrapped run errored: {:?}", res);
}

#[test]
fn run_with_disk_feedback_persists_observations() {
    use samkhya_core::feedback::FeedbackStore;

    let tmp = std::env::temp_dir().join(format!("samkhya-bench-smoke-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    let runner = Runner::new(Suite::Synthetic, true).with_feedback_path(&tmp);
    runner.run().expect("run failed");

    let store = FeedbackStore::open(&tmp).expect("reopen feedback store");
    let count = store.count().expect("count");
    assert_eq!(count, 10, "expected 10 observations, got {count}");

    std::fs::remove_file(&tmp).ok();
}

#[test]
fn unexecutable_suite_is_skipped_without_error() {
    // JobSlow is scaffold-only — should print "skipped" and return Ok(()).
    let runner = Runner::new(Suite::JobSlow, true);
    assert!(runner.run().is_ok());
}
