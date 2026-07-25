# Show HN submission — samkhya

This file is the ready-to-post submission. Copy the title into the HN
title box, the URL into the URL box, and post the first comment yourself
immediately after submitting.

---

## TITLE (exact — paste verbatim, no speedup number)

```
Show HN: samkhya — portable, feedback-driven cardinality correction for DataFusion/DuckDB/Polars (Rust)
```

## URL (the repo ROOT — not a release tag, not theaivibe.org)

```
https://github.com/singhpratech/samkhya
```

---

## FIRST COMMENT (post this as the maker, immediately after submitting)

I build embedded analytical engines, and the recurring pain is that the
query optimizer is only as good as its cardinality estimates — and on
the embedded tier (DuckDB, DataFusion, Polars) there's no long-lived
optimizer process to amortize a big learned model, so most of the
2018–2022 learned-cardinality work doesn't fit. samkhya is my attempt at
a stats-correction layer that does fit: a portable Iceberg Puffin stats
sidecar plus a feedback-driven corrector, with a hard safety property
underneath.

The design is one Rust trait, many backends. The whole corrector surface
is a single method:

```rust
fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>>;
```

Behind that trait I ship a gradient-boosted-tree backend as the default,
TabPFN-2.5 as an opt-in foundation-model backend, and an LLM-pluggable
HTTP backend (canonical Python FastAPI server, plus a Node TypeScript
port with the same wire contract). Swapping the backend is a config
change, not an engine re-integration — the DataFusion/DuckDB/Polars
adapters never know which corrector is behind the trait.

The load-bearing guarantee is *never-regress at the bound level*. Every
corrected estimate is clamped under a provable `LpJoinBound` ceiling
(inspired by Zhang et al., LpBound, SIGMOD 2025 Best Paper — samkhya is
inspired by it, not a reimplementation). So a miscalibrated GBT or a
hallucinating LLM can never push the optimizer past a cardinality it can
*prove* is an upper bound; and with no feedback yet, cold start falls
back to the engine's own native estimate. The safety is at the bound
level — not wallclock. Which brings me to the honest limitation.

One limitation up front: on the real Join-Order Benchmark (JOB-Slow, the
actual IMDb dump, n=55 paired warm-cache queries vs unmodified
DataFusion 46), the end-to-end geomean is **1.038×** wallclock — 17 wins
/ 38 ties / 0 losses, BCa 95% CI [1.026, 1.056], Wilcoxon p=3.00e-6,
BH-FDR rejecting 24/55. It's statistically real but small, and I had
**pre-registered ≥1.35×** end-to-end as the target. That target is
**falsified.** I filed those interval hypotheses with kill-criteria in
DEFENSE.md *before* the IMDb dump was even on the machine, and the
attributions for why the effect is small are named, not hand-waved
(warm-cache only, CSV re-parse dominating the I/O floor, an OOM cap that
biased coverage to easier queries, per-join-node q-error walking
deferred to v1.1).

I'm posting the framework, with the falsification stated up front,
because that's the methodology rule: in the lineage of Leis et al. "How
Good Are Query Optimizers, Really?" and "Are We Ready for Learned
Cardinality Estimation?", the most useful result in this subfield is the
honest one. I shipped the falsification because that's the methodology
rule — pre-register the target, then report what actually happened.

Repo, 13-crate workspace, 10 crates on crates.io, Python wheel on PyPI
as `samkhya`, Apache-2.0, ~90-minute ACM-Artifact-Evaluation-shaped
reproduction. I'd genuinely value feedback on whether the one-trait /
many-backends abstraction fits workloads you actually run, and which
engine adapter is closest to your stack.

---

## PREPARED RESPONSES (keep ready; reply only if the critique comes up)

**"The 40.95× — now THAT'S a real speedup, lead with that."**
It isn't a speedup, and I deliberately keep it out of the title and the
lead for exactly that reason. The 40.95× (BCa CI [30.93, 47.45],
Wilcoxon p=1.73e-6, n=30) is a *bound-tightness* ratio — how much tighter
the LpJoinBound is than the AGM bound versus ground truth — on a single
synthetic star-5, p=1 (uniform-skew) microbenchmark. It is not wallclock
and never was. It also collapses to 1.00× under p=2 / p=∞ heavy-hitter
cells and saturates at size-7 cliques, where it safely falls back to the
product bound. What it buys is a *tight* ceiling, which is what makes the
bound-level never-regress clamp useful rather than vacuous. The wallclock
number is the 1.038× one, with the falsification.

**"55 of 113 queries, with an OOM cap — isn't that cherry-picked?"**
Fair, and I'd rather you know the shape than guess it. The paired
warm-cache comparison is n=55; coverage was capped by an
out-of-memory ceiling past q16a, which biases the surviving set toward
*easier* queries — i.e. it works *against* my numbers, not for them, and
I say so in the receipts (`bench-results/EVIDENCE.md` §4.2). The fuller
STATS-CEB campaign and lifting the OOM cap are named v1.1 items in the
CHANGELOG, not quietly omitted. Even on that biased-easy subset, the
result is 0 losses — which is the bound-level guarantee doing its job.

**"What actually stops a bad corrector from wrecking my plans?"**
The LpJoinBound clamp, by construction. The corrector's output is an
input to a `min()` against a provably-correct pessimistic ceiling, so the
worst a miscalibrated or adversarial backend can do is fail to *help* —
it degrades toward the engine's native estimate, it doesn't corrupt the
planner past a bound the bound itself can prove. That's a bound-level
guarantee, to be precise: it bounds the *estimate*, not your wallclock.
The failure-mode catalogue (`bench-results/17_failure_modes.md`) shows
where wallclock does regress — a mixed/adversarial workload runs ~5%
slower cross-pattern geomean (0.949×), worst cell +12.4% cold-start, and
I did *not* retroactively widen the bound to hide it.

**"Why is a 1.038× result even worth posting?"**
Because the speedup isn't the product — the framework is, and the receipt
is the honesty. The contribution is one trait / many backends with a
provable bound-level safety floor and cross-engine Puffin portability;
the 1.038× is the measurement, reported with its CI and its falsified
pre-registration rather than rounded up into a headline. In this
subfield the respected papers are the honest-result papers (Leis et al.;
"Are We Ready for Learned CE?"). "I pre-registered ≥1.35× and shipped the
falsification" is precisely the thing a benchmark-spammer would never
write — and it's the thing I'd want to read before trusting someone
else's stats layer in my optimizer.

---

## ENGINE-STATUS NOTE (have this handy; do not overstate coverage)

If asked "does it work with my engine today" — be precise about tiers:
- **Production:** DataFusion (three-layer integration vs DF46), Iceberg
  (Puffin reader/writer), Arrow (IPC round-trip).
- **Beta:** DuckDB (staticlib+rlib scaffold; cdylib + runtime LOAD
  blocked on DuckDB issue #11638), Polars.
- **Scaffold:** Postgres (pgrx-shaped stub, double-gated, pg17 pin; real
  planner hooks are v1.1).

The cross-engine Puffin demo — one physical sidecar written by a Python
ELT job, read unchanged by *both* DataFusion and DuckDB in one
experiment — is the obvious next receipt. The round-trip and per-engine
tests are green; the single-experiment two-engine demo is named as the
next thing to measure, not claimed as already-measured.
