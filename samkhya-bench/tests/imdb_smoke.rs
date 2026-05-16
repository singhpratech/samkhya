//! End-to-end smoke test for the IMDb / JOB pipeline.
//!
//! ## Setup needs
//!
//! This test requires the IMDb data dump (~3.7 GB unzipped CSVs). Without
//! it, the test exits as a no-op so `cargo test -p samkhya-bench` stays
//! green for contributors who haven't populated `data/job/`.
//!
//! 1. Follow `samkhya-bench/data/job/README.md` to download and unpack the
//!    dump. The end state is a directory containing `title.csv`,
//!    `cast_info.csv`, etc.
//! 2. Point this test at the directory by exporting `IMDB_DIR`:
//!
//!    ```bash
//!    IMDB_DIR=$PWD/samkhya-bench/data/job cargo test -p samkhya-bench \
//!        --test imdb_smoke -- --ignored
//!    ```
//!
//! The test is marked `#[ignore]` so it never fires in CI by default — the
//! data is too big to ship and too slow to download from a CI runner.

use samkhya_bench::imdb;
use samkhya_bench::queries::Suite;
use samkhya_bench::runner::Runner;

fn imdb_dir_from_env() -> Option<std::path::PathBuf> {
    std::env::var_os("IMDB_DIR").map(std::path::PathBuf::from)
}

#[test]
#[ignore = "requires IMDB_DIR pointing at an unpacked IMDb dump; see data/job/README.md"]
fn job_slow_real_runs_against_imdb_dir() {
    let Some(dir) = imdb_dir_from_env() else {
        eprintln!("IMDB_DIR not set; skipping. See samkhya-bench/data/job/README.md for setup.");
        return;
    };

    imdb::probe_imdb_dir(&dir).expect("IMDB_DIR does not look like an IMDb dump");

    // Run the suite; the scaffolded queries with real SQL (1a, 2b, 6a, 17a,
    // 29a) should land actual estimates and rows. The placeholders are
    // skipped by the runner.
    let runner = Runner::new(Suite::JobSlowReal, true).with_imdb_dir(dir);
    runner.run().expect("JOB-Slow real run failed");
}

#[test]
fn job_slow_real_is_executable_only_with_imdb_dir() {
    // Sanity: the suite is *not* unconditionally executable. The runner
    // must observe an --imdb-dir before it will attempt to register tables.
    assert!(!Suite::JobSlowReal.is_executable());
    assert!(Suite::JobSlowReal.is_executable_with_imdb_dir());
}

#[test]
fn job_slow_real_without_imdb_dir_skips_cleanly() {
    // No --imdb-dir → run() should print a "skipped" notice and return Ok(()).
    let runner = Runner::new(Suite::JobSlowReal, true);
    let res = runner.run();
    assert!(res.is_ok(), "skipped run should not error, got: {:?}", res);
}
