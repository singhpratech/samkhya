//! samkhya-bench CLI entry point.

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
        feedback: Option<std::path::PathBuf>,
    },

    /// Run a suite twice (baseline + samkhya) and print a side-by-side comparison.
    Compare {
        /// Which benchmark suite to run.
        #[arg(long, value_enum)]
        suite: SuiteArg,
    },

    /// Render a report from a feedback store.
    Report {
        /// Path to the feedback store (SQLite) to summarize.
        #[arg(long)]
        feedback: std::path::PathBuf,
    },

    /// Train a GBT residual corrector from a feedback store (requires `gbt` feature on samkhya-core).
    Train {
        /// Path to the feedback store to train from.
        #[arg(long)]
        feedback: std::path::PathBuf,
        /// Template hash to filter observations by (matches the suite name used during run).
        #[arg(long)]
        template: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SuiteArg {
    JobSlow,
    TpcH,
    StatsCeb,
    Synthetic,
}

impl From<SuiteArg> for Suite {
    fn from(value: SuiteArg) -> Self {
        match value {
            SuiteArg::JobSlow => Suite::JobSlow,
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
        } => {
            let mut runner = Runner::new(suite.into(), baseline);
            if let Some(path) = feedback {
                runner = runner.with_feedback_path(path);
            }
            runner.run()
        }
        Command::Compare { suite } => samkhya_bench::report::compare(suite.into()),
        Command::Report { feedback } => samkhya_bench::report::summarize(&feedback),
        Command::Train { feedback, template } => {
            samkhya_bench::report::train_stub(&feedback, &template)
        }
    }
}

fn list_queries() -> Result<()> {
    for suite in [
        Suite::JobSlow,
        Suite::TpcH,
        Suite::StatsCeb,
        Suite::Synthetic,
    ] {
        let queries = suite.queries();
        let exec = if suite.is_executable() {
            "(executable)"
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
