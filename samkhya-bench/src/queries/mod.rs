//! Query corpora for the benchmark harness.
//!
//! Each suite exposes a `queries()` function returning a slice of [`Query`]
//! entries. Suites are intentionally hand-written as `&'static str` so the
//! benchmark binary is hermetic and reproducible — no network fetches at
//! runtime.

pub mod job_slow;
pub mod stats_ceb;
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
    JobSlow,
    TpcH,
    StatsCeb,
}

impl Suite {
    /// Human-readable suite label.
    pub fn label(self) -> &'static str {
        match self {
            Suite::JobSlow => "job-slow",
            Suite::TpcH => "tpc-h",
            Suite::StatsCeb => "stats-ceb",
        }
    }

    /// Queries belonging to this suite.
    pub fn queries(self) -> &'static [Query] {
        match self {
            Suite::JobSlow => job_slow::QUERIES,
            Suite::TpcH => tpc_h::QUERIES,
            Suite::StatsCeb => stats_ceb::QUERIES,
        }
    }
}
