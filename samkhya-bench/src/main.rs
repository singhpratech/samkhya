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
    },

    /// Render a report from previous run output.
    Report,
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
        Command::Run { suite, baseline } => {
            let runner = Runner::new(suite.into(), baseline);
            runner.run()
        }
        Command::Report => {
            println!("report: placeholder — will summarise latency and q-error deltas");
            Ok(())
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
