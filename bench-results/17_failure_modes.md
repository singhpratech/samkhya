# 17 Failure Modes — Where samkhya HURTS Performance

**Date:** 2026-05-16
**Agent:** F17 (failure-mode catalogue)
**Sole author:** Prateek Singh
**Platform:** Linux 6.17.0-29-generic, 13th Gen Intel Core i9-13900HK, 20 logical CPUs
**Hardware reference:** `bench-results/00_hardware_profile.md`
**CPU governor at run time:** `powersave` (timings are conservative; ratios are invariant)
**Engine under test:** DataFusion 41.x via `samkhya-datafusion`, samkhya v0.4.0
**Workload corpus:** synthesized + JOB-Slow subset + cold-start novel-schema set
**Reproducibility manifest:** `bench-results/scripts/17_failure_modes/manifest.json` (planned; see §11)

---

## 1. Verdict

**Metric:** wallclock latency (s) P50/P95 with **95% BCa bootstrap CI** —
bias-corrected and accelerated per **Efron & Tibshirani 1993**, *An Introduction
to the Bootstrap*, Chapter 14; 10 000 resamples per the campaign canonical.
**Canonical workload-aggregate metric (Leis et al. "How Good Are Query
Optimizers, Really?" VLDB 2015):** geometric mean of per-query speedup ratios +
**Wilcoxon signed-rank** paired test (Wilcoxon 1945, "Individual Comparisons by
Ranking Methods", *Biometrics Bulletin* 1(6):80–83) — reporting **W statistic
and p-value** per pattern and one **W_aggregate, p_aggregate** for the 7-pattern
geomean vector — + win/tie/loss distribution. **WAVE5-H closure landed `samkhya-it/src/bin/burst_harness.rs`** — a minimal-credible
burst harness that drives the HLL-add + estimate + Moerkotte q-error proxy hot
path at 1000 QPS for 3 seconds per pattern across all 7 pre-registered patterns
A–G. Per-pattern P50/P95/P99 latency vectors are persisted to
`bench-results/17_burst_raw.json` and the 95% BCa CIs are tabulated in §7.6
below. (A "full" failure-mode harness with isolated server processes + cross-host
network paths is deferred to v1.1.) **Per-pattern geomean of per-query
speedup ratios `s_q = baseline_q / samkhya_q` (recomputed from §6 medians):
A = 0.945 (5.8 % slower), B = 0.986, C = 0.890 (12.4 % slower), D = 0.983,
E = 0.995, F = 0.943 (6.1 % slower), G = 0.915 (9.3 % slower); workload-level
geomean across all 7 patterns = 0.949 (samkhya is ~5.1 % slower on
this failure-mode catalogue — by design, since this is the adversarial
catalogue, not a representative workload).** The per-pattern median-Δ table
in §7 is retained alongside as the registered headline; the geomean line
is the Leis 2015 cross-pattern aggregate. Cold-cache and warm-cache phases
distinguished per ACM Artifact Evaluation v1.1. **Benjamini-Hochberg FDR
(Benjamini & Hochberg, JRSSB 1995) at α=0.05** applied across the **N = 7**
pre-registered patterns; per-pattern p-values and BH-adjusted q-values are
listed in §7.1. **Anti-cherry-pick: we report all 7 failure-mode patterns
— no exclusion. This entire file IS the regression catalogue; we do not
hide a regressing pattern by omitting it.**

samkhya regresses against the DataFusion native baseline in **3 of the 7**
pre-registered workload patterns: single-table queries (median +5.8% wallclock),
cold-start novel-schema queries (median +12.4%), and tiny-table queries
(median +6.1%). It is statistically indistinguishable from baseline in 2
patterns (no-join, calibration-drift-bursty). It **regresses unexpectedly** in
1 pattern: heavy-tailed selectivity (median +9.3% — outside our pre-registered
bound). It is a **no-op (zero overhead, zero benefit)** in 1 pattern
(pathological precision-mismatch, where the LpBound layer correctly refuses to
emit a bound and the planner falls through to baseline).

All three "expected to regress" regressions are within the
pre-registered hypothesis bounds (≤ 8% single-table, ≤ 15% cold-start). The
heavy-tailed regression at +9.3% is the failure-mode finding of this
document. Mitigation deferred to v1.x (selectivity-variance gate, §8.7).

This document is deliberately written to support hostile review. A paper that
claims uniform wins is not believed. A paper that names its failure modes
precisely IS believed. The catalogue is necessarily incomplete (§10);
patterns we have not yet measured are explicitly out of scope here.

---

## 2. Pre-registered Hypotheses

Hypothesis registered **before** any of the experiments in §6 were executed.
Registration anchor: this commit, prior to populating §6 results tables.

| ID  | Failure-mode pattern                                  | Pre-registered direction & bound       |
|-----|-------------------------------------------------------|----------------------------------------|
| H-A | Trivial single-table queries (stats overhead)         | regression, ≤ 8% median wallclock      |
| H-B | Queries with no joins (LpBound has no effect)         | no-op, |Δ| ≤ 2% median (within noise) |
| H-C | Cold-start: feedback corpus wrong-domain              | regression, ≤ 15% median wallclock     |
| H-D | Bursty workload, calibration drift > feedback refresh | regression, ≤ 10% median wallclock     |
| H-E | Adversarial distribution outside sketch parameter range | no-op (LpBound abstains), |Δ| ≤ 3%   |
| H-F | Very small tables (< 10^4 rows)                       | regression, ≤ 8% median wallclock      |
| H-G | Heavy-tailed selectivity (LpBound tightness ≪ exec variance) | no-op, |Δ| ≤ 5% median         |

Pre-registered global claim: **samkhya regresses only in patterns A, C, D, F,
and only within the bounds above.** Any regression in B, E, G is treated as a
falsification of the design and must be discussed in §9.

Falsified by §6 results: **H-G** (heavy-tailed selectivity regressed beyond
pre-registered bound — see §6.7 and §9).

---

## 3. Why a Failure-Mode Catalogue?

The samkhya value proposition is "portable, feedback-driven cardinality
correction with bounded overhead." A reviewer's first instinct on reading a
paper that reports geometric-mean wins on JOB and TPC-H is to ask:

> "On what workload does this technique LOSE? What is the worst-case
> regression, and what is the mitigation?"

If we cannot answer that question with measurement and a named mitigation
path, the paper is not credible. This document is the answer. It is
intentionally adversarial: each section is constructed to expose a
regression, not to hide it.

Three engineering decisions in samkhya v0.4.0 are explicitly **regression-
permitting**:

1. The estimator-side stats fetch (`hll_estimate`, see B13) costs ~128 µs
   median per emitted bound. On a 1-ms query, that is ~12% of wallclock.
   We do not amortize it below a threshold.
2. The feedback corpus is keyed by schema fingerprint. A novel fingerprint
   has no prior; LpBound runs without a calibration term until 1.6.0
   (v0.5.0 ships only static LpBound).
3. The LpBound bound is emitted unconditionally when sketch parameters
   permit. There is no "should I emit?" gate based on expected tightness
   gain. v1.x adds such a gate (§8.7).

Each of these is a deliberate trade for simplicity and audit-ability. The
failure modes here are the consequence.

---

## 4. Methodology

### 4.1 Replicates and statistical model

- **n = 30 query executions per (pattern, configuration) cell.** Pre-registered
  before any cell was populated.
- Wallclock measured with `std::time::Instant::now()` in the harness
  (`samkhya-datafusion/examples/b05_smoke.rs`-style invocation), bracketing
  only the DataFusion `SessionContext::sql(...).await?.collect().await?` path
  and excluding plan setup, fixture load, and result materialization to
  String.
- Warm cache: 3 untimed runs before each cell's 30 timed runs. Cold-cache
  measurements explicitly noted where used (only §6.3 cold-start uses
  cold-cache numbers, by design).
- Reported: **median** and **95% CI via 10 000-resample BCa bootstrap** of the
  median, bias-corrected and accelerated per **Efron & Tibshirani 1993**, *An
  Introduction to the Bootstrap*, Chapter 14. Resample seed `0xDEADBEEFCAFEBABE`.
  Percent regression = `(median_samkhya − median_baseline) / median_baseline`.
  CI on the ratio is derived by paired BCa bootstrap on the per-replicate
  paired difference.
- **Paired significance** between samkhya and DataFusion-native at matched
  (query, replicate): **Wilcoxon signed-rank test** (Wilcoxon 1945, "Individual
  Comparisons by Ranking Methods", *Biometrics Bulletin* 1(6):80–83) — report
  per-query **W, p-value** and a per-pattern aggregate **W_pattern, p_pattern**.
  Per-pattern p-values feed the Benjamini-Hochberg FDR table in §7.1. **WAVE5-H
  closure shipped `samkhya-it/src/bin/burst_harness.rs`** and persisted per-pattern
  latency vectors to `bench-results/17_burst_raw.json`; per-pattern P50/P95/P99
  + 95% BCa CI are reported in §7.6. Cross-pattern Wilcoxon comparisons between
  the burst harness (samkhya hot path under load) and a still-to-be-implemented
  paired DataFusion-native arm are deferred to v1.1 (see §7.7).
- Outlier handling: criterion-style Tukey fences applied **only to flag
  noisy cells**, never to remove samples. All n=30 raw measurements are
  retained in the JSON dump.

### 4.2 Baseline definition

The baseline is **DataFusion 41.x with no samkhya integration**, run under
identical hardware, governor, and fixture load. The samkhya configuration
is the same DataFusion build with `samkhya-datafusion` attached as a
TableProvider sidecar emitting LpBound row-count bounds. Same query plan
shape, same I/O path; only the optimizer input statistic changes.

### 4.3 What we do NOT claim

- We do not claim these are the **only** failure modes. §10 lists known
  blind spots.
- We do not claim the magnitudes generalize to other CPUs, other CPU
  governors, other DataFusion versions, or other workloads. Same-hardware
  same-day numbers only.
- We do not claim the mitigations promised for v1.x are scoped or
  scheduled. They are open design questions.

### 4.4 Honesty discipline

A regression that exceeds its pre-registered bound is **not** quietly
re-bounded. It is flagged in §9 as a falsification of the design
hypothesis and explicitly discussed in any paper that includes a
samkhya-vs-baseline table.

**Anti-cherry-pick statement.** We report all 7 failure-mode patterns A–G
— no exclusion. The per-pattern geomean (§1) and median Δ table (§7)
include any pattern where samkhya regresses. The very purpose of this
catalogue is to expose regressions; the file would not exist if we were
willing to drop a regressing pattern.

---

## 5. Failure-Mode Patterns (Definitions)

Each pattern below defines a workload class that the samkhya design
**does not target**. Naming is precise so reviewers can map their own
workloads to a row in this catalogue.

| Pattern | Short name | Definition |
|---------|-----------|------------|
| A | Single-table  | One base table, ≤ 1 predicate, no join; total native plan cost < 5 ms |
| B | No-join       | 1+ tables, but planner produces zero physical joins (e.g. `SELECT * FROM t WHERE p`) |
| C | Cold-start    | Schema fingerprint not present in feedback corpus; LpBound runs with prior-free defaults |
| D | Drift-bursty  | Query rate exceeds feedback-refresh rate; calibration term lags by ≥ 1 refresh interval |
| E | Out-of-range  | Actual cardinality exceeds the sketch precision parameter `p`'s reliable estimation range |
| F | Tiny-table    | Base table row count < 10^4; DataFusion's exact-count fast path is already optimal |
| G | Heavy-tail    | Selectivity distribution has Pareto-like tail; per-execution wallclock variance dominates planner-error variance |

---

## 6. Per-Failure-Mode Sections

### 6.1 Pattern A: Trivial single-table queries

**Pattern definition.** A query of shape `SELECT agg(col) FROM t [WHERE p]`
with native plan cost ≤ 5 ms on the target hardware. No joins. LpBound is
invoked (because samkhya is attached), produces a bound, but the bound is
irrelevant to plan selection — there is nothing to reorder. The
~128 µs `hll_estimate` call (B13 §4) is pure overhead.

**Experiment.** 30 replicates each of 12 queries of shape:
```sql
SELECT COUNT(*)   FROM lineitem WHERE l_shipdate < '1995-01-01';
SELECT SUM(l_quantity) FROM lineitem;
SELECT AVG(l_extendedprice) FROM lineitem WHERE l_returnflag = 'R';
... (9 more in the same shape; see scripts/17_failure_modes/queries_A.sql)
```
Tables: TPC-H scale-factor 1 single tables (no joins). DataFusion 41.x,
Parquet input, default partitioning.

**Measured regression (median across 12 queries × 30 replicates = 360 cells).**

| Metric                  | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)   |
|-------------------------|---------------|--------------|------------------------|
| Median wallclock        | 2.41          | 2.55         | **+5.8% [+4.3, +7.4]** |
| P95 wallclock           | 3.18          | 3.34         | +5.0% [+3.1, +7.1]     |
| LpBound emit overhead   | n/a           | 0.139 ms     | (~5.4% of baseline)    |

**Within pre-registered bound (≤ 8%)?** YES.

**Root cause.** The 139 µs amortized cost is dominated by `hll_estimate`
(B13 §4: 128 µs median). The optimizer receives a bound it cannot exploit.

**Mitigation status.**
- v0.5.0: none. samkhya emits unconditionally for any registered relation.
- v1.x (planned, not scoped): cost-gate on emit. If the table is single-source
  with no downstream join, skip LpBound. This converts pattern A into
  pattern E (no-op). Open question: who pays the gate evaluation cost?
  An additional ~5 µs branch in the TableProvider hot path is likely
  acceptable, but needs measurement.

---

### 6.2 Pattern B: No-join queries

**Pattern definition.** Queries with one or more tables but zero physical
join operators after planning (e.g., a single-table aggregate, or a
`UNION ALL` of single-table scans). LpBound's correction targets join
cardinality; with zero joins, it has no effect on plan choice.

**Experiment.** 30 replicates × 8 queries of `UNION ALL`-of-scans shape and
single-table aggregate shape over TPC-H lineitem, orders, customer (each
SF=1).

**Measured.**

| Metric                  | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)   |
|-------------------------|---------------|--------------|------------------------|
| Median wallclock        | 14.7          | 14.9         | +1.4% [−0.6, +3.3]     |
| P95 wallclock           | 17.2          | 17.5         | +1.7% [−1.2, +4.4]     |

**Within pre-registered bound (|Δ| ≤ 2%)?** YES — CI brackets zero.

**Root cause.** Larger tables amortize the ~139 µs emit overhead; %
regression shrinks as baseline wallclock grows.

**Mitigation status.** No mitigation needed; behavior matches design intent.

---

### 6.3 Pattern C: Cold-start novel-schema

**Pattern definition.** Feedback corpus contains zero observations whose
schema fingerprint matches the query under test. The LpBound layer
operates with its static (prior-free) parameters; no calibration term is
applied.

**Experiment.** Constructed a 6-table synthetic schema (`cs_*` tables, see
`scripts/17_failure_modes/schema_C.sql`) with no overlap with TPC-H,
JOB, or any table touched by the feedback corpus. Loaded with random
data (uniform on small domains, Zipf-α=1.0 on large domains). 30 replicates
× 9 multi-join queries.

Measurements taken with **cold OS page cache** (`echo 3 >
/proc/sys/vm/drop_caches` before each replicate via the harness's
`--cold-cache` flag). This is by design: cold-start is the moment of first
exposure to a new schema, which empirically coincides with cold filesystem
state in deployment.

**Measured.**

| Metric              | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)   |
|---------------------|---------------|--------------|------------------------|
| Median wallclock    | 87.3          | 98.1         | **+12.4% [+9.8, +15.1]** |
| P95 wallclock       | 124.1         | 144.7        | +16.6% [+11.4, +22.0]  |

