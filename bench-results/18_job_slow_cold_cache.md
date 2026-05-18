# 18 — JOB-Slow cold-cache head-to-head — **INSUFFICIENT DATA (Wave-5M)**

**Date:** 2026-05-17
**Sole author:** Prateek Singh
**Hardware:** Linux 6.17.0-29-generic x86_64, i9-13900HK, 20 threads, 31 GiB RAM,
RTX 4090 Laptop (unused for this campaign); see
[`00_hardware_profile.md`](./00_hardware_profile.md).
**Corpus:** IMDb CSV dump, SHA-256
`25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988`
(`samkhya-bench/data/job/`, 21 CSVs, fetched 2026-05-16; mirror
`https://event.cwi.nl/da/job/imdb.tgz`).
**Suite:** `Suite::JobSlowReal` — full 113-query Leis et al. roster.
**Engine:** DataFusion 46 (in-process, `--all-features`, release profile).
**Cold-cache discipline:** `posix_fadvise(POSIX_FADV_DONTNEED)` per file, plus
`sync && drop_caches` between trials (Wave-5M harness).

---

## 1 — Headline

> **The cold-cache corrected arm did not run.** All three JSON files in
> `bench-results/wave5m_raw/` self-report `mode == "baseline"`. There is no
> samkhya-corrected cold-cache run to compare against; the "corrected" column
> below is sourced from `cold_t1.json`, which the runner labelled `baseline`
> at emit time. This file is therefore a **baseline-cold-cache characterization
> only**; the corrector-arm head-to-head twin of file 12 is **deferred** to a
> future Wave (when the corrected-cold arm runs without OOM).

| Metric | Value | Note |
|---|---:|---|
| Queries timed (baseline 3-trial, `cold_baseline.json`) | 113 / 113 | all `status == ok` |
| Queries in second-arm replay (`cold_t1.json`, also baseline-mode) | 113 / 113 | 1 trial each |
| Intersection used for "speedup" computation | 113 / 113 | not a real head-to-head — see §8 |
| Geomean wallclock — first arm (baseline, n=3 trials/query) | 450.0 ms | BCa CI [370.5, 577.8] |
| Geomean wallclock — second arm (also baseline, n=1 trial/query) | 449.8 ms | BCa CI [368.4, 578.7] |
| "Speedup" (arm-1 / arm-2) geomean | 1.0005× | BCa CI [0.981, 1.021] — straddles 1.0× |
| Wilcoxon W (paired arm-1 vs arm-2) | 3066.0 | n_nonzero=113 |
| Wilcoxon p (two-sided) | 0.659 | fail-to-reject H0 — no difference |
| BH-FDR per-query rejects (α=0.05) | 63 / 113 | reflects within-baseline run-to-run noise, **not** corrector wins |

**Industry-standard citations (mandatory):**

- BCa bootstrap: Efron & Tibshirani, *An Introduction to the Bootstrap*, 1993,
  Chapter 14 — 10 000 resamples, seed `0xDEADBEEFCAFEBABE`.
- Paired significance: Wilcoxon, *Individual Comparisons by Ranking Methods*,
  Biometrics Bulletin 1(6):80–83 (1945) — normal-approximation tail w/
  continuity correction; Pratt's tie handling.
- FDR control: Benjamini & Hochberg, *Controlling the False Discovery Rate*,
  JRSSB 57(1):289–300 (1995) — α=0.05.
- Workload: Leis et al., *How Good Are Query Optimizers, Really?*, VLDB 2015 —
  113-query Join-Order-Benchmark over IMDb.
- q-error definition (not exercised on this run): Moerkotte et al., VLDB 2009.
- Memory hierarchy / cold-cache rationale: Hennessy & Patterson, *Computer
  Architecture: A Quantitative Approach*, 6th ed., Ch. 2.

---

## 2 — Pre-registration vs measured

The pre-registered hypothesis carried over from file 12 was already falsified
honestly there (warm-cache geomean 1.038× vs the ≥ 1.35× pre-reg target). The
cold-cache target for this file was: *"same geomean speedup as warm within
overlapping CI"*. That comparison **cannot be made**, because the
samkhya-corrected cold-cache arm did not land. We refuse to substitute baseline
self-comparison for it.

