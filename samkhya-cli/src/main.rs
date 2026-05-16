//! samkhya — operator-facing CLI.
//!
//! Single binary that surfaces samkhya-core primitives so operators can
//! debug sidecars, inspect feedback stores, and build sketches from CSV
//! inputs without writing Rust. See `samkhya --help` for the full
//! subcommand tree.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use samkhya_core::Result;

mod cmd;

#[derive(Debug, Parser)]
#[command(
    name = "samkhya",
    about = "Operator CLI for samkhya: inspect sidecars, query feedback stores, build sketches",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Dump a Puffin sidecar's footer and decode known sketch kinds.
    Inspect {
        /// Path to the `.puffin` file.
        path: PathBuf,
    },

    /// Summarize a FeedbackStore SQLite file: observations, q-error,
    /// latency percentiles, per-template breakdown.
    Stats {
        /// Path to the SQLite feedback store.
        path: PathBuf,
    },

    /// Build a sketch from a CSV column.
    #[command(subcommand)]
    Sketch(SketchCmd),

    /// Pack or verify Puffin sidecars.
    #[command(subcommand)]
    Puffin(PuffinCmd),
}

#[derive(Debug, Subcommand)]
enum SketchCmd {
    /// Build an HLL sketch from a CSV column.
    Hll {
        /// CSV input path.
        #[arg(long)]
        input: PathBuf,
        /// 0-based column index to feed into the sketch.
        #[arg(long)]
        column: usize,
        /// HLL precision (4..=18).
        #[arg(long)]
        precision: u8,
        /// CSV has a header row; skip the first record.
        #[arg(long, default_value_t = false)]
        header: bool,
        /// Optional path to write the serialized sketch payload.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Build a Bloom filter from a CSV column.
    Bloom {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        column: usize,
        /// Capacity hint — expected number of distinct items.
        #[arg(long)]
        capacity: usize,
        /// Target false-positive rate (e.g. 0.01).
        #[arg(long, value_name = "RATE")]
        fp_rate: f64,
        #[arg(long, default_value_t = false)]
        header: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Build a Count-Min Sketch from a CSV column.
    Cms {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        column: usize,
        /// CMS depth (number of hash rows).
        #[arg(long)]
        depth: u32,
        /// CMS width (number of counters per row).
        #[arg(long)]
        width: u32,
        #[arg(long, default_value_t = false)]
        header: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Build an equi-depth histogram from a numeric CSV column.
    Histogram {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        column: usize,
        /// Number of equi-depth buckets.
        #[arg(long)]
        buckets: usize,
        #[arg(long, default_value_t = false)]
        header: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum PuffinCmd {
    /// Pack one or more sketch payload files into a single Puffin sidecar.
    ///
    /// Each `--hll/--bloom/--cms/--histogram FILE` may be repeated. Files
    /// must be raw sketch payloads (the format `samkhya sketch --output`
    /// writes). The CLI tags each blob with the matching KIND so the
    /// reader can decode them via the standard `Sketch::from_bytes`.
    Pack {
        /// Output Puffin file path.
        out: PathBuf,
        /// Payload file produced by `samkhya sketch hll --output`.
        #[arg(long, value_name = "FILE")]
        hll: Vec<PathBuf>,
        /// Payload file produced by `samkhya sketch bloom --output`.
        #[arg(long, value_name = "FILE")]
        bloom: Vec<PathBuf>,
        /// Payload file produced by `samkhya sketch cms --output`.
        #[arg(long, value_name = "FILE")]
        cms: Vec<PathBuf>,
        /// Payload file produced by `samkhya sketch histogram --output`.
        #[arg(long, value_name = "FILE")]
        histogram: Vec<PathBuf>,
    },
    /// Full structural validation of a Puffin file. Exits non-zero on any error.
    Verify {
        /// Path to the `.puffin` file.
        path: PathBuf,
    },
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect { path } => cmd::inspect::run(&path),
        Command::Stats { path } => cmd::stats::run(&path),
        Command::Sketch(sk) => match sk {
            SketchCmd::Hll {
                input,
                column,
                precision,
                header,
                output,
            } => cmd::sketch::hll(&input, column, precision, header, output.as_deref()),
            SketchCmd::Bloom {
                input,
                column,
                capacity,
                fp_rate,
                header,
                output,
            } => cmd::sketch::bloom(
                &input,
                column,
                capacity,
                fp_rate,
                header,
                output.as_deref(),
            ),
            SketchCmd::Cms {
                input,
                column,
                depth,
                width,
                header,
                output,
            } => cmd::sketch::cms(&input, column, depth, width, header, output.as_deref()),
            SketchCmd::Histogram {
                input,
                column,
                buckets,
                header,
                output,
            } => cmd::sketch::histogram(&input, column, buckets, header, output.as_deref()),
        },
        Command::Puffin(p) => match p {
            PuffinCmd::Pack {
                out,
                hll,
                bloom,
                cms,
                histogram,
            } => cmd::puffin::pack(&out, &hll, &bloom, &cms, &histogram),
            PuffinCmd::Verify { path } => cmd::puffin::verify(&path),
        },
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
