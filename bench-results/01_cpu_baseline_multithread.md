# 01 — CPU baseline, multi-thread sweep

**Date:** 2026-05-16 (UTC)
**Hardware reference:** [`bench-results/00_hardware_profile.md`](./00_hardware_profile.md) — 13th Gen Intel i9-13900HK, 14 cores / 20 threads (6P+8E hybrid), 24 MiB L3, single NUMA node.
**Crate under test:** `samkhya-core` v0.4.0
**License:** Apache-2.0

---

## 1. Verdict

**Metric:** wallclock P50/P95/P99 + speedup with **95% BCa bootstrap CI** (Efron & Tibshirani 1993,
"An Introduction to the Bootstrap", chapter 14, "Better Bootstrap Confidence Intervals";
10,000 resamples on the median; **bootstrap seed `0xB007_5EED`** — distinct from the
measurement seed `0x9E37_79B9_7F4A_7C15` described in §3.1; cold-cache and warm-cache
distinguished per ACM Artifact Evaluation v1.1).

> **Anchor-cell update (WAVE5-H closure):** the `t=1` anchor cells in §4 (rows
> marked ¹, ², ³) were originally drawn from `B13_criterion.md`, which used
> Criterion's default **percentile bootstrap** at 10K/100K scales — not the
> 10M-element single-threaded anchor file 01 references. WAVE5-H landed a new
> precision-only anchor example at
> `samkhya-core/examples/hll_precision_sweep_10m.rs` that runs HLL p=14 at the
> 10M-row anchor scale and emits a per-trial RSE vector
> (`bench-results/01_hll_precision_raw.json`). 30 trials produced **mean abs
> RSE = 0.586 %, 95% BCa CI [0.447 %, 0.772 %]** (10 000 resamples, seed 42,
> via `bootstrap_ci.py --method bca --statistic mean`). The Flajolet 2007
> theoretical bound at p=14 is 0.812 %; the measured mean sits inside it and
> the BCa upper bound clears it, confirming the standard HLL precision claim
> at the file-01 anchor scale.  The §4 multi-thread (predicted) cells still
> use the canonical BCa specification described above and will be
> materialised once the multi-threaded harness ships. Per-iteration *wallclock*
> at 10M (the original `[131,331] ms` percentile bound) remains a B13
> snapshot follow-up — what WAVE5-H establishes is the **precision anchor**,
> which is the empirical claim file 01 actually loads onto its row 1.
**Multi-cell correction:** Benjamini-Hochberg FDR at α=0.05 (Benjamini & Hochberg JRSSB 1995)
applied to the 24-cell (operation × thread-count) grid. Speedup is reported per cell; an
aggregate **geometric mean of per-operation speedup** (Leis et al. VLDB 2015 convention) is
used when summarising across the four sketch operations.

Samkhya's three core sketches (HLL `p=14`, Bloom, CMS) compose by an associative `merge()`,
so a partition-then-merge layout is the load-bearing parallel pattern. At the 10 M-item
working-set scale this document targets:

- **HLL build** is predicted to scale near-linearly to the **P-core count (8 threads)**,
  then taper. Reason: each per-partition sketch is 16 KiB (`2^14` registers), fits in L1d
  per worker (32 KiB L1d × 14 cores on this CPU), and the merge is a 16 384-entry
  byte-wise `max` — negligible vs build cost.
- **CMS build** (ε=1e-4, δ=1e-4 → depth=10, width=10 000, ~390 KiB per sketch) is
  predicted to scale well to **~8 threads** and then become **L2-bandwidth-bound**
  (the working set per worker spills L1d; on this Raptor Lake the L2 is shared per
  E-core cluster — 4 MiB shared across 4 E-cores).
- **Bloom build** (10 M items, 0.01 FPR → ~12 MiB bit-array) is predicted to be
  **memory-bandwidth-bound past ~4–8 threads**: the bit-array does not fit in L2 per
  worker, so each `insert()` is an L3/DRAM round-trip on a random bit.
- **HLL merge of 1 000 partitions** (16 KiB each → 16 MiB aggregate, just over L3)
  is predicted to be **memory-bandwidth-bound from 2 threads onward**. Streaming
  byte-wise max over 16 MiB is a near-pure DRAM read benchmark.