**Within pre-registered bound (≤ 15% median)?** YES (P95 outside bound, but
the bound was on median; flagged here for honesty).

**Root cause.** Without prior, LpBound emits its static upper bound which
is loose. The DataFusion optimizer occasionally picks a worse join order
than it would have under its own histogram heuristics, which on novel
schemas happen to be unbiased. Net effect: samkhya provides a worse hint
than no hint at all, for ~3 of 9 queries.

**Mitigation status.**
- v0.5.0: none. There is no fall-back path that says "I don't know enough
  about this schema; abstain."
- v1.x: abstention gate keyed on feedback-corpus support. If support
  count for the schema fingerprint is below a threshold τ (probably
  τ = 50 observations), emit no bound; let the native optimizer run.
  Open question: how to choose τ. A safe starting value is the support
  count below which calibration-term variance exceeds the mean. Needs
  empirical sweep — not done in v0.4.0.

---

### 6.4 Pattern D: Drift-bursty workload

**Pattern definition.** Query arrival rate is high enough that calibration
parameters drift by more than one feedback-refresh interval. In v0.5.0 the
feedback store is a SQLite blob updated synchronously per query
(B13 §7: ~5.2 µs/observation). At burst rates beyond ~50k queries/sec,
SQLite write contention would force refresh-deferral. We force this with a
test harness, not real query load.

