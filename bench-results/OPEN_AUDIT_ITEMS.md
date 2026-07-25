# Open audit items — 2026-07-24

**Author:** Prateek Singh (sole)
**Opened by:** the 1.2.0 deep-dive audit that produced
[`20_bound_soundness.md`](./20_bound_soundness.md)

The same audit that found and fixed the bound-soundness defect raised questions
about other published measurements that **1.2.0 does not answer**. They are
recorded here rather than quietly left in place, because a project whose selling
point is honest measurement does not get to sit on unverified doubts about its
own receipts.

## How to read the status column

| Status | Meaning |
| ------ | ------- |
| **CONFIRMED** | Independently reproduced in this repo. The claim it affects is corrected. |
| **CREDIBLE, UNVERIFIED** | Raised by a code read that cites specific lines. Not reproduced. The affected claim is flagged, not withdrawn — withdrawing a measured number on the strength of an unreproduced reading would be its own kind of dishonesty. |
| **OPEN** | A known gap, no dispute about the facts. |

Nothing below is presented as a corrected number. Where a headline is in
question, the honest position is "in question", and it is stated as such.

---

## 1. Bound family — CONFIRMED and closed

Fully reproduced, repaired, and re-measured. See
[`20_bound_soundness.md`](./20_bound_soundness.md). Retained here only so the
register is complete.

---

## 2. JOB-Slow campaign — CONFIRMED, and it is not what it says it is

**Status: CONFIRMED.** Every claim below was reproduced against the committed
raw artefacts in `bench-results/wave4f_raw/` on 2026-07-24. This section replaces
the earlier "credible, unverified" framing, which understated it.

### 2.1 The "corrected" arm contained no corrector

`run_all_trials.sh` and `run_trial.sh` invoke:

```
samkhya-bench run --suite job-slow-real [--baseline] --imdb-dir ...
```

There is no `--corrector` flag in either script, so the runner takes
`CorrectorArg::None` and attaches nothing (`samkhya-bench/src/main.rs:290`).
And the flag has only two values — `None` and `Identity` — where `Identity`
passes the baseline through unchanged. **The bench CLI has no option that
attaches a trained corrector at all.**

The two arms therefore differ by exactly one thing: whether
`SamkhyaTableProvider` injects portable Puffin sidecar statistics into the
planner. **The flagship result is a portable-statistics result, published under
a cardinality-correction headline.**

That is a real finding and arguably a better one — better statistics from a
portable sidecar, no model anywhere, measurably moving a real workload. It has
to be published as what it is.

### 2.2 All four trials were OOM-killed at 55 of 113 queries

Every `*.meta.json` records `"exit": 137`. Every trial log ends at query `15d`,
and every log opens with `runner: executing 113 job-slow-real queries`. The
n=55 is not a designed sample — it is where the process was killed. The
unexecuted remainder includes 17c through 33c, the queries that stress
cardinality estimation hardest.

### 2.3 Run-order confound — CONFIRMED, and it is most of the effect

The loop is `for mode in baseline corrected`, fixed order, no randomisation and
no interleaving, so the coldest run in the campaign sits in the baseline arm.
Recomputing the geomean with that trial dropped:

| Arm selection | geomean speedup |
| ------------- | --------------: |
| as published (both trials each arm) | **1.0384×** |
| warm baseline t2 vs mean corrected | **1.0127×** |
| warm baseline t2 vs corrected t2 (like-for-like) | **1.0136×** |

Roughly two thirds of the published effect is arm-order. Note the mechanism is
subtler than the elapsed times suggest: summed query time across the 55 paired
queries is 145.9 s / 140.2 s / 138.7 s / 138.7 s, so the cold trial is only ~4%
slower on query time — but when the whole claimed effect is 3.8%, a 4% arm-level
offset dominates it.

### 2.4 Per-query inference is not attainable at n=2

Every one of the 55 entries has `(baseline_n_trials, corrected_n_trials) ==
(2, 2)`. Across all 55 queries there are only 20 distinct
`bootstrap_p_one_sided` values, exactly 24 are literally `0.0`, and exactly
those 24 are the 24 `bh_fdr_reject == true` entries. At 2-vs-2 under
exchangeability the smallest attainable exact p is `1 / C(4,2) = 0.167`, so **a
correct per-query test rejects nothing at α = 0.05.** The "BH rejects 24/55"
claim is an artefact of a degenerate bootstrap and should be replaced by an
explicit "per-query inference not attainable at n=2".

