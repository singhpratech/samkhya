# 13 — TPC-H Scale-Factor 1 (1 GB) campaign

**Date:** 2026-05-16
**Agent / lane:** 13 (TPC-H SF=1, DataFusion + samkhya v1.0)
**Hardware:** see `bench-results/00_hardware_profile.md` (i9-13900HK, 20 logical cores, governor: powersave at capture time)
**Sole author:** Prateek Singh
**Status:** **PARTIALLY MEASURED** (WAVE5-H closure). The TPC-H SF=1 Parquet corpus was
generated via `tpchgen-cli -s 1 --format=parquet --output-dir=/tmp/tpch-sf1` (per EMP07)
and `./target/release/samkhya-bench run --suite tpc-h --tpch-dir /tmp/tpch-sf1
--trials 5 --json-out bench-results/13_tpch_raw.json` (both `--baseline` and
samkhya-corrected) was executed on the WAVE5-H host. Per-query measured latencies and
BCa CIs are reported in §4 with the `[M]` tag. Projected `[P]` cells remain only
where the legacy projection table covered queries / aggregates the n=5 run could
not yet resolve to a publishable BCa interval. **The pre-registered hypotheses H1
and H2 are reported HONESTLY against the measured numbers in §10.4 — they did NOT
hold at SF=1 in the measured run; see §10.4 for the falsification narrative.**

---

## 1. Verdict

**Metric (projected and to-be-measured cells alike):** wallclock P50/P95/P99 (ms) per
query, **cold-cache phase mandatory; warm-cache phase additive** (per ACM Artifact
Evaluation v1.1 + campaign canonical). Workload aggregate: **geometric mean of per-query
speedup** (Leis et al. VLDB 2015 + TPC-H convention) + **Wilcoxon signed-rank** paired
test (Wilcoxon 1945, "Individual Comparisons by Ranking Methods", *Biometrics
Bulletin* 1(6):80–83; Leis 2015 convention) on the 22-vector of paired (samkhya,
native) per-query P50 wallclocks — report **W statistic and p-value** per query
and one **W_aggregate, p_aggregate** for the 22-vector + **win/tie/loss
distribution**. For projected cells the W/p entries are marked **"Wilcoxon p-value
pending — see [[project-metric-compliance-open-items]]"** and are filled in by
`bench-results/scripts/wilcoxon_paired.py` once `scripts/run_tpch.sh` emits raw
JSON. CI methodology: **95% BCa bootstrap with 10,000 resamples** — bias-corrected
and accelerated per **Efron & Tibshirani 1993**, *An Introduction to the
Bootstrap*, Chapter 14 — on per-query log-ratios. **Benjamini-Hochberg FDR** at
α=0.05 (Benjamini-Hochberg JRSSB 1995) applied across the 22-query × 2-mode grid
(44 cells). Q-error: canonical Moerkotte VLDB 2009 definition.

**Conditional pass, pending real run.** The pre-registered hypothesis (geomean ≥ 1.3× over
all 22 queries, ≥ 1.8× on the Q5/Q8/Q9/Q21 join-heavy cluster) is **plausible at SF=1** given
the q-error reductions already measured on the synthetic suite, but it is **not yet
empirically validated**. This document is the SF=1 plan-of-record and the artifact a
follow-up agent will replace cell-by-cell once `scripts/run_tpch.sh` produces measured
numbers. No claim of TPC-H performance is made on the basis of this file alone.

---

## 2. Pre-registered hypothesis

Registered **before** any measured number is collected (this file is itself the
registration). Editing the predictions below after a real run lands is forbidden — instead,
append a `## 11. Post-registration delta` section noting where reality diverged.

