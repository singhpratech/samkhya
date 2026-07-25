# samkhya — social thread (ready to post)

**Primary target:** Bluesky. The same copy mirrors to X/Twitter verbatim — no
per-platform rewrite needed (every post is under 300 chars, which fits both
Bluesky's 300-char limit and an X thread). Post the thread top-to-bottom as a
single reply chain.

**Anchor link:** the personal article (`ARTICLE.md`, self-hosted under
Prateek's own name/domain or dev.to — **not** theaivibe.org). The code link is
the **repo root** `https://github.com/singhpratech/samkhya` (the repositioned
README), never a release-candidate tag.

**Hook:** the falsification is the asset. Lead with the honest miss, in the
lineage of Leis et al. *"How Good Are Query Optimizers, Really?"* (PVLDB 2015)
and *"Are We Ready for Learned Cardinality Estimation?"* (PVLDB 2021) — in this
subfield the most-respected result is the honest one.

**Tone rules baked in:** no number-led hype, no "AI database" framing, no
mass-tagging. Branding stays *portable / feedback-driven / self-correcting*.

---

## The thread (5 posts)

### 1/5 — the falsification hook

> I set out to make embedded query engines count rows better. I pre-registered
> a >=1.35x end-to-end speedup on the real Join-Order Benchmark (real IMDb),
> then measured 1.038x. So I shipped the falsification, not a spin. What I
> learned building samkhya, an open Rust SDK: [1/5]

*(274 chars)*

### 2/5 — durable contribution #1: never-regress at the bound level

> 1. A never-regress guarantee at the bound level. Every corrected row-count
> estimate is clamped under a provable LpJoinBound ceiling, so a miscalibrated
> model (or a hallucinating LLM) can never push the optimizer past a bound it
> can prove. No feedback yet? Cold start = engine native. [2/5]

*(289 chars)*

### 3/5 — durable contribution #2: Puffin cross-engine portability

> 2. Write the stats once, read them everywhere. One Iceberg Puffin sidecar,
> written by a Python ELT job, is read unchanged by both DataFusion and DuckDB.
> No engine owns the stats; the sidecar does. The single-sidecar / two-engines
> / one-experiment demo is the next receipt I want to land. [3/5]

*(293 chars)*

### 4/5 — durable contribution #3: the Corrector framework

> 3. One Corrector trait, many backends: fn correct(&self, &CorrectionFeatures)
> -> Result<Option<u64>>. Default is a sub-MB gradient-boosted tree. TabPFN-2.5
> and an LLM-over-HTTP slot are opt-in. Swap the model, keep the bound-level
> safety envelope. [4/5]

*(253 chars)*

### 5/5 — closing: what I'd love feedback on

> What I would love feedback on: does the never-regress-at-the-bound framing
> actually hold up against your workload, and which engine adapter is closest
> to what you run? The honest failure-mode catalogue (where samkhya is ~5%
> slower) is in the repo. Article + code linked below. [5/5]

*(282 chars)*

---

## Links to attach to post 5/5 (or as the first reply after it)

- Article (self-hosted under Prateek's name): `<ARTICLE_URL>` — replace with
  the canonical self-hosted URL once `ARTICLE.md` is published (own
  domain or dev.to; **not** theaivibe.org).
- Code: https://github.com/singhpratech/samkhya  (repo root, not a tag)

> Note on link placement: on Bluesky, a bare URL in the post body auto-cards;
> on X, putting both links in the same post can suppress reach, so prefer
> attaching the article URL to 5/5 and dropping the repo link as the first
> reply.

---

## Who to engage — sparingly, and why

Reply-engage **DB / systems researchers and embedded-engine maintainers**, not
a broad tech audience: people who work on cardinality estimation, query
optimization, DataFusion / DuckDB / Polars internals, or Iceberg stats. They
are the readers for whom "I pre-registered >=1.35x and shipped the
falsification" reads as a credibility signal rather than a letdown — the exact
audience that respects the Leis / "Are We Ready for Learned CE?" lineage.

Engage **a few, individually, with substance** (a real question or a pointer to
the failure-mode catalogue). Do **not** mass-tag, do not @-pile a list of big
names, do not chase reach with the 40.95x or 31.15 ms numbers out of context —
those are scoped microbenchmark / latency figures, never speedups. Let the
honesty be the thing that travels.

---

## Guardrails (do not violate when editing this thread)

- The 1.038x is the **only** end-to-end real-workload speedup number, and it is
  a **miss** against the pre-registered >=1.35x. Never present it as a win.
- 40.95x is a **bound-tightness ratio vs the AGM bound on a synthetic star-5
  microbenchmark**, not a wallclock speedup. Keep it out of the lead; if it
  ever appears, label it as bound-tightness and note it collapses to 1.00x on
  heavy-hitter cells.
- The never-regress guarantee is **bound-level**, not wallclock — samkhya can
  and does regress on wallclock (cross-pattern geomean 0.949x, ~5% slower;
  worst cold-start cell +12.4%). The failure-mode catalogue is a credibility
  asset; link it, don't hide it.
- The Puffin two-engine demo is the **next** receipt (format round-trip +
  per-engine tests are green; the single-physical-sidecar / two-engines /
  one-experiment run is named as upcoming, not claimed as measured).
- TabPFN-2.5 and the LLM-over-HTTP corrector are **opt-in backends**, not the
  default and not "an AI database."
- Attribution: LpJoinBound is *inspired by* Zhang et al., LpBound (SIGMOD 2025
  Best Paper) — inspired by, not a reimplementation. If the article expands on
  it, keep that wording.
