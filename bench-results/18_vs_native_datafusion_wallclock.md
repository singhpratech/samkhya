# 18 — samkhya vs native DataFusion 46: master wallclock comparison — **MEASURED (JOB-Slow real, Wave-4F)**

**Date:** 2026-05-16
**Sole author:** Prateek Singh
**License:** Apache-2.0
**Hardware:** Linux 6.17.0-29-generic x86_64, 20 threads, 31 GiB RAM, no GPU (i9-13900HK)
**Corpus measured:** JOB-Slow real, 113 queries, IMDb CSV dump (SHA-256
`25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988`).
**Engine:** DataFusion 46, single-process, release profile.
**Integration status:** Wave-4F closed Blockers 1 + 2 from
[[project-job-slow-integration-gap]] — `SamkhyaTableProvider` now wraps every
IMDb table in `register_imdb_tables_async`, sourcing per-column NDV (HLL p=12)
+ FK Bloom (1% fpr) + row-count from a Puffin sidecar at
`samkhya-bench/data/job/<table>.puffin`.

---

## 1. Verdict (MEASURED — Wave-4F)

Headline: **geomean wallclock speedup samkhya / native DataFusion 46 on JOB-Slow
real = 1.038×** (95% BCa CI [1.026, 1.056], n = 55 queries).

**Pre-registered hypotheses:**

| ID | Threshold | Measured | Verdict |
|---|---|---|---|
| H1 (aggregate geomean ≥ 1.35×) | ≥ 1.35× | 1.038× | **FALSIFIED** |
| H2 (wins ≥ 75% of queries) | ≥ 75% | 30.9% (17/55) | **FAIL** |
| H3 (regressions ≤ 8%) | ≤ 8% | 0.0% (0/55) | PASS |
| H6 (JOB-Slow geomean ≥ 1.50×) | ≥ 1.50× | 1.038× | **FALSIFIED** |

## 2. Aggregate measurement

| Cell | Value |
|---|---|
| Queries timed | 55 / 113 |
| Geomean wallclock — native DataFusion 46 | 2047.9 ms |
| Native BCa 95% CI on geomean | [1682.489, 2463.125] ms |
| Geomean wallclock — samkhya (sidecar-fed) | 1972.2 ms |
| samkhya BCa 95% CI on geomean | [1607.785, 2381.352] ms |
| Speedup geomean (native / samkhya) | 1.038× |
| Speedup BCa 95% CI | [1.026, 1.056] |
| Wilcoxon signed-rank W | 212.0 |
| Wilcoxon p (two-sided) | 3.00e-06 |
| BH-FDR rejects (α=0.05) | 24 / 55 |
| Wins (speedup ≥ 1.05×) | 17 / 55 |
| Ties (0.95× ≤ speedup < 1.05×) | 38 / 55 |
| Losses (speedup < 0.95×) | 0 / 55 |

Industry-standard citations: Moerkotte VLDB 2009 (q-error), Efron-Tibshirani
1993 ch. 14 (BCa), Wilcoxon 1945 (signed-rank), Benjamini-Hochberg 1995 (FDR),
Leis VLDB 2015 (JOB).

## 3. Regressed queries (speedup < 0.95, honest report)

_None — no queries regressed by >5% in the measured run._


## 4. Caveats

- **Cold-cache discipline not applied.** All trials are warm-cache (kernel
  page cache retained CSV pages between queries). Root-only
  `echo 3 > /proc/sys/vm/drop_caches` is not exercised; absolute wallclocks
  are conservative-upward by ~10-30% per `B13_criterion.md §1`.
- **CSV not Parquet.** The IMDb dump is 3.7 GB of raw CSV; every query
  re-parses the relevant column slices. Both arms pay this overhead so the
  speedup ratio cancels, but absolute milliseconds are inflated relative to
  a Parquet-converted corpus.
- **Three trials per mode.** Replicate budget is 3 (not 30 from the prior
  PROJECTED pre-registration) — the BCa CIs are honest for n=3 but wider
  than they would be at n=30. A full 30-replicate sweep is v1.1 work.
- **CPU governor: `powersave`.** Per `B13_criterion.md §1`, absolute
  milliseconds may run 10–30% above `performance`-governor numbers.



<details>
<summary>Original PROJECTED revision (pre-Wave-4F, preserved for audit trail)</summary>

# 18 — samkhya vs native DataFusion 46: master wallclock comparison

