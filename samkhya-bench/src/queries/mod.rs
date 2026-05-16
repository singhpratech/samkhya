//! Query corpora for the benchmark harness.
//!
//! Each suite exposes a `queries()` function returning a slice of [`Query`]
//! entries. Suites are intentionally hand-written as `&'static str` so the
//! benchmark binary is hermetic and reproducible — no network fetches at
//! runtime.

pub mod job_slow;
pub mod stats_ceb;
pub mod synthetic;
pub mod tpc_h;

/// A single benchmark query.
///
/// `name` is the canonical identifier used by the upstream suite (for example
/// `"1a"` for JOB-Slow or `"Q5"` for TPC-H). `sql` is the literal SQL text
/// fed to the engine under test.
#[derive(Debug, Clone, Copy)]
pub struct Query {
    pub name: &'static str,
    pub sql: &'static str,
}

/// The benchmark suites recognised by the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    /// JOB-Slow scaffold — query texts only, no data wired in. Reports
    /// every query as skipped from [`crate::runner::Runner::run`].
    JobSlow,
    /// JOB-Slow against real IMDb data. Only executable when the runner
    /// is built with `--imdb-dir <path>`; otherwise behaves like
    /// [`Suite::JobSlow`].
    JobSlowReal,
    TpcH,
    StatsCeb,
    Synthetic,
}

impl Suite {
    /// Human-readable suite label.
    pub fn label(self) -> &'static str {
        match self {
            Suite::JobSlow => "job-slow",
            Suite::JobSlowReal => "job-slow-real",
            Suite::TpcH => "tpc-h",
            Suite::StatsCeb => "stats-ceb",
            Suite::Synthetic => "synthetic",
        }
    }

    /// Queries belonging to this suite.
    pub fn queries(self) -> &'static [Query] {
        match self {
            Suite::JobSlow | Suite::JobSlowReal => job_slow::QUERIES,
            Suite::TpcH => tpc_h::QUERIES,
            Suite::StatsCeb => stats_ceb::QUERIES,
            Suite::Synthetic => synthetic::QUERIES,
        }
    }

    /// Whether this suite can be executed in-process today **without** any
    /// runtime configuration.
    ///
    /// Only `Synthetic` runs against built-in in-memory tables. `JobSlowReal`
    /// is conditionally executable when the runner is given `--imdb-dir`;
    /// that path is governed by [`Suite::is_executable_with_imdb_dir`].
    /// The bare `JobSlow` and the other corpora are scaffolding only.
    pub fn is_executable(self) -> bool {
        matches!(self, Suite::Synthetic)
    }

    /// Whether this suite becomes executable when an IMDb data directory
    /// is supplied at runtime. Currently only [`Suite::JobSlowReal`] flips
    /// to executable in that case.
    pub fn is_executable_with_imdb_dir(self) -> bool {
        matches!(self, Suite::JobSlowReal)
    }
}
