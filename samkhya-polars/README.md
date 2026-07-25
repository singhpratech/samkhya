# samkhya-polars

[![crates.io](https://img.shields.io/crates/v/samkhya-polars.svg)](https://crates.io/crates/samkhya-polars)
[![docs.rs](https://docs.rs/samkhya-polars/badge.svg)](https://docs.rs/samkhya-polars)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

Build samkhya sketches from a Polars `Series`, and feed those sketches into a
provable join-cardinality ceiling — a number the join provably cannot exceed.

This is an adapter, not a Polars optimizer integration. Polars has no public
optimizer-rule extension API ([pola-rs/polars#23345](https://github.com/pola-rs/polars/issues/23345)),
so nothing here rewrites a `LazyFrame` plan, injects statistics, or changes how
Polars executes anything. Every helper is called explicitly by your code.

## Install

```toml
[dependencies]
samkhya-polars = { version = "1.2", features = ["engine"] }
samkhya-core = "1.2"
```

The `engine` feature is off by default. Without it the crate is two no-op
placeholders and a single `samkhya-core` dependency, so workspace members that
never touch Polars do not pay for polars in their build graph.

## From a Series to a ceiling

10 orders join 100 line items over 10 distinct customer ids. The Cartesian
product says 1,000 rows. The ceiling says 100, which is the exact answer.

```rust
use polars::prelude::{NamedFrom, Series};
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_polars::sketcher::cms_from_series;

let orders =
    Series::new("customer_id".into(), &[0i64, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
let items: Vec<i64> = (0..100i64).map(|i| i % 10).collect();
let items = Series::new("customer_id".into(), items);

let orders_cms = cms_from_series(&orders, 5, 1024).unwrap();
let items_cms = cms_from_series(&items, 5, 1024).unwrap();

const CUSTOMER_ID: u32 = 0; // attribute ids are caller-assigned
let orders_deg = AttributeDegree::from_count_min(10, &orders_cms);
let items_deg = AttributeDegree::from_count_min(100, &items_cms);

let graph = JoinGraph::new(vec![
    JoinRelation::new(10).with_degree(CUSTOMER_ID, orders_deg),
    JoinRelation::new(100).with_degree(CUSTOMER_ID, items_deg),
])
.with_edge(0, 1, CUSTOMER_ID);

assert_eq!(graph.ceiling(), 100);
```

The row counts are yours to supply (`df.height()`, a catalog, a Parquet
footer). The sketch supplies the degree.

## Which degree constructor is sound

A degree bound must never under-state the true maximum number of rows sharing
one key value. Under-stating it produces a ceiling below the true cardinality,
which is the exact failure the ceiling exists to prevent.

- `AttributeDegree::from_count_min(rows, &cms)` — the tightest sketch source.
  Count-Min never undercounts, so its largest counter bounds every key at once.
- `AttributeDegree::from_hll_floor(rows, &hll)` — uses the sketch's
  distinct-count *floor*, not its point estimate.
- Do not pass `HllSketch::estimate()` to `AttributeDegree::from_distinct`. That
  estimate is two-sided, so it exceeds the truth about half the time, and
  `from_distinct` subtracts it — an over-stated distinct count yields an
  unsound ceiling.

## Surface

Behind `engine`, in `sketcher`:

- `hll_from_series(&Series, precision) -> Result<HllSketch>`
- `bloom_from_series(&Series, fp_rate) -> Result<BloomFilter>`, sized to `series.len()`
- `cms_from_series(&Series, depth, width) -> Result<CountMinSketch>`
- `histogram_from_series(&Series, buckets) -> Result<EquiDepthHistogram>`, numeric
  columns only; anything else returns `Error::InvalidSketch`

All four implement `samkhya_core::sketches::Sketch`, so `to_bytes()` drops
straight into a Puffin blob via `samkhya_core::puffin` for another engine to
read back.

In `feedback_wrapper`: `lazy_collect_with_feedback(lf, &store, template_hash)`
times `LazyFrame::collect()` and records the resulting `actual_rows` into a
`FeedbackStore`. See the caveat below before planning around it.

Always compiled: `column_stats_for(table, col)`, which returns `Ok(None)`
unconditionally, and `build_sketches_from_series_stub()`, a no-op. Both are
placeholders held for API stability.

## Value encoding

Numeric variants hash as little-endian bytes, matching `samkhya-core`'s own
encoding, so a value hashed here collides with the same value added directly
through the core API. `Boolean` is one byte; strings and binary hash their
underlying bytes. Everything else — `List`, `Struct`, `Categorical`, temporal —
falls back to debug-format bytes, which is fine for distinct-count and
membership within one build but is not a format to persist across Polars
versions. Nulls are skipped; multi-chunk series are rechunked first.

## Caveats

- The feedback wrapper writes `est_rows = 0`, because Polars 0.44 exposes no
  plan-level row estimate publicly. `GbtCorrector::train` skips observations
  with `est_rows == 0`, so these rows cannot train a residual corrector. They
  are an actual-row log — enough to check a ceiling against truth, nothing more.
- Pinned to `polars = "0.44"`, with every `dtype-*` sub-feature except
  `dtype-decimal`, whose feature graph pulls a second `pyo3` and collides with
  `samkhya-py` under workspace feature unification.

Apache-2.0. Part of [samkhya](https://github.com/singhpratech/samkhya).