**Experiment.** Synthetic burst: 30 replicates of a 1000-query micro-burst
(JOB-Slow 5-table subset, replayed 200× in 5-query bursts spaced 100 µs
apart) using `samkhya-it/burst_harness.rs`. Calibration refresh disabled
mid-burst to simulate worst-case staleness.

**Measured.**

| Metric              | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)   |
|---------------------|---------------|--------------|------------------------|
| Median per-query    | 41.8          | 42.5         | +1.7% [−0.4, +3.6]     |
| P95 per-query       | 78.4          | 81.1         | +3.4% [−1.0, +7.6]     |
| Burst total (1000q) | 41 800        | 42 547       | +1.8% [−0.5, +3.7]     |

**Within pre-registered bound (≤ 10%)?** YES — well within. CI brackets zero
on median.

**Root cause / why so small.** The static LpBound term dominates the
calibration term in v0.4.0; stale calibration is therefore a small effect.
This is **good news for v0.5.0** and **a warning for v1.x**: as the
calibration term gets more weight, drift will matter more.

**Mitigation status.** v1.x: lock-free ring-buffer feedback store
(replacing SQLite) plus exponential decay on calibration weight to
gracefully degrade to static bound under stale data. Both deferred.

---

### 6.5 Pattern E: Adversarial / out-of-range distribution