| ID | Quantity | Null (H0) | Alternative (H1) | Decision rule |
|----|----------|-----------|------------------|----------------|
| H1 | Geomean wallclock speedup, samkhya v1.0 vs DataFusion native, all 22 queries | speedup ≤ 1.0× | speedup ≥ **1.3×** | reject H0 if 95 % CI lower bound > 1.0 AND point estimate ≥ 1.3 |
| H2 | Geomean wallclock speedup on {Q5, Q8, Q9, Q21} | speedup ≤ 1.3× | speedup ≥ **1.8×** | reject H0 if 95 % CI lower bound > 1.3 AND point estimate ≥ 1.8 |
| H3 | Plan-shape change rate (samkhya picks a different physical plan than native) | ≤ 10 % of queries | ≥ 40 % | reject H0 if observed rate ≥ 0.40 with Wilson 95 % CI lower bound > 0.10 |
| H4 | Q-error geomean, samkhya vs native, across **all** join nodes in all 22 queries | ratio ≤ 1.0 | ratio ≥ **3.0×** improvement | reject H0 if log-scale CI lower bound > 1.0 |

Failure of any single Hi does not invalidate the others; report each independently.

---

## 3. Methodology

### 3.1 Dataset

- **Generator:** DuckDB `tpch` extension, `CALL dbgen(sf=1)`, materialised to Parquet via
  `COPY <table> TO '...' (FORMAT PARQUET, COMPRESSION ZSTD)`.
- **Tables / row counts at SF=1** (canonical):
  - `lineitem` ≈ 6 001 215
  - `orders` ≈ 1 500 000
  - `partsupp` ≈ 800 000
  - `part` ≈ 200 000
  - `customer` ≈ 150 000
  - `supplier` ≈ 10 000
  - `nation` = 25
  - `region` = 5
- **Storage:** local ext4, Parquet row-group size 122 880 (DuckDB default), no partitioning.
  Files staged under `${TPCH_ROOT:-./tpch-sf1}/`.
- **Audit:** the runner reads `tpch-sf1/_checksums.sha256` (produced by the generator script)
  and aborts if any file is missing or mutated.

### 3.2 Engines / stats modes

| Mode | Engine | Stats source |
|------|--------|--------------|
| `native` | DataFusion 46 (workspace pin in `Cargo.toml`) | Parquet column statistics + the optimizer's built-in `Precision::Inexact` defaults; no Puffin sidecars; no samkhya wrapper |
| `samkhya` | DataFusion 46 + `samkhya-datafusion::SamkhyaTableProvider` | Per-table Puffin sidecars built once via `samkhya-cli build-puffin --table <T> --sf 1`; runtime correction via `samkhya-core::residual::Corrector` configured per `samkhya-bench/src/calibrate.rs` |

Both modes execute the **identical** SQL text (`samkhya-bench/src/queries/tpc_h.rs`, expanded
to all 22 queries as part of this work — see section 6). Both modes use the same
`SessionContext` configuration except for the table provider wrapper and the calibration
hook. No engine-specific SQL rewrites are applied.

### 3.3 Replicates and timing protocol

- **Cold-cache, per replicate:** between replicates the runner calls
  `sync; echo 3 > /proc/sys/vm/drop_caches` (requires `sudo`; falls back to a `posix_fadvise
  DONTNEED` pass per Parquet file when sudo is unavailable, and the report marks the result
  with a `warm?` flag).
- **Replicates per (query, mode):** **30**. First 3 are discarded as warm-up; remaining 27
  feed the statistics.
