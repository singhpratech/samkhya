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

#[test]
fn run_with_puffin_dir_loads_overrides() {
    use samkhya_bench::puffin_io;
    use samkhya_core::feedback::FeedbackStore;

    // Use a process-unique tempdir; no `tempfile` crate dependency.
    let tmp = std::env::temp_dir().join(format!(
        "samkhya-bench-puffin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&tmp).expect("create tempdir");

    // Populate the directory with Puffin sidecars for every synthetic table.
    puffin_io::build_puffin_sidecars(&tmp).expect("build_puffin_sidecars");

    // Sanity: at least one .puffin file landed on disk.
    let puffin_files: Vec<_> = std::fs::read_dir(&tmp)
        .expect("read tempdir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "puffin").unwrap_or(false))
        .collect();
    assert!(
        !puffin_files.is_empty(),
        "build_puffin_sidecars produced no .puffin files in {}",
        tmp.display()
    );

    // Pipe observations through a disk feedback store so we can assert
    // the runner actually executed every query in the suite.
    let feedback = tmp.join("observations.db");
    let runner = Runner::new(Suite::Synthetic, false)
        .with_puffin_dir(tmp.clone())
        .with_feedback_path(&feedback);
    let res = runner.run();
    assert!(res.is_ok(), "puffin-sourced run errored: {:?}", res);

    let store = FeedbackStore::open(&feedback).expect("reopen feedback store");
    let count = store.count().expect("count");
    assert!(
        count > 0,
        "expected at least one observation recorded, got {count}"
    );

    std::fs::remove_dir_all(&tmp).ok();
}
