# samkhya — सांख्य

> **A join-cardinality ceiling you can prove, and the portable statistics that
> feed it.** samkhya clamps a corrected estimate under a bound the join provably
> cannot exceed, so a miscalibrated model — or a hallucinating LLM — cannot push
> the optimizer past a number that has been proved. With no feedback yet, cold
> start falls back to the engine's own estimate. One Puffin sidecar written with
> `samkhya-core` is read unchanged through Iceberg, DataFusion and DuckDB. One
> `Corrector` trait, many swappable backends. **15-crate workspace**, Apache-2.0,
> sole author.
>
> **Read the correction first.** A 2026-07-24 audit found that the ceiling shipped
> through v1.1 was *not actually provable* — it fell below the true cardinality in
> 58.8% of measured trials, and the harness measuring it clamped every violation
> to "perfectly tight" so it could not report the fact. v1.2.0 repairs it:
> **0 violations in 3,704 bound-evaluations**, and on a foreign-key join the
> ceiling is exactly the true output. Two headline numbers were withdrawn in the
> process — the 40.95× bound-tightness figure and the 1.038× JOB-Slow speedup.
> Both retractions are reproduced against committed raw data in
> [`bench-results/20_bound_soundness.md`](bench-results/20_bound_soundness.md) and
> [`bench-results/OPEN_AUDIT_ITEMS.md`](bench-results/OPEN_AUDIT_ITEMS.md).