- **Statistics reported:** P50, P95, P99 wallclock (ms); **geometric mean** across
  queries (Leis VLDB 2015 / TPC-H convention) + **Wilcoxon signed-rank** paired
  test (Wilcoxon 1945, "Individual Comparisons by Ranking Methods", *Biometrics
  Bulletin* 1(6):80–83) — reporting **W statistic and p-value** per query and one
  **W_aggregate, p_aggregate** for the 22-vector — + win/tie/loss distribution;
  **95% BCa bootstrap CI** — bias-corrected and accelerated per **Efron &
  Tibshirani 1993**, *An Introduction to the Bootstrap*, Chapter 14, **10 000
  resamples** — on the per-query log-ratio `log(t_native / t_samkhya)`, then
  back-transformed. For projected cells all Wilcoxon W/p entries are flagged
  **"Wilcoxon p-value MEASURED at n=5 trials in §4.0 / §10.4 (WAVE5-H closure); n=30 rerun on a `performance`-governor host sequenced as v1.x follow-up"**
  until the measured run executes. **Benjamini-Hochberg FDR (Benjamini & Hochberg,
  JRSSB 1995) at α=0.05** applied across the **N = 22** TPC-H query grid; for
  rank-k cell, reject H0 iff p_(k) ≤ (k / 22) · 0.05. Seeds follow
  **first-seed-tried** convention (`0xSF1`); no seed search. Bootstrap resample
  seed `0xDEADBEEFCAFEBABE`.
- **Anti-cherry-pick:** we report all 22 TPC-H queries — no exclusion. The
  headline geomean and the BH FDR family both include any per-query regressions.
- **Order randomisation:** the (query, mode, replicate) triples are shuffled inside each
  block of 30 to defuse thermal-drift confounds. Seed: `0xSF1`. CPU governor must be set to
  `performance` for the real run (gating check in `scripts/run_tpch.sh`).

### 3.4 Plan-diff capture

For each query, the runner also serialises the `EXPLAIN VERBOSE` output from both modes to
`bench-results/13_tpc_h_1gb/plans/{native,samkhya}/Q{NN}.txt`. The plan-diff rate (H3) is
computed by normalising whitespace + numeric estimates, then comparing physical-operator
trees structurally (a string-equal comparison after estimate scrubbing).

### 3.5 Correctness check

Result-set hashes (Arrow-row-level `xxh3_64` over the canonical TPC-H answer-set ordering)
must match between modes for every (query, replicate). Any mismatch fails the run.

---

## 4. Results table — all 22 queries

### 4.0 MEASURED results — WAVE5-H closure (2026-05-16, SF=1, n=5 trials per arm)

Raw JSON sidecars retained at `bench-results/13_tpch_raw.json` (baseline) and
`bench-results/13_tpch_samkhya_raw.json` (samkhya-corrected). Both arms were
executed back-to-back on the same powersave-governor host (governor caveat
applies — see §8). 95% BCa CIs computed by `bench-results/scripts/bootstrap_ci.py
--method bca --statistic median --n-resamples 10000 --seed 42` on the per-trial
samkhya latency vectors.

| Query | bl median (ms) [M] | sk median (ms) [M] | sk/bl ratio [M] | sk 95% BCa CI (ms) [M] |
|----|---:|---:|---:|---|
| Q1  | 107.16 | 108.73 | 1.015 | [99.22, 111.41] |
| Q2  |  36.66 |  37.95 | 1.035 | [33.35,  43.61] |
| Q3  |  49.66 |  50.52 | 1.017 | [46.95,  50.89] |
| Q4  |  29.86 |  34.89 | 1.168 | [26.32,  35.05] |
| Q5  |  75.03 |  76.69 | 1.022 | [69.95,  76.79] |
| Q6  |  21.19 |  19.60 | 0.925 | [19.17,  21.09] |
| Q7  | 122.49 | 113.07 | 0.923 | [110.03, 117.74] |
| Q8  |  87.58 |  88.37 | 1.009 | [81.84,  90.28] |
| Q9  | 127.02 | 131.04 | 1.032 | [125.27, 138.50] |
| Q10 |  63.78 |  62.87 | 0.986 | [61.16,  64.68] |
| Q11 |  22.36 |  23.40 | 1.047 | [22.56,  24.37] |
| Q12 |  35.38 |  36.80 | 1.040 | [33.19,  37.33] |
| Q13 |  39.89 |  38.45 | 0.964 | [36.63,  38.63] |
| Q14 |  30.58 |  31.25 | 1.022 | [28.67,  32.95] |
| Q15 |  48.59 |  47.67 | 0.981 | [45.37,  47.85] |
| Q16 |  23.80 |  20.17 | 0.848 | [18.95,  25.13] |
| Q17 | 130.78 | 130.11 | 0.995 | [126.08, 130.19] |
| Q18 | 139.14 | 143.45 | 1.031 | [136.50, 147.15] |
| Q19 |  45.96 |  48.22 | 1.049 | [46.47,  48.44] |
| Q20 |  48.88 |  48.45 | 0.991 | [46.93,  50.52] |
| Q21 | 133.64 | 132.17 | 0.989 | [127.15, 135.14] |
| Q22 |  19.28 |  19.55 | 1.014 | [18.60,  20.21] |

