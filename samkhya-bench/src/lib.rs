//! samkhya-bench — benchmark harness for portable cardinality correction.
//!
//! Provides query corpora (JOB-Slow, TPC-H, STATS-CEB) and a runner that
//! compares samkhya-corrected plans against baseline plans on embedded
//! analytical engines.

pub mod queries;
pub mod runner;
