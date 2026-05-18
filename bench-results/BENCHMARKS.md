# samkhya v1.0 empirical campaign — master index

**Date:** 2026-05-16 (UTC)
**Sole author:** Prateek Singh
**License:** Apache-2.0
**Companion documents:** [`../DEFENSE.md`](../DEFENSE.md), [`../EVIDENCE.md`](../EVIDENCE.md),
[`../KILL_CRITERIA_REPORT.md`](../KILL_CRITERIA_REPORT.md),
[`../ARCHITECTURE.md`](../ARCHITECTURE.md), [`../paper/draft.md`](../paper/draft.md).

This file is the **navigation hub** for the v1.0 empirical campaign. The campaign
is the second layer on top of the 20-agent binary-acceptance wave already
recorded in [`BINARY_ACCEPTANCE_REPORT.md`](./BINARY_ACCEPTANCE_REPORT.md) and
the `B0x_*.md` companion files. Binary acceptance proves the artefacts *install
and run*; the campaign indexed here proves the engineering and statistical
claims samkhya makes about cardinality, sketches, LpBound, and end-to-end
behaviour on real workloads.

---

## Preface

Samkhya is a portable, feedback-driven cardinality-correction library. Every
claim in the README, paper draft, and `DEFENSE.md` rebuttals is backed by one
or more numbered files in this directory. The campaign covers four concerns,
in order:

1. **Hardware envelopes** — what this host can actually do (CPU baseline, GPU
   hash throughput). All downstream throughput numbers are bounded by these.
2. **Sketch validation** — each cardinality sketch (HLL, Bloom, CMS,
   histograms) is checked against its theoretical envelope on this host.
3. **LpBound layer** — the tightness and solve-latency of the bound that
   samkhya feeds into a planner.
4. **End-to-end and workloads** — Puffin I/O, DataFusion integration,
   memory profile, JOB-Slow, TPC-H 1 GB, TabPFN on 4090, and the two
   ablation sweeps that isolate which layer is doing the work.
5. **Honest accounting** — failure-mode catalogue and head-to-head wallclock
   against native DataFusion, the two files a hostile reviewer will read
   first.

A meta layer (`METHODOLOGY.md`, `JOURNEY.md`) records the statistical rules of
the campaign and the chronological record of what was learned, including the
hypotheses that were **rejected** (per the methodology rigour rule:
pre-registered, multi-tier baselines, CIs not single-run).

**Canonical metrics contract.** Every numbered file in this campaign follows the
hard rule recorded in `METHODOLOGY.md`: q-error (Moerkotte VLDB 2009) for
cardinality, **BCa bootstrap 95% CI with 10,000 resamples** (Efron-Tibshirani 1993)
for latency, **geometric mean speedup + Wilcoxon signed-rank** (Leis VLDB 2015,
TPC-H convention) for workload aggregates, **Benjamini-Hochberg FDR** at α=0.05
(Benjamini-Hochberg JRSSB 1995) for multi-cell tables (N > 5), `1.04/sqrt(2^p)`
HLL RSE bound (Flajolet 2007), `m = -n·ln(p)/(ln 2)^2` Bloom sizing (Bloom
CACM 1970), `ε·N` CMS max-overestimate bound (Cormode-Muthukrishnan 2005),
AGM/LpBound tightness vs ground truth (Atserias-Grohe-Marx FOCS 2008, Khamis
PODS 2017, Zhang SIGMOD 2025), and **ACM Artifact Evaluation v1.1** discipline
for reproducibility. GPU files pin SM version + driver + CUDA version
(MLPerf rules) and report kernel-only + end-to-end. Latency files distinguish
cold-cache and warm-cache phases. Seeds follow the **first-seed-tried**
convention — no seed search.

---

## How to read this campaign (hostile-reviewer order)

A hostile reviewer should not open file 01 first. The maximally adversarial
read order is:

1. **`METHODOLOGY.md`** — what counts as a measurement here? what are the
   pre-registration rules, the baseline tiers, the CI policy, the kill
   criteria? If the methodology is sloppy, every downstream file is suspect.
2. **`17_failure_modes.md`** — what does samkhya **fail at**? A campaign
   that lists no failures is hiding something. Read this before any pass
   result.
3. **`18_vs_native_datafusion_wallclock.md`** — does the user actually get
   a faster query? End-to-end wallclock against vanilla DataFusion is the
   only number a practitioner cares about.