**Date:** 2026-05-16
**Sole author:** Prateek Singh
**License:** Apache-2.0
**Hardware reference:** [`00_hardware_profile.md`](./00_hardware_profile.md) — 13th Gen Intel i9-13900HK, 20 threads, 24 MiB L3, single NUMA node.
**System under test:** samkhya v0.4.0 with feedback-driven corrector path enabled.
**Baseline:** DataFusion 46.x native (no `SamkhyaOptimizerRule`, no `SamkhyaStatsExec`, no Puffin sidecars).
**Query corpora location:** `samkhya-bench/src/queries/{synthetic,tpc_h,job_slow,stats_ceb}.rs`
**Timing infrastructure:** `samkhya-bench/src/runner.rs` lines 520–524 (baseline) and 574–578 (corrected) — `std::time::Instant` around `df.collect().await`.

---

## 1. Verdict

**Metric:** end-to-end wallclock latency (ms) P50/P95/P99, **cold-cache and warm-cache
phases distinguished** per ACM Artifact Evaluation v1.1 + campaign canonical. Per-query
speedup `s_i = T_baseline_i / T_samkhya_i`. **Workload-aggregate canonical (Leis et al.
VLDB 2015 / TPC-H convention):** **geometric mean** of `s_i` + **Wilcoxon signed-rank**
paired significance test (Wilcoxon 1945, "Individual Comparisons by Ranking Methods",
*Biometrics Bulletin* 1(6):80–83) on the per-query paired (samkhya, native) P50
wallclock vector — report **W statistic and p-value** per suite and one
**W_aggregate, p_aggregate** across the 113+22+10 combined vector — + **win/tie/loss
distribution** (win = `s_i ≥ 1.05`, tie = `0.95 ≤ s_i < 1.05`, loss = `s_i < 0.95`).
Where the JOB-Slow / TPC-H runner has not yet executed on a populated host, every
Wilcoxon cell is tagged **"Wilcoxon p-value pending — see
[[project-metric-compliance-open-items]]"**. CI methodology: **95% BCa bootstrap
with 10,000 resamples** — bias-corrected and accelerated per **Efron & Tibshirani
1993**, *An Introduction to the Bootstrap*, Chapter 14 — on per-query log-ratios
(this supersedes the prior "percentile bootstrap" text in §3.1). **Benjamini-Hochberg
FDR** at α=0.05 (Benjamini-Hochberg JRSSB 1995) applied across the 113+22+10
query aggregate. Q-error: canonical Moerkotte VLDB 2009 definition.

**PARTIAL — methodology + projection.** Of the four suites listed in this file, only the in-process **synthetic** suite (10 queries) currently runs end-to-end on this host in both arms (native + corrected). The **TPC-H** subset (5 queries) compiles into the bench harness but requires a TPC-H SF=1 SessionContext that is not yet wired into `runner.rs` on this host. The **JOB-Slow** corpus (113 queries) compiles in `samkhya-bench/src/queries/job_slow.rs` and is enumerated by `Suite::JobSlowReal`, but the bench runner's `JobSlowReal` path explicitly requires `imdb_dir` to be configured and the IMDB Parquet corpus to be present (see `samkhya-bench/src/runner.rs:97` and `:276`). The IMDB CSV corpus (~3.7 GB) was fetched into `samkhya-bench/data/job/` on **2026-05-16** (see [`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md)); Parquet re-encoding (`PARQUET=1`) is still pending — DuckDB CLI was not on the fetch host. The 113-query JOB-Slow row is therefore **projected (data acquired 2026-05-16; runner-execution + CSV→Parquet pending)**. The **STATS-CEB** corpus is a placeholder file (zero `Query` entries in `samkhya-bench/src/queries/stats_ceb.rs`); no row is reported.

The aggregate weighted-geomean speedup, total wallclock saved, and win/tie/loss distribution in §6 and §7 are therefore **bounded by the synthetic suite only** and are reported as a lower-confidence preview. The headline number for the paper / blog cannot be finalised until the JOB-Slow row is filled on a host with the IMDB corpus.

---

## 2. Pre-registered hypotheses

Filed before any wallclock run, per `feedback_empirical_methodology`. Intervals are point hypotheses with explicit thresholds, not ranges to be widened post-hoc.

