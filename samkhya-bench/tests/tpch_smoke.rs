//! End-to-end smoke test for the TPC-H pipeline.
//!
//! ## Setup needs
//!
//! This test requires a TPC-H Parquet dump on disk. Without it, the
//! `#[ignore]`'d cases skip silently so `cargo test -p samkhya-bench`
//! stays green for contributors who haven't populated a TPC-H dataset.
//!
//! 1. Produce a Parquet dump at scale-factor 1 (or any SF; the smoke
//!    test only asserts on the presence of `lineitem.parquet`, not its
//!    cardinality), e.g.:
//!
//!    ```bash
//!    tpchgen-cli -s 1 --format=parquet --output-dir=$PWD/samkhya-bench/data/tpch
//!    ```
//!
//! 2. Point this test at the directory by exporting `TPCH_DIR`:
//!
//!    ```bash
//!    TPCH_DIR=$PWD/samkhya-bench/data/tpch cargo test -p samkhya-bench \
//!        --test tpch_smoke -- --ignored
//!    ```
//!
//! Mirrors the shape of `imdb_smoke.rs`. The heavyweight test is marked
//! `#[ignore]` so CI never fires it unless an operator opts in via
//! `--ignored`.

use samkhya_bench::queries::Suite;
use samkhya_bench::runner::Runner;
use samkhya_bench::tpch;

fn tpch_dir_from_env() -> Option<std::path::PathBuf> {
    std::env::var_os("TPCH_DIR").map(std::path::PathBuf::from)
}

#[test]
#[ignore = "requires TPCH_DIR pointing at a TPC-H Parquet dump; see test module docs"]
fn tpc_h_runs_against_tpch_dir() {
    let Some(dir) = tpch_dir_from_env() else {
        eprintln!("TPCH_DIR not set; skipping. See tests/tpch_smoke.rs module docs for setup.");
        return;
    };

    // Probe step: the directory looks like a TPC-H dump (all 8 .parquet
    // files present).
    tpch::probe_tpch_dir(&dir).expect("TPCH_DIR does not look like a TPC-H Parquet dump");

    // Sanity: lineitem.parquet (the heavyweight fact table) is on disk.
    let lineitem = dir.join("lineitem.parquet");
    assert!(
        lineitem.exists(),
        "expected lineitem.parquet under TPCH_DIR={}",
        dir.display()
    );

    // End-to-end: Runner::with_tpch_dir(...).run(...) executes the 22
    // TPC-H queries (Q1 in particular is a single-table aggregate that
    // should always produce a non-empty result set on any non-zero SF).
    let runner = Runner::new(Suite::TpcH, true).with_tpch_dir(dir);
    runner.run().expect("TPC-H real run failed");
}

#[test]
fn tpc_h_is_executable_only_with_tpch_dir() {
    // Sanity: the suite is *not* unconditionally executable. The runner
    // must observe --tpch-dir before it will attempt to register tables.
    assert!(!Suite::TpcH.is_executable());
    assert!(Suite::TpcH.is_executable_with_tpch_dir());
}

#[test]
fn tpc_h_without_tpch_dir_skips_cleanly() {
    // No --tpch-dir → run() should print a "skipped" notice and return Ok(()).
    let runner = Runner::new(Suite::TpcH, true);
    let res = runner.run();
    assert!(res.is_ok(), "skipped run should not error, got: {:?}", res);
}
