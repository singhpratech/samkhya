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

## 2. JOB-Slow 1.038× — possible run-order confound

**Status: CREDIBLE, UNVERIFIED.**

The audit reports that `bench-results/wave4f_raw/run_all_trials.sh` runs the
baseline and corrected arms in a fixed order rather than interleaving or
randomising them, which would let warm-up, page-cache state, or thermal drift
load onto one arm. The estimated inflation quoted was ~2.8×, which would put the
true effect near unity.

**What is not established:** I have not re-run the campaign. The IMDb corpus and
a multi-hour paired run are required, and neither happened for 1.2.0.

**Position taken:** the 1.038× geomean, its BCa CI `[1.026, 1.056]`, and the
Wilcoxon `p = 3.0e-6` remain as published, with this caveat attached wherever
they appear. The pre-registered ≥ 1.35× hypothesis was already reported as
falsified, so nothing downstream depends on the effect being real. **The
resolution is a re-run with randomised arm order, and until that happens the
honest description of the JOB-Slow result is "a small effect that may be an
artefact of run order".**

---

## 3. BH-FDR "rejects 24/55" — possible degenerate-bootstrap artefact

**Status: CREDIBLE, UNVERIFIED.**

The audit reports that `wave4f_raw/aggregate.py` bootstraps from as few as two
samples per query, which can produce a literal `p = 0.0`, which then passes any
Benjamini-Hochberg threshold trivially. If so, the rejection count measures the
bootstrap's degeneracy rather than the effect.

**Resolution:** recompute with a minimum-sample guard and a p-value floor, from
the existing raw data. This does not need a re-run and should land in 1.2.x.

---

## 4. Failure-mode catalogue provenance

**Status: CREDIBLE, UNVERIFIED.**

The audit reports that `17_failure_modes.md` carries a self-declared placeholder
marker while the numbers it contains — the 0.949× mixed workload and the +12.4%
cold-start regression — are cited elsewhere as measured.

Both numbers are *unfavourable* to samkhya, so the risk here is the reverse of
the usual one: the project may be publishing pessimistic figures it has not
actually earned. That is still a provenance defect. **Resolution:** confirm each
number against a receipt or relabel it as projected.

---

## 5. LpJoinBound is not exercised end to end

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

## 6. Ablation L4 "recovery" and file 16 statistics

**Status: CREDIBLE, UNVERIFIED.**

Two reports, both about provenance rather than arithmetic:

* the L4 v3 recovery result — the basis for the v1.0 production deployment
  recommendation — may be evaluated on the data it was trained on;
* `16`'s statistics may be computed from `15`'s raw data, i.e. from a different
  experiment.

**Resolution:** trace both to their raw inputs. Until then the A2-only deployment
recommendation stands, since it is the conservative choice either way.

---

## 7. TPC-H presented as projected where a measured null exists

**Status: CREDIBLE, UNVERIFIED.**

A measured TPC-H null result reportedly exists in the receipts while summary
documents still describe TPC-H as projected ~2×. If so the projection should be
deleted and the null published — a measured null beats a projection, always.

---

## 8. Reproduction commands

**Status: OPEN.**

`REPRODUCIBILITY.md`'s documented commands for the two headline results are
reported not to run as written. Cheap to fix, and it is the first thing a
sceptical reader tries.

---

## 9. Bloom sizing receipts predate the fix

**Status: CREDIBLE, UNVERIFIED.**

`04_bloom_fpr_validation.md` carries a FAIL 0/16 verdict measured against the
pre-fix sizing formula, and `EVIDENCE.md` reportedly still publishes the pre-fix
geometry as analytically correct. The code was fixed; the receipts were not
re-run. **Resolution:** re-run the sweep. It is fast and needs no external data.

---

## Priority

1. §2 and §3 — they touch the headline wallclock claim.
2. §5 — the central contribution has no end-to-end measurement.
3. §9 and §8 — cheap, self-contained, no external data.
4. §4, §6, §7 — provenance tracing.