| ID  | Metric                                                          | Threshold                                              |
| --- | --------------------------------------------------------------- | ------------------------------------------------------ |
| H1  | Aggregate weighted-geomean speedup across all suites combined   | ≥ **1.35×** (samkhya faster than native DataFusion 46) |
| H2  | Fraction of queries where samkhya wins (≥ 5% faster than native)| ≥ **75 %**                                             |
| H3  | Fraction of queries where samkhya regresses (> 5% slower)       | ≤ **8 %**                                              |
| H4  | Per-suite weighted-geomean speedup — synthetic                  | ≥ 1.10× (the in-memory suite has small absolute scope) |
| H5  | Per-suite weighted-geomean speedup — TPC-H                      | ≥ 1.25×                                                |
| H6  | Per-suite weighted-geomean speedup — JOB-Slow                   | ≥ 1.50× (cardinality estimation matters most here)     |

A run that lands outside an interval falsifies the hypothesis and triggers a separate investigation document, not a goalpost shift.

---

## 3. Methodology

### 3.1 Definitions

- **Speedup per query**: `s_i = T_baseline_i / T_samkhya_i`, where `T_*` is median end-to-end wallclock from `df.collect().await` over `R` replicates.
- **Geometric mean speedup**: `geomean(s_1..s_N) = exp(mean(ln s_i))`. Resistant to outliers, the standard aggregate for ratio metrics.
- **Median speedup**: 50th percentile of `s_i`.
- **Win**: `s_i ≥ 1.05` (samkhya at least 5% faster).
- **Tie**: `0.95 ≤ s_i < 1.05`.
- **Loss / regression**: `s_i < 0.95` (samkhya > 5% slower than native).
- **Wallclock saved**: `Σ_i (T_baseline_i − T_samkhya_i)` over all queries in the suite, both arms running once per query for the canonical comparison (not the replicate-budgeted run).
- **95% bootstrap CI on aggregates**: **BCa bootstrap with 10 000 resamples** of
  the per-query speedup vector, with replacement, per the campaign canonical —
  bias-corrected and accelerated per **Efron & Tibshirani 1993**, *An Introduction
  to the Bootstrap*, Chapter 14. CI is the 2.5th / 97.5th BCa percentile (with
  bias-correction `z_0` and acceleration `a`) of the resampled metric. Resample
  seed `0xDEADBEEFCAFEBABE` (splitmix64 mixer).
- **Paired significance (canonical):** **Wilcoxon signed-rank test** (Wilcoxon
  1945, "Individual Comparisons by Ranking Methods", *Biometrics Bulletin*
  1(6):80–83; Leis VLDB 2015 convention) for paired per-query speedup comparisons
  — report **W statistic and p-value** per suite and one **W_aggregate,
  p_aggregate** for the combined 113+22+10 vector. Where the runner has not yet
  executed on a host with the relevant corpus, all Wilcoxon cells are tagged
  **"Wilcoxon p-value pending — see [[project-metric-compliance-open-items]]"**.
  **Benjamini-Hochberg FDR** at α=0.05 (Benjamini-Hochberg JRSSB 1995) across
  the per-query Wilcoxon p-values.
- Seeds follow **first-seed-tried** convention; no seed search.

### 3.2 Trace-back: which B0x file feeds which row

Every cell in §5 / §6 must trace back to an underlying B0x measurement file. The map is fixed up-front:

| Row                            | Underlying measurement                                                                              | Source file                                  |
| ------------------------------ | --------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| Synthetic suite, 10 queries    | `samkhya-bench compare --suite synthetic` with `latency_ms` from `runner.rs:524` (baseline) and `:578` (corrected) | observed in [`B12_valgrind.md §4`](./B12_valgrind.md) on host i9-13900HK, run under sanitizer load |
| TPC-H subset, 5 queries        | TPC-H SF=1 SessionContext run via `samkhya-bench compare --suite tpc-h`                            | **projected, awaiting host with TPC-H SF=1 parquet** |
| JOB-Slow full, 113 queries     | `samkhya-bench compare --suite job-slow-real --imdb-dir <path>`                                    | **projected, data acquired 2026-05-16 (CSVs in `samkhya-bench/data/job/`, see [`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md)); runner-execution pending** |
| Single-table micro suite       | `samkhya-core` criterion benches (HLL/Bloom/CMS micro-ops, not full-query wallclock)                | [`B13_criterion.md §4`](./B13_criterion.md)  |
| Examples (smoke-only sanity)   | `samkhya-datafusion::b05_smoke` 4-query smoke with SamkhyaStatsExec wired                          | [`B14_examples.md §3.4`](./B14_examples.md)  |
| Reproducibility (run-to-run σ) | Same-suite re-run drift                                                                             | [`B19_reproducibility.md`](./B19_reproducibility.md) |
| Hardware envelope              | i9-13900HK topology, CPU governor `powersave` during measurement                                    | [`00_hardware_profile.md`](./00_hardware_profile.md), [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) |

The single-table micro suite (B13) measures sketch construction wallclock, not query wallclock; it does not produce a `T_baseline_i / T_samkhya_i` ratio against a native DataFusion plan and is therefore reported alongside, not folded into, the aggregate. See §8 for why.

### 3.3 Replicate budget

For the rows that are currently measurable on this host (synthetic, B05 smoke), `R = 10` replicates per query were collected in the only end-to-end run we have transcripts of (`B14_examples.md §3.4`: "Internally runs all queries 10 times and checks determinism"). Within-query σ on `b05_smoke` was small enough that all 10 replicates produced identical row counts; per-iteration wallclock variability was bounded by the criterion-style noise envelope of `±10–15 %` documented in [`B13_criterion.md §10`](./B13_criterion.md) under the `powersave` governor.

For the projected rows, the budget specification is `R = 10` replicates × 113 queries × 2 arms = 2 260 query executions for JOB-Slow alone. At an estimated 1.5 s median per query (slow-tier JOB queries), that is ~57 min of wallclock per full sweep. Budget allowance: ≤ 2 h, fits within a single overnight host slot.

### 3.4 Why CPU governor matters here

Both [`B13_criterion.md §1`](./B13_criterion.md) and the run transcripts in [`B14_examples.md`](./B14_examples.md) executed under governor `powersave` (interactive `sudo cpupower` was unavailable in those sessions). Absolute wallclock numbers may be 10–30 % above bare-metal `performance`-governor numbers. **Speedup ratios cancel this**: if both arms run under the same governor in the same session, the multiplier on each cancels in `T_baseline / T_samkhya`. Speedups in this document are therefore valid as ratios even though absolute milliseconds are conservative.

---

## 4. What is and is not in scope

In scope:

- End-to-end wallclock of `df.collect().await` for both arms.
- Plan-time overhead is included in `T_samkhya` because the timed region begins at `ctx.sql(q.sql).await` (see `runner.rs:521` / `:575`), which is after `create_physical_plan`. **Caveat:** the corrector-injection time is *not* inside the timed region; it occurs during `create_physical_plan` (called above, lines 506–510 / 557–561). This is the right choice because the corrector runs as a `PhysicalOptimizerRule` in production and is part of plan time, not execution time. If a future revision moves the corrector into the timed region, this document must be updated.
- Wallclock saved totals are over the queries we ran, not over a hypothetical workload mix.

Out of scope:

- Plan-time overhead in isolation (separate file: this is what [`B13_criterion.md`](./B13_criterion.md) covers for the sketch primitives, not the corrector).
- Multi-tenant interference, NUMA pinning, hyperthread placement — single-host single-process measurement.
- Memory footprint — see [`B14_examples.md`](./B14_examples.md) for binary sizes; no resident-set wallclock cost was tracked here.
- Engines other than DataFusion 46 (DuckDB, Polars, Postgres) — those go in their own `18b_*`, `18c_*`, `18d_*` companion files.

---

## 5. Per-suite results

### 5.1 Synthetic suite (10 queries, in-process)

Source: `samkhya-bench/src/queries/synthetic.rs` (10 `Query` entries, verified by `grep -c "^    Query {"`).

| Cell                                 | Value                                                                  |
| ------------------------------------ | ---------------------------------------------------------------------- |
| Queries in suite                     | 10                                                                     |
| Queries that returned a valid speedup| 10 (all complete end-to-end on this host)                              |
| Median speedup                       | **(projected, requires re-run with timing capture in compare mode)**   |
| Geomean speedup                      | **(projected, requires re-run with timing capture in compare mode)**   |
| 95% bootstrap CI on geomean          | **(projected)**                                                        |
| Wallclock saved (s)                  | **(projected)**                                                        |
| Win count (s ≥ 1.05)                 | **(projected)**                                                        |
| Tie count (0.95 ≤ s < 1.05)          | **(projected)**                                                        |
| Loss count (s < 0.95)                | **(projected)**                                                        |

Status caveat: the synthetic suite **does** run end-to-end (confirmed by [`B12_valgrind.md §4`](./B12_valgrind.md) and [`B14_examples.md §3.4`](./B14_examples.md)) and the runner **does** record `latency_ms` for both arms, but the bench session that produced those transcripts did not emit a side-by-side baseline/corrected wallclock table to a file we can cite here. The numbers are **measurable today, not yet recorded in a B0x file**. A 10-minute re-run of `samkhya-bench compare --suite synthetic --replicates 10 --out bench-results/B21_synthetic_wallclock.json` produces the missing artifact and unblocks this row. This is the cheapest path to a real number in §6.

Known issue, synthetic-only: B12 §6.2 reports `inf` q-error on queries S2–S5, S7, S9, S10 because the DataFusion baseline estimate collapses to 0 (division by zero in q-error). This is a **correction-quality** signal, not a **wallclock** signal. Wallclock measurements on these same queries are still valid — `latency_ms` is independent of whether the estimate was zero. Wallclock speedup may even be negative on synthetic because the in-memory tables are too small (1k–10k rows) for any plan improvement to recoup corrector overhead. Synthetic is **not where we expect a speedup win**; it is in scope as a smoke check that wallclock measurement infrastructure works.

### 5.2 TPC-H subset (5 queries, projected)

Source: `samkhya-bench/src/queries/tpc_h.rs` (5 `Query` entries, verified by `grep -c "^    Query {"`).

| Cell                                 | Value                                                              |
| ------------------------------------ | ------------------------------------------------------------------ |
| Queries in suite                     | 5 (Q1, Q3, Q5, Q6, Q10 — the canonical "easy five" subset)         |
| Median speedup                       | **(projected, awaiting host with TPC-H SF=1 parquet)**             |
| Geomean speedup                      | **(projected)**                                                    |
| 95% bootstrap CI on geomean          | **(projected)**                                                    |
| Wallclock saved (s)                  | **(projected)**                                                    |
| Win / tie / loss                     | **(projected)**                                                    |

Status caveat: `Suite::TpcH` enumerates the queries but `runner.rs` does not yet register a TPC-H SessionContext (no `lineitem`, `orders`, `customer`, etc. tables registered). Pre-condition for filling this row: a `--tpch-dir <path>` flag analogous to `--imdb-dir`, plus parquet snapshots of the 8 TPC-H tables at scale factor 1. Scale-factor-1 fits comfortably in 24 MiB L3 for the partsupp/customer tables and exercises plan-quality differences on Q3/Q5/Q10 (join-heavy).

### 5.3 JOB-Slow (113 queries, projected — headline row)

Source: `samkhya-bench/src/queries/job_slow.rs` — full 113-query roster, verified by the in-file integration test `roster_has_113_queries`.

| Cell                                 | Value                                                                            |
| ------------------------------------ | -------------------------------------------------------------------------------- |
| Queries in suite                     | 113                                                                              |
| Median speedup                       | **(projected; data acquired 2026-05-16, runner-execution pending — see [`EMP09_imdb_fetch.md`](./EMP09_imdb_fetch.md))** |
| Geomean speedup                      | **(projected)**                                                                  |
| 95% bootstrap CI on geomean          | **(projected)**                                                                  |
| Wallclock saved (s)                  | **(projected)**                                                                  |
| Win / tie / loss                     | **(projected)**                                                                  |

Status caveat: `Suite::JobSlowReal` exists and `runner.rs` accepts `--imdb-dir`, but the IMDB Parquet corpus (~3.6 GB extracted from the IMDB CSV dump) is not on this host. This is the **headline row** for the paper. Pre-condition: a host with the IMDB corpus rebuilt to Parquet via the canonical CMU pipeline, runnable in one overnight slot (≤ 2 h compute budget, see §3.3).

### 5.4 Single-table micro suite (criterion, scope-boundary)

Source: `samkhya-core/benches/{sketches,puffin,stress}.rs` — the bench inventory recorded in [`B13_criterion.md §2`](./B13_criterion.md).

This row is **not** a head-to-head speedup row. It measures the absolute cost of sketch construction and Puffin I/O on the samkhya side only; there is no native-DataFusion analog (DataFusion does not construct HLL sketches as part of its baseline plan). The B13 numbers are recapped here as a sanity reference, not as a contribution to the aggregate.

| B13 bench                               | Median   | Per-item   | Throughput  |
| --------------------------------------- | -------- | ---------- | ----------- |
| `hll/hll_add_100k`                      | 977 µs   | 9.77 ns    | 102 M/s     |
| `bloom/bloom_insert_10k`                | 166 µs   | 16.61 ns   | 60 M/s      |
| `puffin_write_10_blobs`                 | 3.60 µs  | —          | —           |
| `stress/feedback_ten_thousand_observations` | 52.4 ms  | 5.24 µs    | —           |

These are the per-build costs that the corrector pays per query (or per dataset, for the HLL build that happens once). They feed into the wallclock comparison only as a **plan-time overhead bound**: any query where `T_baseline < ~5 ms` will see the corrector cost dominate and likely produce a regression. JOB-Slow has zero queries with `T_baseline < 100 ms`; TPC-H SF=1 has one or two on Q1/Q6; synthetic has all ten in this regime.

---

## 6. Aggregate speedup (across all suites combined)

| Metric                                          | Value                                                       |
| ----------------------------------------------- | ----------------------------------------------------------- |
| Total queries with a real measurement           | 0 (no B0x file records baseline vs corrected wallclock pairs yet) |
| Total queries projected after host fills        | 128 (10 synthetic + 5 TPC-H + 113 JOB-Slow)                 |
| Weighted-geomean speedup, all suites            | **(projected, awaiting §5.1 re-run + §5.2 + §5.3 host)**    |
| 95% bootstrap CI on weighted-geomean (resample queries w/ replacement, 10 000 draws) | **(projected)** |
| Win-rate (fraction with s ≥ 1.05)               | **(projected)**                                             |
| Regression-rate (fraction with s < 0.95)        | **(projected)**                                             |
| Tie-rate                                        | **(projected)**                                             |
| Hypothesis H1 (geomean ≥ 1.35×) outcome         | **(pending)**                                               |
| Hypothesis H2 (win-rate ≥ 75 %) outcome         | **(pending)**                                               |
| Hypothesis H3 (regression-rate ≤ 8 %) outcome   | **(pending)**                                               |

Weighting convention, fixed up-front to avoid post-hoc tuning: each query contributes 1 to the geometric mean regardless of suite. **No** by-suite reweighting is applied. Rationale: re-weighting (e.g., "JOB-Slow counts 10× because it is the headline corpus") makes the aggregate non-falsifiable. The natural query count per suite already gives JOB-Slow 113/128 ≈ 88 % of the aggregate weight, which is the correct empirical emphasis.

---

## 7. Win / tie / loss distribution

| Suite                    | Win (≥ 1.05×) | Tie (0.95–1.05) | Loss (< 0.95) | Total |
| ------------------------ | ------------- | --------------- | ------------- | ----- |
| Synthetic                | (projected)   | (projected)     | (projected)   | 10    |
| TPC-H subset             | (projected)   | (projected)     | (projected)   | 5     |
| JOB-Slow                 | (projected)   | (projected)     | (projected)   | 113   |
| **Total**                | (projected)   | (projected)     | (projected)   | 128   |

Anti-cherry-pick guarantee: every query in every suite contributes one row to this table, even queries where samkhya regresses. Loss-bin queries are listed by name in the companion JSON artefact `bench-results/B21_synthetic_wallclock.json` (and `B22_*`, `B23_*` once filled). The paper / blog cannot quote the win-rate without also quoting the regression list.

---

## 8. Wallclock-saved analysis

| Suite                    | `Σ T_baseline` (s) | `Σ T_samkhya` (s) | Wallclock saved (s) | Saved % |
| ------------------------ | ------------------ | ----------------- | ------------------- | ------- |
| Synthetic                | (projected)        | (projected)       | (projected)         | (projected) |
| TPC-H subset             | (projected)        | (projected)       | (projected)         | (projected) |
| JOB-Slow                 | (projected)        | (projected)       | (projected)         | (projected) |
| **Total across workload**| **(projected)**    | **(projected)**   | **(projected)**     | **(projected)** |

Interpretation guard: "wallclock saved" is the **sum of differences**, not the **difference of sums after re-ordering**. A single query that goes from 30 s to 5 s contributes the same 25 s to this column as 25 queries that each go from 2 s to 1 s. Both regimes will be visible in the per-query JSON; the aggregate cell hides the distribution. Readers who care about long-tail latency improvements should consult the per-query JSON, not this cell.

---

## 9. Discussion: what the headline number means

A few notes before any cell in §6 is filled:

1. **Speedup is the right metric, throughput is not.** This is a latency document. Wallclock saved per query is the unit; throughput-per-core is a derived quantity that requires a multi-query concurrent workload, which is out of scope. If a follow-up paper measures throughput, it must specify its concurrency model and not reuse this file's numbers.

2. **Geomean over individual queries, not arithmetic mean over wallclocks.** Arithmetic mean on wallclock is dominated by the longest query in the corpus (one 60-second outlier swamps 100 sub-second queries). Geomean over per-query speedup ratios is the standard for cross-corpus aggregates and is what every cardinality-estimation paper from CMU and TUM has used since 2015.

3. **Plan-time corrector overhead is real.** For JOB-Slow, where median `T_baseline` is in the 1–10 s range, plan-time corrector cost (~5 ms for the LpBound evaluation, ~50 µs for the residual corrector by H5/H6 in [`01_cpu_baseline_multithread.md §2`](./01_cpu_baseline_multithread.md)) is a < 0.5 % overhead. For synthetic, where `T_baseline` is in the 1–10 ms range, it can be 50 % or more. Synthetic is **expected** to underperform on wallclock; the value is in correction quality, not wallclock.

4. **The win is concentrated in plan-shape changes, not in per-row execution.** samkhya does not make any execution-engine code faster. It changes which plan DataFusion picks. The wallclock improvement comes entirely from picking better join orders / better aggregate strategies / better partition counts because the row-count estimates are corrected. There is **no** SIMD kernel, no rewritten hash join, no custom vectorized scan in samkhya v0.4. If the baseline plan is already optimal (e.g., a single-table scan), samkhya cannot win.

5. **Caveat: DataFusion 46 is a moving baseline.** This number is valid against DataFusion 46.x. A future DataFusion that improves its native cardinality estimation (e.g., adds histograms by default, or integrates samkhya-style sketches in-tree) would shrink the corrected gap. This file should be re-run against each major DataFusion release and the old numbers archived, not overwritten.

---

## 10. Limitations

1. **No measurement on this host for any speedup row.** Every cell in §5–§8 is currently `(projected)`. The synthetic row is the lowest-cost to fill (10 min on this host); TPC-H is next (one parquet pipeline run); JOB-Slow is the gating dependency (host with IMDB corpus).

2. **STATS-CEB is absent.** `samkhya-bench/src/queries/stats_ceb.rs` contains zero `Query` entries (verified by `grep -c "^    Query {"`). It is enumerated in the `Suite` enum but is a placeholder. Until the STATS-CEB SQL is translated into the harness, this corpus contributes nothing.

3. **Single host, single hardware envelope.** All numbers (once filled) are on the i9-13900HK / 24 MiB L3 / single NUMA node profile documented in [`00_hardware_profile.md`](./00_hardware_profile.md). Server-class hardware (high core-count Xeon / EPYC, NUMA-aware DataFusion) is **not** sampled. Cross-platform sweep is the job of [`B10_cross_platform.md`](./B10_cross_platform.md), which currently reports a build matrix not a perf matrix.

4. **CPU governor was `powersave` in all recorded sessions.** Ratios are unaffected (see §3.4) but absolute milliseconds in any future cell will be 10–30 % above `performance`-governor numbers. The publication run for this file must set the governor to `performance` and document it.

5. **No multi-threaded DataFusion scaling study folded into this aggregate.** The 4-operation multi-thread sweep in [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) is methodology-only at this revision. If the aggregate ever combines wallclock-saved across thread counts, it must explicitly weight by the per-thread-count workload share, which is currently unspecified.

6. **Pre-execution corrector cost is not included in the timed region.** See §4 "in scope". This is the right choice for a "wallclock as seen by the user" metric but it does mean a separate `bench-results/B22_corrector_plan_time_overhead.md` file is needed to bound the off-clock cost.

7. **`bloom_contains_miss` / `hll_estimate` noise.** B13 §6 flagged 10–11 % high-severe outliers on these primitives. Their cost is amortised per-query (one estimate per leaf), so the noise propagates only weakly into per-query wallclock. But on very short queries (synthetic) the propagation is not negligible and the synthetic CI will be wider than the TPC-H / JOB-Slow CIs.

8. **B16 / B17 do not contribute to this file.** Doctests and multi-Python wheels do not measure wallclock against native DataFusion and are excluded from §5 by design. They appear in [`BINARY_ACCEPTANCE_REPORT.md`](./BINARY_ACCEPTANCE_REPORT.md) for the v1.0 acceptance gate, not here.

---

## 11. Reproducibility (ACM Artifact Evaluation v1.1)

To re-run the rows that are currently fillable on this host:

```
# Synthetic, 10 queries, both arms, 10 replicates each.
# Produces bench-results/B21_synthetic_wallclock.json with per-query
# baseline_ms / corrected_ms / replicate vectors.
cargo run --release -p samkhya-bench -- compare \
  --suite synthetic \
  --replicates 10 \
  --out bench-results/B21_synthetic_wallclock.json
```

For the rows that require host setup:

```
# TPC-H SF=1. Pre-condition: 8 parquet files at $TPCH_DIR.
cargo run --release -p samkhya-bench -- compare \
  --suite tpc-h \
  --tpch-dir $TPCH_DIR \
  --replicates 10 \
  --out bench-results/B22_tpch_wallclock.json

# JOB-Slow real, 113 queries. Pre-condition: IMDB parquet at $IMDB_DIR.
cargo run --release -p samkhya-bench -- compare \
  --suite job-slow-real \
  --imdb-dir $IMDB_DIR \
  --replicates 10 \
  --out bench-results/B23_jobslow_wallclock.json
```

After all three JSON files exist, this file is regenerated by:

```
python3 bench-results/scripts/aggregate_wallclock.py \
  --inputs B21_synthetic_wallclock.json \
           B22_tpch_wallclock.json \
           B23_jobslow_wallclock.json \
  --bootstrap-resamples 10000 \
  --out bench-results/18_vs_native_datafusion_wallclock.md
```

CPU governor pre-condition for the publication run:

```
sudo cpupower frequency-set -g performance
# Verify:
cpupower frequency-info | grep "current policy"
```

Governor must be restored to `powersave` afterwards if running on the laptop host. The `cpupower` step is **not** automated in `samkhya-bench` itself; it is an operator pre-condition documented in [`B13_criterion.md §1`](./B13_criterion.md).

### 11.1 Statistical post-processing (canonical pair)

- **95% BCa bootstrap CIs** on the per-suite and combined geomean speedup —
  10 000 resamples on the per-query log-ratio vector with replacement,
  bias-corrected and accelerated per **Efron & Tibshirani 1993**, *An
  Introduction to the Bootstrap*, Chapter 14. Resample seed
  `0xDEADBEEFCAFEBABE`. The `aggregate_wallclock.py` script invokes the BCa
  branch (`--method bca`) explicitly.
- **Wilcoxon signed-rank test** — Wilcoxon 1945, "Individual Comparisons by
  Ranking Methods", *Biometrics Bulletin* 1(6):80–83; Leis VLDB 2015 convention
  — on the per-query paired (T_baseline_i, T_samkhya_i) median wallclocks.
  Report **W, p-value** per suite and one **W_aggregate, p_aggregate** across
  the combined 113+22+10 vector. Until each suite's run JSON lands, every
  Wilcoxon cell carries **"Wilcoxon p-value pending — see
  [[project-metric-compliance-open-items]]"**.

---

## 12. Pointer to next-step files

- [`B21_synthetic_wallclock.json`](./B21_synthetic_wallclock.json) — **to be produced**, blocks synthetic row.
- [`B22_tpch_wallclock.json`](./B22_tpch_wallclock.json) — **to be produced**, blocks TPC-H row.
- [`B23_jobslow_wallclock.json`](./B23_jobslow_wallclock.json) — **to be produced**, blocks the headline JOB-Slow row.
- [`scripts/aggregate_wallclock.py`](./scripts/aggregate_wallclock.py) — **to be written**, the aggregator that fills §6 / §7 / §8 from the three JSON files via 10 000-draw bootstrap.

Until those four artefacts exist, this file is the **specification** of the master comparison, not the **result** of it. The verdict at the top of the file must remain `PARTIAL — methodology + projection` and no cell in §5 / §6 / §7 / §8 may quote a number to a downstream paper or blog post.


</details>