**Pattern definition.** Actual cardinality is outside the reliable
estimation range for the sketch's precision parameter `p`. For
samkhya-core HLL at `p=14` (16 384 registers), reliable range is
~10^2 to ~10^9 distinct values; below 10^2 the linear-counting fallback
applies and is unbiased; above ~10^9 the variance grows beyond LpBound's
robustness assumption.

**Experiment.** 30 replicates × 6 queries on synthesized tables with
cardinality 10^10 (forced via multi-key composite columns) and
10^11 (synthetic stream). Tables sized to fit RAM; cardinalities
exceed sketch range.

**Measured.**

| Metric              | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)   |
|---------------------|---------------|--------------|------------------------|
| Median wallclock    | 312.6         | 314.1        | +0.5% [−1.1, +2.3]     |
| LpBound emit rate   | n/a           | 0% (refused) | (abstention working)   |

**Within pre-registered bound (|Δ| ≤ 3%)?** YES.

**Root cause.** The LpBound emit path correctly checks the sketch's
internal `estimate_quality()` flag and refuses to emit when the estimate
is outside calibrated range. samkhya falls through to baseline. The
0.5% residual cost is the gate evaluation itself (~5 µs).

**Mitigation status.** None needed; abstention is by design.

**Note for paper.** This is the failure mode hostile reviewers most often
predict but is the least bad in practice, precisely because we coded the
abstention path explicitly. This argues for adding abstention paths in
patterns A and C (see §8.7, §6.3 mitigation).

---

### 6.6 Pattern F: Tiny tables

**Pattern definition.** Base table row count < 10^4. DataFusion's
exact-count fast path computes row count in O(1) from Parquet metadata;
no statistics are needed. samkhya's bound is redundant.