| Pre-reg | Status |
|---|---|
| Cold-cache geomean speedup ≈ warm-cache geomean speedup (CI overlap) | **NOT TESTABLE** — corrected cold arm absent |
| Cold-cache regression count ≤ warm-cache regression count | **NOT TESTABLE** — same reason |

---

## 3 — Setup

- **Hardware:** as listed above. Governor: `powersave` (host default).
- **OS cache discipline:** Wave-5M harness pages `posix_fadvise(fd,
  POSIX_FADV_DONTNEED)` over each `*.csv` Parquet sidecar at the start of every
  trial, then a `sync; echo 3 > /proc/sys/vm/drop_caches` (rooted) before the
  trial timer starts. The first-arm `cold_baseline.json` carries 3 trials per
  query under this discipline; the second-arm `cold_t1.json` carries 1.
- **IMDb tables:** 21 CSVs registered through `register_imdb_tables_async`
  (Wave-4F integration). Puffin sidecars (HLL p=12 + 1% Bloom) generated next to
  each CSV; sidecar generation receipt: `bench-results/wave4f_raw/build_puffin.log`.
- **Sidecar consumption:** The corrected-arm code path (`SamkhyaTableProvider`)
  is the one that ingests the sidecars at planning time. It did not execute on
  this run, so the sidecars were not on the hot path.

---

## 4 — Per-query results

The table below is **what landed in the two JSON files**, presented honestly:
the "baseline" columns are the 3-trial cold baseline, and the "corrected"
columns are the single-trial replay that the runner emitted under mode-label
`baseline` (i.e., this is baseline-vs-baseline, not corrector-vs-baseline).
The columns are kept for paper-trail completeness, **not as evidence of any
corrector effect**.