[![Live site](https://img.shields.io/badge/site-live-2F3B8C?logo=github&logoColor=white)](https://singhpratech.github.io/samkhya/)
[![Live demo](https://img.shields.io/badge/demo-interactive-356443?logo=webassembly&logoColor=white)](https://singhpratech.github.io/samkhya/demo.html)
[![CI](https://github.com/singhpratech/samkhya/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/singhpratech/samkhya/actions/workflows/ci.yml?query=branch%3Amain)
[![crates.io](https://img.shields.io/crates/v/samkhya-core.svg)](https://crates.io/crates/samkhya-core)
[![PyPI](https://img.shields.io/pypi/v/samkhya.svg)](https://pypi.org/project/samkhya/)
[![docs.rs](https://img.shields.io/docsrs/samkhya-core)](https://docs.rs/samkhya-core)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](#license)

The name is the Sanskrit word सांख्य — *"enumeration / counting"* — a classical
darshana whose discipline is counting reality's constituents honestly. The
library's only job is to make row counts accurate for the engines that have
been left without an answer: DuckDB, DataFusion, Polars, Postgres, Iceberg,
and gpudb.

---

## Why samkhya?

- **Portability via Iceberg Puffin sidecars.** Classical sketches (HLL, Bloom,
  Count-Min, equi-depth histogram, 2D correlated histogram) are serialized to
  versioned, `KIND`-tagged blobs inside [Iceberg Puffin](https://iceberg.apache.org/puffin-spec/)
  files. The same sidecar an ELT pipeline writes is loaded through Iceberg and
  handed to the DataFusion and client-side DuckDB adapters without rewriting
  its sketch payloads. No engine owns the stats; the sidecar does.
- **Safety via the LpBound clamp.** Every corrected estimate is bounded above
  by a provable pessimistic ceiling inspired by Zhang et al.'s LpBound, SIGMOD 2025
  Best Paper — the idea, not a reimplementation: an LP relaxation over ℓp-norms of
  degree sequences, with no machine learning
  involved. A miscalibrated model can never push a corrected estimate past
  this ceiling, and with no feedback cold start falls back to the engine's
  native estimate — a bound-level never-regress guarantee. (Wallclock is a
  separate axis: on some workloads samkhya is slower; the
  [failure-mode row](#measured-headlines) says exactly where.)
- **Pluggable corrector backend (production GBT default; TabPFN-2.5 + LLM-pluggable opt-in).**
  The `Corrector` trait is the pluggable surface and the *contribution*: one
  trait, a production GBT default plus opt-in research / HTTP backends. Default ships a sub-MB
  gradient-boosted-tree backend (gbdt-rs, Baidu). TabPFN-2.5 (Hollmann ICLR
  2023 + Prior Labs 2026 update) opt-in behind `tabpfn_http` feature —
  **measured P95 31.15 ms at B=8 L=128 on RTX 4090 Laptop, BCa 95% CI [29.39,
  35.32]**, q-error reduction 7.84% vs GBT on synthetic. **LLM-pluggable
  HTTP corrector ships dual transport in v1.0:** canonical Python FastAPI
  server (`samkhya-gpudb/scripts/llm_infer_server.py`, port 8766 —
  this is what `bench-results/19_llm_corrector.md` §4.1 measured) and a
  parity Node TypeScript port (`llm_infer_server.ts`, port 8767, same
  wire contract, broader operator appeal). Four reference backends in
  each: Anthropic, OpenAI, local Ollama, dummy. The TS port's 30-trial
  paired benchmark campaign remains future evidence work (smoke-tested at v1.0).
  Every backend gated behind a Cargo feature flag and capped from above
  by the LpBound safety envelope.

samkhya is a library, not a service. No daemon, no background thread, no GPU
requirement in the default build. The entire workspace builds in under two
minutes on a laptop with no network access.

---

## Why now: you can put an LLM under a provable ceiling

Everybody wants a language model in the query planner. Almost nobody ships one,
and the blocker was never capability — it is blast radius.

A planner does not fail gracefully on a bad row count. Ask a model how many rows
a six-way join emits, get back a confidently wrong number, and the optimizer
builds a hash table on the wrong side and exhausts memory. The expected case is
fine. The tail eats you.

A ceiling changes the shape of that risk:

- **Clamped from above.** Whatever the backend proposes is held under a bound
  derived by counting. Since 1.2 the DataFusion path derives one by default —
  before that, the guarantee was absent unless an operator wired it up by hand.
- **Fail-open.** Any transport error, timeout, malformed reply or provider
  outage returns `Ok(None)` and the engine falls back to its own estimate. A
  provider going down must never surface as a query failure.
- **Opt-in.** Behind the `llm_http` cargo feature, against an endpoint you
  control — Anthropic, OpenAI, local Ollama, or a dummy echo. Off in the
  default build.
- **Swappable.** The same `Corrector` trait takes gradient-boosted trees,
  TabPFN-2.5, or an LLM. The safety contract does not change with the backend.

**What is measured, and what is not.** The transport floor is measured; every
live-LLM accuracy cell is marked *projected*, pending API keys. And samkhya's own
GBT corrector made held-out q-error **worse** in the one honest measurement so
far — 13.46 → 26.41, fitted on a single usable row.

That result is the case *for* the ceiling, not against it. A corrector that
doubles your error is exactly what the clamp exists for. **The claim is not that
a model will help. It is that you can find out safely.**

---

## Quick start

Add the core crate to a Rust project:

```bash
cargo add samkhya-core
```

Or compute a provable ceiling from JavaScript — 84 KB of WebAssembly, generated
TypeScript types, no server and no native module:

```js
import init, { HllSketch, joinCeiling } from 'samkhya';
await init();

// 10 orders joined to 100 line items over 10 distinct keys.
joinCeiling([10, 100], [0, 1], [10, 10]);   // 100 — exactly the true output
joinCeiling([10, 100], [0, 1], []);         // 1000 — the Cartesian product
```

`joinCeiling` subtracts the distinct count, so it needs one that is never *above*
the truth. Pass `hll.distinctFloor()`, not `hll.estimate()` — the point estimate
is two-sided and would produce a ceiling below the truth.

Build a Puffin sidecar from a column:

```rust
use samkhya_core::puffin::{Blob, PuffinWriter};
use samkhya_core::sketches::{HllSketch, Sketch};
use std::fs::File;
use std::io::BufWriter;

let mut hll = HllSketch::new(12)?;
for v in &column { hll.add(v); }
let payload = hll.to_bytes()?;
let file = BufWriter::new(File::create("orders.puffin")?);
let mut w = PuffinWriter::new(file);
let (snapshot_id, sequence_number) = (42, 7); // From the Iceberg snapshot.
// Puffin `fields` contains stable Iceberg field IDs, not zero-based ordinals.
w.add_blob_for_snapshot(
    Blob::new(HllSketch::KIND, vec![17], &payload),
    snapshot_id,
    sequence_number,
)?;
w.finish()?;
```

Consume those stats from DataFusion via the table-provider adapter:

```rust
use datafusion::prelude::SessionContext;
use samkhya_datafusion::SamkhyaTableProvider;
use samkhya_iceberg::snapshot::load_portable_stats_from_table;
use std::sync::Arc;

let ctx = SessionContext::new();
let portable = load_portable_stats_from_table(&iceberg_table).await?;
// Bind stable Iceberg field ID 17 to DataFusion's zero-based column ordinal 0.
let provider = SamkhyaTableProvider::new(inner_provider)
    .try_with_portable_stats(&portable, 17, 0)?;
ctx.register_table("orders", Arc::new(provider))?;
```

The wrapper returns a `SamkhyaStatsExec` from `scan()`, so the validated
portable statistics are visible on the physical plan without a logical-plan
rewrite. LpBound-clamped correction is a separate pre-join rule described
below.

### See the guarantees run (no dataset, no setup)

Both load-bearing properties are a single runnable example:

```bash
cargo run -p samkhya-core --example honest_demo --features lp_solver
```

```text
  TRUE output cardinality (counted from data) = 5
  ProductBound = 96   ChainBound = 6   degree ceiling = 6
  [ok] every bound >= true cardinality (sound inclusive ceiling)
  [ok] ProductBound >= degree ceiling (a real refinement, still provable)
  corrector proposed 1000000 → saturating_clamp = 6 (<= ceiling, never regresses)
  built HllSketch(p=12), 1000 distinct keys → estimate = 997
  wrote Puffin sidecar → reopened → reconstructed estimate = 997
  [ok] estimate round-trips exactly across write→read (997 == 997)
```

Part 1 is the never-regress clamp — an over-eager corrector proposing 1,000,000
rows is held to the provable ceiling of 6. Part 2 is one HLL sidecar written and
read back byte-faithfully — the portability primitive. Every number is computed
by the run, not hardcoded. Source:
[`samkhya-core/examples/honest_demo.rs`](./samkhya-core/examples/honest_demo.rs).

### A provable ceiling from statistics you already have

The ceiling is not an estimate. Given row counts and a distinct count per join
key — what an HLL sidecar already carries — it returns a number the join cannot
exceed, and on foreign-key joins it is *exactly* the true output:

```rust
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};

const ORDER_KEY: u32 = 0;

// 10 orders, 100 line items, 10 distinct order keys on both sides.
let orders = JoinRelation::new(10)
    .with_degree(ORDER_KEY, AttributeDegree::from_distinct(10, 10));
let lineitem = JoinRelation::new(100)
    .with_degree(ORDER_KEY, AttributeDegree::from_distinct(100, 10));

let graph = JoinGraph::new(vec![orders, lineitem]).with_edge(0, 1, ORDER_KEY);

assert_eq!(graph.ceiling(), 100);   // the Cartesian product would say 1000
```

Degrees can come from a row count (`maxdeg ≤ rows`, always true, degrades to the
product), a distinct-count *floor* (`AttributeDegree::from_hll_floor`), or a
Count-Min sketch (`from_count_min`, tightest under skew). The Count-Min route is
what makes the ceiling portable: `true_freq(k) ≤ estimate(k) ≤ max counter`, so
one sketch riding in a Puffin sidecar bounds every key's degree at once in
whichever engine reads it.

The theorem, the proof, and the brute-force verification are in
[`bench-results/20_bound_soundness.md`](bench-results/20_bound_soundness.md).

### Train a corrector and measure it honestly

A corrector is only worth anything if you can show it helps on queries it has
not seen. The bench harness does the three steps separately so that is
enforceable rather than aspirational:

```bash
# 1. Run a training subset, recording the plan features the corrector will see.
samkhya-bench run --suite synthetic --feedback train.db --only S1,S2,S3,S4,S5

# 2. Fit a model and freeze it to disk.
samkhya-bench train --feedback train.db --template samkhya-bench-synthetic \
    --out model.json

# 3. Evaluate on the complement — queries the model has never seen.
samkhya-bench run --suite synthetic --exclude S1,S2,S3,S4,S5 \
    --corrector gbt --model model.json
```

Freezing the model between steps 2 and 3 is the point: a model written before
the evaluation queries ran cannot have seen them. Skip the split and you measure
memorisation — on this suite, evaluating on the training queries shows q-error
improving 4.58 → 1.86, while the honest held-out run shows it *worsening*
13.46 → 26.41. Same model, same code, opposite conclusion.

---

## What's in 1.2

**Thirteen crates** in one Cargo workspace. Licensed under Apache-2.0
(explicit patent grant per §3). Edition 2024. MSRV Rust 1.85; CI tests
on 1.94 (the pinned project toolchain).

Layer 1 — portable stats foundation:
- `samkhya-core` — portable stats layer, feedback recorder, join-ceiling
  envelope, `Corrector` trait. No engine dependencies. 5 sketches all shipping:
  HLL, Bloom, Count-Min, equi-depth histogram, 2D correlated histogram.
  - `degree` (new in 1.2) — the provable spanning-tree degree ceiling, with
    degrees derived from a row count, a distinct-count floor, or a Count-Min
    sketch. This is the bound samkhya actually ships.
  - `lpbound` — `ProductBound`, the repaired `ChainBound`, and
    `LpJoinBound::ceiling_hypergraph` (the genuine fractional-edge-cover LP,
    which needs an explicit attribute hypergraph). `AgmBound` is deprecated:
    its `min × max` shortcut was never an AGM bound.
  - `feedback` — `PlanObservation` records the plan features the corrector sees
    at inference time, so training and serving share a feature space.

Layer 2 — engine adapters (3 production — DataFusion · Iceberg · Arrow;
DuckDB + Polars beta; Postgres scaffold):
- `samkhya-datafusion` — `SamkhyaTableProvider` + `SamkhyaStatsExec` +
  `SamkhyaOptimizerRule` three-layer integration into DataFusion 46.
- `samkhya-duckdb` — Rust-client integration against DuckDB 1.x via
  `bundled` feature flag.
- `samkhya-duckdb-ext` — cxx extension scaffold (staticlib+rlib in v1.0;
  cdylib + runtime LOAD waits on DuckDB Issue #11638).
- `samkhya-polars` — Series-to-sketch helpers + `lazy_collect_with_feedback`
  on polars 0.44, behind `engine` feature.
- `samkhya-postgres` — pgrx-shaped extension, double-gated behind
  `pg_extension` feature + `samkhya_pgrx_enabled` rustc cfg (pg17 pin per
  WAVE5-A).
- `samkhya-iceberg` — Puffin sidecar reader/writer with KIND-tag registration.
- `samkhya-arrow` — Arrow IPC round-trip helpers for all 5 sketch types.

Layer 3 — corrector backends + GPU + Python:
- `samkhya-gpudb` — Layer 4 reservation. `GpuCorrector` trait +
  `CpuFallbackCorrector` reference impl. TabPFN-2.5 backend via opt-in
  HTTP transport (`tabpfn_http` feature). **LLM-pluggable HTTP
  corrector** ships dual transport (Python FastAPI + Node TypeScript,
  same wire contract) under `scripts/llm_infer_server.{py,ts}`, with
  Anthropic / OpenAI / local Ollama / dummy backends for each. See
  `bench-results/19_llm_corrector.md` for the end-to-end campaign.
- `samkhya-py` — PyO3 0.29 bindings, single abi3-py39 wheel, published to
  PyPI as `samkhya`.

Layer 4 — tools:
- `samkhya-cli` — single-binary evaluator: `build`, `decode`, `stats`,
  `info`, `compare`.
- `samkhya-wasm` — the JavaScript / TypeScript surface. `samkhya-core`
  compiled to WebAssembly: five sketches, the provable ceiling, Puffin I/O.
  84 KB, generated `.d.ts`, no server and no native module. Built and verified;
  not yet published to npm.
- `samkhya-qdrant` — provable match-count ceilings for filtered vector search.
  A Count-Min sketch never undercounts, so for an equality condition its
  estimate is an upper bound on matching points — which is exactly what the
  pre-filter / post-filter decision needs. Computes bounds; does not link an
  engine, and says so.
- `samkhya-bench` — clap CLI: `list-queries`, `run`, `compare`, `report`,
  `train`, `calibrate`, `build-puffin`. `train` fits and freezes a real GBT
  corrector; `run --corrector gbt --model <path>` evaluates with it, and
  `--only` / `--exclude` keep the training and evaluation sets disjoint.
- `samkhya-it` — cross-crate integration test harness (`publish = false`).

Workspace clippy `-D warnings`, default tests, optional-engine tests, and the
cross-engine Puffin release fixture run in CI. Historical fuzz and benchmark
receipts remain under `bench-results/`.

**Test surface.** 415 tests on CI's `cargo test --locked --workspace --exclude samkhya-py`, plus 272 under `samkhya-core --all-features`,
all green, clippy and rustfmt clean, MSRV 1.85 compiling. The suites that carry
the load for 1.2 are worth naming, because the defects they cover were all
silent:

| Suite | What it establishes |
| ----- | ------------------- |
| `samkhya-core/tests/soundness_degree.rs` | 6 properties × 2,048 cases: builds relation instances, brute-forces the true join, asserts `ceiling ≥ truth`. The absolute property, not an ordering between bounds. |
| `samkhya-core/tests/corrector_features.rs` | The corrector learns from a non-baseline feature, the legacy path provably does not, and a frozen model predicts identically after reload. |
| `samkhya-core/tests/feedback_migration.rs` | A pre-1.2 store upgrades in place, keeps its rows, and the migration is idempotent. |
| `samkhya-bench/tests/corrector_flow.rs` | `--only` / `--exclude` genuinely partition a suite, and training refuses featureless rows instead of padding them with zeros. |
| `samkhya-core/tests/v1_compat.rs` | Frozen v1 sketch payloads still decode byte-for-byte. |
| `samkhya-wasm/src/lib.rs` | The JS surface bounds a foreign-key join at exactly 100, degrades to the product without distinct counts, and survives a misbuilt join graph without panicking. |
| `samkhya-qdrant/src/lib.rs` | The match-count ceiling dominates brute force across five selectivities, and a saturated sketch gives up rather than lying. |

---

<a name="measured-headlines"></a>
## Measured headlines

samkhya reports the *honest* head-to-head measurement, not a projection — and
when a measurement turns out not to support the claim made from it, the row is
withdrawn rather than defended. A 2026-07-24 audit withdrew two: the JOB-Slow
end-to-end result and the 40.95× bound-tightness figure. Both retractions are
reproduced against committed raw data in
[`bench-results/OPEN_AUDIT_ITEMS.md`](bench-results/OPEN_AUDIT_ITEMS.md) and
[`bench-results/20_bound_soundness.md`](bench-results/20_bound_soundness.md).

What survives is below. Rows still resting on the v1.0 campaign (WAVE-4 /
WAVE-5) are marked; the audit register lists which of them have open questions
against them.

| Headline | Measured | CI / significance | Receipt |
|---|---|---|---|
| **JOB-Slow end-to-end vs DataFusion 46** — ⚠ **WITHDRAWN, under re-run** | no corrector was active in the "corrected" arm; 55 of 113 queries ran before an OOM kill; fixed arm order (removing the cold trial: 1.038× → 1.013×); per-query inference unattainable at n=2; q-error structurally pinned at 1.0 | every point reproduced against the committed raw data | `bench-results/OPEN_AUDIT_ITEMS.md` §2 |
| **Mixed/adversarial workload (7 pre-registered patterns A–G)** — *where samkhya LOSES, reported on purpose; H-G FALSIFIED* | cross-pattern geomean **0.949× (~5% slower)**; worst cell cold-start +12.4% | per-pattern CIs in receipt; burst P99 ≤ 212 µs @ 1000 QPS | `bench-results/17_failure_modes.md` |
| **Bound soundness — does the ceiling ever fall below the truth?** *(1,080 materialised instances × 4 bounds; 154 saturated trials excluded)* | **0 violations in 3,704** bound-evaluations *(v1.1: 2,179 — 58.8%)* | brute-forced true cardinality per instance; 6 properties × 2,048 proptest cases | `bench-results/20_bound_soundness.md` |
| **Bound tightness on a foreign-key join** *(10 orders ⋈ 100 line items, 10 distinct keys)* | ceiling **100** — exactly the true output, vs 1,000 for the Cartesian product | derived from a distinct count samkhya already carries in its sidecar | `bench-results/20_bound_soundness.md` |
| **TabPFN-2.5 inference latency** (RTX 4090 Laptop, B=8 L=128) | P95 **31.15 ms** (H1-A PASS) | BCa 95% CI [29.39, 35.32], strictly below 50 ms bar | `bench-results/14_tabpfn_4090_latency.md` (WAVE5-L2) |
| **HLL precision** (p=14, n=10⁶) | RSE **0.676%** | BCa 95% CI [0.535%, 0.848%] vs Flajolet 2007 0.8125% envelope | `bench-results/03_hll_precision_sweep.md` |
| **L4 v3 ablation** (A2→A3) — *provenance under review, see audit §5* | **−1.7%** median q-error reduction (BH-sig improvement) | BCa 95% CI [−2.8%, −0.7%], Wilcoxon p=0.0209 | WAVE5-E |
| **Corrector on held-out queries** *(synthetic suite: fit on S1–S5, evaluate on S6–S10)* | q-error geomean **13.46 → 26.41** — the corrector makes it **worse** | fitted on 1 usable row; the same model on its own training queries shows 4.58 → 1.86, which is the train-on-eval artefact | `samkhya-bench/tests/corrector_flow.rs` |

**Honest disclosures.** Pre-registered JOB-Slow upper bounds (≥1.6× join-heavy, ≥1.35× aggregate, ≥1.50× headline) were all **FALSIFIED** by WAVE4-F — and as of 2026-07-24 that campaign is withdrawn outright, not merely falsified: it measured portable sidecar statistics rather than correction, on an OOM-truncated corpus, with per-query statistics that cannot support the claims made from them. See `bench-results/OPEN_AUDIT_ITEMS.md` §2. TabPFN-2.5 q-error reduction over GBT is 7.84% (BCa [2.21, 14.62], p=1.04×10⁻⁵) — effect-direction confirmed, magnitude half the 15% pre-reg target (H1-B FALSIFIED on magnitude).

### The 1000 → 42 demonstration (kept for the mechanism it proves)

Without samkhya, a 1000-row table wrapped only in DataFusion 46's default
TableProvider reports `num_rows = 1000` to the physical plan. Wrap the same
provider with `SamkhyaTableProvider` plus the optimizer rule, and the physical
plan reports `num_rows = 42`. The `stats_propagation_demo` example prints:
*"without rule: 1000, with rule: 42"* — proving the corrected estimate, clamped
by LpBound, propagates through `SamkhyaStatsExec::statistics()`. Mechanism, not
headline.

---

## Architecture

The five layers — each replaceable, each failing safely toward the engine's
native plan:

```
+----------------------------------------------------------------+
| Layer 5  Pluggable corrector backend  (Corrector trait surface)
|          GBT default · TabPFN-2.5 opt-in · LLM dual transport  |
|          (FastAPI :8766 + TypeScript :8767), all shipping v1.0 |
+----------------------------------------------------------------+
| Layer 4  GPU Batch Inference  (optional, via gpudb)            |
|          one CUDA / Metal launch scores thousands of subplans  |
+----------------------------------------------------------------+
| Layer 3  LpBound Envelope  (NEVER REGRESS)                     |
|          provable upper bound; corrections clamped from above  |
+----------------------------------------------------------------+
| Layer 2  Feedback Recorder  (LEO / Bao / AutoSteer pattern)    |
|          SQLite (plan, estimate, actual); residual GBT trained |
+----------------------------------------------------------------+
| Layer 1  Portable Stats  (Iceberg Puffin + classical sketches) |
|          HLL / Bloom / CMS / equi-depth / correlated2D         |
+----------------------------------------------------------------+
```

See [ARCHITECTURE.md](./ARCHITECTURE.md) for the full developer-facing design,
including data-flow diagrams and the `samkhya-core` module map.

---

## Cross-engine matrix

| Engine     | Adapter             | Status        | Notes                                                                  |
|------------|---------------------|---------------|------------------------------------------------------------------------|
| DataFusion | `samkhya-datafusion`| Production    | Three-layer integration against DataFusion 46; first-class target.     |
| DuckDB     | `samkhya-duckdb` / `samkhya-duckdb-ext` | Beta + scaffold | Rust-client path behind `bundled`; cxx extension v1.0 staticlib+rlib only; cdylib + runtime LOAD waits on DuckDB Issue #11638. |
| Polars     | `samkhya-polars`    | Beta          | Series-to-sketch helpers behind `engine`; optimizer hook pending upstream Polars Issue #23345. |
| Postgres   | `samkhya-postgres`  | Scaffold      | pgrx-shaped stub. Double-gated behind `pg_extension` feature + `samkhya_pgrx_enabled` rustc cfg, pg17 pin (per WAVE5-A); real planner / executor hooks await a dedicated compatibility release. |
| Iceberg    | `samkhya-iceberg`   | Production    | Puffin sidecar reader/writer with KIND-tag registration for all 5 sketch types. |
| Arrow      | `samkhya-arrow`     | Production    | Arrow IPC round-trip helpers; byte-identical for all 5 sketch types. |
| GPU        | `samkhya-gpudb`     | CPU prod + GPU opt-in | `GpuCorrector` trait + `CpuFallbackCorrector` reference impl. TabPFN-2.5 HTTP backend via `tabpfn_http` feature (measured P95 31.15 ms on RTX 4090 Laptop). LLM-pluggable HTTP corrector dual transport — Python FastAPI :8766 + Node TypeScript :8767, same wire contract. |

---

## Documentation

Reading and reference:

- **[Bound-soundness audit and repair](bench-results/20_bound_soundness.md)** — how the v1.1
  ceiling turned out not to be provable, why the tightness harness could not see it,
  and the theorem the 1.2.0 bound rests on. Paired with
  [`OPEN_AUDIT_ITEMS.md`](bench-results/OPEN_AUDIT_ITEMS.md), the register of what the
  same audit raised that 1.2.0 does *not* answer.
- **[samkhya v1.1: Never Regress](https://theaivibe.org/blog/samkhya-never-regress-cardinality-correction-deep-dive)**
  — the long-form deep dive: *putting a model in your query optimizer without
  letting it wreck the plan.* The narrative-first account of what samkhya is, the
  never-regress guarantee and the LpBound clamp, and the honest empirical results
  (including the pre-registered 1.35× that came in at 1.038× — itself since withdrawn).
- [ARCHITECTURE.md](./ARCHITECTURE.md) — five-layer design, crate layout, data
  flow, integration surfaces, safety guarantees, glossary.
- [SECURITY.md](./SECURITY.md) — supported versions, disclosure policy, and
  the GitHub Security Advisories channel.
- [CHANGELOG.md](./CHANGELOG.md) — release history (v0.0.1 → v1.2.3).
- [CONTRIBUTING.md](./CONTRIBUTING.md) — how to file bugs, PRs, and run the
  test suite.
- [REPRODUCIBILITY.md](./REPRODUCIBILITY.md) — ACM AE v1.1 reviewer entry,
  5-step reproducer workflow.
- [CITATION.cff](./CITATION.cff) — academic citation metadata (cff-1.2.0).

Source repository: <https://github.com/singhpratech/samkhya>.

---

## Prior work, fairly framed

samkhya stands on the shoulders of a substantial body of cardinality-estimation
research — MSCN, Naru, NeuroCard, DeepDB, BayesCard, FLAT, FACE, Neo, Balsa,
RTOS, Bao, AutoSteer, Lero, ALECE, ByteCard, PRICE, TiCard, LpBound. These are
not dead ends; they are prior attempts that hit the embedded-tier budget limit.
The 2018-2020 wave assumed a server-class DBMS with a long-lived optimizer
process that could amortize a 40-300 MB model and 5-50 ms inference. The
embedded reality — sub-50 ms cold start, sub-200 MB total memory, sub-ms
per-estimate latency, single-query lifetimes — was outside that envelope. The
2021-2022 critique papers (*"Are We Ready For Learned CE?"*, *"In-depth Study
of Learned CE"*) were honest about the limitations; the production-database
field routed around them via adaptive query execution, a technique that is
structurally inapplicable to engines without a long-lived process.

samkhya's design exists to transcend the embedded-tier limitations: portable
stats survive between sessions; the feedback recorder borrows the
*observe-and-hint* pattern from Bao and AutoSteer (the only learned-QO pattern
with documented production deployment); the LpBound envelope makes cold-start
safety provable rather than aspirational; and the residual-correction interface
is designed so a future foundation-model backend drops in without churn. The
prior insights are the ones samkhya extends; the prior limitations are the
ones it is built to bypass.

---

## Security

Report vulnerabilities through
[GitHub Security Advisories](https://github.com/singhpratech/samkhya/security/advisories/new).
Do not file public issues for security reports. The disclosure policy and the
list of supported versions are documented in [SECURITY.md](./SECURITY.md).

---

## License

Licensed under **Apache License 2.0** (single license, explicit patent
grant per §3). Sole author: Prateek Singh.

Matches the licensing posture of the surrounding analytical-engine
ecosystem — DataFusion, Iceberg, ClickHouse, Apache Arrow itself — and
gives every downstream user the same explicit patent grant rather than
making it optional via a dual-license toggle. Full text in
[LICENSE-APACHE](./LICENSE-APACHE).

## Citations (industry-standard anchors)

- Hollmann et al. — **TabPFN: Transformers solve small tabular problems.** ICLR 2023.
- Atserias, Grohe, Marx — **Size bounds and query plans for relational joins.** PODS 2008.
- Zhang, Mayer, Abo Khamis, Olteanu, Suciu — **LpBound: Pessimistic Cardinality Estimation Using Lp-Norms of Degree Sequences.** SIGMOD 2025 (Best Paper).
- Leis et al. — **How good are query optimizers, really?** VLDB 2015 (Join Order Benchmark).
- Moerkotte et al. — **Preventing bad plans by bounding the impact of cardinality estimation errors.** VLDB 2009 (q-error).
- Efron & Tibshirani — **An Introduction to the Bootstrap**, ch. 14 (BCa). Chapman & Hall, 1993.
- Wilcoxon — **Individual comparisons by ranking methods.** Biometrics Bulletin 1945.
- Benjamini & Hochberg — **Controlling the false discovery rate.** JRSSB 1995.
- Flajolet et al. — **HyperLogLog.** DMTCS 2007.
- Bloom — **Space/time trade-offs in hash coding with allowable errors.** CACM 1970.
- Cormode & Muthukrishnan — **An improved data stream summary: the Count-Min Sketch.** J. Algorithms 2005.
- Ioannidis & Poosala — **Balancing histogram optimality and practicality.** SIGMOD 1996 (MaxDiff).
- Jagadish et al. — **Optimal histograms with quality guarantees.** VLDB 1998 (V-Optimal).
- Stillger et al. — **LEO: DB2's LEarning Optimizer.** SIGMOD 2001 (feedback-driven QO).
- ACM Artifact Evaluation v1.1 — reproducibility-badge methodology.