4. **`13_tpc_h_1gb.md` + `12_job_slow.md`** — real workloads, not synthetic
   S1–S10 micro-tests. If these are PROJECTED, downgrade confidence
   accordingly.
5. **`07_lpbound_tightness.md`** — is the bound tight enough to be useful?
   A loose pessimistic bound is a benign no-op; the project's contribution
   depends on this file.
6. **Drill into specifics** — read 01–06, 08–11, 14–16 only after the
   above gates have been cleared. These are component-level evidence
   that supports the end-to-end claims.

The reverse order — opening with 03 and admiring the HLL precision sweep —
is exactly how a friendly reader misses that the end-to-end wallclock
might be net-zero or net-negative.

---

## Status legend

Each file is tagged with one of three statuses, pulled from the file's
**Verdict** section (every campaign file follows the same header layout:
H1 → Date → Verdict → Hypothesis → Methodology → Results → Discussion →
Limitations → Reproducibility).

| Status | Meaning |
|--------|---------|
| **MEASURED** | Real numbers were produced on this host (`bench-results/00_hardware_profile.md`). The Verdict section reports a point estimate, a bootstrap or t-CI, and a pre-registered hypothesis pass/fail. |
| **PROJECTED** | Methodology, harness scripts, and an expected range are recorded, but the run is gated on hardware (RTX 4090, larger RAM), data acquisition (IMDb, TPC-H 1 GB), or a sibling-agent harness ship. The file documents how to fill in the numbers. |
| **PARTIAL** | Some rows or cells are MEASURED; others are PROJECTED. The file's Results table marks the boundary explicitly. |

Pre-registration rule: every hypothesis must be filed (with margin) before
the run is executed. A "Verdict: PARTIAL PASS" file is one where some of
the pre-registered claims were narrowly rejected — these are kept in the
record per the methodology rigour rule, not silently revised. See
[`03_hll_precision_sweep.md`](./03_hll_precision_sweep.md) §Verdict for an
example of a hypothesis rejected by the bootstrap CI.

---

## Section 1 — Foundational measurements

The host envelope. Everything downstream is bounded by these two files.

| File | Title | Status |
|------|-------|--------|
| [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) | CPU baseline, multi-thread sweep | PARTIAL (methodology + `t=1` anchored to B13; multi-thread harness scheduled — see file §Reproducibility) |
| [`02_gpu_hash_throughput.md`](./02_gpu_hash_throughput.md) | GPU hash throughput vs CPU SIMD (currently `02_gpu_hash_throughput.json` raw output; markdown writeup pending) | PARTIAL (PyTorch GPU baseline captured; full markdown writeup tracks the JSON) |

What these establish:

- **01** — partition-then-merge is the parallel pattern for HLL / Bloom / CMS;
  hypotheses are pre-registered against the host's L1d / L2 / L3 / DRAM
  bandwidths (13th Gen Intel i9-13900HK, 14C/20T hybrid, 24 MiB L3).
- **02** — GPU hash-build throughput vs CPU SIMD on the same host, so
  `samkhya-gpudb` claims do not float above the hardware ceiling.

---

## Section 2 — Sketch validation

Each cardinality sketch in `samkhya-core::sketches` is checked against its
theoretical envelope, with pre-registered point hypotheses and bootstrap CIs.

| File | Title | Status |
|------|-------|--------|
| [`03_hll_precision_sweep.md`](./03_hll_precision_sweep.md) | HLL Precision Sweep | MEASURED (PARTIAL PASS — hypothesis narrowly rejected on point estimate; CI brackets threshold) |
| [`04_bloom_fpr_validation.md`](./04_bloom_fpr_validation.md) | Bloom filter false-positive-rate validation across configured FPR targets, with empirical vs analytic envelope | MEASURED / PROJECTED — fill from file Verdict |
| [`05_cms_bound_verification.md`](./05_cms_bound_verification.md) | Count-Min Sketch bound verification (ε-additive guarantee on point queries at configured ε, δ) | MEASURED / PROJECTED — fill from file Verdict |
| [`06_histogram_accuracy.md`](./06_histogram_accuracy.md) | Equi-depth / quantile-sketch histogram accuracy on synthetic distributions (uniform, zipf, gauss) | MEASURED / PROJECTED — fill from file Verdict |

What these establish:

- Each sketch obeys its analytic envelope on this hardware. Where the
  observed mean is on the wrong side of the pre-registered point estimate
  but the bootstrap CI brackets the threshold (as in 03), the file
  documents it honestly — the result is statistically indistinguishable
  from the hypothesis, but the point estimate is not below it.

---

## Section 3 — LpBound layer

The bound is the value-add. Two files isolate (a) how tight it is, (b)
whether the solver is fast enough for planner integration.

| File | Title | Status |
|------|-------|--------|
| [`07_lpbound_tightness.md`](./07_lpbound_tightness.md) | LpBound tightness vs ground-truth join cardinality on synthetic S1–S10 + JOB-light slice | PROJECTED — methodology + script + pre-registered q-error range; awaiting harness ship |
| [`08_lpbound_solve_latency.md`](./08_lpbound_solve_latency.md) | LpBound solve-time distribution under the `lp_solver` tier, with P50 / P95 / P99 budget for in-planner usage | PROJECTED — single-thread budget pre-registered at P99 < 1 ms (matches H5 in `01`) |

What these establish:

- The bound's q-error envelope on synthetic + JOB-light. The DEFENSE.md
  Objection 7 ("15.27 → 6.19 q-error is fine but not SOTA") is addressed
  here.
- The solver's latency budget. A bound is only useful in a planner if it
  costs <1 ms; that is the line file 08 defends.

---

## Section 4 — End-to-end and workloads

The campaign turns from component evidence to system claims. These files
back every paper-draft and README claim about end-to-end behaviour.

| File | Title | Status |
|------|-------|--------|
| [`09_puffin_io_throughput.md`](./09_puffin_io_throughput.md) | Puffin sidecar read / write throughput, footer-parse cost, and crash-safety (Iceberg integration) | PROJECTED — scaffold in place; Iceberg-rs harness needed |
| [`10_datafusion_e2e_stats.md`](./10_datafusion_e2e_stats.md) | DataFusion end-to-end statistics injection, plan-shape impact, no-regression sanity on TPC-H Q-set | PROJECTED — `samkhya-datafusion/examples/b05_smoke.rs` is the harness root |
| [`11_memory_profile.md`](./11_memory_profile.md) | Memory profile under campaign workloads (peak RSS, sketch-side allocation, Puffin reader cache) | PROJECTED — heaptrack / massif harness scheduled |
| [`12_job_slow.md`](./12_job_slow.md) | JOB-Slow on real IMDb data (per DEFENSE.md Objection 9), q-error before/after correction, wallclock | PROJECTED — IMDb fetch script lives at `scripts/fetch_imdb.sh` |
| [`13_tpc_h_1gb.md`](./13_tpc_h_1gb.md) | TPC-H scale-factor 1 GB, all 22 queries, plan-shape diff and wallclock with vs without samkhya stats | PROJECTED — scale-factor and Q-set pre-registered |
| [`14_tabpfn_4090_latency.md`](./14_tabpfn_4090_latency.md) | TabPFN-based correction latency on RTX 4090, batch sizes, P95 / P99 — bounded by `02_gpu_hash_throughput` | PROJECTED — gated on 4090 access |
| [`15_ablation_layers.md`](./15_ablation_layers.md) | Layer-by-layer ablation: LpBound only, LpBound + sketches, LpBound + sketches + residual correction. Isolates which layer earns its keep | PROJECTED — design table fixed |
| [`16_ablation_calibration_size.md`](./16_ablation_calibration_size.md) | Calibration-set size sweep for the residual-correction layer (10² → 10⁵ samples), q-error vs cost trade-off | PROJECTED — design table fixed |

What these establish:

- **End-to-end wallclock** (10, 12, 13, 14): the system, not the components,
  must win.
- **Memory cost** (11): no claim about quality matters if the memory
  footprint is unacceptable.
- **Ablations** (15, 16): isolate which layer is doing the work, so the
  paper's contribution is precisely attributable.

---

## Section 5 — Honest accounting

The two files a hostile reviewer reads first. They exist so the campaign
cannot be accused of selecting only flattering numbers.

