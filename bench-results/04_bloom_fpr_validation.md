# 04 — Bloom Filter False-Positive-Rate Validation

**Date:** 2026-05-16
**Crate:** `samkhya-core`
**Module:** `samkhya_core::sketches::bloom::BloomFilter`
**Driver:** `samkhya-core/examples/bloom_fpr_sweep.rs`
**Profile:** `--release`
**Host:** Linux 6.17.0-29-generic, Intel Core i9-13900HK (20 threads), 32 GiB RAM

---

## Verdict

**Metric:** empirical FPR vs configured FPR, validated against the canonical Bloom (CACM
1970; Mitzenmacher-Upfal textbook ch. 5) sizing formula
**`m = -n·ln(p) / (ln 2)^2`** and the textbook `(1 − e^(−k·n/m))^k` FPR formula.
**CI methodology — BCa, measured:** every CI in this file is reported as a **95% BCa
bootstrap CI** (Efron & Tibshirani 1993, "An Introduction to the Bootstrap",
Chapter 14), 10 000 resamples, bootstrap seed 42, re-derived from per-trial
empirical-FPR vectors persisted to `bench-results/04_bloom_fpr_sweep_raw.json` by the
WAVE5G rerun under `SAMKHYA_RAW_OUT`. The in-harness 2 000-resample percentile method
(chapter 13, bootstrap seed `0xBEEF`) is retained in the raw TSV columns `ci95_lo` /
`ci95_hi` for backward comparison; both methods agree to within RNG noise on every
cell, and the FAIL verdict is robust to CI flavour (every CI excludes the target by
multiple factors of the target value itself — far larger than any plausible BCa
shift). **Benjamini-Hochberg FDR** at α=0.05 (Benjamini & Hochberg JRSSB 1995)
applied across the 16-cell sweep.

**FAIL — 0 / 16 cells pass.** Every cell in the configured-FPR × capacity sweep
returns an empirical FPR substantially above its target, with the deviation
**identical to the theoretical FPR predicted by the as-built filter geometry**
(see Discussion). The implementation is internally consistent — the bit-array
machinery and double-hashing work correctly, and the empirical FPR converges
tightly around `(1 − e^(−k·n/m))^k` for the actual `(m, k)` chosen at
construction — but the sizing formula in `BloomFilter::new` uses the wrong
constant, producing filters that are under-sized by a factor of
`1 / ln 2 ≈ 1.443` in bits-per-element (deviation from canonical Bloom CACM 1970
sizing `m = -n·ln(p) / (ln 2)^2`). This is a true bug surfaced by the
pre-registered validation, not a finite-sample artefact.

---

## Pre-registered Hypothesis

H1: For every `(configured_FPR, capacity)` cell with `n ≥ 10^4`, the empirical
FPR over `10^6` negative queries satisfies
`empirical_FPR ≤ configured_FPR × 1.10` (95% CI mean), where the 10% safety
margin allows for finite-sample variance and the discrete `ceil` rounding in
the sizing formula.

H1 is **rejected** for all 16 cells. The deviation is not finite-sample noise:
the 95% bootstrap CI excludes the target on every cell, and the deviation
direction is uniformly negative (empirical > target).

---

## Methodology

### Grid

- **Configured FPR target:** `{0.001, 0.005, 0.01, 0.05}`
- **Capacity n:** `{10^4, 10^5, 10^6, 10^7}`
- **Negative-query budget per trial:** `10^6`
- **Trials per cell:** `30` (independent splitmix64 seeds)
- **Safety margin for PASS:** `1.10 × configured_FPR`

### Procedure (per cell, per trial)

1. Construct `BloomFilter::new(capacity, fp_rate)`.
2. Generate `capacity` distinct 64-bit keys drawn from splitmix64 seeded
   by `(capacity, fp_rate.to_bits(), trial_index)`; force the top bit clear.
   Insert each into the filter.
3. Generate `10^6` 64-bit query keys from a disjoint splitmix64 stream; force
   the top bit set. By construction the query namespace and insert namespace
   are disjoint, so every `contains` returning `true` is a true false positive.
4. Empirical FPR = `(positive returns) / 10^6`.

### Aggregation

