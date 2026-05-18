# METHODOLOGY.md — samkhya empirical campaign

**Date:** 2026-05-16 (UTC).
**Sole author:** Prateek Singh.
**License:** Apache-2.0.
**Pairs with:** [`bench-results/BINARY_ACCEPTANCE_REPORT.md`](./BINARY_ACCEPTANCE_REPORT.md), the per-file `B0x_*.md` artifacts, and the rebuttal kit in [`../DEFENSE.md`](../DEFENSE.md).

This document is the **public contract** for the samkhya empirical campaign. When a hostile reviewer asks *"how do you ensure these numbers are not cherry-picked?"*, the answer is to point at this file. Every rule recorded here is binding on every `B0x_*.md` artifact in this directory; a future bench result that violates a rule below either has the rule amended in this file before the result is published, or the result is withdrawn. The methodology is fixed *before* the measurement, not after.

**Canonical metrics contract (hard rule, non-negotiable):** every empirical measurement in this campaign uses the field's canonical metric and cites the canonical reference. The canonical metric table is the authoritative version; deviations require an explicit reviewer-facing justification.

| Domain | Metric | Canonical reference |
| ------ | ------ | ------------------- |
| Cardinality accuracy | **q-error** = `max(c_est/max(1,c_true), c_true/max(1,c_est))`; report P50/P95/P99/max + geomean | Moerkotte, Neumann, Steidl, VLDB 2009 |
| Query latency | Wallclock P50/P95/P99 + 95% **BCa bootstrap CI** (Efron & Tibshirani 1993), ≥30 replicates, cold + warm phases distinguished | Efron & Tibshirani 1993; ACM Artifact Evaluation v1.1 |
| Workload-aggregate speedup | **Geometric mean** of per-query speedup + **Wilcoxon signed-rank test** for paired significance + win/tie/loss distribution | Leis et al. VLDB 2015; TPC-H convention |
| Multi-hypothesis correction | **Benjamini-Hochberg FDR** at α=0.05 when N cells > 5 | Benjamini & Hochberg JRSSB 1995 |
| HLL precision | RSE vs theoretical `1.04 / sqrt(2^p)` | Flajolet et al. 2007; Heule et al. 2013 |
| Bloom FPR | Empirical / configured FPR; sizing `m = -n·ln(p)/(ln 2)^2` | Bloom CACM 1970 |
| Count-Min Sketch | Empirical bound-exceedance vs δ; max overestimate vs `ε·N` | Cormode & Muthukrishnan J. Algorithms 2005 |
| Histogram | Relative error vs MaxDiff / V-Optimal / equi-depth baselines | Ioannidis-Poosala VLDB 1995; Poosala et al. SIGMOD 1996 |
| AGM / LpBound | Tightness ratio vs ground truth + per-bound ordering | Atserias-Grohe-Marx FOCS 2008; Khamis et al. PODS 2017; Zhang et al. SIGMOD 2025 |
| Reproducibility | ACM Artifact Evaluation v1.1 (code + data + scripts; bit-deterministic seeds; hardware/software pinning) | ACM AE v1.1 guidelines |
| GPU benchmarks | Kernel-only + end-to-end (H2D + D2H); SM version + driver version + CUDA version pinned | NVIDIA developer guide; MLPerf inference rules |
| Statistical reporting | Median + 95% BCa CI ALWAYS (not mean ± SD); pre-registered hypothesis; ≥30 replicates; **first seed tried** (not best seed); first run (not best run) | ASA statement on p-values 2016; ICSE/SIGMOD repro guidelines |

Every Verdict line in every numbered file MUST name the metric used + cite its canonical reference. Every CI MUST be BCa bootstrap with ≥10,000 resamples; the resample count is documented. Every aggregate speedup MUST be geomean (not arithmetic mean) + Wilcoxon signed-rank paired test. Every multi-cell table with N > 5 cells MUST disclose Benjamini-Hochberg FDR at α=0.05. The first-seed-tried convention is binding: no seed search.

---

## 1. Purpose

The samkhya project is a portable, feedback-driven, self-correcting cardinality-correction library targeting the embedded analytical tier (DataFusion, DuckDB, Polars, Postgres, Iceberg). Performance and correctness claims in [`../README.md`](../README.md), [`../paper/draft.md`](../paper/draft.md), [`../BLOG-V1-LAUNCH.md`](../BLOG-V1-LAUNCH.md), and [`../DEFENSE.md`](../DEFENSE.md) are not self-evident; they require a rigorous empirical campaign. The files numbered `00_*.md` through `B20_*.md` in this directory are the campaign. This document specifies the rules they implement: multi-tier baselines, pre-registered hypotheses, confidence intervals, replicate counts, seed pinning, hardware/software pinning, named failure modes, and an explicit projected-vs-measured distinction. The reviewer should be able to read any `B0x_*.md`, see which methodology pillars it relied on, and audit the result against this file.

---

## 2. Empirical-rigor pillars

Eight pillars govern every result. Each is named, expanded, and tied to a verification surface so the campaign cannot drift.

### 2.1 Multi-tier baselines

**Rule.** Every performance or quality comparison in this campaign reports **at least two baselines**, not one. A single-baseline comparison is grounds for the file to be marked draft until a second baseline is added.