| Query | base med (ms) | base p95 (ms) | base p99 (ms) | corr med (ms) | corr p95 (ms) | corr p99 (ms) | speedup | p (two-sided) | BH-FDR |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| 1a | 117.0 | 129.8 | 131.0 | 118.6 | 118.6 | 118.6 | 0.987x | 0.527 | fail-to-reject |
| 1b | 103.2 | 140.7 | 144.1 | 92.1 | 92.1 | 92.1 | 1.120x | 0.000 | reject |
| 1c | 68.8 | 89.9 | 91.8 | 71.2 | 71.2 | 71.2 | 0.966x | 0.520 | fail-to-reject |
| 1d | 103.4 | 105.1 | 105.2 | 86.1 | 86.1 | 86.1 | 1.201x | 0.529 | fail-to-reject |
| 2a | 117.6 | 125.9 | 126.6 | 95.6 | 95.6 | 95.6 | 1.230x | 0.000 | reject |
| 2b | 109.9 | 115.3 | 115.8 | 74.3 | 74.3 | 74.3 | 1.479x | 0.000 | reject |
| 2c | 101.0 | 109.7 | 110.5 | 75.9 | 75.9 | 75.9 | 1.332x | 0.000 | reject |
| 2d | 129.4 | 140.8 | 141.8 | 116.3 | 116.3 | 116.3 | 1.112x | 0.482 | fail-to-reject |
| 3a | 318.2 | 319.0 | 319.1 | 328.7 | 328.7 | 328.7 | 0.968x | 0.000 | reject |
| 3b | 166.9 | 192.8 | 195.0 | 176.5 | 176.5 | 176.5 | 0.946x | 0.532 | fail-to-reject |
| 3c | 309.0 | 328.7 | 330.4 | 302.0 | 302.0 | 302.0 | 1.023x | 0.545 | fail-to-reject |
| 4a | 107.5 | 108.5 | 108.6 | 86.4 | 86.4 | 86.4 | 1.244x | 0.000 | reject |
| 4b | 90.2 | 104.8 | 106.1 | 77.1 | 77.1 | 77.1 | 1.170x | 0.000 | reject |
| 4c | 146.8 | 178.3 | 181.1 | 122.4 | 122.4 | 122.4 | 1.200x | 0.509 | fail-to-reject |
| 5a | 271.9 | 302.3 | 305.0 | 271.1 | 271.1 | 271.1 | 1.003x | 0.530 | fail-to-reject |
| 5b | 163.5 | 167.9 | 168.2 | 154.5 | 154.5 | 154.5 | 1.058x | 0.000 | reject |
| 5c | 316.5 | 321.5 | 322.0 | 303.7 | 303.7 | 303.7 | 1.042x | 0.000 | reject |
| 6a | 1043.1 | 1085.5 | 1089.2 | 1025.8 | 1025.8 | 1025.8 | 1.017x | 0.524 | fail-to-reject |
| 6b | 961.0 | 986.6 | 988.9 | 965.9 | 965.9 | 965.9 | 0.995x | 0.521 | fail-to-reject |
| 6c | 979.2 | 1026.4 | 1030.6 | 962.2 | 962.2 | 962.2 | 1.018x | 0.000 | reject |
| 6d | 1019.7 | 1053.0 | 1056.0 | 1004.6 | 1004.6 | 1004.6 | 1.015x | 0.000 | reject |
| 6e | 968.2 | 1075.3 | 1084.9 | 966.0 | 966.0 | 966.0 | 1.002x | 0.490 | fail-to-reject |
| 6f | 1059.5 | 1112.5 | 1117.2 | 1090.7 | 1090.7 | 1090.7 | 0.971x | 0.526 | fail-to-reject |
| 7a | 341.9 | 392.6 | 397.1 | 300.4 | 300.4 | 300.4 | 1.138x | 0.532 | fail-to-reject |
| 7b | 259.3 | 264.7 | 265.2 | 260.2 | 260.2 | 260.2 | 0.996x | 0.518 | fail-to-reject |
| 7c | 404.5 | 406.4 | 406.5 | 420.9 | 420.9 | 420.9 | 0.961x | 0.000 | reject |
| 8a | 201.6 | 220.1 | 221.8 | 235.3 | 235.3 | 235.3 | 0.857x | 0.000 | reject |
| 8b | 218.8 | 230.3 | 231.3 | 235.3 | 235.3 | 235.3 | 0.930x | 0.000 | reject |
| 8c | 2994.6 | 3062.4 | 3068.4 | 3017.2 | 3017.2 | 3017.2 | 0.993x | 0.517 | fail-to-reject |
| 8d | 3006.0 | 3106.8 | 3115.8 | 2952.4 | 2952.4 | 2952.4 | 1.018x | 0.000 | reject |
| 9a | 421.2 | 432.6 | 433.6 | 448.5 | 448.5 | 448.5 | 0.939x | 0.000 | reject |
| 9b | 314.8 | 339.1 | 341.3 | 323.3 | 323.3 | 323.3 | 0.974x | 0.482 | fail-to-reject |
| 9c | 422.5 | 438.9 | 440.3 | 457.9 | 457.9 | 457.9 | 0.922x | 0.000 | reject |
| 9d | 448.7 | 474.6 | 476.9 | 439.8 | 439.8 | 439.8 | 1.020x | 0.535 | fail-to-reject |
| 10a | 238.5 | 247.7 | 248.5 | 240.4 | 240.4 | 240.4 | 0.992x | 0.518 | fail-to-reject |
| 10b | 196.0 | 222.3 | 224.7 | 203.2 | 203.2 | 203.2 | 0.964x | 0.519 | fail-to-reject |
| 10c | 225.0 | 239.0 | 240.2 | 233.6 | 233.6 | 233.6 | 0.963x | 0.531 | fail-to-reject |
| 11a | 167.7 | 194.0 | 196.3 | 133.2 | 133.2 | 133.2 | 1.259x | 0.000 | reject |
| 11b | 143.3 | 158.3 | 159.6 | 148.4 | 148.4 | 148.4 | 0.965x | 0.494 | fail-to-reject |
| 11c | 138.9 | 152.5 | 153.7 | 176.7 | 176.7 | 176.7 | 0.787x | 0.000 | reject |
| 11d | 451.4 | 462.7 | 463.7 | 438.5 | 438.5 | 438.5 | 1.029x | 0.000 | reject |
| 12a | 225.0 | 227.3 | 227.6 | 221.4 | 221.4 | 221.4 | 1.016x | 0.520 | fail-to-reject |
| 12b | 413.7 | 420.8 | 421.4 | 438.5 | 438.5 | 438.5 | 0.943x | 0.000 | reject |
| 12c | 254.3 | 305.4 | 309.9 | 256.8 | 256.8 | 256.8 | 0.990x | 0.508 | fail-to-reject |
| 13a | 339.1 | 340.9 | 341.1 | 308.8 | 308.8 | 308.8 | 1.098x | 0.000 | reject |
| 13b | 345.7 | 345.8 | 345.9 | 378.0 | 378.0 | 378.0 | 0.914x | 0.000 | reject |
| 13c | 294.8 | 317.7 | 319.8 | 308.3 | 308.3 | 308.3 | 0.956x | 0.518 | fail-to-reject |
| 13d | 376.0 | 394.2 | 395.8 | 368.8 | 368.8 | 368.8 | 1.020x | 0.571 | fail-to-reject |
| 14a | 377.9 | 387.4 | 388.3 | 357.0 | 357.0 | 357.0 | 1.059x | 0.525 | fail-to-reject |
| 14b | 377.3 | 388.8 | 389.8 | 396.3 | 396.3 | 396.3 | 0.952x | 0.000 | reject |
| 14c | 376.7 | 400.3 | 402.4 | 398.7 | 398.7 | 398.7 | 0.945x | 0.501 | fail-to-reject |
| 15a | 311.3 | 316.1 | 316.6 | 296.7 | 296.7 | 296.7 | 1.049x | 0.489 | fail-to-reject |
| 15b | 269.6 | 273.4 | 273.7 | 304.4 | 304.4 | 304.4 | 0.886x | 0.000 | reject |
| 15c | 343.7 | 344.7 | 344.7 | 333.0 | 333.0 | 333.0 | 1.032x | 0.512 | fail-to-reject |
| 15d | 225.6 | 229.6 | 230.0 | 202.7 | 202.7 | 202.7 | 1.113x | 0.000 | reject |
| 16a | 13946.9 | 14099.9 | 14113.5 | 13677.5 | 13677.5 | 13677.5 | 1.020x | 0.000 | reject |
| 16b | 14065.9 | 14101.1 | 14104.2 | 13886.9 | 13886.9 | 13886.9 | 1.013x | 0.514 | fail-to-reject |
| 16c | 13891.5 | 14049.1 | 14063.1 | 13715.5 | 13715.5 | 13715.5 | 1.013x | 0.000 | reject |
| 16d | 14102.0 | 14151.5 | 14155.9 | 13808.5 | 13808.5 | 13808.5 | 1.021x | 0.000 | reject |
| 17a | 4782.9 | 5116.3 | 5145.9 | 4783.2 | 4783.2 | 4783.2 | 1.000x | 0.494 | fail-to-reject |
| 17b | 14541.7 | 16560.4 | 16739.8 | 14555.8 | 14555.8 | 14555.8 | 0.999x | 0.521 | fail-to-reject |
| 17c | 14723.6 | 16989.4 | 17190.9 | 15043.0 | 15043.0 | 15043.0 | 0.979x | 0.526 | fail-to-reject |
| 17d | 15466.2 | 15831.9 | 15864.4 | 15161.5 | 15161.5 | 15161.5 | 1.020x | 0.493 | fail-to-reject |
| 17e | 4982.0 | 5109.5 | 5120.9 | 5701.6 | 5701.6 | 5701.6 | 0.874x | 0.000 | reject |
| 17f | 15471.5 | 15496.8 | 15499.0 | 15243.4 | 15243.4 | 15243.4 | 1.015x | 0.552 | fail-to-reject |
| 18a | 620.5 | 634.7 | 636.0 | 601.0 | 601.0 | 601.0 | 1.033x | 0.000 | reject |
| 18b | 336.1 | 366.7 | 369.4 | 357.7 | 357.7 | 357.7 | 0.940x | 0.533 | fail-to-reject |
| 18c | 434.6 | 476.4 | 480.2 | 392.0 | 392.0 | 392.0 | 1.109x | 0.000 | reject |
| 19a | 449.7 | 460.7 | 461.7 | 422.0 | 422.0 | 422.0 | 1.066x | 0.000 | reject |
| 19b | 420.6 | 443.1 | 445.1 | 400.4 | 400.4 | 400.4 | 1.050x | 0.000 | reject |
| 19c | 476.6 | 479.2 | 479.4 | 446.0 | 446.0 | 446.0 | 1.069x | 0.000 | reject |
| 19d | 762.8 | 765.3 | 765.5 | 778.4 | 778.4 | 778.4 | 0.980x | 0.000 | reject |
| 20a | 402.2 | 404.9 | 405.1 | 376.7 | 376.7 | 376.7 | 1.068x | 0.000 | reject |
| 20b | 396.5 | 401.8 | 402.3 | 477.8 | 477.8 | 477.8 | 0.830x | 0.000 | reject |
| 20c | 357.7 | 430.9 | 437.4 | 439.5 | 439.5 | 439.5 | 0.814x | 0.000 | reject |
| 21a | 326.9 | 372.5 | 376.6 | 354.9 | 354.9 | 354.9 | 0.921x | 0.512 | fail-to-reject |
| 21b | 229.4 | 239.0 | 239.8 | 306.9 | 306.9 | 306.9 | 0.748x | 0.000 | reject |
| 21c | 373.5 | 380.7 | 381.3 | 426.7 | 426.7 | 426.7 | 0.875x | 0.000 | reject |
| 22a | 366.9 | 370.9 | 371.3 | 375.3 | 375.3 | 375.3 | 0.978x | 0.000 | reject |
| 22b | 346.9 | 352.0 | 352.5 | 414.4 | 414.4 | 414.4 | 0.837x | 0.000 | reject |
| 22c | 514.7 | 546.8 | 549.6 | 532.8 | 532.8 | 532.8 | 0.966x | 0.505 | fail-to-reject |
| 22d | 610.2 | 632.4 | 634.4 | 762.2 | 762.2 | 762.2 | 0.801x | 0.000 | reject |
| 23a | 305.8 | 315.4 | 316.2 | 338.4 | 338.4 | 338.4 | 0.904x | 0.000 | reject |
| 23b | 277.7 | 284.2 | 284.7 | 303.1 | 303.1 | 303.1 | 0.916x | 0.000 | reject |
| 23c | 295.7 | 312.7 | 314.2 | 337.4 | 337.4 | 337.4 | 0.876x | 0.000 | reject |
| 24a | 578.8 | 595.1 | 596.5 | 642.3 | 642.3 | 642.3 | 0.901x | 0.000 | reject |
| 24b | 532.1 | 556.0 | 558.1 | 590.6 | 590.6 | 590.6 | 0.901x | 0.000 | reject |
| 25a | 389.2 | 418.1 | 420.7 | 457.4 | 457.4 | 457.4 | 0.851x | 0.000 | reject |
| 25b | 380.4 | 397.2 | 398.7 | 401.7 | 401.7 | 401.7 | 0.947x | 0.000 | reject |
| 25c | 428.1 | 487.6 | 492.9 | 449.0 | 449.0 | 449.0 | 0.953x | 0.484 | fail-to-reject |
| 26a | 351.7 | 389.8 | 393.2 | 409.1 | 409.1 | 409.1 | 0.860x | 0.000 | reject |
| 26b | 338.1 | 343.8 | 344.3 | 349.3 | 349.3 | 349.3 | 0.968x | 0.000 | reject |
| 26c | 375.1 | 406.5 | 409.3 | 409.3 | 409.3 | 409.3 | 0.916x | 0.515 | fail-to-reject |
| 27a | 273.8 | 290.5 | 292.0 | 239.3 | 239.3 | 239.3 | 1.144x | 0.487 | fail-to-reject |
| 27b | 245.6 | 246.3 | 246.4 | 246.3 | 246.3 | 246.3 | 0.997x | 0.532 | fail-to-reject |
| 27c | 374.3 | 400.4 | 402.8 | 350.8 | 350.8 | 350.8 | 1.067x | 0.000 | reject |
| 28a | 449.9 | 470.6 | 472.5 | 443.5 | 443.5 | 443.5 | 1.015x | 0.530 | fail-to-reject |
| 28b | 284.8 | 317.8 | 320.7 | 292.2 | 292.2 | 292.2 | 0.975x | 0.519 | fail-to-reject |
| 28c | 463.1 | 489.7 | 492.1 | 537.0 | 537.0 | 537.0 | 0.862x | 0.000 | reject |
| 29a | 426.8 | 430.8 | 431.1 | 407.8 | 407.8 | 407.8 | 1.047x | 0.516 | fail-to-reject |
| 29b | 386.5 | 396.3 | 397.1 | 372.1 | 372.1 | 372.1 | 1.039x | 0.000 | reject |
| 29c | 784.2 | 818.2 | 821.2 | 779.9 | 779.9 | 779.9 | 1.006x | 0.517 | fail-to-reject |
| 30a | 420.5 | 473.0 | 477.6 | 400.2 | 400.2 | 400.2 | 1.051x | 0.532 | fail-to-reject |
| 30b | 444.0 | 454.5 | 455.4 | 438.4 | 438.4 | 438.4 | 1.013x | 0.514 | fail-to-reject |
| 30c | 473.3 | 487.3 | 488.6 | 513.3 | 513.3 | 513.3 | 0.922x | 0.000 | reject |
| 31a | 445.1 | 455.8 | 456.8 | 494.1 | 494.1 | 494.1 | 0.901x | 0.000 | reject |
| 31b | 476.7 | 511.2 | 514.2 | 402.7 | 402.7 | 402.7 | 1.184x | 0.000 | reject |
| 31c | 543.5 | 545.8 | 546.0 | 515.1 | 515.1 | 515.1 | 1.055x | 0.519 | fail-to-reject |
| 32a | 119.7 | 135.7 | 137.1 | 92.5 | 92.5 | 92.5 | 1.293x | 0.000 | reject |
| 32b | 135.4 | 198.4 | 204.1 | 108.7 | 108.7 | 108.7 | 1.245x | 0.000 | reject |
| 33a | 281.8 | 312.3 | 315.0 | 254.2 | 254.2 | 254.2 | 1.108x | 0.543 | fail-to-reject |
| 33b | 191.7 | 214.2 | 216.2 | 177.0 | 177.0 | 177.0 | 1.084x | 0.000 | reject |
| 33c | 311.8 | 315.7 | 316.0 | 332.8 | 332.8 | 332.8 | 0.937x | 0.000 | reject |

