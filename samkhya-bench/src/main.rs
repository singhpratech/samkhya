//! samkhya-bench CLI entry point.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use samkhya_bench::puffin_io::ImdbFormat;
use samkhya_bench::{queries::Suite, runner::Runner};
use samkhya_core::Result;

#[derive(Debug, Parser)]
#[command(
    name = "samkhya-bench",
    about = "Benchmark harness for JOB-Slow, TPC-H, and STATS-CEB",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the bundled query suites and their counts.
    ListQueries,

    /// Execute a suite (baseline plan vs. samkhya-corrected plan).
    Run {
        /// Which benchmark suite to run.
        #[arg(long, value_enum)]
        suite: SuiteArg,

        /// Run the engine's native plan only; skip samkhya correction.
        #[arg(long, default_value_t = false)]
        baseline: bool,

        /// Persist observations to a SQLite file at this path (default: in-memory).
        #[arg(long)]
        feedback: Option<PathBuf>,

        /// Source samkhya-corrected `ColumnStats` overrides from Puffin
        /// sidecars in this directory (one `.puffin` per table, as
        /// produced by `build-puffin`). Ignored in baseline mode.
        #[arg(long)]
        puffin_dir: Option<PathBuf>,

        /// Path to an unpacked IMDb dump (see `samkhya-bench/data/job/README.md`).
        /// When supplied alongside `--suite job-slow-real`, the runner builds
        /// a DataFusion SessionContext from the real CSVs/Parquets and
        /// executes the JOB queries end-to-end.
        #[arg(long)]
        imdb_dir: Option<PathBuf>,

        /// Path to a TPC-H Parquet dump (e.g. produced by
        /// `tpchgen-cli -s 1 --format=parquet --output-dir=<path>`).
        /// When supplied alongside `--suite tpc-h`, the runner builds a
        /// DataFusion SessionContext from the on-disk Parquet files and
        /// executes the 22 TPC-H queries end-to-end.
        #[arg(long)]
        tpch_dir: Option<PathBuf>,

        /// WAVE-5F: source format for IMDb tables under `--imdb-dir`.
        /// `csv` (default) reads `<imdb_dir>/<table>.csv`; `parquet` reads
        /// the sibling-level `<imdb_dir>/<table>.parquet` produced by
        /// `convert-imdb-csv-to-parquet` and sources sidecars from
        /// `<table>.parquet.puffin`. Ignored unless `--suite job-slow-real`
        /// and `--imdb-dir` are both supplied.
        #[arg(long, value_enum, default_value_t = ImdbFormatArg::Csv)]
        imdb_format: ImdbFormatArg,

        /// WAVE-5J: emit a structured JSON report of every per-query
        /// outcome (including the per-join q-error vector and trial id)
        /// at this path after the run finishes. The existing stdout
        /// reporting path is preserved verbatim.
        #[arg(long)]
        json_out: Option<PathBuf>,

        /// WAVE-5J: number of full-suite trials (replicates) to execute
        /// in one process. Defaults to 1. Each trial's outcomes are
        /// tagged with `trial_id` in the JSON output.
        #[arg(long, default_value_t = 1)]
        trials: usize,

        /// WAVE-5J: per-query wall-clock timeout in seconds. Queries
        /// exceeding the timeout are recorded as TIMEOUT entries (not
        /// dropped from aggregates per [[feedback-empirical-methodology]]).
        /// 0 disables the timeout.
        #[arg(long, default_value_t = 0)]
        query_timeout_s: u64,

        /// WAVE-5M: enable cold-cache discipline. Before each trial the
        /// runner advises the kernel to drop page-cache pages backing
        /// every `*.parquet` file under `--imdb-dir`/`--tpch-dir` via
        /// `posix_fadvise(POSIX_FADV_DONTNEED)` (no root required). Off
        /// by default (warm-cache; backward-compatible). Citation: Leis
        /// et al. VLDB 2015 §3.
        #[arg(long, default_value_t = false)]
        cold_cache: bool,

        /// WAVE5-RC2 prong 1: select a runtime residual corrector for
        /// the trial loop. `none` (default) preserves the prior n-trial
        /// behaviour where only planner-level stat injection runs.
        /// `identity` wires the trivial pass-through corrector — useful
        /// for proving the dispatch path without training data. Ignored
        /// in `--baseline` mode (baseline never invokes a corrector).
        #[arg(long, value_enum, default_value_t = CorrectorArg::None)]
        corrector: CorrectorArg,
        /// Path to a model written by `samkhya-bench train`. Required by
        /// `--corrector gbt`.
        #[arg(long)]
        model: Option<PathBuf>,
        /// Run only these queries (comma-separated names). Combined with
        /// `--exclude`, this is how a training set and an evaluation set are
        /// kept disjoint, which is the whole point of freezing a model.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Skip these queries (comma-separated names). Applied after `--only`.
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
    },

    /// Run a suite twice (baseline + samkhya) and print a side-by-side comparison.
    Compare {
        /// Which benchmark suite to run.
        #[arg(long, value_enum)]
        suite: SuiteArg,

        /// Source samkhya-corrected `ColumnStats` overrides from Puffin
        /// sidecars in this directory. The baseline pass ignores it.
        #[arg(long)]
        puffin_dir: Option<PathBuf>,
    },

    /// Build Puffin sidecars for the Synthetic schema and write them to
    /// the given directory (one .puffin file per table). Subsequent runs
    /// can load `ColumnStats` from these sidecars via `--puffin-dir`.
    ///
    /// When `--imdb-dir` is supplied INSTEAD of `--output`, the binary
    /// iterates over the 21 IMDb tables under `<imdb-dir>` and writes
    /// one `<imdb-dir>/<table>.puffin` sidecar per table (HLL precision-12
    /// per column + 1% Bloom for FK columns + row-count marker). This is
    /// the path the JobSlowReal head-to-head consumes.
    BuildPuffin {
        /// Output directory for the synthetic-table sidecars. Mutually
        /// exclusive with `--imdb-dir`.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Build IMDb-table sidecars next to the CSVs in this directory.
        /// Mutually exclusive with `--output`.
        #[arg(long)]
        imdb_dir: Option<PathBuf>,
        /// WAVE-5F: source format for the IMDb sidecar build path. `csv`
        /// (default) reads `<imdb_dir>/<table>.csv` and writes
        /// `<table>.puffin`; `parquet` reads `<table>.parquet` and writes
        /// `<table>.parquet.puffin`. Ignored unless `--imdb-dir` is set.
        #[arg(long, value_enum, default_value_t = ImdbFormatArg::Csv)]
        format: ImdbFormatArg,
    },

    /// WAVE-5F: convert every `<imdb_dir>/<table>.csv` (21 IMDb tables) to
    /// a sibling `<table>.parquet` using arrow-csv + parquet (Snappy
    /// compression, default page/row-group sizes). The CSV files are not
    /// touched (audit trail).
    ConvertImdbCsvToParquet {
        /// Directory containing the 21 IMDb `<table>.csv` files (see
        /// `samkhya-bench/data/job/README.md`). Parquet outputs are
        /// written as siblings; existing `<table>.parquet` files are
        /// overwritten.
        #[arg(long)]
        imdb_dir: PathBuf,
    },

    /// Run a suite, train a GBT corrector from the observations, re-run with correction applied.
    Calibrate {
        #[arg(long, value_enum)]
        suite: SuiteArg,
        #[arg(long)]
        feedback: Option<PathBuf>,
        /// Source samkhya-corrected `ColumnStats` overrides from Puffin
        /// sidecars in this directory.
        #[arg(long)]
        puffin_dir: Option<PathBuf>,
        /// Path to an unpacked IMDb dump (see `samkhya-bench/data/job/README.md`).
        /// When supplied with `--suite job-slow-real`, calibration runs end-to-end
        /// against real IMDb data. Mutually exclusive with `--tpch-dir`.
        #[arg(long)]
        imdb_dir: Option<PathBuf>,
        /// Path to a TPC-H Parquet dump (e.g. `tpchgen-cli -s 1 --format=parquet
        /// --output-dir=<path>`). When supplied with `--suite tpc-h`, calibration
        /// runs end-to-end across the 22 TPC-H queries. Mutually exclusive with
        /// `--imdb-dir`.
        #[arg(long)]
        tpch_dir: Option<PathBuf>,
    },

    /// Render a report from a feedback store.
    Report {
        /// Path to the feedback store (SQLite) to summarize.
        #[arg(long)]
        feedback: PathBuf,
    },

    /// Train a GBT residual corrector from a feedback store and write the model.
    ///
    /// Trains only on observations that carry plan features. Feed the model
    /// back with `run --corrector gbt --model <path>`, and keep the training
    /// and evaluation query sets disjoint (see `--exclude` / `--only`) so the
    /// measurement is genuinely held out.
    Train {
        /// Path to the feedback store to train from.
        #[arg(long)]
        feedback: PathBuf,
        /// Template hash to filter observations by (matches the suite name used during run).
        #[arg(long)]
        template: String,
        /// Where to write the fitted model.
        #[arg(long)]
        out: PathBuf,
        /// Boosting iterations (one tree each).
        #[arg(long, default_value_t = 50)]
        num_trees: u32,
        /// Maximum tree depth; root is depth 0.
        #[arg(long, default_value_t = 4)]
        max_depth: u32,
        /// Shrinkage applied to each tree's contribution.
        #[arg(long, default_value_t = 0.1)]
        learning_rate: f64,
        /// Minimum samples per leaf.
        #[arg(long, default_value_t = 1)]
        min_leaf_size: usize,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SuiteArg {
    JobSlow,
    JobSlowReal,
    TpcH,
    StatsCeb,
    Synthetic,
}

/// WAVE5-RC2 prong 1: corrector selection for the trial loop.
/// `None` preserves prior n-trial behaviour (planner-level stat
/// injection only). `Identity` is the pass-through corrector — proves
/// the dispatch path works without any training data. Future variants
/// (`Gbt`, `AdditiveGbt`) will train from a `--feedback` store at
/// CLI-startup time; deferred to a follow-up rc.2 commit.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CorrectorArg {
    /// No corrector. The run measures the engine plus whatever statistics
    /// samkhya injected, and nothing else.
    None,
    /// Pass-through. Useful only to prove the corrector path is wired; it
    /// returns the baseline unchanged, so it cannot move any measurement.
    Identity,
    /// A trained gradient-boosted-tree corrector, loaded from `--model`.
    Gbt,
}

