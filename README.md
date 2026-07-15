# samkhya — सांख्य

> **samkhya is the engine-agnostic Rust SDK for portable, feedback-driven cardinality correction in embedded analytical engines.** Its load-bearing guarantee is *never-regress at the bound level*: every corrected estimate is clamped under a provable `LpJoinBound` ceiling, so a miscalibrated model or hallucinating LLM can never push the optimizer past a bound it can prove; with no feedback yet, cold start falls back to the engine's own estimate. One Puffin stats sidecar, written with `samkhya-core`, is loaded unchanged through Iceberg and exposed to DataFusion and the client-side DuckDB adapter. One `Corrector` trait, many swappable backends (GBT default · TabPFN-2.5 · LLM-pluggable). **13-crate workspace** (11 publishable crates), Apache-2.0, sole author.
>
> Every number here is pre-registered and measured, including the ones that missed. On the real Join-Order Benchmark (JOB-Slow — n=55 paired warm-cache queries from the 113-query IMDb suite, vs unmodified DataFusion 46), the geomean is **1.038×** (17 wins / 38 ties / 0 losses, BCa 95% CI [1.026, 1.056], Wilcoxon p=3×10⁻⁶) — statistically real, but **below the ≥1.35× I pre-registered, which is therefore falsified and reported as such.** The `LpJoinBound` is up to 40.95× *tighter than the AGM bound* on a synthetic star-5 microbenchmark (bound-tightness, not wallclock); see the [scoped headlines](#measured-headlines) for where that does and does not carry.

[![CI](https://github.com/singhpratech/samkhya/workflows/CI/badge.svg)](https://github.com/singhpratech/samkhya/actions)
[![crates.io](https://img.shields.io/crates/v/samkhya-core.svg)](https://crates.io/crates/samkhya-core)
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

## Quick start

Add the core crate to a Rust project:

```bash
cargo add samkhya-core
```

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
  ProductBound = 96   AgmBound = 24   ChainBound = 6   LpJoinBound = 6
  [ok] every bound >= true cardinality (sound inclusive ceiling)
  [ok] LpJoinBound <= AgmBound on this path join
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

---

## What's in 1.0

**Thirteen crates** in one Cargo workspace. Licensed under Apache-2.0
(explicit patent grant per §3). Edition 2024. MSRV Rust 1.85; CI tests
on 1.94 (the pinned project toolchain).

Layer 1 — portable stats foundation:
- `samkhya-core` — portable stats layer, feedback recorder, LpBound envelope,
  `Corrector` trait. No engine dependencies. 5 sketches all shipping: HLL,
  Bloom, Count-Min, equi-depth histogram, 2D correlated histogram.

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
- `samkhya-bench` — clap CLI: `list-queries`, `run`, `compare`, `report`,
  `train`, `calibrate`, `build-puffin`.
- `samkhya-it` — cross-crate integration test harness (`publish = false`).

Workspace clippy `-D warnings`, default tests, optional-engine tests, and the
cross-engine Puffin release fixture run in CI. Historical fuzz and benchmark
receipts remain under `bench-results/`.

---

<a name="measured-headlines"></a>
## Measured headlines (WAVE4-F + WAVE5-L2)

samkhya reports the *honest* head-to-head measurement, not a projection. The
numbers below are from the v1.0 empirical campaign (WAVE-4 / WAVE-5); v1.1 is
semver-compatible and carries them forward unchanged — a full re-run of the
JOB-Slow / TPC-H campaign is deferred past v1.1. The end-to-end real-workload
number leads; the synthetic microbenchmarks are scoped to exactly what they
measure. Where a pre-registered target was missed,
the row says so.

| Headline | Measured | CI / significance | Receipt |
|---|---|---|---|
| **JOB-Slow end-to-end vs DataFusion 46 (real IMDb, n=55 paired warm-cache, SF=1)** — *the honest real-workload number; pre-registered ≥1.35× FALSIFIED* | geomean **1.038×** wallclock; **17 wins / 38 ties / 0 losses**; BH-FDR rejects 24/55 | BCa 95% CI [1.026, 1.056]; Wilcoxon W=212 p=3.00×10⁻⁶ | `bench-results/18_vs_native_datafusion_wallclock.md` (WAVE4-F) |
| **Mixed/adversarial workload (7 pre-registered patterns A–G)** — *where samkhya LOSES, reported on purpose; H-G FALSIFIED* | cross-pattern geomean **0.949× (~5% slower)**; worst cell cold-start +12.4% | per-pattern CIs in receipt; burst P99 ≤ 212 µs @ 1000 QPS | `bench-results/17_failure_modes.md` |
| **LpJoinBound vs AGM bound *tightness* — star-5, p=1 (uniform skew)** *(synthetic microbenchmark; bound/truth ratio, NOT wallclock; collapses to 1.00× under p=2/p=∞ heavy-hitter cells)* | **40.95×** tighter than AGM | BCa 95% CI [30.93, 47.45]; Wilcoxon W=0 paired vs AGM p=1.73×10⁻⁶, n=30 | `bench-results/07_lpbound_tightness.md` |
| **TabPFN-2.5 inference latency** (RTX 4090 Laptop, B=8 L=128) | P95 **31.15 ms** (H1-A PASS) | BCa 95% CI [29.39, 35.32], strictly below 50 ms bar | `bench-results/14_tabpfn_4090_latency.md` (WAVE5-L2) |
| **HLL precision** (p=14, n=10⁶) | RSE **0.676%** | BCa 95% CI [0.535%, 0.848%] vs Flajolet 2007 0.8125% envelope | `bench-results/03_hll_precision_sweep.md` |
| **L4 v3 ablation** (A2→A3) | **−1.7%** median q-error reduction (BH-sig improvement) | BCa 95% CI [−2.8%, −0.7%], Wilcoxon p=0.0209 | WAVE5-E |

**Honest disclosures.** Pre-registered JOB-Slow upper bounds (≥1.6× join-heavy, ≥1.35× aggregate, ≥1.50× headline) all **FALSIFIED** by WAVE4-F. The corrector path is statistically real but the effect size is small; attributions are named in `bench-results/EVIDENCE.md` §4.2 (warm-cache only, CSV-not-Parquet, n=2 budget cap, OOM past q16a). TabPFN-2.5 q-error reduction over GBT is 7.84% (BCa [2.21, 14.62], p=1.04×10⁻⁵) — effect-direction confirmed, magnitude half the 15% pre-reg target (H1-B FALSIFIED on magnitude).

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

- **[samkhya v1.1: Never Regress](https://theaivibe.org/blog/samkhya-never-regress-cardinality-correction-deep-dive)**
  — the long-form deep dive: *putting a model in your query optimizer without
  letting it wreck the plan.* The narrative-first account of what samkhya is, the
  never-regress guarantee and the LpBound clamp, and the honest empirical results
  (including the pre-registered 1.35× that came in at 1.038×).
- [ARCHITECTURE.md](./ARCHITECTURE.md) — five-layer design, crate layout, data
  flow, integration surfaces, safety guarantees, glossary.
- [SECURITY.md](./SECURITY.md) — supported versions, disclosure policy, and
  the GitHub Security Advisories channel.
- [CHANGELOG.md](./CHANGELOG.md) — release history (v0.0.1 → v1.1.0).
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
