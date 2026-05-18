# B13 Criterion Benchmark Results — samkhya-core

**Date:** 2026-05-16  
**Agent:** B13 (criterion full-suite run)  
**Platform:** Linux 6.17.0-29-generic  
**CARGO_TARGET_DIR:** `/tmp/samkhya-b13-target`  
**JSON outputs:** `/tmp/samkhya-b13-target/criterion/`

---

## 1. CPU Governor Note

`sudo cpupower frequency-set -g performance` was **skipped** — requires interactive sudo.  
Current governor at run time: **powersave**. This means the CPU may not be running at peak
frequency during the benchmarks; all timings should be interpreted as
conservative (potentially 10–30% slower than a locked-performance run). Medians
and CIs are internally consistent across the session since the governor did not
change between bench targets.

---

## 2. Benchmark Inventory

Only `samkhya-core` carries criterion bench targets. Three bench binaries were
found under `samkhya-core/benches/`:

| Binary | bench_functions | Notes |
|--------|-----------------|-------|
| `sketches` | `hll/hll_add_1k`, `hll/hll_add_100k`, `hll_estimate`, `hll_merge`, `bloom/bloom_insert_10k`, `bloom_contains_hit`, `bloom_contains_miss`, `hll_to_bytes`, `hll_from_bytes` | throughput groups for add/insert |
| `puffin` | `puffin_write_10_blobs`, `puffin_open`, `puffin_read_blob` | no throughput annotation |
| `stress` | `stress/hll_million_inserts`, `stress/puffin_thousand_blobs`, `stress/feedback_ten_thousand_observations` | self-clamped to `sample_size(10)` |

No other workspace crates (`samkhya-arrow`, `samkhya-datafusion`, etc.) carry
criterion bench targets. The bench inventory confirmed by
`cargo metadata --format-version 1`.

---

## 3. Run Parameters

```
cargo bench --bench <name> -p samkhya-core \
  -- --warm-up-time 5 --measurement-time 30 --sample-size 100 --noplot
```

- Warmup: 5 s (criterion's default is 3 s; extended per methodology)
- Measurement: 30 s per bench function
- Sample size: 100 requested; `stress` benches self-clamp to 10 (coded in source)
- Plot backend: plotters (gnuplot not found)

---

## 4. Results Table

All statistics derived from criterion's `new/estimates.json` + `new/sample.json`
at `/tmp/samkhya-b13-target/criterion/`.  
Median and 95% CI come from criterion's bootstrap (bootstrap resamples = 100 000,
confidence level = 95%).  
P95 and P99 are computed from the raw per-iteration sample distribution
(sorted per-iter times at the 95th/99th percentile index).  
Throughput is derived from `median` and the `Throughput::Elements` annotation
present on grouped benches.

