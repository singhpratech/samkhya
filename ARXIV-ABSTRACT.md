# arXiv submission package — samkhya (cs.DB)

**Author:** Prateek Singh (Independent, sole author)
**License of the artifact:** Apache-2.0
**Repository (link the ROOT, never a release tag):** https://github.com/singhpratech/samkhya
**Companion technical report:** `paper/TECHNICAL_PAPER.md` (~13k words, v1.0.0)

---

## 1. Proposed title

> **samkhya: Portable, Feedback-Driven Cardinality Correction with a Never-Regress Bound — and an Honestly Falsified Speedup**

Alternative, slightly more conventional systems-track phrasing if the em-dash title reads as too informal for the listing:

> **samkhya: Portable, Feedback-Driven Cardinality Correction for Embedded Analytical Engines, with a Provable Never-Regress Bound and a Pre-Registered Falsification**

Both put the four defensible contributions (portable / feedback-driven / never-regress / honest-evaluation) in the
title and signal the falsification up front, which is the hook — in the lineage of Leis et al. "How Good Are Query
Optimizers, Really?" (PVLDB 2015) and "Are We Ready for Learned Cardinality Estimation?" (PVLDB 2021), the honest
result is the respected result.

---

## 2. Abstract (≈225 words)

> Embedded analytical engines — DataFusion, DuckDB, Polars — inherit two blind spots from the server-class systems
> they forked from: the optimizer cannot carry statistical knowledge between sessions or between engines, and it has
> no mechanism to bound the damage when a cardinality estimate diverges from reality. We present **samkhya**, an
> engine-agnostic Rust SDK for portable, feedback-driven cardinality correction that addresses both without forking
> any host engine, and we contribute four things. **(1) A never-regress guarantee at the bound level:** every
> corrected estimate is clamped under a provable LpJoinBound ceiling (inspired by Zhang et al., SIGMOD 2025 Best
> Paper), so a miscalibrated model or a hallucinating LLM can never push the optimizer past a bound it can prove; with
> no feedback, cold start falls back to the engine's native estimate. **(2) Cross-engine Puffin portability:** one
> Iceberg Puffin statistics sidecar, written once, is read unchanged by both DataFusion and DuckDB. **(3) A
> one-`Corrector`-trait, many-backends framework** spanning a gradient-boosted-tree default, an opt-in TabPFN-2.5
> foundation-model backend, and an LLM-pluggable HTTP backend. **(4) The measurement rigor itself as a first-class
> artifact:** pre-registered interval hypotheses with kill-criteria, BCa bootstrap, Wilcoxon signed-rank, BH-FDR, and
> q-error. On the real IMDb Join-Order-Benchmark (JOB-Slow, n=55 paired vs. unmodified DataFusion) samkhya yields a
> **1.038x** geomean speedup (BCa 95% CI [1.026, 1.056]; 17 wins / 38 ties / 0 losses) — our pre-registered >=1.35x
> target is reported plainly as **FALSIFIED**. samkhya ships as a 13-crate Apache-2.0 workspace with a published
> Python wheel and a ~90-minute artifact-evaluation reproduction path.

**Word count:** ~225 (inside the 150–250 target). Drop sentence 2 of contribution (4) to land near 200 if arXiv's
length renders long.

### Guard-rails honored in the abstract (do not violate on edits)

- The **40.95x is deliberately NOT in the abstract.** It is a *bound/truth tightness ratio* on a **synthetic**
  star-5, p=1 (uniform-skew) microbenchmark — not a wallclock speedup. It collapses to 1.00x under p=2/p=inf
  heavy-hitter cells and saturates at size-7 cliques. If a reviewer asks for it, present it in the body (§6.6 of the
  report) as bound tightness with that ceiling caveat, never as a headline speedup. (NOTE: the report's own §Abstract
  currently mislabels it "40.95x wallclock speedup over a hand-rolled AGM implementation"; that phrasing is wrong and
  should be corrected to bound-tightness language before any arXiv body upload — flagged below.)
