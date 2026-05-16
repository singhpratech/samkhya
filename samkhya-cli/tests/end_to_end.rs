//! End-to-end smoke test: drive every subcommand against a tempdir.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use samkhya_core::feedback::{FeedbackStore, Observation};

/// Locate the built `samkhya` binary. Cargo sets `CARGO_BIN_EXE_<name>`
/// for `[[bin]]` targets when running integration tests.
fn samkhya_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_samkhya"))
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_tempdir() -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = env::temp_dir().join(format!("samkhya-cli-{pid}-{nanos}-{n}"));
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn run(args: &[&str], cwd: Option<&Path>) -> (bool, String, String) {
    let mut cmd = Command::new(samkhya_bin());
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = cmd.output().expect("spawn samkhya");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn end_to_end_all_subcommands() {
    let dir = unique_tempdir();

    // Write a small CSV: column 0 = id (numeric), column 1 = name.
    let csv = dir.join("data.csv");
    let mut s = String::from("id,name\n");
    for i in 0..200u32 {
        s.push_str(&format!("{i},name_{}\n", i % 50));
    }
    fs::write(&csv, s).unwrap();

    // --- samkhya --help ---
    let (ok, stdout, _) = run(&["--help"], None);
    assert!(ok, "--help should succeed");
    assert!(stdout.contains("inspect"));
    assert!(stdout.contains("stats"));
    assert!(stdout.contains("sketch"));
    assert!(stdout.contains("puffin"));

    // --- samkhya sketch hll ---
    let hll_payload = dir.join("col1.hll");
    let (ok, stdout, stderr) = run(
        &[
            "sketch",
            "hll",
            "--input",
            csv.to_str().unwrap(),
            "--column",
            "1",
            "--precision",
            "10",
            "--header",
            "--output",
            hll_payload.to_str().unwrap(),
        ],
        None,
    );
    assert!(ok, "sketch hll failed: stdout={stdout}\nstderr={stderr}");
    assert!(hll_payload.exists());
    assert!(stdout.contains("HLL"));
    assert!(stdout.contains("estimate:"));

    // --- samkhya sketch bloom ---
    let bloom_payload = dir.join("col1.bloom");
    let (ok, _, stderr) = run(
        &[
            "sketch",
            "bloom",
            "--input",
            csv.to_str().unwrap(),
            "--column",
            "1",
            "--capacity",
            "100",
            "--fp-rate",
            "0.01",
            "--header",
            "--output",
            bloom_payload.to_str().unwrap(),
        ],
        None,
    );
    assert!(ok, "sketch bloom failed: {stderr}");
    assert!(bloom_payload.exists());

    // --- samkhya sketch cms ---
    let cms_payload = dir.join("col1.cms");
    let (ok, _, stderr) = run(
        &[
            "sketch",
            "cms",
            "--input",
            csv.to_str().unwrap(),
            "--column",
            "1",
            "--depth",
            "4",
            "--width",
            "256",
            "--header",
            "--output",
            cms_payload.to_str().unwrap(),
        ],
        None,
    );
    assert!(ok, "sketch cms failed: {stderr}");
    assert!(cms_payload.exists());

    // --- samkhya sketch histogram (numeric column 0) ---
    let hist_payload = dir.join("col0.hist");
    let (ok, stdout, stderr) = run(
        &[
            "sketch",
            "histogram",
            "--input",
            csv.to_str().unwrap(),
            "--column",
            "0",
            "--buckets",
            "8",
            "--header",
            "--output",
            hist_payload.to_str().unwrap(),
        ],
        None,
    );
    assert!(ok, "sketch histogram failed: {stderr}");
    assert!(hist_payload.exists());
    assert!(stdout.contains("Histogram"));

    // --- samkhya puffin pack ---
    let puffin = dir.join("bundle.puffin");
    let (ok, _, stderr) = run(
        &[
            "puffin",
            "pack",
            puffin.to_str().unwrap(),
            "--hll",
            hll_payload.to_str().unwrap(),
            "--bloom",
            bloom_payload.to_str().unwrap(),
            "--cms",
            cms_payload.to_str().unwrap(),
            "--histogram",
            hist_payload.to_str().unwrap(),
        ],
        None,
    );
    assert!(ok, "puffin pack failed: {stderr}");
    assert!(puffin.exists());

    // --- samkhya puffin verify ---
    let (ok, stdout, stderr) = run(&["puffin", "verify", puffin.to_str().unwrap()], None);
    assert!(ok, "puffin verify failed: {stderr}");
    assert!(stdout.contains("ok"));

    // --- samkhya inspect ---
    let (ok, stdout, stderr) = run(&["inspect", puffin.to_str().unwrap()], None);
    assert!(ok, "inspect failed: {stderr}");
    assert!(stdout.contains("samkhya.hll-v1"));
    assert!(stdout.contains("samkhya.bloom-v1"));
    assert!(stdout.contains("samkhya.cms-v1"));
    assert!(stdout.contains("samkhya.histogram-equidepth-v1"));

    // --- samkhya stats (build a fresh feedback store) ---
    let fb = dir.join("feedback.db");
    {
        let store = FeedbackStore::open(&fb).unwrap();
        store
            .record(&Observation {
                template_hash: "tpl_a".into(),
                plan_fingerprint: "plan1".into(),
                est_rows: 100,
                actual_rows: 250,
                latency_ms: Some(12.5),
            })
            .unwrap();
        store
            .record(&Observation {
                template_hash: "tpl_a".into(),
                plan_fingerprint: "plan1".into(),
                est_rows: 100,
                actual_rows: 90,
                latency_ms: Some(8.0),
            })
            .unwrap();
        store
            .record(&Observation {
                template_hash: "tpl_b".into(),
                plan_fingerprint: "plan2".into(),
                est_rows: 50,
                actual_rows: 500,
                latency_ms: Some(40.0),
            })
            .unwrap();
    }
    let (ok, stdout, stderr) = run(&["stats", fb.to_str().unwrap()], None);
    assert!(ok, "stats failed: {stderr}");
    assert!(stdout.contains("total observations: 3"));
    assert!(stdout.contains("distinct templates:  2"));
    assert!(stdout.contains("tpl_a"));
    assert!(stdout.contains("tpl_b"));

    // --- error path: pack with no payloads ---
    let empty_out = dir.join("empty.puffin");
    let (ok, _, _) = run(&["puffin", "pack", empty_out.to_str().unwrap()], None);
    assert!(!ok, "puffin pack with no payloads should fail");

    // Best-effort cleanup.
    let _ = fs::remove_dir_all(&dir);
}