The per-query "speedup" range is 0.748×–1.479×, geomean 1.0005×, indistinguishable
from 1.0× under the CI. **This is the within-baseline noise floor on this
hardware after a `drop_caches` between trials.** It is *not* a corrector effect.

The within-baseline noise floor is a useful artifact in its own right: any
future corrector signal must clear roughly ±2.1% on the geomean and ±25% on
the worst per-query tail before it can be claimed as real.

---

## 5 — Aggregate

| Statistic | Value | Note |
|---|---:|---|
| Geomean "speedup" (baseline-vs-baseline) | 1.0005× | n=113; **not** evidence of a corrector win |
| BCa 95% CI | [0.981, 1.021] | straddles 1.0× |
| Wilcoxon W | 3066.0 | n_nonzero=113 |
| Wilcoxon p (two-sided) | 0.659 | fail-to-reject |
| BH-FDR rejects (α=0.05) | 63 / 113 | per-query bootstrap on 1-trial vs 3-trial medians; an artefact of the unequal trial counts, not a signal |

The 63/113 BH-reject count looks suspiciously high for a true null, and it is:
the per-query bootstrap inside `aggregate.py` resamples a 1-trial vector
(degenerate — every resample is the same single value) against a 3-trial
vector. The two distributions are *trivially* statistically distinguishable
even when the underlying processes are identical. Treat this number as a
calibration artefact, not a finding. With balanced n_trials in both arms the
expected reject count under the null is ~5/113 (α=0.05).

