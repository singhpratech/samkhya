# samkhya-core

[![crates.io](https://img.shields.io/crates/v/samkhya-core.svg)](https://crates.io/crates/samkhya-core)
[![docs.rs](https://docs.rs/samkhya-core/badge.svg)](https://docs.rs/samkhya-core)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE-APACHE)

A provable ceiling on join output cardinality, plus the portable sketches that
feed it. Hand it row counts and a degree bound per join attribute and it returns
a number the join cannot exceed on any database instance consistent with those
statistics, so an estimate can be clamped under something proved rather than
guessed. Engine-agnostic: no query engine appears in its dependency tree, and
the per-engine adapters are built on this crate.

```toml
[dependencies]
samkhya-core = "1.2"
```

## The ceiling

`samkhya_core::degree` implements a spanning-tree degree bound: root a spanning
tree of the join graph at `r`, then `|Q| <= |R_r| * prod maxdeg(R_v, a_uv)` over
its edges. Sound for bag semantics — what SQL engines actually execute — and
exactly tight on foreign-key joins.

```rust
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};

const ORDER_KEY: u32 = 0;

// 10 orders join 100 line items over 10 distinct order keys.
let orders = JoinRelation::new(10)
    .with_degree(ORDER_KEY, AttributeDegree::from_distinct(10, 10));
let lineitem = JoinRelation::new(100)
    .with_degree(ORDER_KEY, AttributeDegree::from_distinct(100, 10));

let graph = JoinGraph::new(vec![orders, lineitem]).with_edge(0, 1, ORDER_KEY);

// Exactly the true output. The Cartesian product would say 1000.
assert_eq!(graph.ceiling(), 100);
```

Degrees come from statistics you already have:

| `AttributeDegree::` | Source | Bound on `maxdeg` |
| ------------------- | ------ | ----------------- |
| `unknown` | row count | `rows`; ceiling degrades to the product |
| `from_distinct` | distinct-count **floor** | `rows - distinct + 1`; 1 on a key |
| `from_hll_floor` | `HllSketch::nonzero_registers` | same, from a sketch |
| `from_count_min` | largest Count-Min counter | tightest; `None` if saturated |

`HllSketch::estimate` is deliberately not a valid source: it is two-sided, so it
exceeds the truth about half the time, and the subtraction above would then hand
back a ceiling below the true cardinality. The Count-Min path is what makes the
ceiling portable — a sketch one engine wrote into a Puffin sidecar proves a
bound in another, with no shared catalog and no re-scan. `lpbound::clamp_estimate`
and `saturating_clamp` apply a ceiling to a corrector output.

## 1.2 repaired the bound family

An audit on 2026-07-24 checked every shipped bound against materialised
instances whose true cardinality was brute-forced. Three of four were unsound:
**2,179 violations in 3,704 bound-evaluations (58.8%)** through v1.1, a
violation being a "ceiling" that came back below the true output. 1.2 measures
**0 / 3,704**. Root cause per defect and reproduction commands are in
[bench-results/20_bound_soundness.md](https://github.com/singhpratech/samkhya/blob/main/bench-results/20_bound_soundness.md).

* `AgmBound` is **deprecated**: its `min * max` shortcut was not an AGM bound
  and was unsound for three or more relations. It now returns `ProductBound`.
* `ChainBound` was repaired — it derives degrees from its distinct counts and
  evaluates the degree ceiling. Larger numbers than v1.1 gave, and provable.
* `LpJoinBound::ceiling` and `ceiling_with_distinct` delegate to the degree
  bound; `ceiling_hypergraph` keeps the real fractional-edge-cover LP (`n^1.5`
  for a triangle) and needs an explicit attribute schema.
* Two published headlines were withdrawn: a 40.95x bound-tightness figure and a
  1.038x JOB-Slow speedup.

## Sketches and Puffin sidecars

Five sketches, each with a `to_bytes` / `from_bytes` codec and a stable `KIND`
tag used as the Iceberg Puffin blob type. `PuffinWriter` / `PuffinReader` write
and read the sidecar; `portable::PortableStatsSnapshot` is its decoded,
engine-neutral view, and unknown blob kinds are carried through, never errored.

| Type | Purpose | `KIND` |
| ---- | ------- | ------ |
| `HllSketch` | distinct count | `samkhya.hll-v1` |
| `BloomFilter` | membership | `samkhya.bloom-v1` |
| `CountMinSketch` | point frequency | `samkhya.cms-v1` |
| `EquiDepthHistogram` | 1D range | `samkhya.histogram-equidepth-v1` |
| `CorrelatedHistogram2D` | 2D joint distribution | `samkhya.correlated2d-v1` |

A build-write-reopen-verify round trip is in
[examples/sketch_to_puffin.rs](https://github.com/singhpratech/samkhya/blob/main/samkhya-core/examples/sketch_to_puffin.rs).

## Feature flags

| flag | default | what it adds |
| ---- | ------- | ------------ |
| `feedback` | **on** | `FeedbackStore` on SQLite; the one wasm32-hostile part |
| `lp_solver` | off | `LpJoinBound` + `ceiling_hypergraph` (`good_lp` / `microlp`) |
| `zstd` | off | zstd-compressed Puffin blobs |
| `gbt` | off | `GbtCorrector`, gradient-boosted residual model (`gbdt`) |
| `additive_gbt` | off | `AdditiveGbtCorrector`, absolute-count variant |
| `tabpfn_http` | off | `TabPfnHttpCorrector` over localhost HTTP |
| `llm_http` | off | `LlmHttpCorrector`, same wire contract, LLM behind it |

`--no-default-features` leaves the sketches, the ceiling and Puffin I/O — the
surface that compiles to wasm32.

## Scope and caveats

* The ceiling is only as sound as the degrees handed to it. Every
  `AttributeDegree` constructor derives that guarantee or documents it as the
  caller's obligation; an under-estimate makes the ceiling unsound.
* With no degree information the ceiling *is* the Cartesian product — the honest
  answer for that input, not a fallback bug.
* Cyclic queries bound loosely unless you supply the attribute schema to
  `ceiling_hypergraph` and declare which relations carry no private columns.
* The `Corrector` trait and its backends live here, but no measurement in this
  repo yet shows the repaired ceiling improving plan quality end to end.
* All `from_bytes` constructors take untrusted input and return `Err` rather
  than panicking. Fuzz targets sit in `samkhya-core/fuzz/`, a standalone
  nightly workspace run before release tags, not in CI.
* Payload codecs and `KIND` tags are pinned at v1 for the 1.x line. A format
  change takes a new tag; readers skip tags they do not know.

## License

Apache-2.0. Sole author: Prateek Singh.
