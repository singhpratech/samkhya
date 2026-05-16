# samkhya-polars

Polars adapter for [samkhya](../).

## Status

Polars currently has no public optimizer-rule extension API
(see [pola-rs/polars#23345](https://github.com/pola-rs/polars/issues/23345)),
so this crate cannot inject corrected cardinality hints into a `LazyFrame`
plan the way `samkhya-datafusion` does through a `TableProvider`. Until
that upstream gap closes, integration is exposed through two surfaces
gated behind the optional `engine` feature.

## Feature flag

```toml
[dependencies]
samkhya-polars = { version = "0.0.1", features = ["engine"] }
```

The default build keeps a single `samkhya-core` dependency and a
backwards-compatible stub, so workspace builds that do not need Polars
stay lean. The `engine` feature pulls in `polars = "0.44"` with every
`dtype-*` sub-feature except `dtype-decimal`; the latter is excluded
because its feature graph in `polars-mem-engine 0.44` ends up pulling
`pyo3 0.21` and collides with `samkhya-py`'s `pyo3 0.22` link line.

## What the `engine` feature ships

### `sketcher` — Series → Sketch builders

Pure-Rust helpers that consume a `polars::Series` and return a sketch
from `samkhya_core::sketches`, ready to serialize through the `Sketch`
trait into a Puffin sidecar:

| Helper | Returns | Notes |
|---|---|---|
| `hll_from_series(&Series, precision)` | `HllSketch` | Distinct-count sketch for equality selectivity. |
| `bloom_from_series(&Series, fp_rate)` | `BloomFilter` | Capacity is sized to `series.len()`. |
| `cms_from_series(&Series, depth, width)` | `CountMinSketch` | Heavy-hitter / skew detection. |
| `histogram_from_series(&Series, buckets)` | `EquiDepthHistogram` | Numeric only; non-numeric columns return `Error::InvalidSketch`. |

Numeric values are hashed as little-endian bytes; strings and binary
values as their raw bytes; everything else falls back to a debug-format
string so distinct-count semantics remain stable. Nulls are skipped.

### `feedback_wrapper` — `LazyFrame::collect()` with observation recording

```rust
use samkhya_polars::feedback_wrapper::lazy_collect_with_feedback;

let df = lazy_collect_with_feedback(lf, &store, "tpl-q42")?;
```

Times the `collect()` call, then writes an `Observation` to the
`FeedbackStore` with `actual_rows = df.height()` and `est_rows = 0`.
The estimate is hard-coded to zero because Polars does not yet expose a
plan-level row estimate through its public API; downstream consumers
should treat `est_rows == 0` rows as actual-only samples until the
upstream estimator is exposed.

## Why this crate exists today

The samkhya architecture diagram in `samkhya.md` §3 lists Polars
alongside DataFusion, DuckDB, and gpudb as integration targets. The
`engine`-gated helpers above let downstream pipelines:

- Materialize samkhya sketches from any Polars DataFrame column and
  publish them via the Iceberg Puffin codec in `samkhya-core::puffin`.
- Record real `(estimated, actual)` row counts into a `FeedbackStore`
  for the residual-correction model in `samkhya-core::residual`, even
  before Polars supports plan-level injection.

The optimizer-rule injection point remains pending upstream
([pola-rs/polars#23345](https://github.com/pola-rs/polars/issues/23345));
once it lands, this crate will gain a `LazyFrame` rewriter analogous to
`SamkhyaTableProvider`.

## License

Apache-2.0, inherited from the workspace.