**Reproducibility status:** the multi-thread harness described below is **methodology-only**
in this revision. The current `samkhya-core/benches/stress.rs` is single-threaded and
`samkhya-core` does not pull `rayon`. Real multi-thread numbers will be filled in once
the harness ships (tracked as a follow-up to `B13`). The single-threaded reference
numbers from [`B13_criterion.md`](./B13_criterion.md) anchor the `t=1` column.

---

## 2. Pre-registered hypothesis

Filed before any multi-thread run is executed, per `feedback_empirical_methodology`.

| ID | Operation | Hypothesis (95% CI) | Mechanism |
|----|-----------|---------------------|-----------|
| H1 | HLL build, 10 M u64, p=14 | Speedup at `t=8` ∈ **[6.0×, 8.0×]**; speedup at `t=16` ∈ **[6.5×, 9.5×]**; at `t=20` ∈ **[6.0×, 9.5×]** | Per-partition sketch (16 KiB) is L1d-resident; merge is `O(2^14)` per partition pair, dwarfed by 10 M / `t` inserts |
| H2 | CMS build, 10 M items, ε=δ=1e-4 | Speedup at `t=8` ∈ **[5.5×, 7.5×]**; at `t=16` ∈ **[5.5×, 8.5×]** | Sketch is 390 KiB → spills L1d, L2-bandwidth-limited past P-cores |
| H3 | Bloom build, 10 M items, 0.01 FPR | Speedup at `t=8` ∈ **[3.0×, 5.5×]**; at `t=16` ≤ **6.0×** | 12 MiB bit-array > L2 (1.25 MiB / P-core); random-bit writes are DRAM-bound past ~4 threads |
| H4 | HLL merge of 1 000 partitions (p=14) | Speedup at `t=8` ≤ **3.0×**; at `t=16` ≤ **3.5×** | 16 MiB aggregate slightly exceeds 24 MiB L3 when other state is co-resident; pure streaming-read with byte-max |
| H5 | LpBound evaluation (placeholder) | Single-thread P99 < **1 ms** per query; no parallel speedup expected (query-level parallelism is the right layer, not intra-query) | LpBound is a closed-form bound; inner loop is O(table count × sketch lookups) |
| H6 | Residual corrector inference (placeholder) | Single-thread P99 < **50 µs**; no parallel speedup expected | Per-query LUT or linear feedback; same logic as H5 |

Hypotheses are intervals, not point estimates. A run that lands inside the interval
is a confirmation; a run outside the interval falsifies the hypothesis and triggers
investigation rather than goalpost-shifting.

Amdahl's law upper-bound: with serial fraction `s = 1/N_partitions ≈ 0.001`
(one merge step at the end), maximum theoretical speedup at `t=20` is
`1 / (s + (1-s)/20) ≈ 19.6×`. H1–H4 all sit well below this because the binding
constraint is memory bandwidth, not serial fraction.

---

## 3. Methodology

### 3.1 Operations (4)

All four operations build or merge on 10 M unique 8-byte keys generated by an LCG
(seed `0x9E37_79B9_7F4A_7C15` × index, **first seed tried — no seed search**),
identical across all thread counts and replicates. Keys are pre-materialised once
outside the timed region.

| Op | Build cost target | Sketch size | Merge cost target |
|----|-------------------|-------------|-------------------|
| HLL build | 10 M `add()` calls on `HllSketch::new(14)` | 16 KiB | byte-wise max over 16 384 registers |
| CMS build | 10 M `add(_, 1)` calls on `CountMinSketch::new(10, 10_000)` | ~390 KiB | element-wise sum over 100 000 cells |
| Bloom build | 10 M `insert()` calls on `BloomFilter::new(10_000_000, 0.01)` | ~12 MiB | bitwise OR (not measured in this doc) |
| HLL merge ×1000 | 1 000 pre-built `HllSketch(14)` partitions, each loaded with 10 k items, merged sequentially into an accumulator | — | 1 000 × 16 384-byte max |

Parameter rationale for CMS: ε=1e-4, δ=1e-4 implies `depth = ⌈ln(1/δ)⌉ = 10`
and `width = ⌈e/ε⌉ = 27 183`; the `width = 10_000` choice trades 2.7× ε
inflation for a clean 100 000-cell sketch that fits in 390 KiB and behaves
predictably under L2-pressure benchmarking. This is documented up front to
avoid the "we tuned to make scaling look good" objection.