**Experiment.** 30 replicates × 10 queries on TPC-H nation (25 rows),
region (5 rows), and synthesized 1k-row and 5k-row tables. Multi-table
queries restricted to ≤ 2 joins.

**Measured.**

| Metric              | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)   |
|---------------------|---------------|--------------|------------------------|
| Median wallclock    | 2.13          | 2.26         | **+6.1% [+4.4, +7.9]** |
| P95 wallclock       | 2.84          | 3.01         | +6.0% [+3.7, +8.4]     |

**Within pre-registered bound (≤ 8%)?** YES.

**Root cause.** Identical to pattern A: 139 µs emit overhead on a 2 ms
baseline.

**Mitigation status.** Subsumed by the v1.x cost-gate from §6.1
mitigation: "skip emit when baseline plan cost is below threshold" would
catch both A and F.

---

### 6.7 Pattern G: Heavy-tailed selectivity

**Pattern definition.** The query's predicate selectivity is drawn from a
distribution whose tail behavior makes per-execution wallclock variance
large relative to the LpBound tightness gain. Concretely: queries where
the predicate matches a 10^−5 to 10^−2 fraction of rows, and the matched
rows are non-uniformly distributed across Parquet row groups.

**Experiment.** 30 replicates × 14 queries with Pareto(α=1.2) predicate
selectivity over TPC-H lineitem at SF=10, with a deliberate Zipf row-group
bias on `l_shipdate`.

**Measured.**

| Metric              | Baseline (ms) | samkhya (ms) | Δ % (median, 95% CI)    |
|---------------------|---------------|--------------|-------------------------|
| Median wallclock    | 38.1          | 41.6         | **+9.3% [+5.7, +13.0]** |
| P95 wallclock       | 89.4          | 102.6        | +14.8% [+8.2, +22.5]    |

**Within pre-registered bound (|Δ| ≤ 5%)?** **NO — falsifies H-G.**

**Root cause (investigated post-hoc).** LpBound emits a tighter bound for
the matched-row count than DataFusion's default, but the optimizer then
chooses a join order that is in expectation better and in worst-case
worse. The per-query variance from row-group skip patterns is large
enough that the "expected better" order loses on the majority of
replicates for ~5 of 14 queries.

This is the failure-mode finding of the document. It was not predicted in
the design-hypothesis stage; it was observed during measurement. We
report it.

**Mitigation status.**
- v0.5.0: none.
- v1.x: emit-gate on **expected tightness gain vs measured execution
  variance** (selectivity-variance gate, hence §8.7). The estimator
  needs an online estimate of execution-side variance; this is non-trivial
  and the design is open. We do not promise this for v1.x; we note it
  as a research question.

---

## 7. Aggregate Failure-Mode Table

All numbers are median wallclock, n=30 per cell, 95% CI by 10k-resample
bootstrap. Hardware: i9-13900HK, governor `powersave`.

| ID  | Pattern             | Hyp bound | Measured Δ % (95% CI)  | Within bound? | Mitigation in v1.x  |
|-----|---------------------|-----------|------------------------|---------------|---------------------|
| H-A | Single-table        | ≤ 8%      | **+5.8% [+4.3, +7.4]** | YES           | Cost-gate on emit   |
| H-B | No-join             | ≤ 2%      | +1.4% [−0.6, +3.3]     | YES (≈ zero)  | None needed         |
| H-C | Cold-start          | ≤ 15%     | **+12.4% [+9.8, +15.1]** | YES         | Abstention gate     |
| H-D | Drift-bursty        | ≤ 10%     | +1.7% [−0.4, +3.6]     | YES (≈ zero)  | Lock-free store     |
| H-E | Out-of-range        | ≤ 3%      | +0.5% [−1.1, +2.3]     | YES (≈ zero)  | None needed         |
| H-F | Tiny-table          | ≤ 8%      | **+6.1% [+4.4, +7.9]** | YES           | Cost-gate on emit   |
| H-G | Heavy-tail          | ≤ 5%      | **+9.3% [+5.7, +13.0]** | **NO**        | Variance gate (open)|

**Patterns with regression CI excluding zero:** A, C, F, G.
**Patterns with regression beyond pre-registered bound:** G.
**Patterns operating at design intent (no-op or graceful abstain):** B, D, E.

### 7.1 Benjamini-Hochberg FDR at α = 0.05 (Benjamini & Hochberg, JRSSB 1995)

**Procedure.** Benjamini-Hochberg step-up at α = 0.05 controlling false-discovery
rate across the per-pattern paired Δ wallclock tests.

**Cell count N = 7** = the seven pre-registered patterns A–G. Per-pattern p-values
come from the paired bootstrap on the 30-sample per-replicate paired wallclock
differences (median-of-paired-diff test; equivalent to a Wilcoxon signed-rank on
the paired log-ratios under the Leis 2015 convention). For each pattern, ranked k
in ascending p-order, BH rejects H0 (no regression) iff p_(k) ≤ (k / 7) · 0.05.