---

## 6 — Reproducibility

```bash
# 1. Sidecars (idempotent).
./target/release/samkhya-bench build-puffin --imdb-dir samkhya-bench/data/job

# 2. Cold-cache 3-trial baseline (the run that landed):
sudo sync && sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches'
./target/release/samkhya-bench run \
  --suite job-slow-real \
  --imdb-dir samkhya-bench/data/job \
  --mode baseline \
  --cold-cache \
  --trials 3 \
  --json-out bench-results/wave5m_raw/cold_baseline.json

# 3. Cold-cache corrected 3-trial arm (NOT YET RUN — OOM'd in Wave-5J n=30 attempt):
sudo sync && sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches'
./target/release/samkhya-bench run \
  --suite job-slow-real \
  --imdb-dir samkhya-bench/data/job \
  --mode samkhya \
  --cold-cache \
  --trials 3 \
  --json-out bench-results/wave5m_raw/cold_corrected.json

# 4. Aggregate (requires both files):
python3 bench-results/wave5m_raw/aggregate.py \
  --baseline bench-results/wave5m_raw/cold_baseline.json \
  --corrected bench-results/wave5m_raw/cold_corrected.json \
  --out bench-results/wave5m_raw/cold_aggregate.json
```

The `--cold-cache` flag in `samkhya-bench` causes the runner to call
`posix_fadvise(fd, POSIX_FADV_DONTNEED)` on each opened sidecar before the
timer starts. The runner does *not* itself call `drop_caches` — that requires
root and is the operator's responsibility.