- The headline real-workload number is **1.038x**, stated with the FALSIFIED pre-registration, in the same breath.
- Never-regress is stated **at the bound level**, not as wallclock-never-worse (the mixed/adversarial catalogue shows
  cells up to +12.4% cold-start and a cross-pattern geomean ~0.949x; wallclock can regress).
- The single-sidecar / two-engine / one-experiment demo is named as the obvious next receipt, not claimed as already
  measured, so the abstract says "read unchanged by both," which the format round-trip + per-engine tests support.
- Branding stays **portable / feedback-driven / self-correcting**; ML/foundation-model/LLM backends are described
  technically but never used as the framing. No "learned" / "adaptive" / "AI."
- TabPFN-2.5 is named as **opt-in**, not the default.

---

## 3. arXiv categories

- **Primary:** `cs.DB` (Databases) — this is squarely query optimization / cardinality estimation.
- **Secondary (cross-list), in priority order:**
  - `cs.PF` (Performance) — the benchmarking-methodology and BCa/Wilcoxon/BH-FDR rigor is a genuine cs.PF
    contribution; strongest secondary.
  - `cs.LG` (Machine Learning) — justified only by the pluggable corrector backends (GBT / TabPFN-2.5 / LLM). Include
    it because it widens the ML-for-systems audience, but it is the weaker of the two; if arXiv pushes back on too
    many cross-lists, keep `cs.PF` and drop `cls.LG`.

**MSC / ACM-class fields** (optional metadata arXiv allows): ACM classes `H.2.4` (Systems — Query processing) and
`H.2.8` (Database Applications). Leave the MSC field blank (this is a systems/methods report, not a math paper).

---

## 4. Endorsement — what a first-time cs.DB submitter needs to line up

arXiv requires a **first-time submitter to an archive who is not auto-endorsed to be endorsed** before the initial
submission to a category will post. cs.DB sits under the Computer Science archive, whose endorsement domain typically
asks for an endorser who has themselves recently published several papers in cs within a defined window. As an
independent author with no `.edu`/institutional affiliation and no prior arXiv history, Prateek should expect to need
an endorsement and should request a code early — arXiv emails a per-author, per-archive **endorsement code** and a
link the moment a submission to a new category is started; that code is what an endorser enters. Good endorser
candidates, in descending ease: (a) any co-author or collaborator already publishing in cs.DB — none here, since this
is sole-authored, so skip; (b) an academic or industry researcher in the cardinality-estimation / query-optimization
community whom Prateek can reach directly (authors of the cited LpBound, JOB, Bao/AutoSteer, or "Are We Ready"
lines are the natural fit, since the paper engages their work substantively and honestly) — send a short, specific
note with the title, the abstract above, and the GitHub root link, and ask whether they'd be willing to endorse for
cs.DB; (c) a maintainer or contributor in the Apache DataFusion / Arrow / Iceberg ecosystem who already has cs.DB
arXiv standing. Endorsement is a statement that the paper is *appropriate for the category*, not peer review, so a
brief honest request that leads with the falsification-as-asset framing is more likely to land than overclaiming.
Allow several days of lead time, and have the PDF ready so the endorser can skim it. If no endorser materializes,
the fallback is a co-listing under a category where Prateek may already be auto-endorsed via a prior submission, or
asking the moderators directly — but lining up a real cs.DB endorser is the clean path and worth doing first.

---

## 5. What to trim from the 13k-word `TECHNICAL_PAPER.md` to make a tight systems/methods report

The full report is a deliberately exhaustive v1.0.0 companion. For an arXiv systems/methods paper (target ~10–12
pages, ~7–8k words), trim toward the four contributions and the honest evaluation, cutting depth that belongs in the
repo docs:

- **Collapse the per-sketch implementation tutorials (§2.2, §5.1–§5.6).** HLL/Bloom/CMS/equi-depth/2D are classical
  with well-known bounds; keep one short "we ship five classical sketches under a uniform `Sketch` trait with stable
  on-disk KIND tags" paragraph plus a citation row, and move the textbook accuracy-bound derivations and the
  `1.04/sqrt(2^p)` / `(1-e^{-kn/m})^k` formulas to an appendix or the repo. Reviewers do not need the Flajolet
  envelope re-derived inline.
- **Demote the bug-fix narratives (B04 Bloom sizing constant, H02 unbounded alloc, H04 from_bytes invariant gap,
  B07 doc-comment correction) into a single short "engineering discipline" subsection or an appendix.** They are
  excellent credibility signals but currently consume prose across §2.2, §5.2, §5.3, §6.4; one compact paragraph
  ("proptest at 100k iterations and ~31M fuzz execs caught the following pre-release defects; all fixed in v1.0.0")
  preserves the signal at a fraction of the length.
- **Cut the security / supply-chain / fuzzing engineering section (§18) down to two or three sentences.** The
  CRITICAL/HIGH/MEDIUM review tally, Puffin caps, cargo-deny triage, and license-consolidation rationale are
  repo/SECURITY.md material; a systems paper needs only "the artifact passes cargo-deny, ~31M fuzz execs with 0
  crashes, and a documented security review" with a pointer.
- **Compress the cross-engine adapter catalogue (§11) and state the engine matrix honestly in one table.** Keep
  DataFusion (production, three-layer) in full because it carries the measured result; reduce DuckDB/Polars/Postgres/
  Arrow/GPU/Python to a single "production / beta / scaffold" maturity table so the paper does not imply uniform
  production coverage. The pgrx double-gate, pyo3 edition pin, and DuckDB issue #11638 details are roadmap, not
  results.
- **Merge the duplicated results prose (§6.6 ≈ §13.2, §10.5 ≈ §13.6, §13.11 ≈ §13.6) and trim §13's per-campaign
  walkthrough.** The headline cells (JOB-Slow 1.038x, LpJoinBound tightness with its collapse caveat, TabPFN P95
  31.15 ms / 7.84% q-error, ablation L1–L5) belong in the body; the synthetic S1–S10 suite, TPC-H 22-query table,
  memory profile, and per-cell sweeps can become a compact results table plus an appendix.
- **Reduce the related-work expansion (§19, "40-entry version") to a focused ~15-citation §2.** Lead with the four
  anchors the thesis rests on — Leis 2015 (JOB), Wang 2021/2022 ("Are We Ready"), Zhang 2025 (LpBound), Bao/AutoSteer
  — and the methods citations (Moerkotte q-error, Efron–Tibshirani BCa, Wilcoxon, Benjamini–Hochberg). The full
  learned-CE roll-call (Naru/NeuroCard/MSCN/DeepDB/BayesCard/FLAT/FACE/ALECE/ByteCard) can be one sentence with
  grouped citations rather than a per-system table.

Optional further trim if length still runs over: fold §3 (design philosophy / non-goals) and §15 (threats to
validity) into shorter forms — the five non-negotiables can be a tight bulleted list, and threats-to-validity can
drop to the two that actually bite (single-machine external validity; warm-cache/CSV-not-Parquet construct validity,
which also explains the falsification).

---

## 6. Pre-upload flag (correctness, not stylistic)

Before any arXiv **body** PDF is built from `TECHNICAL_PAPER.md`, fix the one factual mislabel: the report's own
§Abstract and §1.2/§13.2 cross-references describe the 40.95x as a "wallclock speedup over a hand-rolled AGM
implementation." Per the verified record it is a **bound/truth tightness ratio** on a synthetic star-5 p=1
microbenchmark that collapses to 1.00x under p=2/p=inf and saturates at size-7 cliques. Present it everywhere as
bound tightness with that ceiling caveat — never as a speedup. This arXiv abstract already does so; the body must
match before submission so the listing and the PDF are internally consistent.