- Per-cell **mean empirical FPR** over 30 trials.
- 95% **BCa bootstrap CI** for the mean, 10 000 resamples, bootstrap seed 42
  (Efron & Tibshirani 1993, chapter 14, "Better Bootstrap Confidence Intervals"),
  re-derived by the WAVE5G rerun from the per-trial empirical-FPR vectors
  persisted at `bench-results/04_bloom_fpr_sweep_raw.json` (the harness emits this
  sidecar JSON under `SAMKHYA_RAW_OUT`). The original in-harness 2 000-resample
  percentile method (chapter 13, "Confidence Intervals Based on Bootstrap
  Percentiles") with bootstrap seed `0xBEEF` is retained in the raw TSV columns
  `ci95_lo` / `ci95_hi` for backward comparison and agrees with BCa to within RNG
  noise on every cell. No `rand` dependency was added; the in-harness bootstrap
  reuses the splitmix64 PRNG family used for key generation but with a **distinct
  bootstrap seed `0xBEEF`** (first seed tried — no seed search).

### Determinism / Reproducibility

- All keys are deterministic functions of `(capacity, fp_rate, trial_index)`.
- Re-running the example yields bit-identical empirical FPRs to within the
  precision printed.

---

## Results

### Geometry chosen by `BloomFilter::new(n, p)`

| FPR target `p` | bits-per-element (m/n) | hashes (k) | source |
|---|---|---|---|
| 0.001 | **9.95** | 7 | `m = ceil(-1.44 · n · ln p)`; `k = ceil((m/n) · ln 2)` |
| 0.005 | **7.63** | 6 | as above |
| 0.01  | **6.63** | 5 | as above |
| 0.05  | **4.31** | 3 | as above |

The standard-textbook optimum for the same targets is `m/n = -ln p / (ln 2)^2`:

| FPR target `p` | as-built m/n | textbook m/n | shortfall |
|---|---|---|---|
| 0.001 | 9.95  | 14.38 | −30.8% |
| 0.005 | 7.63  | 11.03 | −30.8% |
| 0.01  | 6.63  |  9.59 | −30.8% |
| 0.05  | 4.31  |  6.24 | −30.8% |

The shortfall is exactly `1 − ln 2 = 0.3069` — see Discussion §1.

### FPR per cell (30 trials, 10^6 negative queries each)

| cell | `p_target` | `n` | `m` (bits) | `k` | `m/n` | empirical mean | 95% CI lo | 95% CI hi | pass |
|---|---|---|---|---|---|---|---|---|---|
| C01 | 0.001 | 10⁴  |     99 472 | 7 |  9.95 | **0.008401** | 0.008353 | 0.008455 | FAIL |
| C02 | 0.001 | 10⁵  |    994 717 | 7 |  9.95 | **0.008400** | 0.008364 | 0.008435 | FAIL |
| C03 | 0.001 | 10⁶  |  9 947 168 | 7 |  9.95 | **0.008405** | 0.008368 | 0.008443 | FAIL |
| C04 | 0.001 | 10⁷  | 99 471 677 | 7 |  9.95 | **0.008380** | 0.008346 | 0.008416 | FAIL |
| C05 | 0.005 | 10⁴  |     76 296 | 6 |  7.63 | **0.025989** | 0.025845 | 0.026128 | FAIL |
| C06 | 0.005 | 10⁵  |    762 958 | 6 |  7.63 | **0.026071** | 0.025997 | 0.026133 | FAIL |
| C07 | 0.005 | 10⁶  |  7 629 578 | 6 |  7.63 | **0.026076** | 0.026004 | 0.026152 | FAIL |
| C08 | 0.005 | 10⁷  | 76 295 771 | 6 |  7.63 | **0.026020** | 0.025968 | 0.026075 | FAIL |
| C09 | 0.01  | 10⁴  |     66 315 | 5 |  6.63 | **0.041625** | 0.041485 | 0.041759 | FAIL |
| C10 | 0.01  | 10⁵  |    663 145 | 5 |  6.63 | **0.041647** | 0.041569 | 0.041726 | FAIL |
| C11 | 0.01  | 10⁶  |  6 631 446 | 5 |  6.63 | **0.041606** | 0.041525 | 0.041686 | FAIL |
| C12 | 0.01  | 10⁷  | 66 314 451 | 5 |  6.63 | **0.041614** | 0.041539 | 0.041692 | FAIL |
| C13 | 0.05  | 10⁴  |     43 139 | 3 |  4.31 | **0.125955** | 0.125569 | 0.126389 | FAIL |
| C14 | 0.05  | 10⁵  |    431 386 | 3 |  4.31 | **0.125816** | 0.125663 | 0.125967 | FAIL |
| C15 | 0.05  | 10⁶  |  4 313 855 | 3 |  4.31 | **0.125965** | 0.125807 | 0.126110 | FAIL |
| C16 | 0.05  | 10⁷  | 43 138 545 | 3 |  4.31 | **0.125764** | 0.125655 | 0.125870 | FAIL |

**Pass count:** 0 / 16 at the pre-registered 10% safety margin.
**Even with a 5× safety margin** (`empirical ≤ 5 · target`), only the
`p = 0.05` cells (ratio ≈ 2.5×) pass; the `p = 0.001` cells (ratio ≈ 8.4×)
still fail.

### Internal consistency check (empirical vs. theory at as-built geometry)

For each cell, the textbook FPR formula at the **as-built** `(m, k, n)`
predicts:

| cell | `(1 − e^(−k·n/m))^k` | empirical mean | gap |
|---|---|---|---|
| C01 | 0.008406 | 0.008401 | −0.0001 |
| C05 | 0.026067 | 0.025989 | −0.0008 |
| C09 | 0.041626 | 0.041625 | −0.0000 |
| C13 | 0.125856 | 0.125955 | +0.0001 |

The empirical FPR matches the theoretical prediction for the as-built
geometry to four decimal places — i.e., the *implementation* of insert /
contains / double-hashing is correct. Only the *sizing constant in
`BloomFilter::new`* is wrong.

---

## Pass / Fail Summary

| `p_target` | cells | pass | observed mean | ratio (mean / target) |
|---|---|---|---|---|
| 0.001 | 4 | 0 / 4 | ≈ 0.00840 | **8.4 ×** |
| 0.005 | 4 | 0 / 4 | ≈ 0.02604 | **5.2 ×** |
| 0.01  | 4 | 0 / 4 | ≈ 0.04162 | **4.2 ×** |
| 0.05  | 4 | 0 / 4 | ≈ 0.12584 | **2.5 ×** |

The deviation is monotone in `p`: tighter targets fail by larger multiples
because `p_emp(p_target) ≈ p_target^(1 / (1 + δ))` where δ is the relative
m-shortfall, so log p amplifies the shortfall.

---

## Throughput

Average over 30 trials per cell. Inserts amortise hashing across `k` bits;
queries short-circuit on the first zero bit (so they're fastest when the
filter is sparse, slowest when full).

| cell | `n` | `p` | insert ns/op | query ns/op | insert Mops/s | query Mops/s |
|---|---|---|---|---|---|---|
| C01  | 10⁴ | 0.001 | 24.2 | 23.9 | 41.3 | 41.9 |
| C04  | 10⁷ | 0.001 | 43.5 | 51.1 | 23.0 | 19.6 |
| C09  | 10⁴ | 0.01  | 18.0 | 24.1 | 55.5 | 41.5 |
| C12  | 10⁷ | 0.01  | 28.3 | 41.8 | 35.3 | 23.9 |
| C13  | 10⁴ | 0.05  | 13.3 | 22.6 | 75.1 | 44.2 |
| C16  | 10⁷ | 0.05  | 21.3 | 35.6 | 46.9 | 28.1 |

Observations:
- **Insert** scales sub-linearly with `k` (lower `k` for higher `p` → fewer bit
  writes); throughput between **23 and 75 Mops/s** depending on cell.
- **Query** is roughly insert-cost-bound at small `n`, slower at large `n`
  because of L2/L3 misses against a multi-megabyte bit array.
- The 10⁷-capacity cells show a clear cache-pressure step (filter sizes
  ~5–12 MB exceed L2; throughput drops ~2×).
- All cells exceed **18 Mops/s** for both insert and query — fast enough to
  not be the bottleneck inside `samkhya-core` planning paths.

---

## Discussion

### 1. Root cause: wrong sizing constant in `BloomFilter::new`

`samkhya-core/src/sketches/bloom.rs:22`:

```rust
let num_bits = ((-1.44 * capacity * fp_rate.ln()).ceil() as u64).max(64);
```

The constant `1.44` is approximately `1 / ln 2 = 1.4427`, which is the constant
for **k = (m/n) · ln 2** (a few lines below, where it is used correctly). The
correct constant for the **m sizing formula** is

```
m / n = -ln(p) / (ln 2)^2  ≈  -2.0814 · ln(p)
```

i.e. **2.0814**, not 1.4427. The current expression under-allocates `m` by a
factor of `(ln 2)^{-1} ≈ 1.4427`, equivalent to a 30.8% bits shortfall, which
is exactly what the geometry table above shows.

### 2. Why the existing unit test passes

`samkhya-core/src/sketches/bloom.rs::tests::fp_rate_close_to_target`
constructs a `(10 000, 0.01)` filter, inserts 10 000 keys, and asserts
`fps / 10 000 < 0.05` — a 5× slack against the 0.01 target. The empirical
~0.0416 falls under 0.05, so the test passes despite the bug. Tightening the
slack to e.g. `< 0.015` (1.5× — generous for a 10k-query empirical estimate)
would have caught this years ago.

### 3. Why empirical matches as-built theory exactly

The Kirsch-Mitzenmacher double-hashing scheme (`bit_index(h1, h2, i, m) =
(h1 + i·h2) mod m`) is the textbook bit-distribution mechanism that the
`(1 − e^(−k·n/m))^k` formula assumes. The XxHash64 seeds (`0xc0ffee`,
`0xbeef`) decorrelate `h1` and `h2` adequately at the bit-uniformity granularity
the bound needs. With key namespaces fully disjoint, no hash collisions
between insert and query sets can falsely inflate the rate. The fact that
empirical FPRs sit on top of the theoretical curve to four decimal places is
the strongest possible confirmation that the **implementation** is sound and
the **calibration** is the lone defect.

### 4. Recommended fix (out of scope for this validation)

Replace line 22 with

```rust
let num_bits = ((-(capacity / (std::f64::consts::LN_2.powi(2))) * fp_rate.ln())
    .ceil() as u64)
    .max(64);
```

or equivalently use the numeric constant `2.0813689810056077` and keep the
present structure. The `num_hashes` line (which uses `LN_2` correctly) needs
no change. Re-running this sweep against the fix should drive all 16 cells
under the 10% safety margin.

### 5. Implications for downstream samkhya use

`BloomFilter` is used in `samkhya-core` to gate the residual-corrector
lookup and to back the membership-style predicates in the LpBound pipeline.
An 8× FPR shortfall against a configured 0.001 target translates directly
into 8× the wasted lookups for negative keys — measurable but not
correctness-affecting (Bloom filters never produce false negatives, which is
preserved here: the `no_false_negatives` unit test verifies this and would
remain green after the fix).

---

## Limitations

1. **Single host.** All measurements were taken on one machine
   (Intel i9-13900HK, Linux 6.17, release profile). FPR is theoretically
   host-independent; throughput is not, and the Mops/s numbers should not be
   compared cross-platform without re-running the example.
2. **Query namespace disjointness via top bit.** Keys with the top bit clear
   are inserted; keys with the top bit set are queried. This guarantees zero
   true positives but means the negative-query namespace explores only the
   upper half of the 64-bit space. The bloom-filter FPR does not depend on
   namespace shape (after hashing), so this is not expected to bias results;
   we have not separately verified shape-independence empirically here.
3. **CI method.** The 95% CIs are **BCa bootstrap** (Efron & Tibshirani 1993,
   chapter 14) at 10 000 resamples (bootstrap seed 42), re-derived from the
   per-trial empirical-FPR vectors persisted to
   `bench-results/04_bloom_fpr_sweep_raw.json` by the WAVE5G rerun. The in-harness
   2 000-resample percentile method (chapter 13, bootstrap seed `0xBEEF`) is also
   reported alongside (raw TSV columns `ci95_lo` / `ci95_hi`). With only 30 trials,
   the CI width is dominated by the between-trial variance of the per-trial
   empirical FPR (each itself a sum of 10^6 Bernoulli trials, so per-trial standard
   error is small). Both CI flavours exclude the target on every cell, so the
   failure verdict is robust to CI construction; the BCa endpoints agree with the
   percentile endpoints to within RNG noise across every cell.
4. **Pre-registered `n = 10^4` minimum.** Behaviour at `n < 10^4` was not
   measured. The 64-bit floor on `num_bits` may dominate small-capacity
   filters and is not exercised by this grid.
5. **Did not test alternative FPR targets** outside `{0.001, 0.005, 0.01,
   0.05}`. The deviation pattern (≈30% bit shortfall → log-scaled FPR
   inflation) predicts that the bug manifests at every `p < 1`.

---

## Reproducibility (ACM Artifact Evaluation v1.1)

### Source

- Driver: `samkhya-core/examples/bloom_fpr_sweep.rs` (this commit)
- Filter under test: `samkhya-core/src/sketches/bloom.rs`, struct `BloomFilter`,
  unchanged from `main` @ commit `0ec1f5d` (`scrub: documents/ + SECURITY.md
  PII`)

### Build

```bash
cargo build --release --example bloom_fpr_sweep -p samkhya-core
```

### Run

```bash
./target/release/examples/bloom_fpr_sweep > bloom_sweep_results.tsv \
    2> bloom_sweep_summary.txt
```

Expected wall time on a workstation-class CPU: **~10 minutes** end-to-end,
dominated by the four `n = 10^7` rows (each inserts 10⁷ keys × 30 trials).

### Determinism

All RNG (key generation + bootstrap) is splitmix64-based with hard-coded seeds
derived from `(capacity, fp_rate, trial_index)`. Two runs on the same binary
produce identical empirical-FPR columns; throughput columns vary at the few
percent level with CPU-frequency-scaling state.

### Acceptance criterion to flip to PASS

A future commit that replaces the `1.44` constant with `1 / (ln 2)^2` (or
otherwise produces `m/n ≥ −ln(p) / (ln 2)^2` for the targets in this grid)
should, when this validation is re-run unchanged, return `16 / 16 cells PASS`
at the 10% safety margin. That is the falsifiable threshold this report locks
in.