### 3.2 Thread sweep

`{1, 2, 4, 8, 16, 20}` worker threads. Mechanism: a single rayon `ThreadPool`
built once per benchmark group with explicit `num_threads(t)`. `RAYON_NUM_THREADS`
is set as a belt-and-braces measure for any transitive rayon user.

The thread counts map to the 13900HK topology as follows:

| `t` | Mapping on 13900HK (6 P-cores HT + 8 E-cores) |
|-----|-----------------------------------------------|
| 1   | 1 P-core, 1 thread (HT sibling idle) |
| 2   | 2 P-cores |
| 4   | 4 P-cores |
| 8   | 6 P-cores + 2 E-cores (OS scheduler decides) |
| 16  | 6 P-cores HT (12 threads) + 4 E-cores |
| 20  | 6 P-cores HT (12 threads) + 8 E-cores (full machine) |

Hybrid P/E topology and HT make `t=16` and `t=20` non-uniform: half the workers
are on physically slower cores or sharing an HT physical core. The
`scaling efficiency` column accounts for this without normalising it away.

### 3.3 Replicates and statistics

- **≥ 30 replicates** per `(operation, t)` cell (ACM Artifact Evaluation v1.1 convention).
- 95% **bias-corrected and accelerated (BCa) bootstrap CI** (Efron & Tibshirani 1993,
  "An Introduction to the Bootstrap", chapter 14, "Better Bootstrap Confidence
  Intervals") on the median wall time, **10,000 resamples** (the campaign-wide floor;
  see METHODOLOGY.md). Bootstrap seed `0xB007_5EED` — separate from the measurement
  seed `0x9E37_79B9_7F4A_7C15` so reshuffling the resamples is independent of any
  reshuffling of the measurement RNG. Criterion's default is percentile bootstrap; BCa is
  preferred because the distribution is right-skewed by OS scheduling tails.
- **Benjamini-Hochberg FDR** at α=0.05 (Benjamini & Hochberg JRSSB 1995) when reporting
  significance across the 24-cell (operation × t) grid.
- **P50, P95, P99** computed directly from the sorted per-iteration sample
  vector. With n=30, P99 has only 0–1 samples beyond it; we report it but flag
  cells with `n < 100` as "worst observed of 30" rather than a distributional P99.
- **Speedup** = `median(t=1) / median(t)`; **scaling efficiency** = `speedup / t`.
- **Warmup**: 5 s (criterion default + 2 s) before each cell to settle the
  rayon thread pool and prime any TLB / branch-predictor state.

### 3.4 System controls

- CPU governor: `performance` (set via `sudo cpupower frequency-set -g performance`).
  If the governor is `powersave` at run time, every cell is flagged and the
  document caveats apply. The B13 run was on `powersave` and saw 10–30% slowdown;
  this run must lock the governor.
- Intel Turbo Boost: leave enabled, since this measures *delivered* throughput,
  not steady-state thermal floor. Document the package temperature at start and
  end of each thread-sweep block; flag any cell with > 90 °C package temp.
- Background load: capture `uptime` and `vmstat 1 5` immediately before each
  cell; abort if 1-min load average > 1.0.
- Address-space randomisation, transparent hugepages, frequency scaling
  driver: record once per session in the run log; treat as fixed.

### 3.5 Multi-tier baselines

Each operation is compared against three reference points:

| Tier | Reference | Purpose |
|------|-----------|---------|
| T0 | Single-thread samkhya (`t=1`, this doc) | Internal speedup denominator |
| T1 | Single-thread streaming over `Vec<u8>` (no sketch, just `XxHash::write` per item) | Lower bound: how fast can we hash 10 M items, ignoring sketch state? |
| T2 | Linux `perf stat -e cache-misses,LLC-load-misses` on the `t=20` run | Confirms or refutes the "memory-bandwidth-bound" story |

Hypothesis H3 (Bloom DRAM-bound) is testable by T2: if `t=20` Bloom build shows
`LLC-load-misses / instruction ≳ 0.02`, the bandwidth story holds.

---

## 4. Results table

> **Status:** harness not yet shipped. The `t=1` column is anchored on
> [`B13_criterion.md`](./B13_criterion.md) where applicable. Multi-thread cells
> are predicted ranges (italicised) that this document will be revised against
> once the harness lands. **No single-run numbers are reported.**

