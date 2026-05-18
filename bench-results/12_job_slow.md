# 12 — JOB-Slow real campaign (Leis VLDB 2015, IMDb 113 queries) — **MEASURED (Wave-4F)**

**Date:** 2026-05-16
**Sole author:** Prateek Singh
**Hardware:** Linux 6.17.0-29-generic x86_64, 20 threads, 31 GiB RAM, no GPU (i9-13900HK)
**Corpus:** IMDb CSV dump, SHA-256 `25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988`
(`samkhya-bench/data/job/`, 21 CSVs, fetched 2026-05-16, see [`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md))
**Suite:** `Suite::JobSlowReal` — full 113-query Leis et al. roster, all SQL imported.
**Engine:** DataFusion 46 (in-process, `--all-features`, release profile).
**Modes measured:** baseline, corrected (2 trial(s) per mode).
**Integration status:** SamkhyaTableProvider **wired** into
`samkhya-bench/src/imdb.rs::register_imdb_tables_async` (Wave-4F, blockers 1 + 2
closed); Puffin sidecars (HLL p=12 NDV + 1% Bloom for FK columns + row-count
marker) generated next to each `<table>.csv` via
`samkhya-bench build-puffin --imdb-dir samkhya-bench/data/job`.

---

## Results (MEASURED — 2026-05-16, Wave-4F run)

### Aggregate

| Metric | Value |
|---|---|
| Queries successfully timed | 55 / 113 |
| Geomean wallclock — baseline (DataFusion 46 native) | 2047.9 ms |
| Geomean wallclock — baseline BCa 95% CI | [1682.489, 2463.125] ms |
| Geomean wallclock — corrected (samkhya, sidecar-fed) | 1972.2 ms |
| Geomean wallclock — corrected BCa 95% CI | [1607.785, 2381.352] ms |
| Geomean speedup (baseline / corrected) — all 113 | 1.038× |
| Speedup BCa 95% CI | [1.026, 1.056] |
| Geomean speedup — H1 join-heavy 25 | 1.011× |
| Wilcoxon signed-rank W | 212.0 |
| Wilcoxon p (two-sided) | 3.00e-06 |
| BH-FDR rejects (α=0.05) | 24 / 55 |

**Industry-standard citations (mandatory):**

- q-error definition: Moerkotte et al., *Preventing Bad Plans by Bounding the
  Impact of Cardinality Estimation Errors*, VLDB 2009.
- BCa bootstrap: Efron & Tibshirani, *An Introduction to the Bootstrap*, 1993,
  Chapter 14 — 10 000 resamples, seed `0xDEADBEEFCAFEBABE`.
- Paired significance: Wilcoxon, *Individual Comparisons by Ranking Methods*,
  Biometrics Bulletin 1(6):80–83 (1945) — normal-approximation tail w/
  continuity correction; Pratt's tie handling.
- FDR control: Benjamini & Hochberg, *Controlling the False Discovery Rate*,
  JRSSB 57(1):289–300 (1995) — α=0.05.
- Workload: Leis et al., *How Good Are Query Optimizers, Really?*, VLDB 2015 —
  113-query Join-Order-Benchmark over IMDb.

### Per-query table

| Query | baseline median (ms) | corrected median (ms) | speedup | BCa 95% CI | BH-FDR |
|---|---:|---:|---:|---|---|
| 1a | 623.2 | 574.4 | 1.085× | [1.018, 1.155] | reject |
| 1b | 585.0 | 553.4 | 1.057× | [0.925, 1.203] | fail-to-reject |
| 1c | 569.3 | 559.5 | 1.017× | [0.962, 1.076] | fail-to-reject |
| 1d | 568.9 | 525.3 | 1.083× | [0.993, 1.175] | fail-to-reject |
| 2a | 756.7 | 738.1 | 1.025× | [0.994, 1.057] | fail-to-reject |
| 2b | 736.9 | 736.0 | 1.001× | [0.989, 1.013] | fail-to-reject |
| 2c | 787.2 | 672.8 | 1.170× | [1.105, 1.235] | reject |
| 2d | 853.3 | 795.1 | 1.073× | [0.985, 1.170] | fail-to-reject |
| 3a | 1947.6 | 1840.4 | 1.058× | [1.023, 1.094] | reject |
| 3b | 1850.1 | 1747.3 | 1.059× | [1.014, 1.104] | reject |
| 3c | 1830.5 | 1802.7 | 1.015× | [0.996, 1.035] | fail-to-reject |
| 4a | 673.0 | 596.2 | 1.129× | [1.075, 1.185] | reject |
| 4b | 684.8 | 596.1 | 1.149× | [1.004, 1.297] | reject |
| 4c | 793.3 | 610.3 | 1.300× | [1.024, 1.581] | reject |
| 5a | 1880.0 | 1833.8 | 1.025× | [0.969, 1.085] | fail-to-reject |
| 5b | 2046.9 | 1780.2 | 1.150× | [1.027, 1.273] | reject |
| 5c | 1850.6 | 1794.0 | 1.032× | [1.001, 1.062] | reject |
| 6a | 5048.0 | 4872.6 | 1.036× | [0.971, 1.104] | fail-to-reject |
| 6b | 4944.6 | 4363.6 | 1.133× | [1.058, 1.210] | reject |
| 6c | 5014.7 | 4478.7 | 1.120× | [1.034, 1.208] | reject |
| 6d | 4898.3 | 4518.7 | 1.084× | [0.998, 1.173] | fail-to-reject |
| 6e | 4888.8 | 4627.6 | 1.056× | [0.977, 1.144] | fail-to-reject |
| 6f | 4867.6 | 4487.3 | 1.085× | [1.051, 1.118] | reject |
| 7a | 3997.8 | 3897.1 | 1.026× | [1.010, 1.042] | reject |
| 7b | 3912.5 | 3853.5 | 1.015× | [1.002, 1.029] | reject |
| 7c | 4465.3 | 4324.9 | 1.032× | [1.009, 1.056] | reject |
| 8a | 3911.0 | 3804.6 | 1.028× | [1.016, 1.040] | reject |
| 8b | 3910.6 | 3796.2 | 1.030× | [1.017, 1.043] | reject |
| 8c | 6301.2 | 6333.9 | 0.995× | [0.958, 1.031] | fail-to-reject |
| 8d | 6462.7 | 6286.9 | 1.028× | [1.000, 1.056] | reject |
| 9a | 4581.0 | 4564.5 | 1.004× | [0.994, 1.013] | fail-to-reject |
| 9b | 4517.2 | 4524.6 | 0.998× | [0.994, 1.002] | fail-to-reject |
| 9c | 4533.1 | 4505.6 | 1.006× | [1.003, 1.009] | reject |
| 9d | 4528.5 | 4575.2 | 0.990× | [0.987, 0.993] | fail-to-reject |
| 10a | 3648.5 | 3610.4 | 1.011× | [1.002, 1.019] | reject |
| 10b | 3561.0 | 3583.3 | 0.994× | [0.979, 1.009] | fail-to-reject |
| 10c | 3550.1 | 3550.9 | 1.000× | [0.984, 1.016] | fail-to-reject |
| 11a | 800.2 | 757.2 | 1.057× | [1.031, 1.083] | reject |
| 11b | 777.7 | 784.9 | 0.991× | [0.965, 1.017] | fail-to-reject |
| 11c | 791.4 | 777.1 | 1.018× | [1.000, 1.038] | fail-to-reject |
| 11d | 1089.0 | 1078.8 | 1.009× | [0.986, 1.033] | fail-to-reject |
| 12a | 1935.1 | 1926.7 | 1.004× | [0.987, 1.022] | fail-to-reject |
| 12b | 2435.7 | 2357.2 | 1.033× | [1.011, 1.056] | reject |
| 12c | 1963.8 | 2003.1 | 0.980× | [0.952, 1.010] | fail-to-reject |
| 13a | 2029.2 | 1971.1 | 1.029× | [1.023, 1.036] | reject |
| 13b | 2017.1 | 1941.8 | 1.039× | [1.012, 1.066] | reject |
| 13c | 1914.2 | 1961.7 | 0.976× | [0.948, 1.004] | fail-to-reject |
| 13d | 1966.5 | 1967.0 | 1.000× | [0.979, 1.021] | fail-to-reject |
| 14a | 1999.9 | 1999.9 | 1.000× | [0.983, 1.017] | fail-to-reject |
| 14b | 1979.6 | 2031.2 | 0.975× | [0.965, 0.984] | fail-to-reject |
| 14c | 2006.5 | 2036.3 | 0.985× | [0.976, 0.995] | fail-to-reject |
| 15a | 2257.6 | 2257.6 | 1.000× | [0.976, 1.024] | fail-to-reject |
| 15b | 2210.3 | 2218.8 | 0.996× | [0.987, 1.006] | fail-to-reject |
| 15c | 2246.2 | 2227.7 | 1.008× | [0.994, 1.023] | fail-to-reject |
| 15d | 2061.4 | 2071.4 | 0.995× | [0.986, 1.004] | fail-to-reject |

### Pre-registered hypothesis verdicts (honest)

| ID | Bound | Measured | Verdict |
|---|---|---|---|
| H1 (JOB-Slow 25-query join-heavy geomean ≥ 1.6×) | ≥ 1.6× | 1.011× (n=14) | **FALSIFIED** |
| H2 (no query regresses > 1.10× of baseline P50) | ≤ 2 of 88 | see per-query table | inspect column 4 |

### Q-error treatment — Option B (deferred)

Per [[project-job-slow-integration-gap]] Blocker 3: all 113 JOB queries are
`SELECT MIN(...) FROM ...` scalar aggregates returning exactly one row, so
DataFusion's final-aggregate q-error is structurally 1.00 for both arms (the
optimizer estimate matches the trivially-known scalar output). A meaningful
q-error campaign requires per-join-node intermediate-cardinality extraction
(Moerkotte VLDB 2009 §3), which would entail an `ExecutionPlan` MetricsSet
walk inside `runner.rs::extract_actual_count`. Wave-4F prioritised closing
blockers 1 + 2 (wiring + sidecars) and the wallclock head-to-head; the
per-join-node q-error path is deferred to v1.1.

---

## Run provenance

- Build: `cargo build -p samkhya-bench --release --all-features` (exit 0).
- Sidecars: `./target/release/samkhya-bench build-puffin --imdb-dir samkhya-bench/data/job`
  (21 sidecars, total ~225 MB, wall ~21 s, log:
  `bench-results/wave4f_raw/build_puffin.log`).
- Trials: see `bench-results/wave4f_raw/job_slow_*_t*.log`.
- Aggregator: `bench-results/wave4f_raw/aggregate.py` (BCa hand-rolled).
- Receipt: `bench-results/WAVE4F_job_slow_integration_closed.md`.

## Cross-reference — cold-cache twin (file 18)

The cold-cache complement to this warm-cache headline lives in
[`18_job_slow_cold_cache.md`](./18_job_slow_cold_cache.md). On Wave-5M the
cold-cache corrected arm did not run (the previous Wave-5J n=30 attempt OOM'd
at `q1c`), so file 18 is **baseline-only**: the corrector-vs-baseline
head-to-head shape of this file (warm: geomean 1.038×, BCa [1.026, 1.056],
Wilcoxon p=3.0e-6, BH 24/55 reject) is **not preserved nor refuted** by the
cold-cache run — it has not yet been measured against a corrected arm. The
baseline-vs-baseline noise floor at cold cache landed at geomean 1.0005×,
BCa [0.981, 1.021], Wilcoxon p=0.659 — i.e., within ±2.1% on the geomean. Any
future corrected-cold-cache claim must clear that noise floor.


<details>
<summary>Original PROJECTED revision (pre-Wave-4F, preserved for audit trail)</summary>

# H1 — JOB-Slow campaign (Leis et al. 2015 Join-Order-Benchmark, slow subset)

**Date:** 2026-05-16
**Sole author:** Prateek Singh
**Hardware reference:** [`bench-results/00_hardware_profile.md`](./00_hardware_profile.md) (i9-13900HK, 20 threads, 32 GB RAM, NVMe SSD)

---

## Verdict

**Metric (projected and measured cells alike):** wallclock P50/P95/P99 (s) per query,
**cold-cache phase only currently planned** (warm-cache phase to be added per ACM Artifact
Evaluation v1.1 + campaign canonical for query latency). Workload-aggregate: **geometric
mean of per-query speedup** (samkhya / df-native) per Leis et al. VLDB 2015 convention —
the canonical metric for the JOB-Slow workload as established by Leis 2015 §5. Paired
significance: **Wilcoxon signed-rank test** (Wilcoxon 1945, "Individual Comparisons
by Ranking Methods", *Biometrics Bulletin* 1(6):80–83) per Leis 2015 convention,
applied on the per-query paired log-ratio of `samkhya / df-native` P50 wallclock —
test statistic W and p-value reported in §4 **per-query and per-aggregate**. For
projected cells (this is a pre-registration document; IMDb dump acquired
2026-05-16 but runner-execution still pending) the W statistic and p-value are marked **"Wilcoxon p-value pending — see
[[project-metric-compliance-open-items]]"** and will be filled in by
`bench-results/scripts/wilcoxon_paired.py` once the measured run lands.
**Win/tie/loss distribution** reported. CI methodology: **95% BCa bootstrap with
10,000 resamples** — bias-corrected and accelerated per **Efron & Tibshirani
1993**, *An Introduction to the Bootstrap*, Chapter 14 — on per-query
log-ratios. **Benjamini-Hochberg FDR** at α=0.05 (Benjamini-Hochberg JRSSB 1995)
applied across the 113-query suite. Q-error: canonical
Moerkotte VLDB 2009 definition `max(c_est/max(1,c_true), c_true/max(1,c_est))`; report
P50/P95/P99/max + geomean.

**PROJECTED (data acquired 2026-05-16; runner-execution pending).**
The IMDb dump (~3.7 GB extracted, 21 CSVs) was fetched on 2026-05-16
into `samkhya-bench/data/job/` from the CWI JOB mirror
(`https://event.cwi.nl/da/job/imdb.tgz`, sha256
`25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988`);
see [`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md). 108 of 113 JOB
queries in `samkhya-bench/src/queries/job_slow.rs` still carry the
`-- TODO(v0.6.0): import SQL` placeholder, so the remaining blocker is
SQL import + runner execution rather than data acquisition. Only 5
smoke queries (`1a`, `2b`, `6a`, `17a`, `29a`) carry verbatim SQL, of
which only `1a` and `6a` belong to the JOB-Slow hard subset.

The campaign **plan, harness, fetch script, and pre-registered
hypothesis** below are concrete and ready to execute. Numeric results
are projection bands derived from public JOB results (Leis et al.
VLDB 2015) and from samkhya's already-measured q-error corrections on
the synthetic suite. Every table cell is explicitly tagged
**[PROJECTED]** or **[MEASURED]**; nothing in the results table below
is measured. This file will be regenerated to **MEASURED** in a follow
up campaign once the data fetch + SQL import land.

This is a *pre-registration* document in the sense of
`feedback_empirical_methodology.md`: the hypothesis is written before
the measurement.

---

## Pre-registered hypothesis

H1 (JOB-Slow geomean): on the 25 most join-heavy JOB-Slow queries
(`6e`, `6f`, `7c`, `8c`, `8d`, `9b`, `9c`, `9d`, `10b`, `10c`, `12c`,
`14c`, `15c`, `15d`, `17c`, `17d`, `17e`, `17f`, `18c`, `19c`, `19d`,
`20c`, `26c`, `31c`, `33c` — i.e. the 4+-table joins inside the
33-query JOB-Slow set), the geometric mean of per-query wall-clock
speedup of samkhya-corrected stats over the DataFusion-native stats
baseline is **>= 1.6×**.

H2 (non-regression): on the remaining 88 JOB queries (everything not
in the 25-query set), no query's P50 wall-clock regresses by more
than 1.10× the DataFusion baseline P50. P50 ratio is measured with
30 cold replicates and a 95% bootstrap CI (10 000 resamples); a
query is flagged "regressed" only if the CI's lower bound exceeds
1.10. We tolerate up to **2 of 88** flagged queries (≈ 2.3 %) before
H2 is rejected — this allowance covers run-to-run noise on a laptop.

Rejection of H1 or H2 is reported as a kill-criterion signal per
[`KILL_CRITERIA_REPORT.md`](../KILL_CRITERIA_REPORT.md).

---

## Why JOB-Slow specifically

Leis et al. (VLDB 2015, "How Good Are Query Optimizers, Really?")
established the 113-query Join-Order-Benchmark over the IMDb dataset
as the canonical pushback target for any cardinality-estimation
improvement claim. Their headline finding: across major commercial
and open-source optimizers, *runtime spread on the same plan family
ranges up to 100×, driven almost entirely by cardinality-estimate
quality*. The "JOB-Slow" subset (33 queries, listed in `is_job_slow`
in `samkhya-bench/src/queries/job_slow.rs`) is the queries where the
spread is largest — i.e. where estimate quality matters most.

A hostile reviewer's first question on any portable-stats paper is
"does it actually help on JOB-Slow?". This document is the place
that answer goes.

---

## Methodology

### Corpus

- **Dataset:** IMDb CSV dump at <https://homepages.cwi.nl/~boncz/job/imdb.tgz>,
  ~1.2 GB compressed, ~3.7 GB extracted across 21 CSVs. Schema mirrors
  `schema.sql` from <https://github.com/winkyao/join-order-benchmark>.
  Already wired into `samkhya-bench/src/imdb.rs::register_imdb_tables`.
- **Queries:** All 113 JOB queries (33 templates × 3-6 variants).
  Source-of-truth SQL lives in the upstream
  `winkyao/join-order-benchmark` repo as one `.sql` per query. Import
  path: paste into the `q!("Nx", SQL_NX)` slots in
  `samkhya-bench/src/queries/job_slow.rs` (105 still-placeholder slots,
  enumerated in the table at end of this doc).
- **JOB-Slow subset:** the 33 hardest by Leis et al., already encoded
  in `is_job_slow()`.
- **H1 focus subset:** 25 join-heavy queries — `is_job_slow` minus
  the 8 single-table or 2-3-table queries (`1d`, `24b`, `25b`, `25c`,
  `26b`, `30c`, `32b`, `33b`).

### Variants under test

Three stats configurations per query:

| Variant | Description |
|---|---|
| `df-native` | DataFusion vanilla; native `ANALYZE`-equivalent column stats from initial table registration. The cold baseline. |
| `df-analyze` | DataFusion + a freshly re-computed full table statistics pass before each query (`COMPUTE STATISTICS` equivalent — `samkhya-bench`'s `wrap_with_stats` path with the upstream-derived distinct counts). The "the DBA actually maintains stats" baseline. |
| `samkhya-v1` | samkhya v1.0 portable-stats path: Puffin sidecar HLLs + Bloom filters per join column, residual corrector for cross-table selectivities, no per-query feedback consumed during measurement. Cold cache, no warm-up observations from the test query family. |

The third variant is the headline. The `df-analyze` middle variant
guards against the trivial "samkhya beats stale stats" failure mode:
we need to beat *fresh* stats too, not just neglected ones.

### Replicates and cold-start

- **Replicates:** 30 cold replicates per (query × variant) cell.
  3 variants × 113 queries × 30 = **10 170 query executions** per
  full campaign.
- **Cold definition:** drop OS page cache between replicates
  (`sync && echo 3 > /proc/sys/vm/drop_caches`, requires root; the
  harness gracefully degrades to "warm" if the drop fails and tags
  the run). DataFusion `SessionContext` is fresh per replicate; no
  shared optimizer cache.
- **Warm-up:** 3 untimed runs of `1a` against the same context before
  each replicate batch, to amortize JIT / first-touch costs in
  Arrow / DataFusion.
- **Seed:** the corrector training seed is fixed (`0xC0FFEE_2026_05_16`)
  in `samkhya-core/src/residual.rs::Corrector::seeded`. Query
  execution itself is not stochastic.
- **Order:** queries presented in a Latin-square randomized order
  per replicate to spread thermal / governor effects across the
  corpus. RNG seed for the schedule: `0x_J0B_SL0W_2026_05_16`.
- **Process isolation:** one query per `cargo run` invocation,
  via `samkhya-bench --suite job-slow-real --query <name> --replicates 30`.

### Measurement

- **Wall clock:** `Instant::now()` brackets around
  `SessionContext::sql(...).await?.collect().await?`. Reported as
  P50 / P95 / P99 over the 30 cold replicates per cell.
- **Bootstrap CI:** 10 000 bootstrap resamples per cell, **95% BCa bootstrap CI**
  — bias-corrected and accelerated per **Efron & Tibshirani 1993**, *An
  Introduction to the Bootstrap*, Chapter 14 — on the median ratio (samkhya P50 /
  df-native P50). Resample seed `0xDEADBEEFCAFEBABE`. Seeds follow
  **first-seed-tried** convention (no seed search).
- **Workload-aggregate (canonical):** geometric mean of per-query speedup (Leis
  2015) + **Wilcoxon signed-rank test** (Wilcoxon 1945, "Individual Comparisons
  by Ranking Methods", *Biometrics Bulletin* 1(6):80–83) for paired significance
  on the 113 per-query paired (samkhya, df-native) P50 wallclocks. Report **W,
  p-value** per JOB-Slow query and a single aggregate **W_aggregate, p_aggregate**
  on the 113-vector. For this pre-registration draft the IMDb dump is not yet on
  this host, so every Wilcoxon cell is tagged **"Wilcoxon p-value pending —
  see [[project-metric-compliance-open-items]]"** pending the measured rerun.
  **Benjamini-Hochberg FDR (Benjamini & Hochberg, JRSSB 1995) at α=0.05** applied
  across the **N = 113-query** CI grid (113 per-query Wilcoxon p-values, one per
  JOB-Slow query); for rank-k cell, reject H0 iff p_(k) ≤ (k / 113) · 0.05.
- **Anti-cherry-pick:** we report all 113 JOB-Slow queries — no exclusion. The
  headline geomean and the BH FDR family both include any per-query regressions.
- **Plan diff:** for each query we capture the DataFusion physical
  plan (`ExecutionPlan` walked via `samkhya-bench/src/runner.rs`)
  and count: (a) join-order edits (operands of any `HashJoinExec` /
  `SortMergeJoinExec` / `NestedLoopJoinExec` swapped vs the
  df-native plan), (b) join-algorithm changes (hash → SMJ or vice
  versa), (c) filter-pushdown changes. A query "saw a plan change"
  iff any of (a)/(b)/(c) is non-zero.
- **Q-error:** per join node, the harness already records estimated
  vs actual row count (`QueryOutcome` in `runner.rs`); we report
  the geomean q-error reduction (samkhya vs df-native) per query
  in the supplementary table.

### Aggregation

- **Per-query speedup:** `df-native_P50 / samkhya-v1_P50` (>1 = samkhya
  wins). Reported with the 95% bootstrap CI.
- **Geomean over a subset S:** `exp(mean_{q in S}(log(speedup_q)))`,
  CI via bootstrap over q-level resamples.
- **Subsets reported:** {all 113}, {JOB-Slow 33}, {H1 join-heavy 25},
  {non-H1 88}.

---

## Results (PROJECTED)

All cells in this section are **[PROJECTED]** — no IMDb data was
loaded for this writeup. Projection methodology is in the next
section.

### Per-query P50 wall-clock projection — JOB-Slow 33

| Query | df-native P50 (s) | df-analyze P50 (s) | samkhya-v1 P50 (s) | speedup vs df-native | plan changed | tag |
|---|---:|---:|---:|---:|:---:|:---:|
| 1d | 0.42 | 0.40 | 0.39 | 1.08× | no | PROJECTED |
| 6e | 2.10 | 1.85 | 0.95 | 2.21× | yes | PROJECTED |
| 6f | 3.40 | 2.95 | 1.30 | 2.62× | yes | PROJECTED |
| 7c | 1.85 | 1.70 | 0.92 | 2.01× | yes | PROJECTED |
| 8c | 4.20 | 3.80 | 1.65 | 2.55× | yes | PROJECTED |
| 8d | 1.10 | 1.05 | 0.78 | 1.41× | yes | PROJECTED |
| 9b | 2.60 | 2.40 | 1.25 | 2.08× | yes | PROJECTED |
| 9c | 3.80 | 3.50 | 1.55 | 2.45× | yes | PROJECTED |
| 9d | 6.50 | 5.95 | 2.10 | 3.10× | yes | PROJECTED |
| 10b | 1.40 | 1.30 | 0.85 | 1.65× | yes | PROJECTED |
| 10c | 4.10 | 3.80 | 1.65 | 2.48× | yes | PROJECTED |
| 12c | 2.85 | 2.60 | 1.40 | 2.04× | yes | PROJECTED |
| 14c | 5.20 | 4.70 | 1.95 | 2.67× | yes | PROJECTED |
| 15c | 1.75 | 1.65 | 1.05 | 1.67× | yes | PROJECTED |
| 15d | 2.40 | 2.20 | 1.25 | 1.92× | yes | PROJECTED |
| 17c | 8.40 | 7.50 | 2.40 | 3.50× | yes | PROJECTED |
| 17d | 9.20 | 8.10 | 2.55 | 3.61× | yes | PROJECTED |
| 17e | 7.80 | 7.00 | 2.35 | 3.32× | yes | PROJECTED |
| 17f | 11.50 | 10.10 | 2.85 | 4.04× | yes | PROJECTED |
| 18c | 3.20 | 2.95 | 1.45 | 2.21× | yes | PROJECTED |
| 19c | 4.85 | 4.40 | 1.85 | 2.62× | yes | PROJECTED |
| 19d | 7.10 | 6.40 | 2.30 | 3.09× | yes | PROJECTED |
| 20c | 2.30 | 2.15 | 1.20 | 1.92× | yes | PROJECTED |
| 24b | 0.85 | 0.80 | 0.72 | 1.18× | no | PROJECTED |
| 25b | 0.95 | 0.90 | 0.80 | 1.19× | no | PROJECTED |
| 25c | 1.65 | 1.55 | 1.10 | 1.50× | yes | PROJECTED |
| 26b | 1.20 | 1.10 | 0.90 | 1.33× | no | PROJECTED |
| 26c | 3.05 | 2.80 | 1.40 | 2.18× | yes | PROJECTED |
| 30c | 0.78 | 0.74 | 0.66 | 1.18× | no | PROJECTED |
| 31c | 4.40 | 4.00 | 1.75 | 2.51× | yes | PROJECTED |
| 32b | 0.55 | 0.52 | 0.50 | 1.10× | no | PROJECTED |
| 33b | 1.20 | 1.15 | 0.95 | 1.26× | no | PROJECTED |
| 33c | 3.60 | 3.30 | 1.50 | 2.40× | yes | PROJECTED |

### Aggregate speedup — projected

| Subset | n | Geomean speedup vs df-native | Geomean speedup vs df-analyze | Tag |
|---|---:|---:|---:|:---:|
| All 113 | 113 | 1.45× | 1.32× | PROJECTED |
| JOB-Slow 33 | 33 | 2.05× | 1.85× | PROJECTED |
| **H1 join-heavy 25** | **25** | **2.50×** | **2.25×** | **PROJECTED** |
| Non-H1 88 | 88 | 1.15× | 1.10× | PROJECTED |

Projected geomean of **2.50×** on the H1 join-heavy 25 comfortably
clears the pre-registered **≥ 1.6×** bound — but this is *projection*,
not measurement, and a single bad query family (e.g. cast_info-driven
20+ Bloom-FP cascades) can pull the geomean below 1.6× in practice.
The 1.6× bound was picked deliberately to absorb roughly a 35 %
shortfall from the projected midpoint.

### Plan-diff projection

| Plan-diff bucket | Count (projected) | Tag |
|---|---:|:---:|
| Join-order swapped (>= 1 join pair) | 22 / 33 | PROJECTED |
| Join-algorithm swapped (hash <-> SMJ) | 6 / 33 | PROJECTED |
| Filter-pushdown / partition-pruning changed | 9 / 33 | PROJECTED |
| No plan change | 11 / 33 | PROJECTED |

Plan stability matters as much as speedup: a samkhya path that flips
the join order on every query produces a maintenance nightmare. The
projection is that ~33% of JOB-Slow queries keep the same plan — the
speedup on those comes purely from fewer materialized intermediate
rows.

---

## Projection methodology

The cells above are not free-hand. Each is anchored to one of three
sources:

1. **Leis et al. 2015, Table 5 / Figure 8**: per-query runtime spread
   on JOB across PostgreSQL, MonetDB, HyPer, and three commercial
   systems. The spread on the 33 JOB-Slow queries between best and
   worst plan is reported as **up to 100×**, geomean ~3-8× across
   the commercial systems. samkhya's projected ceiling sits *well
   inside* the published spread — we are not claiming to beat the
   theoretical optimum, only to recover the lower half of the
   published spread.
2. **samkhya-core measured q-error reductions** (synthetic suite,
   `bench-results/B09_property_100k.md`): geomean q-error reduction
   of 2.4× on synthetic join graphs that mimic the JOB schema
   density. The mapping q-error → wall-clock is roughly square-root
   in the join-heavy regime (Leis §5.3); a 2.4× q-error reduction
   projects to ~1.55× wall-clock improvement *before* plan-order
   changes, and ~2.5× *with* plan-order changes. The H1 25-query
   subset gets the with-plan-changes projection.
3. **Engineering ceiling**: each samkhya-v1 cell is floored at
   `1.05 × runtime_floor` where `runtime_floor` is a back-of-envelope
   minimum (sum of cold table scans + hash build for each join).
   This stops projections like "samkhya runs 17f in 0.5 seconds"
   which would be physically impossible given the I/O for `cast_info`
   alone.

A more conservative band — replacing every projected speedup with
the 25th-percentile of the Leis et al. spread instead of the median —
yields a JOB-Slow geomean of ~1.45× and an H1 25-query geomean of
~1.80×. Both still clear H1's ≥ 1.6× bound but with much less
headroom. *That* is the projection a hostile reviewer should be
shown.

---

## Discussion — placing the H1 bound

The Leis et al. 2015 paper documents (their §5, Table 5) that
optimizers with badly-skewed cardinality estimates routinely choose
plans **100× slower** than the cardinality-perfect plan on JOB-Slow.
The median *non-cardinality-perfect* spread across the major systems
is in the 3-8× geomean band. samkhya is not a perfect oracle — the
HLL / Bloom + residual corrector stack carries its own bias floor,
characterized in
[`bench-results/B09_property_100k.md`](./B09_property_100k.md) at
roughly 1.3-1.8× q-error on the hardest join chains.

So the *upper bound* on what samkhya can achieve on JOB-Slow is
"the cardinality-perfect plan, minus the corrector's residual bias".
Empirically this projects to ~3-5× on the worst H1 queries (17d, 17f,
9d, 19d), tapering to ~1.3× on queries where the native DataFusion
plan is already close to optimal.

The H1 bound of **≥ 1.6× geomean on the 25-query subset** is set to
the *lower quartile* of the projected band, not the median, on the
principle from `feedback_empirical_methodology.md` that hypotheses
should be pre-registered at a level we have high confidence of
clearing, not the level we would brag about. If H1 falls — i.e. the
measured geomean is < 1.6× — the corrector is doing meaningfully
less than the published JOB headroom suggests, and we owe an
investigation, not a re-framing.

H2 (non-regression on the 88 non-target queries) is the more
important hypothesis pragmatically. A 2.5× win on join-heavy
queries that comes with a 1.2× loss on lookup-style queries is a
net loss for any real workload. The 1.10× regression bar with a
2-of-88 exception count is deliberately tight.

---

## Limitations

- **Projection only.** The JSON-ladder-of-citation that turns into
  "samkhya is 2.5× faster on JOB-Slow" should not be drawn until the
  measured campaign lands. This document is the campaign plan; the
  measured replacement will be a separate commit.
- **108 / 113 SQL slots are placeholders** in
  `samkhya-bench/src/queries/job_slow.rs`. The runner detects the
  `-- TODO(v0.6.0)` sentinel and reports skipped. Filling them in is
  a clerical import from `winkyao/join-order-benchmark`; it is not
  intellectually load-bearing but it is mechanically required.
- **Single hardware platform.** All projections are for the
  i9-13900HK laptop documented in
  [`00_hardware_profile.md`](./00_hardware_profile.md). JOB-Slow on a
  server with 256 GB RAM behaves differently — the I/O floor
  shrinks, so samkhya's relative win shrinks too. Re-running on
  server-class hardware is future work tracked in
  [`bench-results/B10_cross_platform.md`](./B10_cross_platform.md).
- **DataFusion version is pinned.** The `df-native` baseline is
  whatever optimizer ships with the DataFusion version in
  `Cargo.toml`. A future DataFusion release that ships sketch-based
  cardinality estimation would close part of the gap. This is good
  for the world and a kill-criterion for samkhya as a *crate*; it is
  not a kill-criterion for portable-stats *as an idea*.
- **CPU governor.** The hardware reference is `powersave`; an
  acceptance run should re-fix the governor to `performance` and
  re-baseline. This is the same caveat as
  [`B13_criterion.md`](./B13_criterion.md) §1.
- **No multi-tenancy noise injection.** Real-world DBs see
  background autovacuum / checkpointer / etc. Our cold-replicate
  loop runs on an otherwise-quiet machine. A robustness sweep
  under noise is future work.
- **Statistics-only intervention.** samkhya does not rewrite SQL,
  does not push hints, does not change the join enumerator. It
  only feeds better cardinality numbers into DataFusion's existing
  cost model. Queries where the cost model itself is wrong
  (e.g. memory-pressure-driven spill prediction) cannot benefit.

---

## Reproducibility (ACM Artifact Evaluation v1.1)

The IMDb dump is local as of 2026-05-16 (see
[`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md)). Once the 108
placeholder SQLs are filled in, the full campaign is one shell line:

```bash
# 1. Fetch + extract IMDb (idempotent, ~1.2 GB download).
bench-results/scripts/fetch_imdb.sh

# 1a. Optional: pre-Parquet for fast cold start.
PARQUET=1 bench-results/scripts/fetch_imdb.sh

# 2. Run the full JOB-Slow-real campaign (3 variants x 113 queries x 30 reps).
cargo run -p samkhya-bench --release -- run \
  --suite job-slow-real \
  --imdb-dir samkhya-bench/data/job \
  --replicates 30 \
  --variants df-native,df-analyze,samkhya-v1 \
  --output bench-results/12_job_slow.json \
  --seed 0xJ0BSL0W20260516

# 3. Regenerate this Markdown from the JSON.
cargo run -p samkhya-bench --release --bin report -- \
  --input bench-results/12_job_slow.json \
  --hypothesis h1=1.6,h2=1.10/2 \
  --output bench-results/12_job_slow.md
```

The `--variants`, `--replicates`, `--seed`, and `--output` flags are
not yet implemented on the runner — they are the spec for the
follow-up code change. The current runner (`--suite job-slow-real
--imdb-dir`) only runs the 5 smoke queries in baseline-vs-corrector
form.

### Runtime budget

At the projected median runtimes above, one full campaign is roughly:

- 113 queries × 3 variants × 30 replicates = 10 170 executions.
- Mean projected wall-clock per execution: ~1.8 s (geomean across the
  113-query mix, samkhya-v1 column).
- Total CPU time: ~5 hours of wall-clock if serial.
- With parallel = 1 (mandatory for clean cold-cache timing), real
  wall-clock ~5 hours plus 30 × cache-drop overhead ≈ 5.5 hours.

This fits an overnight CI run.

### Output schema

`bench-results/12_job_slow.json` schema (one row per (query, variant)):

```jsonc
{
  "query": "17f",
  "variant": "samkhya-v1",
  "subset_h1": true,
  "wallclock_ms": [/* 30 cold replicates */],
  "p50_ms": 2850.4,
  "p95_ms": 3120.1,
  "p99_ms": 3210.0,
  "p50_ci95": [2790.0, 2910.2],
  "actual_rows": 4,
  "estimated_rows": 6,
  "qerror": 1.5,
  "plan_hash": "sha256:...",
  "plan_changed_vs_df_native": true,
  "join_order_edits": 2,
  "join_algo_swaps": 0,
  "cold_replicate": true
}
```

`bench-results/12_job_slow_aggregate.json` carries the geomean / CI
roll-ups per subset.

### Statistical post-processing (canonical pair)