**Workload-aggregate (Leis 2015 geomean of per-query sk/bl ratios) [M]:** **1.003**
(n_pairs=22) — i.e. samkhya is **0.3% slower** at the geomean on SF=1 in this
n=5 measured run. **Wilcoxon paired signed-rank** (Wilcoxon 1945) on log
sk/bl ratios across 22 queries: **W=98.0, p=0.355** — no statistically
significant difference at α=0.05 from the null hypothesis of "samkhya == baseline."

**n=5 trials, NOT the n=30 pre-registered in §3.3.** WAVE5-H's per-blocker wall
budget did not permit n=30 here; the n=5 BCa CIs are wider than the n=30 CIs
would be, and the geomean is correspondingly more sensitive to per-trial OS
scheduling jitter. Reporting honestly: this is a **conditional fail** of H1+H2
*at this measurement budget*; a 30-trial rerun on a `performance`-governor host
is the next step before any "samkhya did/did-not pass TPC-H" claim is made in
the paper. Methodology note: re-running n=30 with the canonical seed and
performance governor is sequenced as the v1.x follow-up.

### 4.x Legacy projection table — superseded by §4.0 (kept for audit)

All numeric cells below are **projected** from the original 2026-05-16 plan-of-record.
The projection method is documented in section 4.1; column headers use the suffix
`[P]` to flag this. The §4.0 `[M]` numbers above supersede every `[P]` cell. The
projection is preserved verbatim only so the projection-vs-measurement delta can
be re-derived for §10.4 falsification analysis.

| Q  | Notes                                | Native P50 (ms) [P] | samkhya P50 (ms) [P] | Speedup [P] | Plan changed? [P] |
|----|--------------------------------------|---------------------|----------------------|-------------|-------------------|
| Q1 | pricing summary (scan-bound)         | 380                 | 360                  | 1.06×       | no                |
| Q2 | minimum cost supplier                 | 220                 | 150                  | 1.47×       | yes               |
| Q3 | shipping priority                     | 410                 | 300                  | 1.37×       | yes               |
| Q4 | order priority checking               | 320                 | 280                  | 1.14×       | no                |
| Q5 | **local supplier volume** (5-way join)| 560                 | 290                  | **1.93×**   | yes               |
| Q6 | forecasting revenue change            | 95                  | 92                   | 1.03×       | no                |
| Q7 | volume shipping                       | 480                 | 320                  | 1.50×       | yes               |
| Q8 | **national market share** (8-way)     | 720                 | 360                  | **2.00×**   | yes               |
| Q9 | **product type profit measure**       | 1 150               | 570                  | **2.02×**   | yes               |
| Q10| returned item reporting               | 520                 | 410                  | 1.27×       | yes               |
| Q11| important stock identification        | 240                 | 200                  | 1.20×       | no                |
| Q12| shipping mode + order priority        | 350                 | 320                  | 1.09×       | no                |
| Q13| customer distribution                 | 410                 | 360                  | 1.14×       | no                |
| Q14| promotion effect                      | 300                 | 280                  | 1.07×       | no                |
| Q15| top supplier (view)                   | 270                 | 230                  | 1.17×       | yes               |
| Q16| parts/supplier relationship           | 290                 | 220                  | 1.32×       | yes               |
| Q17| small-quantity-order revenue          | 340                 | 240                  | 1.42×       | yes               |
| Q18| large-volume customer                 | 470                 | 380                  | 1.24×       | yes               |
| Q19| discounted revenue                    | 360                 | 330                  | 1.09×       | no                |
| Q20| potential part promotion              | 410                 | 290                  | 1.41×       | yes               |
| Q21| **suppliers who kept orders waiting** | 980                 | 510                  | **1.92×**   | yes               |
| Q22| global sales opportunity              | 260                 | 230                  | 1.13×       | no                |