| Benchmark | Median | 95% CI | P95 | P99 | Throughput | n | Noise verdict |
|-----------|--------|--------|-----|-----|------------|---|---------------|
| `hll/hll_add_1k` | 16.828 µs | [14.641, 19.121] µs | 37.843 µs | 48.933 µs | 59.43 Melem/s | 100 | clean |
| `hll/hll_add_100k` | 977.258 µs | [954.562 µs, 1.138 ms] | 1.667 ms | 1.667 ms | 102.33 Melem/s | 20 | clean |
| `hll_estimate` | 128.214 µs | [125.832, 129.944] µs | 184.734 µs | 233.093 µs | — | 100 | **NOISY** (11% severe) |
| `hll_merge` | 6.683 µs | [6.554, 6.841] µs | 9.956 µs | 12.553 µs | — | 100 | mild (4%) |
| `hll_to_bytes` | 7.267 µs | [7.198, 7.368] µs | 8.950 µs | 11.259 µs | — | 100 | mild (5%) |
| `hll_from_bytes` | 17.236 µs | [11.874, 18.789] µs | 29.330 µs | 38.034 µs | — | 100 | clean |
| `bloom/bloom_insert_10k` | 166.059 µs | [163.833, 167.920] µs | 214.089 µs | 343.866 µs | 60.22 Melem/s | 100 | **NOISY** (6% severe) |
| `bloom_contains_hit` | 40.484 ns | [27.297, 47.886] ns | 73.115 ns | 91.050 ns | — | 100 | clean |
| `bloom_contains_miss` | 27.815 ns | [27.262, 27.986] ns | 29.693 ns | 31.498 ns | — | 100 | **NOISY** (10% severe) |
| `puffin_write_10_blobs` | 3.602 µs | [3.293, 3.941] µs | 6.482 µs | 8.139 µs | — | 100 | clean |
| `puffin_open` | 4.422 µs | [4.084, 4.684] µs | 7.838 µs | 8.671 µs | — | 100 | clean |
| `puffin_read_blob` | 3.976 µs | [3.833, 4.150] µs | 6.952 µs | 8.674 µs | — | 100 | mild (4%) |
| `stress/hll_million_inserts` | 15.823 ms | [14.123, 18.729] ms | 33.069 ms | 33.069 ms | — | 10 | **NOISY** (10% severe) |
| `stress/puffin_thousand_blobs` | 868.449 µs | [829.039, 911.308] µs | 1.149 ms | 1.149 ms | — | 10 | **NOISY** (10% severe) |
| `stress/feedback_ten_thousand_observations` | 52.372 ms | [45.829, 87.245] ms | 97.961 ms | 97.961 ms | — | 10 | clean (wide CI) |

> Note: `stress/*` benches use `sample_size(10)` coded in the bench source. At
> n=10, the P95/P99 indices collapse to the maximum observed value. Those
> percentile figures should be read as "worst observed in 10 runs", not a
> distributional P95. The wide CI on `feedback_ten_thousand_observations`
> (45 ms–87 ms) is consistent with OS scheduling jitter on a 275-iteration
> workload. Rerunning with a larger `sample_size` (or on a locked-performance
> system) is recommended before treating this CI as definitive.

---

## 5. Bench API Notes: `bench_function` vs. throughput groups

Some bench functions use `c.bench_function(...)` directly (not inside a
`benchmark_group`). Criterion's linear regression model still applies; it does
**not** degrade to mean ± SD. However, benches without `group.throughput(...)`
produce no throughput estimate — those cells are "—" in the table above.

Affected (no throughput): `hll_estimate`, `hll_merge`, `hll_to_bytes`,
`hll_from_bytes`, `bloom_contains_hit`, `bloom_contains_miss`,
all three `puffin` benches, all three `stress` benches.

---

## 6. Outlier / Noise Analysis

Criterion classifies outliers via Tukey fences on the per-iteration sample
distribution. "High-severe" means the point lies beyond 3× IQR above Q3.

Benches with > 5% high-severity or low-severity outliers:

| Benchmark | Severe outlier % | Assessment |
|-----------|-----------------|------------|
| `hll_estimate` | 11.0% high-severe | NOISY — likely branch-predictor or TLB thrash at 16 384-register scan |
| `bloom_contains_miss` | 10.0% low-severe | NOISY — bimodal: fast path (misses early) vs full k-hash path |
| `bloom/bloom_insert_10k` | 6.0% high-severe | NOISY — allocation + hash pressure; wide P99 (343 µs vs 166 µs median) |
| `stress/hll_million_inserts` | 10.0% high-severe | NOISY — unavoidable at n=10; single outlier = 10% |
| `stress/puffin_thousand_blobs` | 10.0% high-severe | NOISY — same n=10 limitation |

`bloom_contains_miss` showing 10% **low**-severe outliers (faster than the lower
fence) is unusual — this is consistent with the branch predictor occasionally
short-circuiting the k-hash chain when all k bits are unset in the first word.

---

## 7. Sanity Check: Sub-µs Amortized Insert Cost

Requirement from `samkhya.md §3`: HLL `insert` sub-µs amortized per call.