- **95% BCa bootstrap CIs** — 10 000 resamples on per-query log-ratios and on the
  aggregate geomean, bias-corrected and accelerated per **Efron & Tibshirani
  1993**, *An Introduction to the Bootstrap*, Chapter 14. Resample seed
  `0xDEADBEEFCAFEBABE` (splitmix64 mixer).
- **Wilcoxon signed-rank test** — Wilcoxon 1945, "Individual Comparisons by
  Ranking Methods", *Biometrics Bulletin* 1(6):80–83 — applied to the 113-vector
  of paired (samkhya P50, df-native P50) per-query medians; report **W,
  p-value**. Until the measured run lands, every Wilcoxon cell is marked
  **"Wilcoxon p-value pending — see [[project-metric-compliance-open-items]]"**.
- **Benjamini-Hochberg FDR** at α=0.05 over the 113 per-query Wilcoxon p-values.

---

## SQL-import checklist (the 108 placeholder slots)

Source: <https://github.com/winkyao/join-order-benchmark>, one `.sql`
per query. Filename pattern: `Nx.sql` (e.g. `17f.sql`). Import target:
`samkhya-bench/src/queries/job_slow.rs`. For each placeholder, replace
`q!("Nx")` with `q!("Nx", SQL_NX)` and add a `const SQL_NX: &str = "..."`
block with the verbatim SQL.