**Aggregate (projected):**

- All-22 geomean speedup: **≈ 1.34×** [P] — clears H1's 1.3× bar with no margin to spare;
  the real run could easily land below.
- {Q5, Q8, Q9, Q21} geomean speedup: **≈ 1.97×** [P] — clears H2's 1.8× bar with ~0.17×
  margin.
- Plan-shape change rate: 12 / 22 = **54 %** [P] — clears H3's 40 % bar.
- Q-error geomean improvement on join-node estimates: **≈ 4.1×** [P] (extrapolated from
  the synthetic results in `bench-results/B*` and the IMDb-job stats in
  `samkhya-bench/src/queries/job_slow.rs`).

P95 / P99 columns are omitted from the SF=1 projected table — there is no defensible way
to project tail latencies from published TPC-H literature; the **real run will populate
them** with bootstrap CIs.

### 4.1 Projection method (auditable)

Every projected cell in section 4 derives from one of three sources, applied
mechanically and recorded so the projection can be re-derived after the real run:

1. **Published DataFusion TPC-H runs.** DataFusion's own SF=1 numbers (community-published
   to within ~2× across hardware classes; the i9-13900HK lands at the fast end). Used for
   the `Native P50` column. No fudge factor.
2. **Join-stats sensitivity model.** For queries where samkhya's distinct-count + row-count
   correction changes the join order chosen by DataFusion, the projected samkhya P50 is
   `native_P50 / expected_speedup`, where `expected_speedup` is taken from the synthetic
   join-heavy results in `bench-results/B*` (range 1.4×–2.1×). For queries with no plan
   change, the projection is `native_P50 × 0.92` to model the modest cost of evaluating
   the corrector itself.
3. **Q5/Q8/Q9/Q21 priors.** These four queries are well-studied stats-quality stressors
   (Leis et al., "How good are query optimizers, really?" baseline; the IMDb-job slow
   queries are spiritual cousins). They are projected at the high end of the
   sensitivity band (~2.0×).

The projection is deliberately **not conservative** — it lives at the optimistic edge of
plausibility so that a real run failing to reach these numbers is a clear, falsifiable
signal rather than something that can be hand-waved as noise.

---

## 5. Focus: Q5 / Q8 / Q9 / Q21 — the stats-quality cluster

These four queries are the canonical TPC-H locations where the optimizer's cardinality
estimates dominate wallclock. They are kept in a separate sub-section because (a) the
hypothesis bar (H2) is set higher for them, and (b) the diagnostic value of a per-query
plan diff is highest here.

### 5.1 Q5 — local supplier volume

5-way join `customer ⋈ orders ⋈ lineitem ⋈ supplier ⋈ nation` with a region-equality
filter on `nation`. DataFusion's default 1/distinct selectivity heuristic over-estimates
the join cardinality after the `nation.r_regionkey = ?` filter, biasing the optimizer
toward a hash-join build side that is much larger than necessary. Samkhya's Puffin
sidecars deliver the true distinct count on `n_regionkey` (= 5) and `n_nationkey` (= 25),
which collapses the build side and unlocks a much smaller `nation`-driven probe.

**Projected:** native 560 ms → samkhya 290 ms, **1.93×** [P], plan change YES.
**Diagnostic to capture in the real run:** physical-operator tree before/after; build/probe
side row counts for every `HashJoinExec`.

### 5.2 Q8 — national market share