### 4.1 HLL build, 10 M u64 inserts, p=14

| `t` | Median wall | 95% CI (BCa for multi-thread; see anchor note for `t=1`) | P95 | P99 | Throughput | Speedup | Efficiency |
|-----|-------------|--------------|-----|-----|------------|---------|------------|
| 1   | *158.2 ms*¹ | *[141.2, 187.3] ms*¹ — 95% percentile bootstrap CI (wallclock anchor; WAVE5-H landed the *precision* anchor at 10M scale — see verdict note above and `bench-results/01_hll_precision_raw.json` — but the *wallclock* anchor for this row still requires a 10M Criterion bench whose `sample.json` is snapshotted into bench-results/; sequenced as a follow-up after WAVE5-H) | *331 ms*¹ | *331 ms*¹ | *63 M elem/s*¹ | 1.00× | 100% |
| 2   | *82–95 ms*  | *predicted* | — | — | *105–120 M/s* | *1.7–1.9×* | *85–95%* |
| 4   | *42–52 ms*  | *predicted* | — | — | *190–240 M/s* | *3.0–3.8×* | *75–95%* |
| 8   | *21–28 ms*  | *predicted* | — | — | *360–470 M/s* | *5.7–7.5×* | *71–94%* |
| 16  | *16–25 ms*  | *predicted* | — | — | *400–625 M/s* | *6.3–9.9×* | *39–62%* |
| 20  | *15–26 ms*  | *predicted* | — | — | *385–667 M/s* | *6.1–10.5×* | *30–53%* |

¹ Scaled from `stress/hll_million_inserts` median 15.82 ms / 1 M items ×
10 (B13, powersave). On `performance` governor expect 12–14 ms / M → 120–140 ms / 10 M.
The predicted range straddles both governor states.

### 4.2 CMS build, 10 M items, ε=δ=1e-4 (depth=10, width=10 000)

