# samkhya-datafusion

[![crates.io](https://img.shields.io/crates/v/samkhya-datafusion.svg)](https://crates.io/crates/samkhya-datafusion)
[![docs.rs](https://docs.rs/samkhya-datafusion/badge.svg)](https://docs.rs/samkhya-datafusion)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

The [Apache DataFusion](https://datafusion.apache.org/) adapter for the
[samkhya](https://github.com/singhpratech/samkhya) project — portable,
feedback-driven cardinality correction for embedded analytical engines.

This crate wraps any DataFusion `TableProvider` with samkhya's cardinality
envelope and corrector, then re-injects the corrected row-count estimate
back into the physical plan so downstream planning (join ordering, hash
vs. sort-merge selection, partition fan-out) sees the better number.

## What this crate provides

A three-layer integration that plugs into stock DataFusion 46 without
forking the planner:

```
samkhya-datafusion
├── SamkhyaTableProvider          wraps any TableProvider; reads sidecar stats
├── SamkhyaStatsExec              physical wrapper that emits corrected statistics
└── SamkhyaOptimizerRule          rewrites plans so SamkhyaStatsExec is used
```

The three layers compose:

1. **TableProvider** — the user registers a `SamkhyaTableProvider` over
   their existing source (parquet, csv, custom). It looks up sidecar
   sketches in the table's storage layout and prepares the per-column
   stats.
2. **StatsExec** — at execution time, `SamkhyaStatsExec` overrides
   `ExecutionPlan::statistics()` with the corrected number from the
   `samkhya-core` corrector chain. DataFusion's planner reads the
   corrected stats and re-evaluates downstream cost.
3. **OptimizerRule** — registered on the `SessionContext`, the optimizer
   rule walks the physical plan and injects `SamkhyaStatsExec` over any
   leaf that has a registered sidecar. Plans without sidecars are
   untouched.

## Quick start

```rust
use std::sync::Arc;
use datafusion::prelude::*;
use samkhya_datafusion::{SamkhyaTableProvider, SamkhyaOptimizerRule};

let ctx = SessionContext::new();
ctx.add_optimizer_rule(Arc::new(SamkhyaOptimizerRule::new()));

let inner = /* any DataFusion TableProvider */;
let provider = SamkhyaTableProvider::new(inner, "stats.puffin")?;
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

## Compatibility

- DataFusion **46.x** is the supported line. Earlier DF versions had a
  different `TableProvider::scan` signature; samkhya does not try to
  back-port.
- The corrector chain consumed by `SamkhyaStatsExec` is `samkhya-core`'s
  `Corrector` trait — anything that implements `Corrector` works,
  including the identity, GBT, additive-GBT, and TabPFN backends.
- Sidecars consumed by `SamkhyaTableProvider` are Iceberg Puffin v1
  files. They can live next to the table data or in a separate stats
  store; the provider takes a path.

## Feature flags

| flag         | default | what it adds                              |
| ------------ | ------- | ----------------------------------------- |
| `gbt`        | on      | propagates `samkhya-core`'s `gbt` feature |
| `lp_solver`  | off     | propagates `samkhya-core`'s `lp_solver`   |

## Integration

This crate is the reference embedding pattern for samkhya. Engine authors
adding samkhya to another query system should mirror the three layers
(provider / stats-override / optimizer rule). The DuckDB and Polars
adapters do exactly this, adapted to their respective planner surfaces.

## License

Apache-2.0. Sole author: Prateek Singh.