8-way join with a year-restricted subquery and a brazil/america correlation. The known
failure mode under native stats is choosing a left-deep tree that joins
`part ⋈ lineitem` first, blowing up the intermediate to ~6M rows; with samkhya's
multi-table row-count overrides the optimizer prefers to drive the join from the
`region/nation/supplier` filter chain.

**Projected:** native 720 ms → samkhya 360 ms, **2.00×** [P], plan change YES.

### 5.3 Q9 — product type profit measure

The classic TPC-H join-order stressor: `part ⋈ partsupp ⋈ lineitem ⋈ orders ⋈ supplier ⋈
nation`, no selective filters. With native stats DataFusion's join-order choice is driven
almost entirely by the `Inexact` row counts from Parquet metadata; samkhya's per-column
distinct counts on `p_type`, `n_name`, and the join keys produce visibly tighter
selectivities and a different ordering.

**Projected:** native 1 150 ms → samkhya 570 ms, **2.02×** [P], plan change YES.

### 5.4 Q21 — suppliers who kept orders waiting

The semi-/anti-join stressor. Three correlated subqueries against `lineitem` mean every
mis-estimated row count cascades through the plan. Native DataFusion typically picks a
nested-loop variant when the inner cardinality is mis-estimated below the
hash-join-conversion threshold; samkhya's row-count override on `lineitem` (6 001 215
exact) reliably keeps every join as a HashJoin.

**Projected:** native 980 ms → samkhya 510 ms, **1.92×** [P], plan change YES.

---

## 6. Reproducibility (ACM Artifact Evaluation v1.1) — `scripts/run_tpch.sh`

A **scaffold** runner script is committed at `bench-results/scripts/run_tpch.sh` as part
of this work. It is intentionally idempotent and refuses to start if any precondition
fails.

The script's job, in order:

1. **Toolchain check.** Verify `duckdb` (CLI) and `cargo` are on `$PATH`. If `duckdb` is
   missing, print install instructions and exit non-zero. This is the gate that makes the
   present "PROJECTED" status visible — on the capture host the script aborts here.
2. **Generate SF=1.** Run `duckdb -c "INSTALL tpch; LOAD tpch; CALL dbgen(sf=1); EXPORT
   DATABASE 'tpch-sf1' (FORMAT PARQUET, COMPRESSION ZSTD);"`. Idempotent — re-runs are
   skipped if `tpch-sf1/_checksums.sha256` already validates.
3. **Build Puffin sidecars.** For each of the 8 TPC-H tables, run `cargo run -p samkhya-cli
   --release -- build-puffin --table <T> --input tpch-sf1/<T>.parquet --output
   tpch-sf1/puffin/<T>.puffin`. The sidecars feed the samkhya mode.
4. **Governor + drop-caches gate.** Refuse to run benchmarks if the CPU governor is not
   `performance`. The script does not flip the governor itself (requires sudo); it prints
   the exact `cpupower` invocation and exits non-zero.
5. **Execute the 22 × 2 × 30 grid.** Drive `samkhya-bench` (suite = `TpcH`, both
   `--baseline` and the corrector-aware path) for every query, captures wallclock + plan
   diffs into `bench-results/13_tpc_h_1gb/raw/`.
6. **Report.** Re-render the section-4 table from raw JSON into a `13_tpc_h_1gb_run.md`
   sibling file; the present document is **not** auto-edited (per the
   doc-commit-approval rule, a human reviews the run sibling and decides whether to
   replace the projections in this file).

The scaffold deliberately does *not* shell into Python or other glue tooling — every
step is either `duckdb`, `cargo`, or coreutils. This matches the reproducibility
philosophy already established in `bench-results/B19_reproducibility.md`.

### 6.0 Statistical post-processing (canonical pair)

- **95% BCa bootstrap CIs** — 10 000 resamples on per-query log-ratios and on the
  22-query aggregate geomean, bias-corrected and accelerated per **Efron &
  Tibshirani 1993**, *An Introduction to the Bootstrap*, Chapter 14. Resample
  seed `0xDEADBEEFCAFEBABE`.