| Bench | Median total | Items | Per-item | Sub-µs? |
|-------|-------------|-------|----------|---------|
| `hll/hll_add_1k` | 16.828 µs | 1 000 | **16.83 ns** | YES |
| `hll/hll_add_100k` | 977.258 µs | 100 000 | **9.77 ns** | YES |
| `stress/hll_million_inserts` | 15.823 ms | 1 000 000 | **15.82 ns** | YES |
| `bloom/bloom_insert_10k` | 166.059 µs | 10 000 | **16.61 ns** | YES |
| `stress/puffin_thousand_blobs` | 868.449 µs | 1 000 blobs | **868 ns/blob** | YES (per-blob) |
| `stress/feedback_ten_thousand_observations` | 52.372 ms | 10 000 obs | **5 237 ns/obs** | NO (SQLite) |

All HLL and Bloom paths meet the sub-µs amortized criterion.  
`feedback_ten_thousand_observations` at ~5.2 µs/observation is expected —
the `FeedbackStore` uses SQLite (`record()` issues an `INSERT` per call under
`open_in_memory()`). SQLite in-memory inserts are typically in the 2–10 µs
range. This is the amortized cost for the v0.6.0 JOB-Slow 113-query workload;
acceptable for a store that is written once per query execution, not per-row.

---

## 8. Benches That Did Not Run

None — all three bench binaries compiled and ran to completion.

Gnuplot was not found; criterion fell back to the plotters backend for any
HTML reports it would generate (irrelevant since `--noplot` was passed).

---

## 9. Pointer to JSON Outputs

All criterion raw data is at:

```
/tmp/samkhya-b13-target/criterion/
  bloom/bloom_insert_10k/new/{estimates,sample,benchmark,tukey}.json
  bloom_contains_hit/new/{...}
  bloom_contains_miss/new/{...}
  hll/hll_add_1k/new/{...}
  hll/hll_add_100k/new/{...}
  hll_estimate/new/{...}
  hll_merge/new/{...}
  hll_to_bytes/new/{...}
  hll_from_bytes/new/{...}
  puffin_write_10_blobs/new/{...}
  puffin_open/new/{...}
  puffin_read_blob/new/{...}
  stress/hll_million_inserts/new/{...}
  stress/puffin_thousand_blobs/new/{...}
  stress/feedback_ten_thousand_observations/new/{...}
```

Note: `/tmp` is ephemeral. For persistent storage, re-run with
`CARGO_TARGET_DIR` pointed at a path under the repo (e.g.
`target/criterion-b13/`).

---

## 10. Top Observations

1. **All HLL insert paths are well under 20 ns/insert** across 1k, 100k, and 1M
   element scales. The amortized cost is sub-linear (9.77 ns at 100k vs 15.82 ns
   at 1M), consistent with the 16 384-register HLL at p=14 fitting in L2 cache
   for moderate cardinalities and spilling to L3 at scale.

2. **`hll_estimate` is the noisiest hot-path benchmark** (11% high-severe
   outliers, median 128 µs, P99 233 µs). The estimate walk over 16 384 registers
   is memory-bound; this function should be profiled for cache-miss patterns if
   estimate latency becomes a bottleneck in the v0.6.0 JOB-Slow path.

3. **Puffin round-trip cost is comfortably sub-ms** at all tested scales:
   write 10 blobs = 3.6 µs, open = 4.4 µs, read first blob = 4.0 µs.
   At 1 000 blobs (stress), the full write+read round-trip takes 868 µs median
   — well within the sub-ms sidecar access budget.

4. **`bloom_contains_miss` shows 10% low-severe outliers** — faster than the
   lower Tukey fence. Likely branch-predictor fast-exit when the first hash bit
   is absent; not a correctness concern, but the bimodal distribution means P95
   (29.7 ns) overstates typical miss cost vs the 27.8 ns median.

5. **Stress benches at n=10 have unreliable percentiles.** The
   `stress/feedback_ten_thousand_observations` CI spans 45–87 ms, and P95/P99
   collapse to the max observed value. Re-run at `sample_size(50)` on a
   performance-governor machine for publishable numbers.

6. **CPU governor was `powersave`.** All medians are valid for relative
   comparisons within this session but may be 10–30% above bare-metal
   performance-mode numbers. Lock the governor before any wall-clock comparison
   or publication run.
