# samkhya-polars

[![crates.io](https://img.shields.io/crates/v/samkhya-polars.svg)](https://crates.io/crates/samkhya-polars)
[![docs.rs](https://docs.rs/samkhya-polars/badge.svg)](https://docs.rs/samkhya-polars)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Polars adapter for samkhya. Builds samkhya sketches directly from a
`polars::series::Series` and wraps `LazyFrame::collect()` with feedback
recording, so downstream pipelines can produce Puffin sidecars and train the
residual corrector even before Polars exposes an optimizer-rule extension
API upstream.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## What this crate provides

Behind the `engine` cargo feature:

- **`sketcher::hll_from_series(&Series, precision)`** — `HllSketch` for
  distinct-count / equality selectivity.
- **`sketcher::bloom_from_series(&Series, fp_rate)`** — `BloomFilter` sized
  to `series.len()`, for join pre-filtering.
- **`sketcher::cms_from_series(&Series, depth, width)`** — `CountMinSketch`
  for heavy-hitter and skew detection.
- **`sketcher::histogram_from_series(&Series, buckets)`** —
  `EquiDepthHistogram` for range-predicate selectivity. Numeric only;
  non-numeric columns return `Error::InvalidSketch`.
- **`feedback_wrapper::lazy_collect_with_feedback(lf, &store, template)`** —
  times `collect()` on a `LazyFrame` and writes an `Observation` to the
  given `FeedbackStore` with `actual_rows = df.height()` and `est_rows = 0`.

Always available (no feature flag):

- **`column_stats_for(table, col)`** — placeholder accessor for a future
  Polars-side stats provider; returns `None` until Polars exposes optimizer
  hooks.

## Quick start

```rust
use polars::prelude::*;
use samkhya_polars::sketcher::{hll_from_series, histogram_from_series};

let s = Series::new("customer_id".into(), &[1i64, 2, 3, 1, 2]);
let hll = hll_from_series(&s, 12)?;
println!("approx distinct = {}", hll.estimate());

let nums = Series::new("amount".into(), &[1.0f64, 2.5, 3.7, 4.1, 5.0]);
let hist = histogram_from_series(&nums, 4)?;
println!("buckets = {:?}", hist.buckets());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Feature flags

- `engine` (off by default) — pulls in `polars = "0.44"` with every
  `dtype-*` sub-feature except `dtype-decimal` (excluded to avoid a
  resolver collision through workspace feature unification with other
  crates depending on `pyo3 0.22`). With the feature off, the default
  build keeps a single `samkhya-core` dependency and a back-compat stub —
  workspace builds that do not need Polars stay lean.

## Hashing strategy

- Numeric variants → little-endian bytes (matches the encoding used by
  `samkhya-core`'s own sketch tests, so values hashed here collide with
  values added directly via the core API).
- `Boolean` → single byte.
- Strings and binary → underlying bytes.
- Anything else → debug-format string bytes; stable enough for
  distinct-count / membership purposes.

Nulls are skipped. Multi-chunk inputs from `LazyFrame::collect()` are
rechunked transparently before iteration.

## Status

Polars currently has no public optimizer-rule extension API (tracked
upstream in
[pola-rs/polars#23345](https://github.com/pola-rs/polars/issues/23345)),
so this crate cannot inject corrected cardinality hints into a `LazyFrame`
plan the way `samkhya-datafusion` does through a `TableProvider`. Until
that upstream gap closes, integration is exposed through the surfaces
documented above. Once optimizer hooks land, this crate will gain a
`LazyFrame` rewriter analogous to `SamkhyaTableProvider`.

## Integration

A Polars-based pipeline imports `samkhya-polars` with `features = ["engine"]`
to:

- Materialize samkhya sketches from any DataFrame column and publish them
  via the Iceberg Puffin codec in `samkhya-core::puffin`.
- Record real `(estimated, actual)` row counts into a `FeedbackStore` for
  the residual-correction model in `samkhya-core::residual`, even before
  Polars supports plan-level injection.

## License

Apache-2.0. Sole author: Prateek Singh.