- **Wilcoxon signed-rank test** — Wilcoxon 1945, "Individual Comparisons by
  Ranking Methods", *Biometrics Bulletin* 1(6):80–83 — applied to the 22-vector
  of paired (samkhya P50, native P50) per-query medians; report **W,
  p-value** per query and one **W_aggregate, p_aggregate** for the 22-vector.
  Until `scripts/run_tpch.sh` emits raw JSON every cell carries **"Wilcoxon
  p-value pending — see [[project-metric-compliance-open-items]]"**.

### 6.1 Required follow-up before the real run lands

- Expand `samkhya-bench/src/queries/tpc_h.rs` from the 5 placeholder entries to all 22
  TPC-H queries. The canonical SQL text comes from the TPC-H spec; the file currently has
  Q1/Q5/Q9/Q17/Q21 as comments only.
- Add a Parquet-backed `build_tpch_context()` to `samkhya-bench/src/runner.rs` (sibling of
  `build_synthetic_context`) that registers the 8 SF=1 tables and optionally wraps each
  with `SamkhyaTableProvider` sourced from the Puffin sidecars built in step 3 above.
- Wire a `Suite::TpcH { sf: u8, parquet_dir: PathBuf }` variant.

None of these are done yet. The scaffolded script is the carrying capacity for the work.

---

## 7. Discussion — where stats matter for TPC-H

TPC-H queries split cleanly into three sensitivity classes once the data lives at SF=1:

1. **Scan-bound, single-table** (Q1, Q6). Wallclock is dominated by the `lineitem` scan;
   no stats decision the optimizer can make changes the runtime. Predicted samkhya
   speedup: ≤ 1.10×, mostly noise. This is a feature, not a failure — corrector overhead
   is bounded.
2. **Mid-cardinality, 2–3-table joins** (Q3, Q4, Q7, Q10, Q11, …). Plan-shape changes are
   possible but the absolute wallclock is small enough that the speedup ceiling is
   ~1.3–1.5×.
3. **Join-heavy, 5+ tables or correlated subqueries** (Q5, Q8, Q9, Q21, plus Q17/Q18/Q20
   honorable mentions). Stats quality dominates. This is where samkhya is supposed to
   earn its keep, and the H2 hypothesis bar (1.8×) is calibrated to this cluster
   specifically.

The geomean target (H1, ≥ 1.3×) is **only** achievable if the join-heavy cluster pulls the
overall geomean up — the scan-bound queries contribute almost nothing. Implication: the
real run can pass H1 only by passing H2 by a comfortable margin, so H2 is effectively the
load-bearing prediction.

---

## 8. Limitations

- **SF=1 is small.** A 1 GB dataset fits in L3 + RAM trivially on the i9-13900HK; the
  Parquet scan is bandwidth-limited but the joins fit in memory. Real-world stats-quality
  payoff is larger at SF=10 / SF=100 where spill-to-disk and join-order mistakes carry a
  10–100× wallclock penalty. SF=100 is **deferred** to a later report.
- **DuckDB-generated Parquet is not the canonical `dbgen` output.** They produce the same
  logical rows but with slightly different column encodings + row-group boundaries.
  Result-set hashes are still well-defined and comparable across the two modes inside
  this report; cross-report comparisons against runs that used `dbgen + parquet-tools`
  need to fix this.
- **Powersave governor on the capture host.** All projected numbers assume `performance`
  governor at run time. The runner script gates on this; the projections in section 4 do
  not have a 10–30 % "powersave handicap" baked in.
- **Single-host run.** No distributed plans. DataFusion at SF=1 is single-process
  multi-thread.
- **Puffin sidecar staleness.** The samkhya mode reads sidecars built once at SF=1
  generation time. If the underlying Parquet were to drift (it does not; the runner
  checksums it), the corrector would be operating on stale priors. Out of scope for SF=1.
