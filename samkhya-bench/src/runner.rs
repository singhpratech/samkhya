//! Benchmark runner — placeholder implementation.
//!
//! In the full harness, [`Runner`] will execute each query twice: once with
//! the engine's native plan and once with samkhya's cardinality correction
//! enabled, then emit latency, plan-shape, and q-error deltas. For now it is
//! a stub that records its configuration and prints what it would do.

use crate::queries::Suite;
use samkhya_core::Result;

/// Configuration for a single benchmark run.
#[derive(Debug, Clone)]
pub struct Runner {
    suite: Suite,
    baseline: bool,
}

impl Runner {
    /// Build a new runner.
    ///
    /// `baseline = true` means "execute the engine's native plan only" —
    /// no samkhya correction. `baseline = false` enables the full
    /// portable-stats + feedback-driven correction path.
    pub fn new(suite: Suite, baseline: bool) -> Self {
        Self { suite, baseline }
    }

    /// The suite under test.
    pub fn suite(&self) -> Suite {
        self.suite
    }

    /// Whether the runner is in baseline (native plan only) mode.
    pub fn is_baseline(&self) -> bool {
        self.baseline
    }

    /// Execute the configured suite.
    ///
    /// This is a placeholder until the DataFusion and DuckDB adapters land.
    /// It enumerates the queries it would run and returns `Ok(())`.
    pub fn run(&self) -> Result<()> {
        let mode = if self.baseline {
            "baseline (native plan)"
        } else {
            "samkhya-corrected"
        };
        let queries = self.suite.queries();
        println!(
            "runner: would execute {} {} queries from suite {} in {} mode",
            queries.len(),
            self.suite.label(),
            self.suite.label(),
            mode,
        );
        for q in queries {
            println!("  - {}", q.name);
        }
        Ok(())
    }
}
