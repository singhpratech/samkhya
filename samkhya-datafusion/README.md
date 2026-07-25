# samkhya-datafusion

[![crates.io](https://img.shields.io/crates/v/samkhya-datafusion.svg)](https://crates.io/crates/samkhya-datafusion)
[![docs.rs](https://docs.rs/samkhya-datafusion/badge.svg)](https://docs.rs/samkhya-datafusion)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

The Apache DataFusion adapter for
[samkhya](https://github.com/singhpratech/samkhya): it publishes portable,
feedback-driven cardinality statistics into a DataFusion physical plan, and
optionally corrects join-input row counts before DataFusion picks a hash-join
build side. Corrected values are clamped under a ceiling, so a bad corrector
degrades the plan rather than blowing up planning.

Supported line: DataFusion **46.x** only.

## Install

```toml
[dependencies]
samkhya-datafusion = "1.2"
datafusion = "46"
```

## Pieces

- `SamkhyaTableProvider` — wraps any `TableProvider`, binds decoded samkhya
  statistics to explicit DataFusion column ordinals, and overrides `scan()` to
  return a `SamkhyaStatsExec` over the inner exec.
- `SamkhyaStatsExec` — passthrough `ExecutionPlan` whose `statistics()` returns
  preset `Statistics`. This is the surface the mainline planner reads;
  `TableProvider::statistics()` is not consulted by DataFusion 46.
- `SamkhyaOptimizerRule` — observe-only; validates the wrappers are present and
  counts `SamkhyaStatsExec` leaves for diagnostics.
- `SamkhyaPreJoinRule` + `install_pre_join_corrector` — run a
  `samkhya_core::residual::Corrector` on direct join inputs, immediately before
  DataFusion's built-in `join_selection` rule.

## Correcting join inputs

Appending a physical rule with `with_physical_optimizer_rule` runs *after*
`join_selection`, too late to change build-side or `PartitionMode`. Use the
installer, which inserts the rule at the right index. It is idempotent by rule
name, and returns `Err` when the session has no `join_selection` rule rather
than appending where it would be useless.

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
    PreJoinCorrectionOptions::default(),
));
let state = install_pre_join_corrector(state, rule)?;
// SessionContext::new_with_state(state) now corrects join inputs.
```

## What gets published

For each direct join input with a usable native row count, the rule publishes
`max(native, min(proposal, ceiling, SAFE_MAX_ROWS))` as `Precision::Inexact`,
and scales `total_byte_size` by the row ratio (DataFusion's `JoinSelection`
prefers bytes when both sides report them).

`PreJoinCorrectionOptions` fields:

- `ceiling` — operator-supplied upper bound, default `u64::MAX` ("none
  explicit").
- `derive_ceiling` — **new in 1.2, default `true`.** Derives a per-input
  ceiling from the input subplan's own shape: a join emits at most the product
  of its children's row counts, a filter at most its child's. Anything else,
  including leaves, yields no derived ceiling: a scan's row count *is* the
  statistic under correction, so treating it as a ceiling would forbid every
  upward correction. Arithmetic that saturates reports no ceiling rather than
  a meaningless one. Before 1.2 a stock configuration had no finite bound.
- `allow_below_native` — default `false`. Downward corrections are floored at
  DataFusion's native estimate so a corrector cannot make hash-build memory
  sizing more optimistic. If the floor and a ceiling conflict, the floor wins.

`SAFE_MAX_ROWS` is `1 << 40` (~1.1e12 rows) and always applies. It is a sanity
cap, not an overflow proof: DataFusion multiplies published row counts without
overflow checks, and two values at the cap still overflow when multiplied.

The rule is fail-open. `Ok(None)` and `Err(_)` from the corrector both retain
native statistics; correction never fails a query.
`SamkhyaPreJoinRule::metrics()` returns process-local counts of attempts,
applied, abstained, errors, clamped, and floored.

## Scope and caveats

- The ceiling described above is derived from the DataFusion plan's shape. The
  degree-based join-cardinality ceiling in `samkhya_core::degree` (`JoinGraph`,
  `JoinRelation`, `AttributeDegree`) is **not** wired into this adapter yet;
  supplying it is on the caller, via `ceiling`.
- `SamkhyaTableProvider` does not discover or open sidecar files. Load an
  Iceberg Puffin v1 file through `samkhya-iceberg` and pass the resulting
  `PortableStatsSnapshot` to `try_with_portable_stats(&snapshot, field_id,
  ordinal)` with an explicit field-ID-to-ordinal binding; the adapter never
  casts one numbering into the other.
- DataFusion 46 has no equi-depth-histogram statistics slot. A histogram stays
  in the portable snapshot but cannot populate `ColumnStatistics`; provider
  binding needs a scalar statistic (row count, distinct count, null count,
  min/max).
- No end-to-end query-speedup number is claimed for this adapter. The
  previously published 1.038x JOB-Slow figure was withdrawn in 1.2.

Runnable examples live at
https://github.com/singhpratech/samkhya/tree/main/samkhya-datafusion/examples

## License

Apache-2.0. Sole author: Prateek Singh.