### 2.5 The campaign publishes no cardinality evidence

`baseline_qerror_geomean` and `corrected_qerror_geomean` are `1.0` for all 55
queries and in the aggregate. Every JOB query is `SELECT MIN(...)`, so the root
cardinality is always 1 and the whole-query q-error is structurally pinned at
1.0. **The flagship campaign contains zero evidence about estimation accuracy,
which is the thing samkhya exists to improve.** Per-join-node q-error is the
metric that would carry signal; the collection machinery exists in
`runner.rs` but is not aggregated.

### 2.6 What follows

The 1.038× geomean, its BCa CI, the Wilcoxon p, the "0 losses", and the "BH
rejects 24/55" should not be quoted as a cardinality-correction result. The
defensible statement from this data is narrower and needs its own re-run:
*injecting portable sidecar statistics into DataFusion moved a truncated
55-query JOB-Slow subset by about 1%, with per-query significance not
attainable at the trial count used.*

Required before any of it is promoted again: a `--corrector` option that
attaches a real corrector; randomised or interleaved arm order; enough memory
headroom (or small enough working set) to finish all 113; at least 5 trials per
arm; and per-join-node q-error instead of the root metric.

---

## 3. Failure-mode catalogue provenance

**Status: CREDIBLE, UNVERIFIED.**

`17_failure_modes.md` reportedly carries a self-declared placeholder marker while
the numbers it contains — the 0.949× mixed workload and the +12.4% cold-start
regression — are cited elsewhere as measured.

Both are *unfavourable* to samkhya, so the risk runs the opposite way to usual:
the project may be publishing pessimistic figures it has not earned. Still a
provenance defect. **Resolution:** confirm each against a receipt or relabel it
as projected.

---

## 4. LpJoinBound is not exercised end to end

**Status: OPEN.**

No measurement in this repo runs the ceiling inside a query plan and reports its
effect on plan quality. The clamp is unit-tested and property-tested; its
*value* to a real optimizer is unmeasured. 1.2.0 adds a derived per-input
ceiling to the DataFusion adapter (`PreJoinCorrectionOptions::derive_ceiling`),
which makes the guarantee present by default for the first time — and that
change is likewise unmeasured.

This is the single largest hole in the empirical story. The bound is the
project's central contribution and no end-to-end number attaches to it.

---

## 5. Ablation L4 "recovery" and file 16 statistics

**Status: CREDIBLE, UNVERIFIED.**

Two reports, both about provenance rather than arithmetic:

* the L4 v3 recovery result — the basis for the v1.0 production deployment
  recommendation — may be evaluated on the data it was trained on;
* `16`'s statistics may be computed from `15`'s raw data, i.e. from a different
  experiment.

**Resolution:** trace both to their raw inputs. Until then the A2-only deployment
recommendation stands, since it is the conservative choice either way.

---

## 6. TPC-H presented as projected where a measured null exists

**Status: CREDIBLE, UNVERIFIED.**

A measured TPC-H null result reportedly exists in the receipts while summary
documents still describe TPC-H as projected ~2×. If so the projection should be
deleted and the null published — a measured null beats a projection, always.

---

## 7. Reproduction commands

**Status: OPEN.**

`REPRODUCIBILITY.md`'s documented commands for the two headline results are
reported not to run as written. Cheap to fix, and it is the first thing a
sceptical reader tries.

---

## 8. Bloom sizing receipts predate the fix

**Status: CREDIBLE, UNVERIFIED.**

`04_bloom_fpr_validation.md` carries a FAIL 0/16 verdict measured against the
pre-fix sizing formula, and `EVIDENCE.md` reportedly still publishes the pre-fix
geometry as analytically correct. The code was fixed; the receipts were not
re-run. **Resolution:** re-run the sweep. It is fast and needs no external data.

---

## Priority

1. §2 — CONFIRMED. The headline is mislabelled, the corpus is truncated, the
   per-query statistics are void, and there is no cardinality evidence. Nothing
   else matters until this is re-run honestly.
2. §4 — the central contribution has no end-to-end measurement.
3. §7 and §8 — cheap, self-contained, no external data.
4. §3, §5, §6 — provenance tracing.