| File | Title | Status |
|------|-------|--------|
| [`17_failure_modes.md`](./17_failure_modes.md) | Catalogue of cases where samkhya does **not** improve or actively regresses (e.g. uniform distributions where LpBound is loose; small tables where overhead exceeds savings; correlated predicates outside the sketch family) | PROJECTED — failure-mode taxonomy fixed; numbers attached as workloads land |
| [`18_vs_native_datafusion_wallclock.md`](./18_vs_native_datafusion_wallclock.md) | Head-to-head wallclock against unmodified DataFusion on the same queries / data / host, with CI and per-query breakdown | PROJECTED — co-runs with `10_datafusion_e2e_stats` and `13_tpc_h_1gb` |

What these establish:

- The campaign records its own failure surface. Per `DEFENSE.md` §"Honest
  limitations", the four grounds samkhya does **not** try to defend are
  enumerated; file 17 attaches numbers to that taxonomy.
- The single most-cited number a practitioner will quote — "is samkhya
  faster than vanilla DataFusion?" — lives in 18, not on the README.

---

## Section 6 — Meta

The two files that govern the rest of the campaign.

| File | Title | Status |
|------|-------|--------|
| [`METHODOLOGY.md`](./METHODOLOGY.md) | Statistical rules: pre-registration, multi-tier baselines, bootstrap / t-CI policy, kill criteria, host fixed-point reference. Cited from every per-file Verdict | PROJECTED — pulls from `feedback_empirical_methodology` and from existing per-file Methodology sections |
| [`JOURNEY.md`](./JOURNEY.md) | Chronological narrative: which hypotheses were rejected, which were narrowly missed, which produced clean wins, and which workloads forced design revisions | PROJECTED — written after Sections 1–5 stabilise; living document |

---

## DEFENSE.md cross-reference

Each of the 12 objections in [`../DEFENSE.md`](../DEFENSE.md) is addressed
by one or more files in this campaign (and by sibling docs where the
objection is non-empirical). The table is the auditor's checklist:
no objection should be answered by rhetoric alone.

| # | Objection (verbatim short form) | Primary file(s) | Secondary evidence |
|---|---------------------------------|-----------------|--------------------|
| 1 | Why a separate library? Upstream into DataFusion / DuckDB. | `10_datafusion_e2e_stats.md`, `18_vs_native_datafusion_wallclock.md` | `../ARCHITECTURE.md` §2 (dependency graph); `B04_samkhya_duckdb_install.md` |
| 2 | Iceberg Puffin sidecars are over-engineered. Use engine native stats. | `09_puffin_io_throughput.md`, `10_datafusion_e2e_stats.md` | `../ARCHITECTURE.md` §Puffin |
| 3 | LpBound is just AGM + a clamp. | `07_lpbound_tightness.md`, `15_ablation_layers.md` | `../paper/draft.md` §LpBound; `../KILL_CRITERIA_REPORT.md` |
| 4 | TabPFN / learned correction is Naru / NeuroCard again. | `14_tabpfn_4090_latency.md`, `16_ablation_calibration_size.md` | `../paper/draft.md` §Residual correction |
| 5 | DuckDB / Polars planners are good. You're solving a non-problem. | `12_job_slow.md`, `13_tpc_h_1gb.md`, `18_vs_native_datafusion_wallclock.md` | `17_failure_modes.md` (where they really are good) |
| 6 | Pre-1.0 software making safety claims is sketchy. | `BINARY_ACCEPTANCE_REPORT.md`, `B11_sanitizer.md`, `B12_valgrind.md`, `B09_property_100k.md` | `../SECURITY.md` |
| 7 | 15.27 → 6.19 q-error is fine but not SOTA. | `07_lpbound_tightness.md`, `12_job_slow.md`, `15_ablation_layers.md` | `../paper/draft.md` §Comparison table |
| 8 | Pessimistic envelopes lead to over-conservative plans. | `10_datafusion_e2e_stats.md`, `17_failure_modes.md`, `18_vs_native_datafusion_wallclock.md` | `15_ablation_layers.md` |
| 9 | Synthetic S1–S10; where's JOB-Slow on real IMDb? | `12_job_slow.md` | `13_tpc_h_1gb.md` |
| 10 | Why Sanskrit naming? Looks like marketing. | (non-empirical) | `../DEFENSE.md` Objection 10 |
| 11 | Spark AQE already solves runtime adaptive query execution. | `10_datafusion_e2e_stats.md`, `15_ablation_layers.md` | `../paper/draft.md` §Related work |
| 12 | Apache 2.0 with patent grant — what's the IP story? | (non-empirical) | `../DEFENSE.md` Objection 12; `../LICENSE-APACHE` |