| `t` | Median wall | 95% CI (BCa) | P95 | P99 | Throughput | Speedup | Efficiency |
|-----|-------------|--------------|-----|-----|------------|---------|------------|
| 1   | *no anchor* — predicted *250–350 ms* (10 hash funcs per insert vs HLL's 1) | — | — | — | *28–40 M/s* | 1.00× | 100% |
| 2   | *130–185 ms* | — | — | — | *54–77 M/s* | *1.7–1.9×* | *85–95%* |
| 4   | *68–98 ms*   | — | — | — | *102–147 M/s* | *3.0–3.7×* | *75–93%* |
| 8   | *37–55 ms*   | — | — | — | *182–270 M/s* | *5.0–7.0×* | *63–88%* |
| 16  | *30–50 ms*   | — | — | — | *200–333 M/s* | *5.5–8.5×* | *34–53%* |
| 20  | *28–50 ms*   | — | — | — | *200–357 M/s* | *5.5–8.5×* | *28–43%* |

### 4.3 Bloom build, 10 M items, 0.01 FPR (≈ 95.85 Mbit ≈ 12 MiB array, k=7)

| `t` | Median wall | 95% CI (BCa for multi-thread; see anchor note for `t=1`) | P95 | P99 | Throughput | Speedup | Efficiency |
|-----|-------------|--------------|-----|-----|------------|---------|------------|
| 1   | *166 ms*²   | *[164, 168] ms*² — 95% percentile bootstrap CI (wallclock anchor; WAVE5-H landed the *precision* anchor at 10M scale — see verdict note above and `bench-results/01_hll_precision_raw.json` — but the *wallclock* anchor for this row still requires a 10M Criterion bench whose `sample.json` is snapshotted into bench-results/; sequenced as a follow-up after WAVE5-H) | *214 ms*² | *344 ms*² | *60 M/s*² | 1.00× | 100% |
| 2   | *90–105 ms* | — | — | — | *95–111 M/s* | *1.6–1.85×* | *80–93%* |
| 4   | *50–70 ms*  | — | — | — | *143–200 M/s* | *2.4–3.3×* | *60–83%* |
| 8   | *35–55 ms*  | — | — | — | *182–286 M/s* | *3.0–4.7×* | *38–59%* |
| 16  | *30–55 ms*  | — | — | — | *182–333 M/s* | *3.0–5.5×* | *19–34%* |
| 20  | *30–60 ms*  | — | — | — | *167–333 M/s* | *2.8–5.5×* | *14–28%* |

² Anchored on `bloom/bloom_insert_10k` median 166.06 µs / 10 k items
× 1 000 = 166 ms / 10 M items (B13, powersave). The hash-and-set-bit cost
scales linearly with input size in the single-thread case until the bit-array
spills L2, which it already does at 10 M / 12 MiB on this CPU. So the `t=1`
anchor is already in the bandwidth-bound regime; multi-thread gains are
proportionally smaller than HLL.

### 4.4 HLL merge of 1 000 partitions (each p=14, 10 k items pre-loaded)

| `t` | Median wall | 95% CI (BCa for multi-thread; see anchor note for `t=1`) | P95 | P99 | Throughput (partitions/s) | Speedup | Efficiency |
|-----|-------------|--------------|-----|-----|---------------------------|---------|------------|
| 1   | *6.7 ms*³   | *[6.55, 6.84] ms*³ — 95% percentile bootstrap CI (wallclock anchor; WAVE5-H landed the *precision* anchor at 10M scale — see verdict note above and `bench-results/01_hll_precision_raw.json` — but the *wallclock* anchor for this row still requires a 10M Criterion bench whose `sample.json` is snapshotted into bench-results/; sequenced as a follow-up after WAVE5-H) | *9.96 ms*³ | *12.55 ms*³ | *150 k merges/s*³ | 1.00× | 100% |
| 2   | *4.0–4.5 ms*| — | — | — | *222–250 k/s* | *1.5–1.7×* | *75–85%* |
| 4   | *2.6–3.2 ms*| — | — | — | *312–385 k/s* | *2.1–2.6×* | *53–65%* |
| 8   | *2.2–2.8 ms*| — | — | — | *357–455 k/s* | *2.4–3.0×* | *30–38%* |
| 16  | *2.0–2.7 ms*| — | — | — | *370–500 k/s* | *2.5–3.4×* | *16–21%* |
| 20  | *2.0–2.7 ms*| — | — | — | *370–500 k/s* | *2.5–3.4×* | *13–17%* |

³ The B13 anchor `hll_merge` is a single 2-way merge in 6.68 µs. Extrapolated
to a 1 000-way tree-reduction this is 1 000 × 6.68 µs / partition pair across
`log2(1 000) ≈ 10` levels of `partitions/2^level` merges → ~6.7 ms serial.
Real measurement at `t=1` may differ because the working set is 16 MiB
(spills L3) rather than 32 KiB per merge in the microbench.

---

## 5. Scaling efficiency analysis

The four operations split along two axes — working-set fit and arithmetic
intensity per byte — and the predicted scaling efficiencies fall out of those.

| Operation | Per-worker WS | Fits where? | Arithmetic intensity | Predicted regime |
|-----------|---------------|-------------|----------------------|------------------|
| HLL build | 16 KiB sketch + key stream | L1d | Hash + 1 register write per item | Compute-bound; near-linear to P-core count |
| CMS build | 390 KiB sketch + key stream | L2 (1.25 MiB / P-core) | 10 hashes + 10 writes per item | Compute-bound to ~P-cores, then L2 BW |
| Bloom build | 12 MiB sketch + key stream | L3 (24 MiB shared) | 7 hashes + 7 random-bit writes per item | LLC/DRAM BW past ~4 threads |
| HLL merge 1 000× | 16 MiB total HLL data | Near-L3-boundary | 1 byte-max per byte | DRAM-streaming-BW, ~2-thread saturation |

**Hybrid-topology penalty.** P-cores on the 13900HK clock to ~5.4 GHz under
single-thread load; E-cores top out around 4.0 GHz and have ~75% of the IPC
on integer-hash workloads. So a perfectly parallel operation going from
`t=8` (mostly P-cores) to `t=16` adds 8 weaker workers and the
speedup-per-added-thread drops to ~0.5–0.7. This is independent of memory
bandwidth and accounts for most of the predicted efficiency cliff between
`t=8` and `t=16`.

**HT penalty.** At `t=16` and `t=20` we are using HT siblings on the P-cores.
For arithmetic-bound work (HLL/CMS build) HT typically yields a 20–30% gain
per physical core; for memory-bound work (Bloom build, HLL merge) it yields
~5–10% because the two siblings compete for the same load/store unit.

**Amdahl floor.** For partition-then-merge HLL/CMS, the serial fraction is
`merge_cost / build_cost ≈ 16 KiB / (10 M × per-insert work) ≈ 10^-4`.
Amdahl's law caps the speedup at `1 / (10^-4 + (1-10^-4) / 20) ≈ 19.6×`.
None of H1–H4 sit anywhere near this ceiling, so Amdahl is not the binding
constraint. The binding constraints are L2/L3 bandwidth (H2–H4) and hybrid
core asymmetry (H1).

---

## 6. Discussion

**Where Amdahl bites and where it doesn't.** A naive "partition then merge"
pattern has near-zero serial fraction for HLL and CMS (the merge is
constant-time relative to build). So Amdahl is *not* the right model for the
6 P-core → 8/16/20 thread cliff. The right model is the **roofline**:
arithmetic intensity × machine balance. On this CPU the roofline knee for
random-access workloads sits around 8 threads; past that, throughput is
capped by ~60–80 GB/s of usable DDR5 bandwidth.

**Bloom is the worst-scaling sketch.** A 12 MiB bit-array under random
inserts has zero spatial locality per item (the 7 hash bits land in 7
random cache lines). Per-thread effective throughput is bounded by
`LLC fill latency / item`, which is roughly insensitive to thread count
once `t > L3_ways`. This is why H3's `t=16` upper bound is 6× rather than
the 8–10× HLL gets.

**HLL merge of 1 000 partitions is essentially a streaming-read benchmark.**
The byte-max operation is one instruction per byte; the 16 MiB working set is
read-mostly. Parallel merge can split the partition tree across workers but
each worker reads its share from DRAM at roughly the same aggregate rate,
yielding the modest 2.5–3.4× ceiling in H4.

**LpBound and residual corrector are deliberately not parallelised at the
operator level.** A LpBound query touches a small fixed number of sketch
lookups (single-digit microseconds in B13's `hll_estimate` ≈ 128 µs P50
range). The right parallelism for a query workload is *query-level*, not
intra-query: 113 JOB-Slow queries fan out across 20 rayon workers, each
worker runs the full single-thread LpBound pipeline. H5/H6 record this as
a single-thread budget rather than a scaling target.

**Hybrid-topology recommendation.** For partition-then-merge workloads on
this CPU, the cost-effective operating point is `t = 8` (≈ 6 P-cores +
2 E-cores). The `t = 16` and `t = 20` runs add E-core workers and HT
siblings that contribute < 50% of a P-core's throughput while doubling
power draw. For batch ingest where wall-clock matters more than energy,
use `t = 20`; for steady-state OLAP where energy matters, use `t = 8`.

---

## 7. Limitations

1. **Harness is methodology-only.** `samkhya-core/benches/stress.rs` is
   single-threaded and `samkhya-core` does not currently depend on `rayon`.
   The thread-sweep cells in §4 are pre-registered predicted ranges, not
   measurements. Real numbers require a follow-up crate-level change to
   add a `rayon`-backed bench target. This document records the prediction
   *before* implementation specifically so the implementation cannot be
   tuned to a moving target.
2. **`stress` bench `sample_size(10)`.** Even the single-thread anchor
   `stress/hll_million_inserts` runs at n=10, so its P95/P99 are
   "worst observed of 10" not distributional percentiles. The harness must
   raise this to n ≥ 30 (preferably n=100) for the multi-thread version.
3. **CPU governor.** The anchor B13 run was on `powersave`. The multi-thread
   sweep must run on `performance`; otherwise the `t=1` baseline shifts
   by 10–30% and every speedup number is off by the same factor.
4. **Single-NUMA-node assumption.** This CPU is one NUMA node; on a 2-socket
   server the partition-then-merge pattern needs explicit NUMA-local
   allocation to hit the predicted scaling. The H1–H4 intervals do not
   transfer to multi-socket without revision.
5. **CMS parameter choice (`width = 10_000`).** This is a clean round number
   that yields a 390 KiB sketch but inflates ε from the nominal 1e-4 to
   ~2.7e-4. A `width = 27_183` run (true ε=1e-4) would produce a ~1.06 MiB
   sketch that spills the P-core L1d harder and would move the H2 interval
   downward by 5–15 percentage points of efficiency at `t = 4`.
6. **No power / thermal measurement.** A 13900HK in `performance` mode under
   `t = 20` sustained load will hit PL2 (~115 W) and throttle within
   30–60 s. The replicates must complete within the un-throttled window or
   include thermal-state telemetry in the run log.
7. **Bloom merge not measured.** Bloom filters merge by bitwise OR — fast
   and embarrassingly parallel — but this document only measures Bloom
   *build*. The merge operation would be DRAM-bandwidth-bound at ~20 GB/s
   per worker on a 12 MiB array; we expect it to scale similarly to
   H4 (HLL merge).

---

## 8. Reproducibility

### 8.1 Pinned environment (ACM Artifact Evaluation v1.1 discipline)

- Hardware: per [`00_hardware_profile.md`](./00_hardware_profile.md).
- Toolchain: `rustc --version` recorded in the run log; expect 1.83+ given
  `Cargo.toml` rust-version pin.
- Governor: `sudo cpupower frequency-set -g performance` before any run.
- Background: close every other CPU consumer; `systemctl --user stop`
  any user services that idle-poll.

### 8.2 Exact invocation (target harness — to be added)

The multi-thread harness will live alongside `stress.rs` as a new file
`samkhya-core/benches/parallel.rs`. Expected invocation:

```bash
# Set governor and confirm.
sudo cpupower frequency-set -g performance
cpupower frequency-info | grep "current policy"

# Sweep all (operation, t) cells; criterion writes to target/criterion/parallel/.
for T in 1 2 4 8 16 20; do
  RAYON_NUM_THREADS=$T \
  CARGO_TARGET_DIR=/tmp/samkhya-mt-target \
  cargo bench -p samkhya-core --bench parallel -- \
    --warm-up-time 5 --measurement-time 30 \
    --sample-size 30 --noplot \
    "t${T}/"
done

# Generate the markdown table from criterion's estimates.json.
python3 scripts/collect_parallel.py \
  --target /tmp/samkhya-mt-target/criterion/parallel \
  --out bench-results/01_cpu_baseline_multithread.results.json
```

Until `samkhya-core/benches/parallel.rs` exists, the single-thread anchor
can be reproduced directly from B13:

```bash
sudo cpupower frequency-set -g performance
CARGO_TARGET_DIR=/tmp/samkhya-b13-target \
cargo bench -p samkhya-core --bench stress -- \
  --warm-up-time 5 --measurement-time 30 \
  --sample-size 10 --noplot
```

### 8.3 Roofline / bandwidth attribution (T2 tier)

```bash
RAYON_NUM_THREADS=20 \
perf stat -e cache-misses,LLC-load-misses,LLC-store-misses,\
mem_load_retired.l3_miss,instructions,cycles \
  cargo bench -p samkhya-core --bench parallel -- \
    --profile-time 30 "t20/bloom_build"
```

Acceptance criterion for the "Bloom build is DRAM-BW-bound" claim (H3):
`LLC-load-misses / instructions ≳ 0.02` at `t = 20` and the per-thread
throughput at `t = 20` is within 15% of the per-thread throughput at
`t = 8`. Both conditions must hold for H3 to be confirmed.

### 8.4 Hashes for the run log

The run log under `bench-results/` for this document is the criterion
output tree itself (`new/estimates.json`, `new/sample.json` per cell).
Hash the tarball with `sha256sum target/criterion/parallel.tar.zst` and
record the digest in the run log alongside the rustc + governor state.
That tarball is the canonical artifact for this document.

---

## 9. What this document is and is not

- It **is** a pre-registered methodology with falsifiable hypothesis
  intervals on a fixed-point hardware reference, written before the
  measurement harness exists. The hypotheses cannot be retroactively
  edited to match results.
- It **is not** a results document. Section 4 contains predicted ranges
  and one anchor cell per row drawn from the single-thread B13 run.
  No multi-thread number in this document is a measurement.
- The next revision (`01_cpu_baseline_multithread.results.md` or an
  in-place edit with a `Revised: 2026-MM-DD` header) will fill the table
  with measured medians, BCa CIs, P95/P99 from `n ≥ 30` runs, and a
  per-hypothesis confirm/refute call.