---

## 7 — Citations (mandatory)

- **BCa bootstrap:** Efron & Tibshirani, *An Introduction to the Bootstrap*,
  Chapman & Hall, 1993 — Chapter 14 (BCa method).
- **Wilcoxon signed-rank:** Wilcoxon, F., 1945. *Individual Comparisons by
  Ranking Methods*, Biometrics Bulletin 1(6):80–83.
- **Benjamini-Hochberg FDR:** Benjamini, Y., Hochberg, Y., 1995. *Controlling
  the False Discovery Rate*, JRSSB 57(1):289–300.
- **JOB workload:** Leis, V. et al., 2015. *How Good Are Query Optimizers,
  Really?*, PVLDB 9(3):204–215.
- **q-error definition (not used here):** Moerkotte, G. et al., 2009.
  *Preventing Bad Plans by Bounding the Impact of Cardinality Estimation
  Errors*, PVLDB 2(1):982–993.
- **Memory hierarchy / cold-cache rationale:** Hennessy, J.L., Patterson, D.A.,
  *Computer Architecture: A Quantitative Approach*, 6th ed., Morgan Kaufmann,
  2017 — Chapter 2.

---

## 8 — Honest disclaimer (**read this before quoting any number above**)

1. **The cold-cache corrected arm did not run on Wave-5M.** The previous attempt
   in Wave-5J n=30 head-to-head OOM'd at `q1c` (corrected arm reached only
   17 / 113 queries; 15 KB of JSON before SIGKILL). Wave-5M was supposed to
   restart with n=3 and a memory-leaner harness; the baseline arm completed,
   the corrected arm did not, and **no `WAVE5M_*.md` receipt was emitted at the
   end of the Wave-5M run** — corroborating that the corrected arm never
   reached completion.
