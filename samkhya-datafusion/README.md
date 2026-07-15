# samkhya-datafusion

[![crates.io](https://img.shields.io/crates/v/samkhya-datafusion.svg)](https://crates.io/crates/samkhya-datafusion)
[![docs.rs](https://docs.rs/samkhya-datafusion/badge.svg)](https://docs.rs/samkhya-datafusion)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

The [Apache DataFusion](https://datafusion.apache.org/) adapter for the
[samkhya](https://github.com/singhpratech/samkhya) project — portable,
feedback-driven cardinality correction for embedded analytical engines.

This crate wraps any DataFusion `TableProvider`, binds decoded samkhya
statistics to explicit DataFusion column positions, and publishes those
statistics from the physical scan plan. An optional pre-join rule can then
apply a bounded corrector before DataFusion chooses a hash-join build side.

## What this crate provides

A four-part integration that plugs into stock DataFusion 46 without
forking the planner:

```
samkhya-datafusion
├── SamkhyaTableProvider          binds portable stats to provider columns
├── SamkhyaStatsExec              physical wrapper that emits those statistics
├── SamkhyaOptimizerRule          validates wrappers and records diagnostics
└── SamkhyaPreJoinRule            applies a Corrector before join selection
```

The parts compose:

1. **TableProvider** — the user registers a `SamkhyaTableProvider` over
   their existing source (Parquet, CSV, Iceberg, or custom). The caller loads
   a `PortableStatsSnapshot` and explicitly binds each Iceberg field ID to a
   zero-based DataFusion column ordinal.
2. **StatsExec** — at execution time, `SamkhyaStatsExec` overrides
   `ExecutionPlan::statistics()` with the bound scalar statistics.
3. **OptimizerRule** — registered on the `SessionContext`, the optimizer
   rule observes the physical plan, validates that scan-time wrappers are
   present, and records diagnostic counts. `SamkhyaTableProvider::scan`
   installs the wrappers.
4. **PreJoinRule** — when installed with `install_pre_join_corrector`, applies
   a `Corrector` immediately before DataFusion's built-in join selection.

## Quick start

```rust
use std::sync::Arc;
use datafusion::datasource::TableProvider;
use datafusion::prelude::*;
use samkhya_core::PortableStatsSnapshot;
use samkhya_datafusion::SamkhyaTableProvider;

let ctx = SessionContext::new();

let inner: Arc<dyn TableProvider> = /* any DataFusion TableProvider */;
let portable: PortableStatsSnapshot = /* load through samkhya-iceberg */;

// Iceberg field ID 17 belongs to DataFusion column ordinal 0. The adapter
// never casts a field ID into a column position.
let provider = SamkhyaTableProvider::new(inner)
    .try_with_portable_stats(&portable, 17, 0)?;
ctx.register_table("orders", Arc::new(provider))?;

let df = ctx.sql("SELECT customer_id, COUNT(*) FROM orders GROUP BY 1").await?;
let plan = df.create_physical_plan().await?;
// Plan now contains a SamkhyaStatsExec node above the scan; EXPLAIN shows it.
```

A runnable end-to-end example, including before/after q-error numbers, is
at [`examples/b05_smoke.rs`](examples/b05_smoke.rs).

## Why a physical-plan wrapper, not a logical rule

DataFusion's logical optimizer runs before the physical-plan stage knows
which `ExecutionPlan` will be used for a scan, so a logical-only rewrite
can't carry corrected statistics into the right place. By inserting a
physical `SamkhyaStatsExec` above the scan and overriding `statistics()`
there, the planner's join-ordering and parallelism decisions see the
corrected number without us having to fork the cost model.

## Apply a Corrector before join selection

DataFusion 46 runs its built-in `join_selection` rule before distribution and
sorting enforcement. Appending a custom physical rule therefore runs too late
to change hash build-side or partition-mode decisions. Install samkhya's
pre-join rule with the ordering-aware helper:

```rust
use std::sync::Arc;
use datafusion::execution::session_state::SessionStateBuilder;
use samkhya_core::residual::{Corrector, IdentityCorrector};
use samkhya_datafusion::{
    install_pre_join_corrector, PreJoinCorrectionOptions, SamkhyaPreJoinRule,
};

let state = SessionStateBuilder::new().with_default_features().build();
let corrector: Arc<dyn Corrector> = Arc::new(IdentityCorrector);
let rule = Arc::new(SamkhyaPreJoinRule::new(
    corrector,
    PreJoinCorrectionOptions::with_ceiling(1_000_000),
));
let state = install_pre_join_corrector(state, rule).expect("join_selection is installed");
// SessionContext::new_with_state(state) now runs correction immediately
// before DataFusion's built-in join_selection rule.
```

The ceiling is an adapter-side clamp supplied by the caller; this rule does
not derive an LpBound. `PreJoinCorrectionOptions::default()` uses `u64::MAX`,
which means no finite adapter-side bound. `Ok(None)` and corrector errors retain
DataFusion's native statistics and do not fail query planning. Downward
corrections are also floored at DataFusion's native estimate by default, so a
model cannot make hash-build memory sizing more optimistic. Operators must use
the explicit `.with_allow_below_native(true)` opt-in to disable that floor; if
the native floor and adapter ceiling conflict, the native floor wins.

## Compatibility

- DataFusion **46.x** is the supported line. Earlier DF versions had a
  different `TableProvider::scan` signature; samkhya does not try to
  back-port.
- `SamkhyaPreJoinRule` consumes `samkhya-core`'s `Corrector` trait. Any
  enabled backend implementing that trait can be supplied by the caller.
- `SamkhyaTableProvider` does not discover or open sidecar paths. Load an
  Iceberg Puffin v1 file through `samkhya-iceberg`, then pass its
  `PortableStatsSnapshot` with an explicit field-to-ordinal binding.
- DataFusion 46 has no native equi-depth-histogram statistics slot. A
  histogram remains available in the portable snapshot, but an HLL-derived
  distinct count or another scalar statistic is required for provider binding.

## Integration

This crate is the reference embedding pattern for samkhya. Engine authors
adding samkhya to another query system should reuse the portable snapshot
handoff and provide an explicit engine-schema binding. Native planner hooks
vary by engine; the DuckDB client adapter does not currently override DuckDB's
optimizer estimates.

## License

Apache-2.0. Sole author: Prateek Singh.