For objections added in v1.0.1+, append rows to this table and link the
new B-file. The cross-reference is the source of truth that every paper
claim has an empirical anchor.

---

## Companion: `bench-results/B0x_*.md` (binary acceptance from earlier wave)

The campaign indexed above is the **second layer**. The first layer is the
20-agent binary-acceptance wave: it proves the artefacts (CLI binary,
Python wheel, DuckDB extension, DataFusion crate, Polars adapter, Postgres
client, GPU bridge, Arrow / Iceberg integration) **install and run** on
this host, under sanitizers, across Python versions, with a deterministic
build. Without this layer, every campaign number above would be moot — a
fast number from a broken build is worth nothing.

The synthesis is in [`BINARY_ACCEPTANCE_REPORT.md`](./BINARY_ACCEPTANCE_REPORT.md).
Per-agent files:

| File | Concern |
|------|---------|
| [`B01_samkhya_cli_install.md`](./B01_samkhya_cli_install.md) | `samkhya-cli` install and smoke test |
| [`B02_samkhya_py_install.md`](./B02_samkhya_py_install.md) | `samkhya-py` wheel install across CPython versions |
| [`B04_samkhya_duckdb_install.md`](./B04_samkhya_duckdb_install.md) | `samkhya-duckdb` Rust-client install (duckdb 1.x, bundled) |
| [`B07_supply_chain.md`](./B07_supply_chain.md) | Supply-chain audit (`cargo deny`, `cargo audit`) |
| [`B08_fuzz_inventory.md`](./B08_fuzz_inventory.md) | Fuzz-target inventory (HLL / Bloom / CMS / equi-depth / correlated parsers) |
| [`B09_property_100k.md`](./B09_property_100k.md) | Property tests at `PROPTEST_CASES=100000` |
| [`B10_cross_platform.md`](./B10_cross_platform.md) | Cross-platform build matrix (Linux / macOS / Windows) |
| [`B11_sanitizer.md`](./B11_sanitizer.md) | ASan / UBSan / TSan sweeps |
| [`B12_valgrind.md`](./B12_valgrind.md) | Valgrind memcheck on examples and benches |
| [`B13_criterion.md`](./B13_criterion.md) | Criterion micro-benchmark suite (single-thread anchor for `01`) |
| [`B14_examples.md`](./B14_examples.md) | All `examples/*.rs` build and run |
| [`B15_clippy_fmt.md`](./B15_clippy_fmt.md) | `clippy --all-targets -- -D warnings`, `rustfmt --check` |
| [`B16_doctests.md`](./B16_doctests.md) | Doctest sweep across the workspace |
| [`B17_python_versions.md`](./B17_python_versions.md) | Python 3.9 → 3.13 wheel matrix |
| [`B19_reproducibility.md`](./B19_reproducibility.md) | Reproducible build (deterministic bytes) |
| [`B20_cargo_metadata.md`](./B20_cargo_metadata.md) | `cargo publish --dry-run` for every crate |

The campaign trusts these gates. If any of them regresses, the
corresponding campaign Verdict downgrades to PROJECTED until the gate is
restored.

---

## Reproducibility — one-line entry point

To reproduce every MEASURED row in this campaign from a clean checkout on
the same host class (per [`00_hardware_profile.md`](./00_hardware_profile.md)):

```bash
cargo run --release -p samkhya-bench -- --campaign-all --out bench-results/
```

Each per-file Reproducibility section overrides this with the exact
sub-command used for that file (e.g. `cargo bench -p samkhya-core --bench
stress` for `01`, `python3 bench-results/scripts/run_02_gpu.py` for `02`).
PROJECTED files document the command that *will* produce the numbers when
the gating dependency (hardware, data, harness ship) is satisfied.

The `bench-results/` directory and `samkhya-bench` crate together form a
self-contained reproducer: no hidden state outside the repo, no manual
data prep beyond the scripts in `bench-results/scripts/`.

---

## License and contact

This campaign and every file it indexes are licensed Apache-2.0. Sole
author: Prateek Singh. Security reports follow [`../SECURITY.md`]
(../SECURITY.md) (GHSA-only contact, per repo policy). All host details
are in [`00_hardware_profile.md`](./00_hardware_profile.md); no other
personal information is collected or recorded by the harness.
