# samkhya — visibility / repositioning plan (2026-06-20)

> Goal: **portfolio / visibility credibility** (Prateek's name on a credible, respected artifact —
> not a user base, not installs). Distribution: **seed the communities**. Positioning:
> **lead with the honest story**, demote the 40.95× synthetic hero number.
> Source: workflow `wf_ebc3d122-4b6` (7 agents). Status of each item marked below.

## The thesis

The strongest visibility asset is the thing the old README hid: **you pre-registered a ≥1.35×
end-to-end target and honestly shipped the falsification (1.038×).** That is rare and respected.
It sits in the explicit lineage of the most-cited, most-respected work in cardinality estimation —
Leis et al. *"How Good Are Query Optimizers, Really?"* (PVLDB 2015) and *"Are We Ready for Learned
Cardinality Estimation?"* (PVLDB 2021) — both honest/negative results. Lead with the falsification +
the things that **do** hold up: the never-regress `LpJoinBound` clamp, cross-engine Puffin
portability, the one-`Corrector`-trait framework, and the measurement rigor itself.

## Lede variants (README)

- **A — Never-Regress First** *(APPLIED to README.md:3)* — leads with the provable never-worse
  guarantee + Puffin portability + framework; states 1.038× / ≥1.35× FALSIFIED; demotes 40.95×.
- **B — Falsification First** — opens on "I pre-registered ≥1.35×, measured 1.038×, shipped the
  falsification." Best when the README and the article must rhyme. (This is the article's voice.)
- **C — Portability First** — opens on "one Puffin sidecar written by a Python ELT job, read
  unchanged by DataFusion and DuckDB." Best single framing for the DuckDB/DataFusion communities.

## The article (durable anchor link)

**Working title:** *"I pre-registered a 1.35× speedup for my cardinality-correction SDK. It came
in at 1.038×. Here is the honest writeup."*
**Angle:** falsification-as-headline in the Leis-2015 lineage; body turns to what holds up.
**Republish under Prateek's own name** (personal domain / dev.to), **NOT theaivibe.org**.
Reuse `MEDIUM-DRAFT.md` with five corrections: (a) fix the `Corrector` trait signature to the real
`correct(&self, &CorrectionFeatures) -> Result<Option<u64>>`; (b) soften "SIGMOD 2025 Best Paper" →
"inspired by Zhang et al., SIGMOD 2025"; (c) cut the §8 cold-cache speculation (corrected arm never
ran — present as open question); (d) one test-count figure repo-wide; (e) drop the AI-Vibe provenance.

Outline: TL;DR · 1 the problem nobody owns (Leis/Moerkotte + Puffin gap) · 2 what samkhya is ·
3 the never-regress guarantee made provable · 4 the 40.95× microbenchmark, honestly scoped ·
5 the pre-registered hypothesis I shipped falsified · 6 where samkhya LOSES (failure-mode catalogue) ·
7 how I measured it (the rigor as the product) · 8 shipped vs deferred · 9 what I'd love feedback on.

## Channel ladder

| When | Channel | Framing | Payoff |
|---|---|---|---|
| Day −3→0 | **Prep gate** | Repositioned README + compiling snippets + `honest_demo` + asciinema; reconcile counts; fix stale ANNOUNCE | Landing page survives a skeptical click-through |
| Day 0/+1 | **arXiv cs.DB** | "Portable, feedback-driven CE with a never-regress bound — and an honestly falsified speedup." 4 defensible contributions + honest eval in the abstract | **Highest durability**: citable, indexed, name-attached |
| Day 0 (a few hrs pre-HN) | **Personal long-form** | The falsification writeup, self-hosted under your name | The anchor link everything else points at |
| Day 0 | **Show HN** | "Show HN: samkhya — portable, feedback-driven cardinality correction for DataFusion/DuckDB/Polars (Rust)". **No number in title.** Honest first comment names one limitation up front | Spike; real win = one respected DB/Rust person in-thread |
| Day 0 | **Bluesky (+X mirror)** | Falsification-hook thread anchored on the writeup | Asymmetric; near-zero downside if honesty holds |
| Day +1 | **r/rust** | Design-led: one trait / many backends, LpBound clamp as a type-level safety property, fuzz/MIRI rigor | Reliable tens-to-low-hundreds; design-level comments |
| Day +1/+2 | **lobste.rs** | The honest-results writeup, neutral title, `show` tag (needs invite) | Low volume, high signal — senior systems/DB/PL |
| Day +3+ | **DataFusion** (#datafusion + GH Discussion) | "I built an LpBound-clamped optimizer rule on the DF46 stats API — honest end-to-end measurement" | Possibly **highest-value**: audience calibrated to the contribution |
| Day +3+ | **DuckDB** (#extensions + GH Discussions) | Cross-engine Puffin sidecar; transparent about #11638 LOAD blocker | Medium-high credibility; months-long relationship |
| After arXiv | **DBTest** (primary) + **aiDM@SIGMOD** (secondary) | Methodology/falsification → DBTest; Corrector framework → aiDM | **Gold-standard** peer-reviewed, citable CV line |

Not a marketing channel: the ASF `dev@` mailing list (release-coordination only).

## Remaining README / repo edits (TODO)

1. Soften "Zhang et al., SIGMOD 2025 **Best Paper**" → "inspired by Zhang et al., SIGMOD 2025" (match
   `lpbound.rs:3`); same in MEDIUM-DRAFT + paper.
2. State the **corrected** bound partial order: Product ≥ {Chain, AGM} ≥ LpJoin (B07), NOT a strict
   chain; clamp never-regresses (total), strict-tightness-over-AGM is partial.
3. Reconcile **test count** to one figure repo-wide (recommend "284 unit + 17 property across 51
   binaries, 0 failures").
4. Reconcile **crate count** on-page: "13-crate workspace, 10 published to crates.io".
5. Fix stale `ANNOUNCE.md` (says "Apache-2.0 OR MIT"; project is Apache-2.0 only since 2026-05-17;
   still references rc.1 not v1.0.0).
6. Add `examples/honest_demo.rs` (never-regress LpJoinBound numbers + Puffin round-trip via the real
   API, CI-asserted) + ~90s asciinema embedded in README.
7. Point newcomers to the runnable surfaces (`pip install samkhya`; `cargo install samkhya-cli` +
   sample CSV); soften the hero engine-list to match the honest production/beta/scaffold matrix.

## Prioritized actions

1. **[DONE]** Repositioned README hero + demoted 40.95× + reordered honest headlines.
2. **[DONE]** Fixed the two broken quick-start snippets to the real public API (verified vs source).
3. Honesty/consistency sweep (TODO 1–5 above). *Effort S.*
4. `examples/honest_demo.rs` + asciinema. *Effort M.*
5. Republish corrected MEDIUM-DRAFT under your own name. *Effort M.*
6. arXiv cs.DB report (line up an endorser first). *Effort M.*
7. Launch day: Show HN + personal writeup + Bluesky/X; Day +1 r/rust + lobste.rs. *Effort M.*
8. Sustained DataFusion / DuckDB community engagement. *Effort L.*
9. DBTest + aiDM@SIGMOD submissions (check deadlines now). *Effort L.*
