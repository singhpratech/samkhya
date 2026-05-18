# B09 — Property-Based Tests Scaled to 100k Cases

**Agent:** B09  
**Date:** 2026-05-16  
**Cargo target:** `/tmp/samkhya-b09-target`  
**Profile:** `--release`  
**Case count:** `PROPTEST_CASES=100000`

---

## Per-File Results

| File | Tests | Cases/test | Wall time (ms) | Verdict |
|---|---|---|---|---|
| `property_sketches` | 11 | 100,000 | 31,601 | PASS |
| `property_lpbound` (default) | 5 | 100,000 | 4,843 | PASS |
| `property_lpbound` (`lp_solver`) | 7 | 100,000 | 85,236 | PASS* |
| `property_puffin` | 4 | 100,000 | 41,061 | PASS |
| **Total (4 property files)** | **27** | — | **162,741** | **PASS*** |

\* See LpJoinBound section below. All test binaries reported `ok`; the known bug was not reproduced due to regression-replay failure (documented below).

No run exceeded 30 minutes; no case count was scaled down.

---

## Unit + Integration Suite (regression check)

```
cargo test --release -p samkhya-core
```

| Suite | Tests | Verdict |
|---|---|---|
| Unit tests (`src/lib.rs`) | 56 | PASS |
| Integration (`tests/integration.rs`) | 1 | PASS |
| `tests/properties.rs` | 16 | PASS |
| `tests/property_lpbound.rs` | 5 | PASS |
| `tests/property_puffin.rs` | 4 | PASS |
| `tests/property_sketches.rs` | 11 | PASS |

No regression from the 100k-case configuration. Wall time: 35,806 ms.

---

## LpJoinBound Known Bug — Verification

### Bug origin

The regression file at `samkhya-core/tests/property_lpbound.proptest-regressions` records:

```
cc 14ac92d2a15dd1072f731b7b436c995629b1fa3f789db893950955ef58d55372
# shrinks to rows = [4058, 534, 4051], preds_raw = [(1, 2)]
```

### Manual reproduction (confirmed)

With `rows = [4058, 534, 4051]` and `preds_raw = [(1, 2)]` (which maps to `preds = [(1, 2)]` after `i % n, j % n` with `n = 3`):

**Connected components:**
- Component `{0}`: singleton, ceiling = `rows[0]` = 4,058
- Component `{1, 2}`: two-relation join, LP minimises `x₁·ln(534) + x₂·ln(4051)` subject to `x₁ + x₂ ≥ 1`

**LP optimum for component `{1, 2}`:** place all weight on the smaller coefficient: `x₁ = 1, x₂ = 0` → objective = `ln(534)` → `exp(ln(534))`.

**Floating-point rounding:** `534.0_f64.ln().exp()` evaluates to `534.000000000000114` (IEEE-754 round-trip noise), so `raw.ceil() as u64 = 535`.

**Result:** `LpJoinBound = 4058 × 535 = 2,171,030`  
**AGM bound:** `min(rows) × max(rows) = 534 × 4058 = 2,166,972`  
**Excess:** `2,171,030 − 2,166,972 = 4,058 rows (+0.1873%)`  
**Test slack:** the `lp_le_agm` property asserts `lp_b ≤ agm.saturating_add(2)`, so the bug exceeds the allowed slack by **4,056 rows**.

### Root cause

Per-component `raw.ceil() as u64` in `LpJoinBound::solve_component` (file: `samkhya-core/src/lpbound.rs`, line ~400). When the LP's floating-point objective value rounds up even a single integer — here `534.0` rounds to `535` — the surplus propagates multiplicatively through all singleton components (here ×4,058), producing a multi-thousand-row overcount.

### Why the 100k run reported PASS

Proptest's `FileFailurePersistence::SourceParallel` mode requires a `src/lib.rs` or `src/main.rs` alongside the test binary's source tree in order to locate the `.proptest-regressions` file. For test binaries compiled from `tests/property_lpbound.rs` (an integration-test binary, not a library root), proptest emits:

```
proptest: FileFailurePersistence::SourceParallel set, but failed to find lib.rs or main.rs
```

and silently disables regression replay. Consequently the known failing seed was **not replayed** in the 100k run. The 100k fresh random cases did not independently rediscover the exact counterexample `[4058, 534, 4051]` against a search space of `[1..10,000]^3`, and all 100,000 cases passed.

**The bug is not fixed. It is reproducible via manual computation and the saved regression seed.**

---

## New Failures

None. The 100k scale-up uncovered no counterexamples beyond the known LpJoinBound bug.

Sketch properties (HLL, Bloom, CMS, EquiDepthHistogram), scaffolding bound properties (ProductBound, AgmBound, ChainBound), LP finiteness, and Puffin sidecar round-trip/robustness properties all held across 100,000 cases.

---

## Secondary Findings

1. **Regression replay is broken for integration-test binaries.** `SourceParallel` cannot locate the regressions file from a `tests/foo.rs` binary. The existing `property_lpbound.proptest-regressions` file is present and correct but is never loaded. The lp_solver bug effectively becomes invisible to CI unless a future change to proptest's persistence mode (e.g., switching to `FileFailurePersistence::WithSource` explicitly, or adding a `proptest_config!` with an explicit path) is made.

2. **Performance is well within budget.** Total wall time for all four property files at 100k cases: 162.7 s (~2.7 min). The lp_solver tier is the slowest at 85.2 s (LP compilation + 100k solve attempts). No scaling to 50k was required.

3. **No new sketch correctness failures.** The Bloom false-positive rate test (single deterministic run over 100k probes) held at ≤8× target tolerance. CMS never-undercount and classical error-bound properties held. HLL merge/monotonicity/round-trip properties held.

---

## Overall Verdict

**PASS** (27 property tests × 100,000 cases each, plus full unit/integration suite).  
No new failures. The one known bug — `LpJoinBound` ceiling exceeds `AgmBound` by ~0.19% for `rows=[4058,534,4051], preds=[(1,2)]` — is documented and carries over unchanged to this tier. Fix is deferred to the dedicated fix-it task.