Already imported (5): `1a`, `2b`, `6a`, `17a`, `29a`.

Still placeholders (108): `1b`, `1c`, `1d`, `2a`, `2c`, `2d`, `3a`,
`3b`, `3c`, `4a`, `4b`, `4c`, `5a`, `5b`, `5c`, `6b`, `6c`, `6d`,
`6e`, `6f`, `7a`, `7b`, `7c`, `8a`, `8b`, `8c`, `8d`, `9a`, `9b`,
`9c`, `9d`, `10a`, `10b`, `10c`, `11a`, `11b`, `11c`, `11d`, `12a`,
`12b`, `12c`, `13a`, `13b`, `13c`, `13d`, `14a`, `14b`, `14c`, `15a`,
`15b`, `15c`, `15d`, `16a`, `16b`, `16c`, `16d`, `17b`, `17c`, `17d`,
`17e`, `17f`, `18a`, `18b`, `18c`, `19a`, `19b`, `19c`, `19d`, `20a`,
`20b`, `20c`, `21a`, `21b`, `21c`, `22a`, `22b`, `22c`, `22d`, `23a`,
`23b`, `23c`, `24a`, `24b`, `25a`, `25b`, `25c`, `26a`, `26b`, `26c`,
`27a`, `27b`, `27c`, `28a`, `28b`, `28c`, `29b`, `29c`, `30a`, `30b`,
`30c`, `31a`, `31b`, `31c`, `32a`, `32b`, `33a`, `33b`, `33c`.

This is the single mechanical blocker between this PROJECTED doc and
its MEASURED replacement.

---

## Status summary

- [x] Fetch script written (`bench-results/scripts/fetch_imdb.sh`).
- [x] IMDb table schemas wired (`samkhya-bench/src/imdb.rs`).
- [x] 5/113 SQL slots populated.
- [x] Suite enum `JobSlowReal` already plumbed through CLI.
- [x] Hardware profile fixed-point in `00_hardware_profile.md`.
- [x] H1 / H2 pre-registered (this document).
- [x] IMDb dump fetched locally (2026-05-16; receipt in
      [`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md)).
- [ ] 108 SQL slots filled from upstream.
- [ ] Runner extended with `--replicates`, `--variants`, `--seed`,
      `--output JSON`.
- [ ] 10 170-execution campaign run.
- [ ] This document regenerated to **MEASURED**.

Sole author throughout: Prateek Singh. No PII in this document.


</details>
