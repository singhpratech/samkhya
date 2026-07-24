# samkhya-qdrant

**Provable match-count ceilings for filtered vector search.**

## The decision

"Find the 10 nearest vectors *where* `category = 'shoes'`" can run two ways:

- **Pre-filter** — materialise the matches, compare them exactly. Cheap when
  the filter is selective.
- **Post-filter** — search the whole index, discard non-matches. Roughly
  constant cost, but wasteful when the filter is selective, and it may return
  too few surviving results to fill `k`.

Choosing requires knowing how many points match *before* matching them. Every
engine that supports filtered search estimates this; Qdrant keeps payload-index
cardinality statistics for exactly this decision.

## Why a bound beats an estimate here

The failure that hurts is **under**-estimating: the planner decides the filter
is selective, pre-filters, then walks a set far larger than budgeted. A
two-sided estimate under-shoots about half the time.

A Count-Min sketch never *under*-counts. So for an equality condition its
estimate is a **provable upper bound** on matching points — the planner decides
against a number the truth cannot exceed. Same one-sided-error argument that
makes samkhya's join ceiling provable, applied to filter selectivity.

The bound composes soundly:

| Filter | Sound ceiling |
| ------ | ------------- |
| `A AND B` | `min(bound(A), bound(B))` |
| `A OR B` | `min(total, bound(A) + bound(B))` |
| `NOT A` | `total` — no lower bound on `A` is available, so none is claimed |

`NOT` is deliberately weak rather than quietly wrong. A loose ceiling costs a
suboptimal plan; an unsound one costs a wrong plan.

## Usage

```rust
use samkhya_qdrant::{Condition, Filter, PayloadStats, SearchStrategy, StrategyParams};
use samkhya_core::sketches::CountMinSketch;

let mut category = CountMinSketch::with_defaults();
for _ in 0..40 { category.add(b"shoes", 1); }
for _ in 0..9_960 { category.add(b"other", 1); }

let stats = PayloadStats::new(10_000).with_field("category", category);
let filter = Filter::must(vec![Condition::match_value("category", "shoes")]);

assert!(stats.bound_matches(&filter) >= 40);          // never below the truth
assert_eq!(
    stats.choose_strategy(&filter, &StrategyParams::default()),
    SearchStrategy::PreFilter
);
```

## Scope — read this before assuming more

This crate computes bounds and recommends a strategy. **It does not link
Qdrant, run a server, or execute a search.** The decision surface is a pure
function so it can be tested against brute force and embedded wherever you like.

Wiring it into a running engine is a separate piece of work, and this README
will not pretend otherwise until that work exists.

Apache-2.0. Sole author: Prateek Singh.
