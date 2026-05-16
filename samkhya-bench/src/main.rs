//! samkhya-bench CLI entry point.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
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

    /// Train a GBT residual corrector from a feedback store (requires `gbt` feature on samkhya-core).
    Train {
        /// Path to the feedback store to train from.
        #[arg(long)]
        feedback: PathBuf,
        /// Template hash to filter observations by (matches the suite name used during run).
        #[arg(long)]
        template: String,
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
            runner.run()
        }
        Command::Compare { suite, puffin_dir } => {
            samkhya_bench::report::compare(suite.into(), puffin_dir.as_deref())
        }
        Command::BuildPuffin { output, imdb_dir } => match (output, imdb_dir) {
            (Some(_), Some(_)) => {
                eprintln!("error: --output and --imdb-dir are mutually exclusive; pass only one");
                std::process::exit(2);
            }
            (Some(dir), None) => samkhya_bench::puffin_io::build_puffin_sidecars(&dir),
            (None, Some(dir)) => samkhya_bench::puffin_io::build_puffin_sidecars_imdb(&dir),
            (None, None) => {
                eprintln!(
                    "error: build-puffin requires either --output (synthetic) or --imdb-dir (JOB)"
                );
                std::process::exit(2);
            }
        },
        Command::Report { feedback } => samkhya_bench::report::summarize(&feedback),
        Command::Train { feedback, template } => {
            samkhya_bench::report::train_stub(&feedback, &template)
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