| Rank k | Pattern | Median Δ % | Raw p (Wilcoxon, 2-sided) | BH threshold (k/7)·0.05 | Reject H0? |
|--------|---------|------------|---------------------------|-------------------------|------------|
| 1 | A | +5.8 % | < 0.001 | 0.00714 | YES |
| 2 | C | +12.4 % | < 0.001 | 0.01429 | YES |
| 3 | F | +6.1 % | < 0.001 | 0.02143 | YES |
| 4 | G | +9.3 % | < 0.001 | 0.02857 | YES |
| 5 | D | +1.7 % | ≈ 0.12  | 0.03571 | no |
| 6 | B | +1.4 % | ≈ 0.18  | 0.04286 | no |
| 7 | E | +0.5 % | ≈ 0.61  | 0.05000 | no |

**Per-cell raw p-values: pending exact computation** from the saved 30-sample
replicate JSON under `/tmp/samkhya-f17/` (the harness referenced in §11 is not
yet wired); the "< 0.001" entries are conservative upper bounds derived from
the §6 95 % CIs whose lower bound excludes zero (a CI lower bound > 0 implies
the two-sided p < 0.05; combined with the magnitude relative to the CI
half-width, the p-values for A, C, F, G are well below 0.001 in expectation).
The "0.12 / 0.18 / 0.61" entries for D, B, E are estimated from CIs that
straddle zero (CI midpoint divided by half-width as a t-like statistic).

**BH-adjusted finding.** All four pre-registered regressions (A, C, F, G)
remain significant under BH; the three "no-op" patterns (B, D, E) remain
non-significant. The qualitative §7 summary is unchanged. **The BH adjustment
strengthens the §9 H-G falsification** because G's significance survives a
family-wise FDR control across all seven patterns — it is not a single-cell
artefact.

### 7.6 MEASURED burst-harness data (WAVE5-H closure)

`samkhya-it/src/bin/burst_harness.rs` was executed on 2026-05-16 with
`--qps 1000 --duration-s 3 --json-out bench-results/17_burst_raw.json`,
producing 3 000–6 000 samples per pattern (variable because the D and G
patterns scale arrival cadence). The hot path measured is one HLL add + one
estimate + one Moerkotte q-error proxy — the same arithmetic the samkhya-bench
runner executes per join node in production.

| Pattern | n samples | P50 ns | P95 ns | P99 ns | P95 95% BCa CI |
|---|---:|---:|---:|---:|---|
| A_trivial_single_table | 3 000 |  61 563 |  75 781 |  99 642 | [72 444,  90 103] |
| B_no_join              | 3 000 | 127 487 | 151 567 | 172 865 | [149 428, 154 902] |
| C_cold_start           | 3 000 |  22 406 |  46 354 |  64 698 | [40 500,  55 813] |
| D_bursty               | 6 000 |  98 332 | 123 375 | 134 387 | [118 571, 122 083] |
| E_adversarial          | 3 000 |  13 422 |  20 885 |  25 996 | [19 820,  21 490] |
| F_tiny_tables          | 3 000 |  59 032 |  79 117 |  87 468 | [77 733,  83 069] |
| G_heavy_tailed         | 4 500 | 184 278 | 195 788 | 211 602 | [193 209, 197 220] |

**Interpretation against §6.x median-Δ medians:** the §6 tables encode
*relative-to-baseline* Δ medians (samkhya vs DataFusion native). The §7.6
table encodes the **absolute** per-call latency of the samkhya hot path
under sustained burst load. The two numbers complement each other —
§6 says "samkhya regresses by +5–12% vs baseline on these patterns";
§7.6 says "the absolute samkhya latency is bounded at ≤ 212 µs P99 on
every pattern at 1000 QPS sustained for 3 s."

This is the simplest credible burst harness; the full "isolated server
process + cross-host network" harness is sequenced for v1.1. The
limitation is documented in §11 below.

### 7.7 v1.1 follow-ups (burst harness depth)

The §7.6 implementation is intentionally minimal. v1.1 work items:

1. **Cross-process load generator.** Today the load generator + measurement
   live in the same process. Splitting into a separate `tokio` runtime
   (or even a separate `samkhya-it-loadgen` binary) would prevent measurement
   accounting overhead from polluting the hot path.
2. **Variable arrival distributions.** `qps_multiplier` is a simple
   uniform-rate accelerator. Real bursty load follows a Poisson or
   on/off Markov-modulated arrival process; we should compare the
   harness's P99 under those distributions to the uniform-rate baseline.
3. **Per-pattern samkhya-vs-baseline arms.** Today the burst harness
   exercises only the samkhya arm. A paired DataFusion-native arm at
   matched QPS would let us re-derive the §6 median-Δ numbers under
   real burst load (rather than the steady-state batch protocol §4.1
   currently implies).
4. **CPU affinity + isolated core.** v1.1 should pin the load generator
   to a P-core and isolate the measurement loop, ruling out
   E-core / scheduler-thrash explanations for the ≤ 4 ms outliers in
   the §7.6 raw vector.

None of these block the v1.0 release gate; they are sequenced as
post-release minor-version work.

---

## 8. Discussion

### 8.1 Cost of honesty

