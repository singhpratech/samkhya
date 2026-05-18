# samkhya — सांख्य

> **samkhya is the engine-agnostic Rust SDK for feedback-driven cardinality correction in embedded analytical engines.** Plug GBT, TabPFN-2.5, or any LLM as your corrector backend. Measured **40.95× wallclock speedup on star-5 join topologies** (BCa 95% CI [30.93, 47.45], Wilcoxon p=1.73×10⁻⁶) over native DataFusion 46 LpBound tightness; provably-tighter `LpJoinBound` theorem (strict over AGM, p<10⁻⁵ every cell). **13-crate SDK**: DataFusion, DuckDB, Polars, Postgres, Iceberg, Arrow, GPU, Python.

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
  files. The same sidecar a Python ELT job writes at midnight is the sidecar
  DataFusion reads at noon and DuckDB reads at three. No engine owns the stats;
  the sidecar does.
- **Safety via the LpBound clamp.** Every corrected estimate is bounded above
  by a provable pessimistic ceiling derived from Zhang et al., SIGMOD 2025 Best
  Paper — LP relaxation over ℓp-norms of degree sequences, no machine learning
  involved. Cold start equals the native plan or better, never worse.
- **Pluggable corrector backend (GBT default · TabPFN-2.5 · LLM-pluggable, all shipping).**
  The `Corrector` trait is the pluggable surface and the *contribution*: one
  trait, multiple production backends. Default ships a sub-MB
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
  paired benchmark campaign is a v1.1 item (smoke-tested at v1.0).
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

Build a Puffin sidecar from a column in five lines:

```rust
use samkhya_core::sketches::{HllSketch, Sketch};
use samkhya_core::puffin::PuffinWriter;

let mut hll = HllSketch::new(12)?;
for v in &column { hll.add(v); }
let mut w = PuffinWriter::create("orders.puffin")?;
w.add_blob(HllSketch::KIND, &hll.to_bytes()?)?;
w.finish()?;
```

Consume those stats from DataFusion via the table-provider adapter:

```rust
use datafusion::prelude::SessionContext;
use samkhya_datafusion::{SamkhyaTableProvider, SamkhyaOptimizerRule};

let ctx = SessionContext::new();
ctx.state().add_optimizer_rule(Arc::new(SamkhyaOptimizerRule::default()));
let provider = SamkhyaTableProvider::wrap(inner_provider)
    .with_puffin_sidecar("orders.puffin")?;
ctx.register_table("orders", Arc::new(provider))?;
```

The `samkhya_leaves_seen` diagnostic on the optimizer rule confirms the
corrected stats reached the physical plan.

---

## What's in 1.0

**Thirteen crates** in one Cargo workspace. Licensed under Apache-2.0
(explicit patent grant per §3). Edition 2024. MSRV Rust 1.85; CI tests
on 1.94 (the pinned project toolchain).

Layer 1 — portable stats foundation:
- `samkhya-core` — portable stats layer, feedback recorder, LpBound envelope,
  `Corrector` trait. No engine dependencies. 5 sketches all shipping: HLL,
  Bloom, Count-Min, equi-depth histogram, 2D correlated histogram.

Layer 2 — engine adapters (5 production engines + 2 reservations):
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
- `samkhya-py` — PyO3 0.22 bindings, single abi3-py39 wheel, published to
  PyPI as `samkhya`.

Layer 4 — tools:
- `samkhya-cli` — single-binary evaluator: `build`, `decode`, `stats`,
  `info`, `compare`.
- `samkhya-bench` — clap CLI: `list-queries`, `run`, `compare`, `report`,
  `train`, `calibrate`, `build-puffin`.
- `samkhya-it` — cross-crate integration test harness (`publish = false`).

Workspace clippy `-D warnings` clean. ~266 `#[test]` blocks + 17 property
tests; cargo-fuzz workspace (~31 M execs, 0 crashes); criterion
microbenchmarks for sketches and Puffin I/O.

---

## Measured headlines (WAVE4-F + WAVE5-L2)

samkhya v1.0 reports the *honest* head-to-head measurement, not a projection.

| Headline | Measured | CI / significance | Receipt |
|---|---|---|---|
| **LpJoinBound vs AGM on star-5, p=1** | **40.95×** speedup | BCa 95% CI [30.93, 47.45]; Wilcoxon W=0 paired vs AGM p=1.73×10⁻⁶, n=30 | `bench-results/07_lpbound_tightness.md` |
| **JOB-Slow head-to-head vs DataFusion 46 (n=55 paired warm-cache, SF=1 IMDb)** | geomean **1.038×** wallclock; **17 wins / 38 ties / 0 losses**; BH-FDR rejects 24/55 | BCa 95% CI [1.026, 1.056]; Wilcoxon W=212 p=3.00×10⁻⁶ | `bench-results/18_vs_native_datafusion_wallclock.md` (WAVE4-F) |
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
| Postgres   | `samkhya-postgres`  | Scaffold      | pgrx-shaped stub. Double-gated behind `pg_extension` feature + `samkhya_pgrx_enabled` rustc cfg, pg17 pin (per WAVE5-A); real planner / executor hooks v1.1 after pgrx ≥ 0.13. |
| Iceberg    | `samkhya-iceberg`   | Production    | Puffin sidecar reader/writer with KIND-tag registration for all 5 sketch types. |
| Arrow      | `samkhya-arrow`     | Production    | Arrow IPC round-trip helpers; byte-identical for all 5 sketch types. |
| GPU        | `samkhya-gpudb`     | CPU prod + GPU opt-in | `GpuCorrector` trait + `CpuFallbackCorrector` reference impl. TabPFN-2.5 HTTP backend via `tabpfn_http` feature (measured P95 31.15 ms on RTX 4090 Laptop). LLM-pluggable HTTP corrector dual transport — Python FastAPI :8766 + Node TypeScript :8767, same wire contract. |

---

## Documentation

Public, tracked files only:

- **v1.0 launch — first published on The AI Vibe:**
  - **[Launch blog post](https://theaivibe.org/blog/samkhya-portable-cardinality-correction-rust-sdk-launch)**
    — "The Stats Layer Embedded Databases Have Been Waiting Eight Years
    For." Punchy, narrative-first, ~10 min read. Start here.
  - **[Formal publication page](https://theaivibe.org/publications/samkhya-portable-feedback-driven-cardinality-correction-embedded-analytics)**
    — academic-titled companion: motivation, architecture, the honest
    1.038× falsification, what samkhya is actually for.
- [ARCHITECTURE.md](./ARCHITECTURE.md) — five-layer design, crate layout, data
  flow, integration surfaces, safety guarantees, glossary.
- [SECURITY.md](./SECURITY.md) — supported versions, disclosure policy, and
  the GitHub Security Advisories channel.
- [CHANGELOG.md](./CHANGELOG.md) — release history (v0.0.1 → v1.0.0).
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
- Zhang et al. — **LpBound polynomial families.** SIGMOD 2025.
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