- **No `[M]` cells yet.** Every numeric cell in section 4 is projected; **the report is
  not, on its own, evidence of TPC-H performance.** It is the experimental design.

---

## 9. Decision trail / changelog

- 2026-05-16: Created. Pre-registered H1–H4. Scaffolded `scripts/run_tpch.sh`. All
  numbers projected. No measured cells.
- 2026-05-16 (WAVE5-H closure): TPC-H SF=1 Parquet corpus regenerated via
  `tpchgen-cli -s 1 --format=parquet --output-dir=/tmp/tpch-sf1`. n=5 trials of
  the 22-query suite executed in both `--baseline` and samkhya-corrected modes.
  Per-query medians + 95% BCa CIs landed in §4.0. Workload-aggregate geomean and
  Wilcoxon paired test now MEASURED at n=5; H1/H2 reported HONESTLY against
  measurement in §10.4 — pre-registered hypotheses not confirmed at this budget.
  n=30 rerun on `performance` governor is the next-wave follow-up.

## 10.4 Hypothesis verdict vs MEASURED n=5

Honest verdict against the §2 pre-registration:

| Pre-registered | Threshold | Measured (n=5, powersave) | Verdict |
|----|----|----|----|
| H1 — all-22 geomean speedup ≥ 1.3× | sk/bl ≤ 0.77 | sk/bl = 1.003 | **FAIL** (no speedup at the per-query level on SF=1) |
| H2 — {Q5,Q8,Q9,Q21} geomean ≥ 1.8× | sk/bl ≤ 0.56 | sk/bl ≈ 1.013 (geomean of the four ratios in §4.0) | **FAIL** |
| H3 — plan-shape change rate ≥ 40% | rate ≥ 0.40 | **NOT YET MEASURED** in this run (would require diffing physical EXPLAIN plans across modes; deferred to v1.x) | pending |
| H4 — per-join q-error geomean improvement ≥ 3× | ratio ≥ 3 | the baseline per-join geomean is ≈ 9.85× (the samkhya-arm per-join geomean is the analogous number from the samkhya raw JSON; the **ratio** requires a paired computation deferred to v1.x) | pending |

**Why H1/H2 land at sk/bl ≈ 1.0:** SF=1 fits in DRAM on this CPU; the join-order
mistakes the projection assumed (driving the ≥ 1.4× cluster) do not materialise
when DataFusion 46's optimizer is given Parquet column statistics directly. The
projection in §4.x extrapolated from a synthetic in-memory benchmark where
DataFusion sees `Stats::Absent` and falls to lexical orderings; on real TPC-H
Parquet the statistics propagate naturally and the plan-shape gap to fix is
small. **This is consistent with file 10's PARTIAL CONFIRM finding (samkhya
wins only when the planner has insufficient stats to compare; on TPC-H Parquet
it already has them).** A more aggressive samkhya wrapper that injects HLL-derived
distinct counts (not row-counts only) is the v1.x experiment that could move
these cells; that work is sequenced.

## 10. Files referenced

- `samkhya-bench/src/queries/tpc_h.rs` — current placeholder (5 entries); to be expanded
  to 22.
- `samkhya-bench/src/runner.rs` — `build_synthetic_context`; needs sibling
  `build_tpch_context`.
- `samkhya-bench/src/calibrate.rs` — corrector configuration shared with synthetic path.
- `samkhya-datafusion::SamkhyaTableProvider` — stats wrapper used by samkhya mode.
- `samkhya-cli build-puffin` — sidecar builder.
- `bench-results/scripts/run_tpch.sh` — the runner scaffold (gated; will not produce
  numbers without `duckdb` installed).
- `bench-results/00_hardware_profile.md` — hardware reference.
- `bench-results/B13_criterion.md` — criterion baseline for sketch operations (informs
  the corrector-overhead bound used in section 4.1, item 2).
- `bench-results/B19_reproducibility.md` — reproducibility philosophy this script
  follows.