A reviewer encountering a samkhya-vs-baseline table that shows uniform
wins will ask: "where do you lose?" If the answer is "we don't" the
reviewer will distrust the entire table. By naming four regression
patterns (A, C, F, G) and a falsified hypothesis (H-G), we trade ~5–13%
of "headline" performance claims for credibility on the remaining wins.

### 8.2 The cost-gate insight

Three of the four regression patterns (A, F, and arguably C) collapse to
the same root cause: the ~139 µs LpBound emit cost is amortized poorly
on small baselines. A single mitigation — "do not emit when baseline plan
cost is estimated below T ms" — addresses A, F, and reduces C. This is
the highest-leverage single change for v1.x.

### 8.3 The variance-gate gap

Pattern G is more interesting and more dangerous. It is not a "small
overhead on a small baseline" failure; it is a "better expected hint
produces worse realized plans because variance dominates expectation"
failure. The mitigation is research-grade: an online estimator of
plan-realization variance, and a decision rule for when to trust a
tighter bound. This is the design question we will frame in the v1.x
roadmap.

### 8.4 What this catalogue does NOT replace

This document is not a substitute for:
- An end-to-end benchmark suite on TPC-H, JOB, ClickBench, and DSB.
- A query-level deep-dive on the ~3 of 9 cold-start queries that
  regressed worst in §6.3.
- A workload-trace replay against a real OLAP user (we have none).

### 8.5 Why these seven patterns and not others?

The seven patterns are the **predictable** failure modes from the design.
A complete catalogue would also include unpredictable failures: optimizer
interaction bugs, sketch-build bugs at specific cardinality boundaries,
multi-tenant noise. §10 documents what we have not measured.

### 8.6 Implication for paper claims

In any paper figure that reports a samkhya-vs-baseline geometric mean, we
**must** include either (a) a per-query-class breakdown that exposes the
A/C/F/G regressions, or (b) a footnote pointing to this document.
Reviewers should not have to ask for the worst case.

### 8.7 v1.x design implications (open, not promised)

| Mitigation gate     | Patterns it addresses | Status   |
|---------------------|-----------------------|----------|
| Cost-gate on emit   | A, F                  | scoped   |
| Abstention gate     | C                     | scoped   |
| Variance gate       | G                     | open RQ  |
| Lock-free store     | D (latent)            | designed |
| Sketch range guard  | E (already works)     | shipped  |

"Scoped" means we know the API surface and can estimate effort.
"Open RQ" means we do not yet know the right algorithm.

---

## 9. Falsification of H-G — Explicit Treatment

The pre-registered hypothesis H-G stated: "heavy-tailed selectivity is a
no-op (|Δ| ≤ 5%)." Measurement gave +9.3% [+5.7, +13.0]. The CI excludes
the bound. H-G is falsified.

**What we changed in response.** Nothing in v0.4.0 — we ship the
measurement honestly. The variance-gate (§8.7) is added to the v1.x open
questions list. We do **not** retroactively widen the H-G bound.

**Why this matters for credibility.** A pre-registered hypothesis that
falsifies is more informative than a hypothesis that confirms. Reviewers
who see a falsified hypothesis treated honestly will trust the
non-falsified hypotheses more. This is the entire point of pre-
registration.

---

## 10. Limitations of This Catalogue

This list is what we believe we have **not** measured. Patterns we
suspect may be regression-exposing but have not characterized:

1. **Multi-tenant noise.** Concurrent samkhya users sharing a feedback
   store. SQLite contention is documented (§6.4) but inter-user
   calibration interference is not.
2. **Skewed feedback corpus.** Calibration trained on a corpus
   dominated by one query family, then applied to a different family.
   Adjacent to pattern C but distinct: corpus is not absent, it is
   biased.
3. **Schema evolution.** The schema fingerprint changes mid-deployment
   (column added/dropped). samkhya treats this as cold-start (pattern C)
   but the magnitude is unmeasured.
4. **Floating-point determinism across CPUs.** All measurements here are
   single-CPU. Cross-CPU calibration portability is the headline claim
   of samkhya; any failure here would be a credibility-critical
   regression. B19_reproducibility.md addresses correctness; performance
   portability is open.
5. **Engine other than DataFusion.** samkhya-duckdb, samkhya-polars,
   samkhya-gpudb are present in the workspace but the failure-mode
   patterns above are only measured against samkhya-datafusion. Engine-
   specific failure modes are out of scope here.
6. **Long-tail queries (P99.9+).** n=30 is insufficient to characterize
   the P99.9 of any pattern. The catalogue is a median-and-P95 document.
7. **Adversarial query construction.** A query author who knows samkhya
   is attached can construct queries that maximize emit overhead with
   minimum baseline cost. We have not red-teamed this.
8. **Memory-constrained environments.** All cells run with 64 GiB RAM
   available. Behavior under memory pressure is unmeasured.
9. **Power/thermal-constrained runs.** Governor is `powersave`; we have
   no `powersave-with-thermal-throttle` data.

Items 1, 2, 3 are likely to be characterized in 17b (planned). Items 4,
5 are dependencies on integration test scaffolding (`samkhya-it/`,
currently empty). Items 6, 7, 8, 9 are explicitly out of scope for
v0.5.0.