/// WAVE-5F: CLI-facing IMDb source format. Maps 1:1 to
/// [`samkhya_bench::puffin_io::ImdbFormat`].
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ImdbFormatArg {
    Csv,
    Parquet,
}

impl From<ImdbFormatArg> for ImdbFormat {
    fn from(value: ImdbFormatArg) -> Self {
        match value {
            ImdbFormatArg::Csv => ImdbFormat::Csv,
            ImdbFormatArg::Parquet => ImdbFormat::Parquet,
        }
    }
}

impl From<SuiteArg> for Suite {
    fn from(value: SuiteArg) -> Self {
        match value {
            SuiteArg::JobSlow => Suite::JobSlow,
            SuiteArg::JobSlowReal => Suite::JobSlowReal,
            SuiteArg::TpcH => Suite::TpcH,
            SuiteArg::StatsCeb => Suite::StatsCeb,
            SuiteArg::Synthetic => Suite::Synthetic,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::ListQueries => list_queries(),
        Command::Run {
            suite,
            baseline,
            feedback,
            puffin_dir,
            imdb_dir,
            tpch_dir,
            imdb_format,
            json_out,
            trials,
            query_timeout_s,
            cold_cache,
            corrector,
            model,
            only,
            exclude,
        } => {
            let mut runner = Runner::new(suite.into(), baseline);
            if let Some(path) = feedback {
                runner = runner.with_feedback_path(path);
            }
            if let Some(dir) = puffin_dir {
                runner = runner.with_puffin_dir(dir);
            }
            if let Some(dir) = imdb_dir {
                runner = runner.with_imdb_dir(dir);
            }
            if let Some(dir) = tpch_dir {
                runner = runner.with_tpch_dir(dir);
            }
            runner = runner.with_imdb_format(imdb_format.into());
            if let Some(path) = json_out {
                runner = runner.with_json_out(path);
            }
            runner = runner.with_trials(trials);
            runner = runner.with_query_timeout_s(query_timeout_s);
            runner = runner.with_cold_cache(cold_cache);
            // WAVE5-RC2 prong 1: attach corrector if requested.
            match corrector {
                CorrectorArg::None => {}
                CorrectorArg::Identity => {
                    let c: std::sync::Arc<dyn samkhya_core::residual::Corrector> =
                        std::sync::Arc::new(samkhya_core::residual::IdentityCorrector);
                    runner = runner.with_corrector(c);
                }
                CorrectorArg::Gbt => {
                    let Some(path) = model.as_ref() else {
                        eprintln!(
                            "error: --corrector gbt requires --model <path>; train one with \
                             `samkhya-bench train --feedback <db> --template <t> --out <path>`"
                        );
                        std::process::exit(2);
                    };
                    let trained = samkhya_core::residual::gbt::GbtCorrector::load(path, u64::MAX)?;
                    let c: std::sync::Arc<dyn samkhya_core::residual::Corrector> =
                        std::sync::Arc::new(trained);
                    runner = runner.with_corrector(c);
                }
            }
            if !only.is_empty() {
                runner = runner.with_only(only);
            }
            if !exclude.is_empty() {
                runner = runner.with_exclude(exclude);
            }
            runner.run()
        }
        Command::Compare { suite, puffin_dir } => {
            samkhya_bench::report::compare(suite.into(), puffin_dir.as_deref())
        }
        Command::BuildPuffin {
            output,
            imdb_dir,
            format,
        } => match (output, imdb_dir) {
            (Some(_), Some(_)) => {
                eprintln!("error: --output and --imdb-dir are mutually exclusive; pass only one");
                std::process::exit(2);
            }
            (Some(dir), None) => samkhya_bench::puffin_io::build_puffin_sidecars(&dir),
            (None, Some(dir)) => samkhya_bench::puffin_io::build_puffin_sidecars_imdb_with_format(
                &dir,
                format.into(),
            ),
            (None, None) => {
                eprintln!(
                    "error: build-puffin requires either --output (synthetic) or --imdb-dir (JOB)"
                );
                std::process::exit(2);
            }
        },
        Command::ConvertImdbCsvToParquet { imdb_dir } => {
            let start = std::time::Instant::now();
            let converted = samkhya_bench::csv_to_parquet::convert_imdb_csvs_to_parquet(&imdb_dir)?;
            let elapsed = start.elapsed();
            println!(
                "convert-imdb-csv-to-parquet: wrote {} table(s) in {:.2}s",
                converted.len(),
                elapsed.as_secs_f64()
            );
            Ok(())
        }
        Command::Report { feedback } => samkhya_bench::report::summarize(&feedback),
        Command::Train {
            feedback,
            template,
            out,
            num_trees,
            max_depth,
            learning_rate,
            min_leaf_size,
        } => {
            let options = samkhya_core::residual::gbt::GbtOptions {
                learning_rate,
                max_depth,
                num_trees,
                ceiling: u64::MAX,
                min_leaf_size,
            };
            samkhya_bench::report::train(&feedback, &template, &out, options)
        }
        Command::Calibrate {
            suite,
            feedback,
            puffin_dir,
            imdb_dir,
            tpch_dir,
        } => {
            if imdb_dir.is_some() && tpch_dir.is_some() {
                eprintln!("error: --imdb-dir and --tpch-dir are mutually exclusive; pass only one");
                std::process::exit(2);
            }
            samkhya_bench::calibrate::calibrate(
                suite.into(),
                feedback.as_deref(),
                puffin_dir.as_deref(),
                imdb_dir.as_deref(),
                tpch_dir.as_deref(),
            )
        }
    }
}

fn list_queries() -> Result<()> {
    for suite in [
        Suite::JobSlow,
        Suite::JobSlowReal,
        Suite::TpcH,
        Suite::StatsCeb,
        Suite::Synthetic,
    ] {
        let queries = suite.queries();
        let exec = if suite.is_executable() {
            "(executable)"
        } else if suite.is_executable_with_imdb_dir() {
            "(executable with --imdb-dir)"
        } else if suite.is_executable_with_tpch_dir() {
            "(executable with --tpch-dir)"
        } else {
            "(scaffold)"
        };
        println!("{} {}: {} queries", suite.label(), exec, queries.len());
        for q in queries {
            println!("  - {}", q.name);
        }
    }
    Ok(())
}