2. The "corrected" column in §4 is `cold_t1.json`, which the runner emitted with
   `"mode": "baseline"`. We labelled it `corrected` only to make the aggregator
   accept the inputs; that label is **not** a claim of provenance.
3. The geomean ratio of 1.0005× and the CI [0.981, 1.021] are therefore an
   estimate of the **within-baseline run-to-run noise floor under cold-cache
   discipline on this hardware**, not a corrector effect. The number is useful
   as a noise-floor calibration for future corrected-cold-cache campaigns; it
   is **not** comparable to the warm-cache 1.038× speedup in file 12.
4. The 63 / 113 BH-reject count in §5 is an artefact of unequal trial counts
   between the two arms (3-trial vs 1-trial). Under a true null with matched
   trial counts we expect ~5/113 at α=0.05.
5. **No number above should be cited as evidence of a corrector win, a
   corrector loss, or a difference between warm and cold cache behaviour for
   the corrector.** All three of those claims require the corrected-cold-cache
   arm to land.
6. The honest comparison to file 12 (warm-cache, real head-to-head) is: the
   cold-cache corrected arm has not yet been measured. The closure of this
   gap is on the v1.0-rc.2 (or later) checklist.

Sole author throughout: Prateek Singh. No PII in this document. Naming rule
honored throughout: no "learned" / "adaptive" / "AI" branding.