**Tiers used in this campaign.**

| Tier | Identity | Purpose |
|------|----------|---------|
| T0 | Native engine default (DataFusion 46 default planner, DuckDB native stats, Polars native, Postgres EXPLAIN baseline) | The system samkhya claims to improve on — the *outer* baseline |
| T1 | samkhya L1 only (sketches + LpBound ceiling, **no residual corrector**, `IdentityCorrector` passthrough) | Isolates the contribution of bound construction from the contribution of the optional corrector — the *inner* baseline |
| T2 | samkhya L1 + L2 (sketches + LpBound + active corrector at the default backend, typically `GbtCorrector`) | The full system as a default operator would deploy it |
| T3 (optional) | A *lower-bound* reference such as `XxHash::write` per item, or `cargo bench`'s noop, or `SELECT 1` round-trip | Establishes the floor below which no realistic system can go |

A comparison that reports only `T0 vs T2` is forbidden because the reader cannot tell whether the win came from the bound or from the corrector. A comparison that reports `T0 vs T1` plus `T1 vs T2` is the canonical pair: it decomposes the contribution and prevents the "everything got bundled into the headline number" failure mode.

The four-tier vocabulary above is the one the planning files commit to (see [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §3.5 *Multi-tier baselines*, T0/T1/T2 explicitly enumerated; [`../paper/draft.md`](../paper/draft.md) §5 ablation plan also commits to four arms: DF baseline, samkhya stats-only, samkhya-full, LpBound-only).

### 2.2 Pre-registered hypotheses

**Rule.** Every `B0x_*.md` file states its hypothesis in a clearly-marked **Pre-registered hypothesis** section that appears *before* the results section. Hypotheses are **intervals**, not point estimates: a hypothesis must be falsifiable.

**Format.** Each hypothesis carries an ID (`H1`, `H2`, …), the operation under test, the predicted interval at a stated confidence level (95% by default), and the mechanism by which the interval is derived (typically a roofline argument, an Amdahl bound, or a closed-form analytic prediction). Example reference: [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §2 *Pre-registered hypothesis*, table of `H1`–`H6`.

**Why intervals.** A point estimate is unfalsifiable: any measurement is "approximately correct." An interval with explicit bounds is falsifiable: a measurement outside `[L, U]` refutes the hypothesis. When a hypothesis is falsified, the campaign records the falsification rather than moving the goalposts. A "Failure modes" section (see §2.7 below) is the institutional home for these falsifications.

**No retroactive editing.** A hypothesis filed in a `B0x_*.md` file and committed to the git history may not be edited after the corresponding measurement is taken. If the measurement falsifies the hypothesis, the falsification is recorded in a follow-up `Revised: 2026-MM-DD` section in the same file; the original hypothesis remains visible in git history. This is the same discipline the OSF and AsPredicted pre-registration platforms enforce; the git log is our pre-registration registry.

### 2.3 Confidence intervals on every aggregate

**Rule.** Every reported aggregate (mean, median, geomean, percentile, q-error mean) carries a **95 % bootstrap CI computed from 10 000 resamples**. Single-run numbers are forbidden in aggregate cells.

**Bootstrap variant.** Bias-corrected and accelerated (BCa) bootstrap on the median (and on the geometric mean for q-error aggregates). The BCa correction is preferred over percentile bootstrap because samkhya's wall-clock distributions are right-skewed by OS scheduling tails; percentile bootstrap under-covers the upper tail.

**Resample count.** 10 000 resamples is the floor. For tail percentiles (P95, P99) where the relevant order statistic is sparse, the floor rises to 100 000 resamples and the file flags the cell. Below this floor, a cell is reported as "worst observed of *n*" rather than a distributional percentile, with the sample size stated alongside (see [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §3.3 for the `n=30` and `n=100` rules).

**Exceptions.** Binary-outcome cells (PASS / FAIL, e.g. fuzz-target crash counts, doctest pass counts) report a Clopper–Pearson 95 % exact CI on the proportion of failures, not a bootstrap. The CI must be reported even when zero failures are observed (the upper bound on a zero-failure run of *n* trials is approximately `3/n` for *n* trials by the rule of three).

### 2.4 Replicate counts

**Rule.** Wall-clock and throughput cells require **≥ 30 cold-cache replicates** per `(query × config)` cell. Memory-footprint and peak-RSS cells require **≥ 10 replicates**. Correctness cells (q-error, accuracy) inherit the replicate count from the corresponding wall-clock cell so the joint statistical envelope is computable.

**Cold-cache discipline.** Each replicate begins with caches cleared at the level the measurement targets:

- *CPU caches:* a workload-sized scratch buffer is written before each replicate to evict the previous replicate's lines. The scratch-buffer size is `2 × L3 size` (48 MiB on the campaign target machine; see §4 for hardware).
- *Page cache:* `sync; echo 3 > /proc/sys/vm/drop_caches` between replicates for any measurement that reads from disk. For benchmarks that prohibit root, an `O_DIRECT` open or a `posix_fadvise(POSIX_FADV_DONTNEED)` after read is used instead, and the substitution is noted in the file.
- *Engine caches:* DataFusion's `SessionContext` is rebuilt; DuckDB's `Database` is re-opened; Polars's `DataFrame` is re-materialised; Postgres `DISCARD ALL` is issued between replicates.

**Warm-cache cells.** Allowed as a *secondary* measurement when the operator-facing question is steady-state throughput rather than first-query latency. Warm-cache cells must be labelled `(warm)` and reported *in addition to* the cold-cache cell, never as a substitute.

**Why 30.** The bootstrap CI half-width at 95 % shrinks as `1/sqrt(n)`; the difference between *n* = 30 and *n* = 100 is roughly 1.8 × in CI width. We pay the 30-replicate floor because below it the median bootstrap is noticeably biased; we do not always pay the 100-replicate ceiling because the wall-clock budget on the campaign target machine is 30 minutes per file and 100 replicates × 6 thread counts × 4 operations exceeds that.

### 2.5 Random seeds documented

**Rule.** Every RNG-driven measurement in this campaign quotes its **first** seed list, not a post-hoc-best seed list.

**Seed inventory.**

- Wall-clock and quality measurements on synthetic data S1–S10 use the LCG `seed = 0x9E37_79B9_7F4A_7C15 × index` (golden-ratio constant). The seed and the LCG constant are recorded in [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §3.1 and propagated unchanged to every dependent file.
- `proptest` runs use `PROPTEST_CASES=100000` and the saved counterexample directory `samkhya-core/tests/property_lpbound.proptest-regressions` (committed to git as a public attestation: the seeds that have produced counterexamples are visible in the repository). The proptest fork-and-replay seed list is the one in `PROPTEST_SEED` if set, otherwise the OS RNG seed recorded at run start in the `B09_property_100k.md` artifact.
- `cargo-fuzz` corpora are seeded from the public `fuzz/corpus/` directories under `samkhya-core/fuzz/corpus/`. Seeds added by libFuzzer in a particular run are recorded by libFuzzer itself in the corpus directory; the campaign commits the corpus directory after the run so the seed set is reproducible.
- Bootstrap resampling uses `numpy.random.default_rng(seed=0)` (or the Rust equivalent `rand::SeedableRng::seed_from_u64(0)`) in every aggregation script. Seed 0 is the first seed tried; no seed search.

**No seed search.** It is forbidden to run the same measurement on multiple seeds and report the seed that produces the best number. The seed list above is the seed list; the result obtained on that seed list is the result reported.

### 2.6 Hardware and software pinning

**Rule.** Every `B0x_*.md` file links to [`00_hardware_profile.md`](./00_hardware_profile.md) for the hardware reference and records its own software pinning in a §Reproducibility block at the bottom.

**Hardware reference.** Single-machine, single-run: Intel i9-13900HK (14 cores / 20 threads, 6 P-cores + 8 E-cores hybrid, no AVX-512, 24 MiB L3), 31 GiB RAM, NVMe SSD, RTX 40-series Mobile GPU. The full `lscpu` / `/proc/meminfo` / `nvidia-smi` capture is in [`00_hardware_profile.md`](./00_hardware_profile.md). Caveats explicitly recorded there:

- This is a laptop CPU; sustained 20-thread workloads thermal-throttle within 30–60 s. The replicates must complete inside the un-throttled window or include thermal telemetry.
- Hybrid P/E topology: untuned multi-thread runs see asymmetric per-thread throughput; the campaign records this rather than normalising it away.
- The mobile GPU was offline at hardware-profile capture (NVML/kernel-module driver-version mismatch); all GPU cells are flagged provisional until the driver is reloaded.

**Software pinning.** Every file's §Reproducibility block records:

```
rustc:      <output of `rustc --version` — expect 1.94.1 stable, 2026-03-25 build>
cargo:      <output of `cargo --version`>
samkhya:    <commit hash, output of `git rev-parse HEAD` in the workspace>
DataFusion: <version pin in samkhya-datafusion/Cargo.toml>
DuckDB:     <version pin in samkhya-duckdb/Cargo.toml>
Polars:     <version pin in samkhya-polars/Cargo.toml>
Python:     <output of `python3 --version` for samkhya-py tests>
kernel:     <output of `uname -a` for OS pinning>
governor:   <output of `cpupower frequency-info | grep "current policy"`>
```

The block is mechanical; the campaign provides a `scripts/pin_env.sh` helper (planned, see §8) that emits exactly these lines.

### 2.7 Failure modes named

**Rule.** The campaign **commits** to surfacing the queries / configurations where samkhya *loses*. A "uniform wins" narrative is not credible, and is forbidden.

**Mechanism.** A dedicated file, `B17_python_versions.md` (currently used for multi-Python wheel verification) is to be re-purposed at the next major revision into a `failure_modes.md` artifact whose sole job is to list:

1. Queries on which samkhya's q-error is **worse** than the native baseline (the corrector over-corrected).
2. Configurations under which the L2 corrector is **rejected** by the LpBound clamp and the native estimate is used (this is the safety mechanism, not a regression, but it must be visible).
3. Workloads on which the sketch-build cost exceeds the optimisation benefit (samkhya is net-negative).
4. Engines on which the adapter has a coverage gap (e.g. window functions, recursive CTEs, specific predicate shapes the corrector has no features for).

Every released bench artifact must either contain its own *Limitations* section enumerating its known regressions (see [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §7 for the template) or link to a row in the failure-modes artifact. A file with zero declared limitations is in violation of this pillar.

The S1 case in [`../KILL_CRITERIA_REPORT.md`](../KILL_CRITERIA_REPORT.md) — q-error 1.96 baseline → 8.88 corrected — is the canonical example of an honestly-reported regression at the q-error level that is *not* a regression at the plan-quality level (the LpBound ceiling keeps the plan stable). This is the failure mode named.

### 2.8 Projected-vs-measured distinction

**Rule.** When the hardware (RTX 4090 desktop, or a multi-socket server) or the data (real IMDb dump, full TPC-H SF=100, JOB-Slow on real cardinalities) required for a measurement is not available at run time, the cell is marked **projected** in italics, with a footnote citing the analytic basis for the projection. Projected cells must never be reported in summary tables, executive summaries, or marketing copy as if they were measured.

**Visual convention.** Measured cells use plain numerals: `158.2 ms`. Projected cells use italics: `*82–95 ms*`. Predicted *ranges* (not points) are required for projections; a projected point is forbidden because the reader cannot tell the uncertainty. The convention is enforced in [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §4 results table, which the rest of the campaign mirrors.

**Promotion path.** A projected cell becomes a measured cell only after a real run on the indicated hardware / data. The file's git history shows the promotion: the projected entry is in the prior commit; the measured entry replaces it in the promotion commit. A `Revised: 2026-MM-DD` header at the top of the file records the promotion.

---

## 3. Baseline taxonomy

The four tiers in §2.1 deserve a concrete instantiation per engine. The following table is the campaign's commitment for which baselines run where.

| Engine | T0 (native) | T1 (samkhya L1) | T2 (samkhya L1+L2) | T3 (floor) |
|--------|-------------|-----------------|--------------------|------------|
| DataFusion 46 | Default `SessionContext`, no Samkhya rule | `SamkhyaOptimizerRule` with `IdentityCorrector` | `SamkhyaOptimizerRule` with `GbtCorrector` (and, when feature-gated, `AdditiveGbtCorrector` and `TabPfnHttpCorrector`) | `SELECT 1` round-trip; or `XxHash::write` per scanned row |
| DuckDB | Native planner, no extension loaded | Extension loaded, `samkhya_register` called with sketches only | Extension loaded, `samkhya_register` called with sketches + corrector | DuckDB `PRAGMA enable_object_cache=false` raw scan |
| Polars | Native lazy planner, no Samkhya plan rewrite | Samkhya plan rewrite with `IdentityCorrector` | Samkhya plan rewrite with `GbtCorrector` | Polars `collect()` over a `LazyFrame` with no joins |
| Postgres | Native EXPLAIN baseline, no `pg_samkhya` extension | `pg_samkhya` loaded, sketches only | `pg_samkhya` loaded, sketches + corrector | `EXPLAIN (ANALYZE, BUFFERS) SELECT 1` |
| Iceberg sidecar | Engine reads Parquet stats only | Engine reads Puffin sidecar with samkhya HLL/Bloom/CMS blobs, `IdentityCorrector` | Engine reads Puffin sidecar with samkhya blobs, `GbtCorrector` | Raw Parquet scan with no statistics |

A bench file that reports only T0 vs T2 (skipping T1) is a violation; the contribution decomposition is not auditable without T1. A bench file that reports only T1 vs T2 (skipping T0) is also a violation; the value-vs-native question is not answered without T0.

---

## 4. Pre-registration template

Each `B0x_*.md` artifact in the campaign follows the structure below. The structure is what makes the artifacts auditable as a corpus rather than a heap.

```
# B<NN> — <one-line title>

**Date:** YYYY-MM-DD (UTC)
**Hardware reference:** ./00_hardware_profile.md
**Crate(s) under test:** <crate names + versions>
**License:** Apache-2.0

## 1. Verdict
<two-paragraph summary: what was measured, did the hypothesis hold, headline number with CI>

## 2. Pre-registered hypothesis
<H1, H2, … as a table with operation / interval / mechanism columns>

## 3. Methodology
3.1 Operations
3.2 Parameter sweep (thread count, scale factor, query mix, etc.)
3.3 Replicates and statistics (replicate count, bootstrap variant, percentile reporting rule)
3.4 System controls (governor, ASLR, background load, thermal)
3.5 Multi-tier baselines (T0 / T1 / T2 / T3 explicit identities)

## 4. Results table
<measured cells: plain text. projected cells: italics. CI: 95% BCa bootstrap, n=10000>

## 5. Scaling / quality / coverage analysis
<the per-cell narrative — why the numbers landed where they did>

## 6. Discussion
<roofline / Amdahl / cost-model interpretation>

## 7. Limitations
<at least 3 honestly-stated limitations of this measurement>

## 8. Reproducibility
8.1 Pinned environment (per §2.6)
8.2 Exact invocation (the bash command, copy-pastable)
8.3 Hash of the run artifact (sha256 of the criterion output, or of the run log)

## 9. What this document is and is not
<one paragraph: measurement vs methodology, scope of generalisation>
```

A reviewer reading any `B0x_*.md` file should be able to find each numbered section in roughly the same place. Deviation from the structure is allowed only when explicitly justified in §9 of the file.

---

## 5. Statistical machinery

### 5.1 Bootstrap procedure

The campaign uses **bias-corrected and accelerated (BCa) bootstrap**, 10 000 resamples by default. The procedure for the median of a sample `x_1, …, x_n`:

1. Compute the observed statistic `theta_hat = median(x)`.
2. Generate 10 000 bootstrap resamples `x*_1, …, x*_10000` of size `n` with replacement.
3. Compute `theta*_b = median(x*_b)` for each.
4. Compute the bias-correction `z_0 = Phi^{-1}(P(theta* < theta_hat))`.
5. Compute the acceleration `a` from the jackknife on `theta_hat`.
6. Compute the BCa percentiles `alpha_1`, `alpha_2` from `z_0`, `a`, and the target 0.025 / 0.975 tails.
7. Report `[theta*_{alpha_1}, theta*_{alpha_2}]` as the 95 % BCa CI.

The Rust implementation lives in `scripts/bootstrap.py` (Python is the implementation language for the aggregation layer because matplotlib and `scipy.stats` are easier to audit than ad-hoc Rust); the campaign treats the Python script as the canonical implementation.

### 5.2 q-error definition

**Definition.** For a true cardinality `c_true` and an estimated cardinality `c_est`, q-error is

```
q(c_true, c_est) = max(c_true, c_est) / max(1, min(c_true, c_est))
```

with the `max(1, …)` denominator preventing division-by-zero when either value is zero. This is the standard definition used in the JOB-light and JOB-Slow literature ([Leis2015], [Naru2020]) and is the definition adopted by [`../paper/draft.md`](../paper/draft.md) §5.

**Reporting.** q-error aggregates use **geometric mean**, not arithmetic mean. The reason is multiplicative: a q-error of 10 and a q-error of 0.1 should average to 1.0 (not 5.05). The campaign reports geomean with a 95 % BCa CI on `log(q)` exponentiated back to the q-error scale.

**Per-query and aggregate.** Every q-error file reports both the per-query q-error (with CI) and the geomean across queries (with CI). The aggregate is never reported alone.

### 5.3 Geomean weighting

**Rule.** Geometric means across query suites are **unweighted** unless a per-query weight is published in the same file before any geomean is computed. If weights are introduced, the weight vector is part of the pre-registration and may not be revised after measurement.

This rule exists to prevent the "we weighted the easy queries by 0.1" trick. The default is unweighted; weighting requires justification.

### 5.4 Exclusion criteria

**Rule.** The campaign reports **all queries**, including regressions. There is no exclusion criterion that drops a measured query from the aggregate.

**The single exception** is a query that fails to execute (the engine errors out, the corrector panics, the harness crashes). In that case the query is reported as `FAIL` and is excluded from the geomean by construction; the `FAIL` count is reported alongside the geomean so the reader can compute the suite coverage.

**No "warm-up" exclusion.** Some bench harnesses discard the first *k* iterations as "warm-up." samkhya's harness does not: warm-up is part of the measurement and is reported separately as "cold" vs "warm" cells (§2.4). A campaign cell labelled "cold" includes the first iteration; a cell labelled "warm" excludes it explicitly.

---

## 6. Hardware / software pinning template

The §Reproducibility block of every `B0x_*.md` file pins the environment in the exact form below. The block is mechanically generated; deviation requires a paragraph of justification.

```
## 8. Reproducibility

### 8.1 Pinned environment

- Hardware: per ./00_hardware_profile.md (i9-13900HK, 14C/20T, 31 GiB RAM,
  NVMe SSD, RTX 40-series Mobile GPU; NVML mismatch at capture).
- Kernel: 6.17.0-23-generic (Ubuntu 24.04 LTS HWE on Linux Mint 22.3 Zena).
- Rust:   rustc 1.94.1 (e408947bf 2026-03-25), cargo 1.94.1.
- Python: 3.12.3 (for samkhya-py tests and aggregation scripts).
- samkhya commit: <git rev-parse HEAD at run time>
- DataFusion / DuckDB / Polars / Postgres / Iceberg versions: <Cargo.toml pins>
- CPU governor: performance (set via `cpupower frequency-set -g performance`)
- ASLR / THP / scaling driver: as captured at run start; recorded in run log.
- Background load: `uptime` and `vmstat 1 5` recorded at run start;
  cells aborted if 1-min load average > 1.0.
- Thermal envelope: package temperature recorded at run start and end;
  cells with sustained > 90 °C flagged.

### 8.2 Exact invocation
<copy-pastable bash, with all environment variables explicit>

### 8.3 Hash of run artifact
<sha256 of the criterion output tarball, or of the run log>
```

---

## 7. DEFENSE.md ↔ measurement pairing

Every objection in [`../DEFENSE.md`](../DEFENSE.md) is paired below with the `B0x_*.md` artifact (or `00_*`/`01_*`) that materially addresses it. A reviewer asking *"where is the evidence for the counter to Objection N?"* can look up Objection N in this table and find the file path. Conversely, every objection without a paired file is a campaign gap — and a campaign gap is a publishable result in its own right (see §9 anti-cherry-picking).

| Objection | Title | Primary evidence file | Secondary evidence | Status |
|-----------|-------|----------------------|--------------------|--------|
| O1 | "Just upstream into DataFusion / DuckDB" | [`B05_samkhya_datafusion_install.md`](./B05_samkhya_datafusion_install.md) (planned), [`B04_samkhya_duckdb_install.md`](./B04_samkhya_duckdb_install.md) | [`B01_samkhya_cli_install.md`](./B01_samkhya_cli_install.md) (the engine-agnostic CLI itself) | Architecture is the rebuttal; bench files demonstrate the same Puffin sidecar round-trips across engines |
| O2 | "Iceberg Puffin sidecars are over-engineered" | [`B13_criterion.md`](./B13_criterion.md) (sketch-encoding microbench), [`B14_examples.md`](./B14_examples.md) (`sketch_to_puffin`, `inspect_puffin`) | [`B09_property_100k.md`](./B09_property_100k.md) (Puffin round-trip property) | Round-trip cost is measured; Puffin size budget (5–10 % of Parquet) is to be added in a Puffin-overhead file |
| O3 | "LpBound is just AGM + a clamp" | [`B09_property_100k.md`](./B09_property_100k.md) (LpBound proptest, 100 000 cases, lp_solver tier) | [`B13_criterion.md`](./B13_criterion.md) (LpJoinBound vs AgmBound timing) | Four-bound stack is exercised by property tests; the LP-vs-AGM tightness ratio is a planned addition |
| O4 | "TabPFN / learned correction is Naru again" | [`B17_python_versions.md`](./B17_python_versions.md) (multi-Python wheel = optional surface stays optional), [`B14_examples.md`](./B14_examples.md) (default build links no model) | [`../KILL_CRITERIA_REPORT.md`](../KILL_CRITERIA_REPORT.md) §"Solver-failure fallback path" | Default-build-has-no-ML is verified; the corrector backends are feature-gated and tested separately |
| O5 | "DuckDB / Polars planners are good" | [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) (samkhya-core sketch throughput), planned `B0?_job_slow.md` for the JOB-Slow head-to-head | [`../KILL_CRITERIA_REPORT.md`](../KILL_CRITERIA_REPORT.md) §"Supporting evidence" Synthetic S1–S5 | The 0 → 6924 row demonstration is paper-grade; a public bench file remains to be promoted from synthetic to real IMDb |
| O6 | "Pre-1.0 software making safety claims" | [`B07_supply_chain.md`](./B07_supply_chain.md), [`B08_fuzz_inventory.md`](./B08_fuzz_inventory.md), [`B11_sanitizer.md`](./B11_sanitizer.md), [`B12_valgrind.md`](./B12_valgrind.md), [`B15_clippy_fmt.md`](./B15_clippy_fmt.md) | [`B09_property_100k.md`](./B09_property_100k.md), [`B16_doctests.md`](./B16_doctests.md), [`../SECURITY.md`](../SECURITY.md) | Supply-chain / fuzz / sanitizer / valgrind / lint surfaces all paired with files |
| O7 | "q-error 6.19 is not SOTA, Naru gets sub-2" | Planned `B0?_qerror_envelope.md` (samkhya q-error on synthetic + JOB-light, with hardware-budget contract) | [`../paper/draft.md`](../paper/draft.md) §5 four-arm ablation plan | Embedded-tier contract enforced; the head-to-head Naru run is excluded by hardware budget and the exclusion is documented |
| O8 | "Pessimistic envelopes lead to over-conservative plans" | [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) (sketch-build cost, the input to the clamp), planned `B0?_plan_stability.md` (plan-identical-under-clamp measurement) | [`../KILL_CRITERIA_REPORT.md`](../KILL_CRITERIA_REPORT.md) criterion 2 (S1 q-error 1.96 → 8.88 with no plan regression) | The "corrected estimate is rejected if it breaches the ceiling" path is to be measured with a dedicated bench file |
| O9 | "Benchmarks are synthetic — where's JOB-Slow on real IMDb?" | Planned `B0?_job_slow_real_imdb.md`; `scripts/fetch_imdb.sh` already exists in `bench-results/scripts/` | [`01_cpu_baseline_multithread.md`](./01_cpu_baseline_multithread.md) §1 reproducibility status note | Synthetic-only is the current state and is **declared**; the IMDb fetch script is the promotion path |
| O10 | "Sanskrit naming looks like marketing" | n/a — naming is a documentation matter, not a measurement | [`../samkhya.md`](../samkhya.md) §3 naming rules | No measurement applies; the framing-rule file is the rebuttal |
| O11 | "Spark AQE already solves this" | Planned `B0?_aqe_comparison.md` (head-to-head shape comparison: feedback granularity, persistence, portability) | [`../paper/draft.md`](../paper/draft.md) §8 related work | The "AQE feedback dies with the SparkContext" point is structural and to be documented with the persistence cell |
| O12 | "Apache 2.0 + patent grant — what's the IP story?" | [`B07_supply_chain.md`](./B07_supply_chain.md) (`cargo-deny` license rule), [`B20_cargo_metadata.md`](./B20_cargo_metadata.md) (license-file presence per crate) | [`../SECURITY.md`](../SECURITY.md) §"Reporting a vulnerability" | License hygiene measured at the metadata level; patent-grant assertion is non-empirical |

**Open campaign gaps.** Five files in the planned column above have not yet been written (`B0?_job_slow.md`, `B0?_qerror_envelope.md`, `B0?_plan_stability.md`, `B0?_job_slow_real_imdb.md`, `B0?_aqe_comparison.md`). These gaps are explicit; closing them is the next campaign phase.

---

## 8. Anti-cherry-picking commitments

The campaign commits in writing to the following anti-cherry-picking rules. A violation is grounds for retraction of the affected file.

1. **No rerun-until-green.** A measurement run is executed once per `(file, hardware, software-pin)` triple. If the result falsifies the hypothesis, the falsification is recorded in §7 *Limitations* (or in a follow-up `Revised:` section); the measurement is **not** rerun on a different machine, a different governor, a different RNG seed, or a different time of day in order to land inside the hypothesis interval.
2. **First-seed-tried, not best-seed-found.** The seed list in §2.5 is the seed list. No seed search is permitted. If a result is bad on the documented seed, the result is bad on the documented seed.
3. **All queries reported.** No exclusion of regressions from aggregates beyond the single hard-failure exception in §5.4.
4. **Pre-registered intervals are immutable post-measurement.** The hypothesis recorded in git history at the time the measurement was taken is the hypothesis being tested. Editing the hypothesis to land inside the measured range is forbidden and is detectable in `git log`.
5. **Projected cells are visibly projected.** The italic convention in §2.8 is mechanical; a reviewer scanning a table sees projected cells without reading the prose.
6. **Failure modes have a home.** The campaign maintains a place for `samkhya loses here` results (§2.7); a file with zero declared regressions across hundreds of queries is presumed to be hiding them.
7. **Hardware caveats are loud.** [`00_hardware_profile.md`](./00_hardware_profile.md) §"Caveats for interpreting numbers" is mandatory reading for every file in the campaign and is linked from every §8.1 *Pinned environment* block.
8. **The git log is the audit trail.** The state of the campaign at any past commit is what the campaign claimed at that commit. Force-pushes to bench-results history are forbidden.

---

## 9. Reproducibility as code

The campaign's reproducibility surface is in `bench-results/scripts/`. Current contents:

- `fetch_imdb.sh` — fetches the public IMDb data dump used by JOB / JOB-Slow head-to-heads. Promotion path from synthetic S1–S10 to real-IMDb cells.
- `run_02_gpu.sh`, `run_02_gpu.py` — the GPU benchmark harness (will populate the `02_gpu_*` files once the NVML/driver mismatch in [`00_hardware_profile.md`](./00_hardware_profile.md) §GPU is resolved by reboot).

**Planned additions (campaign roadmap).** The following helper scripts are tracked as follow-ups to this methodology file; they implement the rules above as code rather than prose.

| Script | Purpose | Pillar enforced |
|--------|---------|-----------------|
| `pin_env.sh` | Emit the §6 template by reading `rustc`, `cargo`, `git rev-parse HEAD`, `cpupower`, etc. | §2.6 hardware/software pinning |
| `bootstrap.py` | BCa bootstrap on median / geomean with 10 000 resamples | §2.3 confidence intervals, §5.1 bootstrap procedure |
| `collect_parallel.py` | Walk a `target/criterion/<bench>/` tree and emit a results JSON for the `B0x_*.md` results table | §2.4 replicate counts, §2.3 CIs |
| `qerror.py` | Compute per-query q-error and geomean q-error with CI from a `(true, estimate)` pair list | §5.2 q-error definition |
| `check_hypothesis.py` | Given a `B0x_*.md` filename and a results JSON, verify each hypothesis interval was either confirmed or honestly recorded as falsified | §2.2 pre-registered hypotheses, §8.4 immutability |

The campaign treats these scripts as load-bearing: a result that cannot be reproduced by running the corresponding script against the data in the artifact tarball is not a result. The scripts are versioned alongside the markdown artifacts so a checkout of an old commit reproduces the old methodology, not the current one.

---

## 10. Limitations of the methodology itself

This methodology has known limitations. Recording them here is part of the contract.

1. **Single-machine, single-run.** Every measurement is on one Intel i9-13900HK 14C/20T laptop (see [`00_hardware_profile.md`](./00_hardware_profile.md)). The campaign does not currently replicate on a second machine class (e.g. AMD EPYC, ARM Graviton, AWS m7i). Hardware-specific effects (Intel hybrid topology, AVX-VNNI presence, mobile thermal envelope) are baked into every number. The campaign records this; the campaign does not pretend to have eliminated it.
2. **Laptop thermal envelope.** Sustained 20-thread loads thermal-throttle the i9-13900HK within 30–60 s. The replicate budget is constrained to the un-throttled window or the file must include thermal telemetry. This is a soft cap on the *n* in §2.4.
3. **Mobile GPU is provisional.** The RTX 40-series Mobile in this machine had an NVML/driver-version mismatch at hardware-profile capture. Every GPU cell is projected until the reboot lands and the driver matches. Desktop RTX 4090 numbers are not extrapolable from this machine.
4. **Synthetic data dominates the current campaign.** Real IMDb, JOB-Slow, and full TPC-H at SF=100 are not yet measured (Objection 9 in §7). The `fetch_imdb.sh` script is the promotion path; until those measurements land, the campaign is **synthetic-first** and **declares it**.
5. **No formal verification of the corrector.** The LpBound clamp is property-tested at 100 000 cases per property (B09), which is high but not exhaustive. A formal proof of `Corrector(x) ≤ Bound(x)` over the corrector's input space is out of scope; the property-testing surface is the substitute.
6. **Bootstrap CI assumes IID samples.** Replicate-to-replicate dependence (e.g. thermal drift across a 30-replicate run) inflates the true CI. The campaign mitigates by randomising the order of `(operation, t)` cells across a sweep and by rejecting cells with measured 1-min load average > 1.0 (§2.4 system controls), but residual dependence is not formally accounted for.
7. **Pre-registration discipline is honour-bound.** Git history is the registry, not an external service. A motivated adversary could rewrite history (force-push to a bench-results branch and reset the upstream). The campaign's commitment in §8.8 — that bench-results history is append-only — is enforced by repository policy, not by cryptographic signature. This is honest.
8. **Engine-version drift.** DataFusion 46 / DuckDB 1.x / Polars current are the pinned versions today; the next major release of any of them may invalidate cells without samkhya itself changing. The §6 software-pinning block makes the drift detectable; the campaign re-runs as needed at the next minor-version cadence ([`../SECURITY.md`](../SECURITY.md) §"Operator obligations (pre-1.0)").

---

## 11. Glossary

- **BCa bootstrap.** Bias-corrected and accelerated bootstrap, the resampling variant the campaign uses for percentile CIs on skewed distributions. Default: 10 000 resamples, 95 % CI.
- **Clamp.** The `lpbound::clamp_estimate` operation that bounds the corrector's output beneath the LpBound ceiling. If the corrector's proposal breaches the ceiling, the correction is **rejected** and the engine sees its **native** estimate — not the ceiling.
- **Cold cache.** A measurement begun after every relevant cache layer has been evicted (CPU L1/L2/L3, OS page cache, engine session state). Default replicate condition unless explicitly marked `(warm)`.
- **Corrector (residual).** The optional L2 layer above LpBound. Backends include `IdentityCorrector` (passthrough), `GbtCorrector` (gradient-boosted trees, sub-MB), `AdditiveGbtCorrector` (unblocks baseline=0 cases), and `TabPfnHttpCorrector` (TabPFN over HTTP, feature-gated). Default build links none.
- **Cross-engine round-trip.** The Iceberg Puffin sidecar pattern in which sketches written by one engine (e.g. samkhya-py on a Python ELT) are consumed by another engine (e.g. samkhya-duckdb at query time). The portability moat.
- **Embedded tier.** The deployment class samkhya targets: in-process analytical engines (DataFusion, DuckDB, Polars, Postgres, Iceberg readers) with sub-50 ms cold-start, sub-200 MB total memory, and sub-ms per-estimate latency budgets. Naru / NeuroCard / TabPFN do not fit this envelope unmodified.
- **Geometric mean (geomean).** The default aggregation for q-error. Reported with a 95 % BCa CI on `log(q)` exponentiated back.
- **Hypothesis interval.** A pre-registered `[L, U]` predicted range for a measurement, filed *before* the measurement is taken. A run outside `[L, U]` falsifies the hypothesis (§2.2).
- **LpBound.** The pessimistic-envelope layer. Four bounds compose: `ProductBound` ⟶ `AgmBound` ⟶ `ChainBound` ⟶ `LpJoinBound`, each progressively tighter, each with a fallback if the next-tighter one fails numerically.
- **Multi-tier baseline.** T0 (native engine), T1 (samkhya L1 — sketches + bound, no corrector), T2 (samkhya L1 + L2 — sketches + bound + corrector), optional T3 (floor). Every comparison reports at least T0 and T1; a T0-vs-T2-only comparison is forbidden (§2.1).
- **Pre-registration.** The act of filing a hypothesis in git history *before* the corresponding measurement is taken. The git log is the registry. Editing a pre-registered hypothesis after measurement is forbidden (§2.2, §8.4).
- **Projected cell.** A results-table cell shown in italics because the hardware or data required for a real measurement is not available. The reader can distinguish projected from measured at a glance (§2.8).
- **Puffin sidecar.** The Iceberg-standardised auxiliary file format samkhya extends with four `KIND` tags (`samkhya.hll-v1`, `samkhya.bloom-v1`, `samkhya.cms-v1`, `samkhya.equi-depth-v1`, plus `samkhya.correlated2d-v1`). The cross-engine wire format.
- **Q-error.** `max(c_true, c_est) / max(1, min(c_true, c_est))`. The standard cardinality-estimation quality metric ([Leis2015], [Naru2020]).
- **Replicate.** A single complete run of one `(operation, configuration)` cell, with all relevant caches cold (§2.4). Replicate counts: ≥ 30 for wall-clock, ≥ 10 for memory, ≥ 100 for tail percentiles below the bootstrap floor.
- **Roofline.** The arithmetic-intensity × machine-balance plot used to predict whether an operation is compute-bound, L1/L2/L3-bound, or DRAM-bound on the campaign target hardware. The mechanism that drives most of the campaign's hypothesis intervals.
- **Seed (RNG).** A documented integer or constant fed to a pseudorandom generator. The campaign quotes the first seed tried, never a post-hoc-best seed (§2.5).

---

*End of METHODOLOGY.md.*
