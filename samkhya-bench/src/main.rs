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
    BuildPuffin {
        /// Output directory for the sidecars.
        #[arg(long)]
        output: PathBuf,
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
            runner.run()
        }
        Command::Compare { suite, puffin_dir } => {
            samkhya_bench::report::compare(suite.into(), puffin_dir.as_deref())
        }
        Command::BuildPuffin { output } => samkhya_bench::puffin_io::build_puffin_sidecars(&output),
        Command::Report { feedback } => samkhya_bench::report::summarize(&feedback),
        Command::Train { feedback, template } => {
            samkhya_bench::report::train_stub(&feedback, &template)
        }
        Command::Calibrate {
            suite,
            feedback,
            puffin_dir,
        } => samkhya_bench::calibrate::calibrate(
            suite.into(),
            feedback.as_deref(),
            puffin_dir.as_deref(),
        ),
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