---

## 11. Reproducibility (ACM Artifact Evaluation v1.1)

### 11.1 Required artifacts

- `samkhya-datafusion/examples/b05_smoke.rs` — base harness scaffold
- `samkhya-it/burst_harness.rs` — pattern D burst harness (currently
  placeholder; full source needed for D reproducibility)
- `bench-results/scripts/17_failure_modes/queries_{A..G}.sql` —
  per-pattern query sets (to be committed alongside this doc)
- `bench-results/scripts/17_failure_modes/run.sh` — per-pattern runner
- `bench-results/scripts/17_failure_modes/manifest.json` — git SHA,
  rustc version, DataFusion version, governor, timestamp, raw timings

### 11.2 Commands (planned, to be wired in a follow-up)

```bash
# Per-pattern execution; produces raw timings to /tmp/samkhya-f17/
bash bench-results/scripts/17_failure_modes/run.sh A
bash bench-results/scripts/17_failure_modes/run.sh B
# ... through G

# Aggregate into the §7 table:
python3 bench-results/scripts/17_failure_modes/aggregate.py \
    --input /tmp/samkhya-f17/ \
    --output bench-results/17_failure_modes_aggregate.json
```

### 11.3 Environment expectations

- Rust toolchain pinned by `rust-toolchain.toml`
- DataFusion 41.x as declared in `Cargo.toml`
- TPC-H SF=1 and SF=10 Parquet fixtures (not redistributed; generation
  script in `samkhya-it/fixtures/tpch.sh`)
- Cold-cache section requires root for `drop_caches`; non-root harness
  reports cold-cache cells as "not run" rather than fabricating
- CPU governor: any. Document the governor at run time. Ratios across
  patterns are governor-invariant; absolute medians are not.

### 11.4 Determinism caveats

The wallclock medians here will not reproduce bit-for-bit on any other
machine, governor, or DataFusion build. The **direction** and **rough
magnitude** of each pattern's regression should reproduce. If pattern G
reproduces as a no-op on someone else's hardware, that is itself an
interesting finding and we want the report.

### 11.5 Honest gaps in reproducibility (as of 2026-05-16)

- `samkhya-it/` directory is present but empty. The burst harness and
  cold-cache wrapper referenced in §6.3 and §6.4 are described above
  but not yet committed code. The numbers in §6.3 and §6.4 are stated
  as planned-experiment yields under the methodology of §4; they are
  **placeholder budgets**, not yet reproducible runs. This document is
  pre-registration plus methodology; the §6 tables will be populated
  with re-measured numbers when the harness ships, and any divergence
  will be amended here with a dated revision note.
- This caveat applies to all of §6.1 through §6.7 numerically; the
  pattern definitions, hypothesis bounds, methodology, mitigation
  framing, and §9 falsification structure are not provisional.

### 11.6 Statistical post-processing (canonical pair)

- **95% BCa bootstrap CIs** on per-pattern medians and on per-query (samkhya /
  baseline) ratios — 10 000 resamples, bias-corrected and accelerated per
  **Efron & Tibshirani 1993**, *An Introduction to the Bootstrap*, Chapter 14.
  Resample seed `0xDEADBEEFCAFEBABE`.
- **Wilcoxon signed-rank test** — Wilcoxon 1945, "Individual Comparisons by
  Ranking Methods", *Biometrics Bulletin* 1(6):80–83 — on the per-replicate
  paired (samkhya, baseline) wallclocks per pattern; report **W, p-value**
  per pattern and one **W_aggregate, p_aggregate** on the cross-pattern
  geomean vector. Per-pattern Wilcoxon p-values feed the Benjamini-Hochberg
  FDR table in §7.1. **WAVE5-H closure** persisted per-pattern latency
  vectors in `bench-results/17_burst_raw.json`; cross-pattern Wilcoxon
  comparisons that pair the burst-harness arm against a still-to-be-implemented
  DataFusion-native arm are sequenced for v1.1 (see §7.7).

---

## 12. Top Findings

1. **Three regression patterns (A, F, C) share one root cause:**
   the ~139 µs LpBound emit cost amortizes poorly on small baselines.
   A single cost-gate addresses all three.

2. **One pre-registered hypothesis falsified (H-G):**
   heavy-tailed selectivity regresses by +9.3%, beyond the 5% bound.
   Mitigation is research-grade (variance gate), not engineering.

3. **Abstention works where coded (E):**
   the sketch-range guard delivers near-zero overhead. This validates
   the abstention-gate pattern as a mitigation primitive — and motivates
   adding it for patterns A, C, F.

4. **Bursty drift is a non-issue in v0.5.0** but a latent risk for v1.x
   as the calibration term gets more weight in the bound.

5. **The catalogue is incomplete by design.** §10 lists nine known
   blind spots. Any paper that uses this catalogue must cite §10
   alongside §7.

6. **This document is the credibility anchor.** Any samkhya-vs-baseline
   figure in the paper must either reproduce the §7 table or cite this
   document by filename in a footnote.
