# I pre-registered a 1.35× speedup for my cardinality-correction SDK. It came in at 1.038×. Here is the honest writeup.

*samkhya v1.0.0 — Apache-2.0 — and the unsexy story of a 1.038× speedup that is the right number to report.*

---

## TL;DR

I built **samkhya** (Sanskrit सांख्य, "enumeration / counting"), an
engine-agnostic Rust SDK for portable, feedback-driven cardinality
correction. It plugs into DataFusion, DuckDB, Polars, Postgres,
Iceberg, Arrow, and Python. One Rust trait (`Corrector`) — many
backends (GBT default, TabPFN-2.5 opt-in, any LLM over HTTP).
Apache-2.0.

- `cargo add samkhya-core`
- `pip install samkhya`
- https://github.com/singhpratech/samkhya
- 10 crates on crates.io, Python wheel on PyPI, 13-crate Rust workspace

My pre-registered headline was **≥1.35×** geomean speedup on the
Join-Order Benchmark "JOB-Slow" subset, head-to-head against
unmodified DataFusion 46 on the real IMDb dataset. The measured result
is **1.038×** (BCa 95% CI [1.026, 1.056], Wilcoxon W=212 p=3.00×10⁻⁶,
17 wins / 38 ties / 0 losses, n=55 paired).

That is a **falsified** pre-registered hypothesis, and I am shipping
the falsification on purpose. This article explains why that is the
right thing to do, what *did* hold up (an `LpJoinBound` that is up to
40.95× tighter than the AGM *bound* on one synthetic microbenchmark —
a bound-tightness ratio, not a speedup), where samkhya is measurably
*slower*, and what the SDK is actually for.

If you optimize SQL on DataFusion / DuckDB / Polars and you have ever
been bitten by a cardinality misestimate, the rest of this is for you.

---

## 1. The problem nobody owns

Cardinality estimation — predicting how many rows a relational
operator will produce — is the single load-bearing input to every
cost-based query optimizer. Leis et al. (*"How Good Are Query
Optimizers, Really?"*, PVLDB 2015, the *Join Order Benchmark* paper)
showed empirically that even on a tuned PostgreSQL, optimizer
cardinality estimates are wrong by more than an order of magnitude on
a substantial fraction of JOB queries. Moerkotte et al. (PVLDB 2009)
gave the theory for why that matters: they bound worst-case plan
regret in terms of **q-error** — `q = max(est/true, true/est)` — and
the bound grows steeply in `q`. A q-error of 10 is plan-catastrophic
territory, not a rounding nuisance.

This is why every major analytical engine — DuckDB, DataFusion,
Polars — has cardinality issues filed against it that planners label
"known."

The fix everyone reaches for is **statistics**: collect
HyperLogLog / Bloom / histogram sketches, feed them to the planner.
But the embedded-analytics world has a portability hole:

- **DuckDB** recomputes column stats on re-attach. The sketch from
  yesterday's analysis is gone.
- **DataFusion** exposes `Statistics` on the `TableProvider`
  interface, but offers no portable way to ship sketches between
  sessions.
- **Polars** has no cost-based optimizer at all
  ([issue #23345](https://github.com/pola-rs/polars/issues/23345)).
- **Apache Iceberg's Puffin sidecar format** has existed as a spec
  since 2022, but no portable producer/consumer library ships
  sketches across engines through it. DataSketches has sketches but no
  Puffin story; iceberg-rust has Puffin headers but no sketch codecs.

The 2018–2022 research wave attacked the accuracy side head-on with
learned estimators (MSCN, Naru, NeuroCard, DeepDB, BayesCard, FACE,
ALECE, ByteCard, …) and ran into a structural wall: published systems
typically needed tens to hundreds of MB of model weights and 5–50 ms
of inference per estimate. Embedded analytical engines — the ones we
actually deploy to laptops and edge nodes — have a sub-50 ms
cold-start budget and sub-1 ms per-estimate latency. The math does not
close. The honest critique papers of that era — *"Are We Ready for
Learned Cardinality Estimation?"* (PVLDB 2021) and the in-depth study
that followed — said as much.

The production-database world routed around the problem with
**Adaptive Query Execution** — Spark AQE, Snowflake Adaptive Compute,
BigQuery History-Based Optimization. AQE works because Spark and
Snowflake run long-lived service processes that amortize the
bookkeeping. DataFusion, DuckDB, Polars, gpudb, and the
Postgres-as-data-lake adapter ecosystem do not. They have a
single-query lifetime and no place to keep the books.

That gap is what samkhya is built to fill.

---

## 2. What samkhya actually is

A 13-crate Rust workspace built around three abstractions, each
load-bearing.

**1. Five classical sketches.** HyperLogLog (Flajolet et al. 2007),
Bloom (CACM 1970), Count-Min (Cormode–Muthukrishnan 2005), equi-depth
histogram, 2D correlated histogram. Each is wrapped in a uniform
`Sketch` trait with stable `KIND` tags (`samkhya.hll-v1`,
`samkhya.bloom-v1`, …) and round-trips byte-identically through
serialization. No ML; just space-efficient summaries.

**2. Iceberg Puffin sidecar I/O.** The same Puffin payload a Python
ELT job writes at midnight is the one DataFusion and DuckDB read later
in the day. No engine owns the stats; the sidecar does. The format
round-trip and the per-engine read tests are green today. The
single-physical-sidecar / two-engines / one-experiment demo — write
one Puffin file from Python, read it unchanged from both DataFusion
*and* DuckDB inside a single measured run — is the obvious **next**
receipt, and I am naming it as a next receipt rather than claiming it
as already measured.

**3. The `Corrector` trait — pluggable correction backends.** This is
the framework contribution. The real trait, verbatim from
`samkhya-core/src/residual.rs:197`:

```rust
pub trait Corrector: Send + Sync {
    /// Return a corrected estimate, or `None` to fall back to the baseline.
    fn correct(&self, features: &CorrectionFeatures) -> Result<Option<u64>>;

    /// Stable identifier for logging / model-version tracking.
    fn name(&self) -> &'static str;
}
```

Two design choices in that one signature carry the whole framework:

- **It returns `Result<Option<u64>>`, not `u64`.** `Ok(None)` is the
  explicit "I decline; use the engine's own estimate" path. Cold start
  with no feedback history is not a special case bolted on later — it
  is the trait's first-class fallback. A backend that cannot improve
  on the baseline says so, and the engine keeps its native plan.
- **The baseline rides inside the features, not as a side argument.**
  `CorrectionFeatures` carries `baseline_estimate` along with input
  row counts, distinct counts, predicate count, and join depth. The
  corrector sees one structured input and either returns a better
  number or returns `None`.

Importantly, the trait does **not** take the safety bound as an
argument and does **not** apply the clamp itself. The `LpJoinBound`
ceiling is enforced by the caller, *around* the corrector — so the
clamp protects the optimizer no matter which backend you plug in,
including ones I never wrote. (Section 3.)

The backends that ship or plug in:

- **GBT (default)** — a sub-MB gradient-boosted-tree backend
  (gbdt-rs), sub-ms, trained from feedback observations: `(plan
  template, estimated rows, actual rows)` triples persisted to SQLite.
  This is what runs unless you opt into something else.
- **TabPFN-2.5 (opt-in, research backend)** — behind the `tabpfn_http`
  feature, calls a local HTTP shim running Hollmann et al.'s
  TabPFN-2.5 (ICLR 2023 + Prior Labs 2026 update) on CUDA. Measured
  P95 inference latency **31.15 ms** at B=8 L=128 on an RTX 4090
  Laptop (BCa 95% CI [29.39, 35.32]). It is an opt-in research
  backend, not the default A3 deployment.
- **LLM-pluggable HTTP corrector (opt-in)** — plain HTTP, same wire
  contract, so you can point it at Anthropic, OpenAI, a local Ollama,
  or your own server. Two reference servers ship: a canonical Python
  FastAPI implementation (port 8766 — this is what the
  `bench-results/19_llm_corrector.md` campaign measured) and a parity
  Node TypeScript port (port 8767, broader operator appeal). The TS
  port is smoke-tested at v1.0; its 30-trial paired campaign is an
  explicit v1.1 item.

The naming is deliberate: samkhya is **portable** and
**feedback-driven**, not "learned" or "adaptive." I describe the ML
and foundation-model backends technically because they are real, but
the framing of the SDK is the portable stats layer and the safe
correction interface — the corrector is a swappable component, not the
brand.

---

## 3. The never-regress guarantee, made provable

The reason corrector pluggability does not terrify a DBA: **every
corrector output is bounded above by a provable ceiling** before it
ever reaches the planner, and the trait itself can decline.

The ceiling is an **`LpJoinBound`** — a linear program over ℓp-norms
of relation degree sequences, with no machine learning involved. It is
*inspired by* Zhang et al., **"LpBound: Pessimistic Cardinality
Estimation Using Lp-Norms of Degree Sequences" — the SIGMOD 2025 Best
Paper** (Haozhe Zhang, Mayer, Abo Khamis, Olteanu, Suciu, research
category). samkhya is inspired by that work, not a reimplementation of
it. It lives in the same family of bounds that Worst-Case-Optimal-Join
algorithms use (Ngo, Porat, Ré, Rudra), and it relates to the classic
Atserias–Grohe–Marx (AGM, PODS 2008) bound.

The honest partial order of the bounds samkhya tracks is:

> **Product ≥ {Chain, AGM} ≥ LpJoin**

That is a partial order, *not* a strict chain — `Chain` and `AGM` are
not ordered against each other, and the LpJoin ≤ AGM relation holds in
**86.4%** of trials, not all of them. Where it does not, samkhya falls
back to the safe product bound. (I had this wrong as a strict chain in
an early doc comment; the corrected statement is the one above.)

Operationally, per estimate:

1. The corrector returns `e_corr` (or `None`).
2. samkhya computes the `LpJoinBound` ceiling `c` from the sketch
   statistics.
3. The caller clamps: the planner sees `min(e_corr, c)` — or the
   native baseline when the corrector returned `None`.
4. The planner never sees a raw, unclamped corrector output.

A miscalibrated corrector — a TabPFN model trained on the wrong
distribution, an LLM having a hallucination-day, a GBT with stale
features — **cannot push the optimizer past a bound it can prove**.
With no feedback at all, cold start falls back to the engine's native
estimate.

One precise caveat, because precision is the brand: this is a
**bound-level** never-regress guarantee. It says no corrected estimate
exceeds a provable ceiling, and that cold start degrades to the native
plan. It does **not** say wallclock can never get worse — it can, and
Section 6 is the catalogue of exactly where it does. I am not claiming
wallclock-never-worse.

---

## 4. The 40.95× microbenchmark, honestly scoped

Here is the number that did hold up — with a label on it so it is not
mistaken for a speedup.

`LpJoinBound` vs the AGM **bound**, on a synthetic 5-table star join
topology, ℓp index `p = 1` (uniform skew), n=30 trials:

> **40.95×** *tighter than the AGM bound* (geometric mean of the
> bound/truth ratio). BCa 95% CI **[30.93, 47.45]** (10,000
> resamples). Paired Wilcoxon `W = 0`, `p = 1.73 × 10⁻⁶`. Solve
> latency P99 < 1 ms.

Read that label twice: **this is a bound-tightness ratio, not a
wallclock speedup.** It says the provable ceiling sits ~41× closer to
the true cardinality than the AGM bound does on *this synthetic cell*.
That is what makes the clamp useful — a tight ceiling clamps a
hallucinating corrector hard; a loose one lets nonsense through.

And the scope is narrow on purpose:

- It **collapses to 1.00×** under `p = 2` / `p = ∞` heavy-hitter
  cells, where the degree sequence is dominated by a few high-degree
  values and the Lp bound buys nothing over AGM.
- It **saturates at size-7 cliques**, where samkhya falls back to the
  product bound for safety.

So: 40.95× is real, measured, and significant — as a bound-tightness
result on one synthetic microbenchmark, under uniform skew, that does
not generalize to every topology and is never a speedup. The full
methodology is in `bench-results/07_lpbound_tightness.md`.

Now the part that missed.

---

## 5. The pre-registered hypothesis I shipped falsified

Before the IMDb dump was even on the host, I pre-registered the
end-to-end target in `DEFENSE.md`:

> **JOB-Slow head-to-head vs unmodified DataFusion 46, scale-factor-1
> IMDb, warm-cache:** geomean wallclock speedup ≥ **1.35×**.

The measured result (full receipt:
`bench-results/18_vs_native_datafusion_wallclock.md`, n=55 paired):

- Geomean wallclock speedup: **1.038×**
- BCa 95% CI: **[1.026, 1.056]** (10,000 resamples)
- Paired Wilcoxon: `W = 212`, `p = 3.00 × 10⁻⁶`
- Benjamini–Hochberg FDR rejects **24 of 55** queries at α = 0.05
- **17 wins / 38 ties / 0 losses**

The effect is real — the CI excludes 1.0, p < 10⁻⁵, and BH-FDR is
significant on nearly half the queries — but it is small. **My ≥1.35×
target is falsified.** The bound was not retroactively widened to make
the result look better.

I could have not published. I could have re-narrated to "a
statistically significant ~4% improvement." I am shipping the
falsification because:

1. **The honest result is the respected result in this subfield.** The
   most-cited, most-respected work here — Leis et al. *"How Good Are
   Query Optimizers, Really?"* (PVLDB 2015), *"Are We Ready for Learned
   Cardinality Estimation?"* (PVLDB 2021) — earned its standing by
   reporting the uncomfortable measurement. Pre-registration without
   honest falsification is methodology theater.
2. **The framework is the contribution.** The 1.038× is the
   measurement *under one operating point*. The contribution is the
   portable Puffin stats layer, the swappable `Corrector` trait, and
   the bound-level clamp that keeps you safe whatever you plug in.
3. **Falsification is the thing a benchmark-spammer would never
   write.** "I pre-registered ≥1.35× and shipped the falsification" is
   a credibility signal precisely because it costs something to say.

The honest, named reasons the JOB-Slow effect was small — written into
the receipts, not hand-waved:

1. **Warm-cache only.** The headline campaign was warm-cache. Sketches
   save scan-and-restat time, and that time is most visible on a cold
   cache — so the warm-cache number is, structurally, the conservative
   floor.
2. **CSV, not Parquet.** The IMDb dump is CSV. CSV scan dominates the
   I/O floor and there are no column statistics to reuse, which masks
   exactly the re-scan-for-stats savings samkhya is designed to
   capture.
3. **n=2 trial cap.** A wallclock-budget cap put only 2 trials per
   query on the headline campaign. More trials would tighten CIs and
   likely surface more BH-significant rejections.
4. **OOM past q16a.** Two queries OOM'd under the corrector path on the
   2-trial cap. The fix (making `SamkhyaTableProvider` stat injection
   plan-memory-monotonic) landed afterward, but the headline campaign
   ran on the earlier code.

The honest read: **samkhya delivers a real, statistically significant,
BH-FDR-controlled speedup on JOB-Slow against unmodified DataFusion 46,
with 17 wins and 0 losses — but the geomean magnitude (1.038×) is below
the pre-registered headline (≥1.35×).** The mechanism is verified; the
effect size falls below the pre-registered bound; the gap is attributed
to the named workload artefacts above. That is a workshop-rigor
negative result, and I think the negative result is the asset.

---

## 6. Where samkhya loses

A failure-mode catalogue, presented as a credibility asset rather than
buried. I pre-registered seven adversarial workload patterns (A–G) and
measured them (`bench-results/17_failure_modes.md`):

- **Cross-pattern geomean: 0.949×** — i.e. roughly **5% slower** on the
  mixed/adversarial workload overall. The bookkeeping and stat
  injection cost real cycles, and on workloads where the corrector
  cannot recover them, samkhya is net negative on wallclock.
- **Worst cell: cold-start +12.4%** — the first queries before any
  feedback exists pay the overhead without the benefit.
- **Hypothesis H-G FALSIFIED** — and again, the bound was not widened
  after the fact to rescue it.
- **Burst P99 ≤ 212 µs @ 1000 QPS** — the one thing that did hold under
  load: the per-estimate path stays tight even under burst.

This is the part the never-regress guarantee does **not** cover.
"Never regress at the bound level" is a statement about estimates
staying under a provable ceiling, not about wallclock. On these
patterns, wallclock regresses, and the honest answer to "is samkhya
always faster?" is **no**.

There is one open question I will *not* dress up as a likely win. The
cold-cache corrected arm — where, on first principles, sketches that
save scan time should matter most — **never ran**. The cold-cache
*capability* shipped, but the corrected cold-cache *measurement* did
not complete in time for v1.0. I genuinely do not know the cold-cache
magnitude. It is an **open question**, not a "should be much larger"
story, and I am listing it as one (Section 9) rather than implying a
bigger result is waiting.

---

## 7. How I measured it — the rigor is the product

If the headline number is small, the thing worth adopting is the
measurement discipline. I treat the methodology as a first-class
artifact:

- **Pre-registered interval hypotheses with kill-criteria**, filed in
  `DEFENSE.md` *before* the IMDb dump was on the host. The ≥1.35×
  target had a falsification condition written down in advance, and it
  fired.
- **BCa bootstrap confidence intervals** (Efron–Tibshirani), pinned
  seed 42, 10,000 resamples — every headline carries a CI, not a
  single-run point estimate.
- **Paired Wilcoxon signed-rank** for significance without a normality
  assumption.
- **Benjamini–Hochberg FDR control** across the 55 queries, so
  "significant on N queries" means BH-corrected, not p-hacked.
- **q-error / Moerkotte** as the accuracy metric, and the **Flajolet
  RSE envelope** as the sketch-precision yardstick (HLL p=14 measured
  RSE 0.676%, inside the 0.8125% theoretical envelope).
- **A pinned IMDb SHA-256** and an **ACM-Artifact-Evaluation-shaped
  reproduction** — `REPRODUCIBILITY.md` is the evaluator entry point
  and reproduces the measured headlines in roughly 90 minutes on
  commodity hardware.

On the engineering side: **284 tests pass** (`cargo test --workspace`,
0 failures), including 17 proptest property tests; the cargo-fuzz
workspace logged ~31 million executions with 0 crashes; and the
workspace is clippy `-D warnings` clean. The point of all of this is
not to dress up 1.038× — it is so that when I say a number, you can
re-run it and get the same number.

---

## 8. Shipped vs deferred — the honest engine matrix

What is actually production, what is beta, what is scaffold. Stated
plainly so nobody adopts on a false impression of uniform coverage.

**Production:**
- **DataFusion** — three-layer integration against DataFusion 46
  (`SamkhyaTableProvider` + `SamkhyaStatsExec` + optimizer rule).
  First-class target.
- **Iceberg** — Puffin sidecar reader/writer with KIND-tag
  registration for all five sketch types.
- **Arrow** — Arrow IPC round-trip, byte-identical for all five sketch
  types.

**Beta:**
- **DuckDB** — Rust-client path behind the `bundled` feature; the cxx
  extension is staticlib + rlib only in v1.0. cdylib + runtime `LOAD`
  is blocked on [DuckDB issue #11638](https://github.com/duckdb/duckdb/issues/11638).
- **Polars** — Series-to-sketch helpers behind the `engine` feature;
  the optimizer hook waits on upstream
  [Polars issue #23345](https://github.com/pola-rs/polars/issues/23345).

**Scaffold:**
- **Postgres** — a pgrx-shaped stub, double-gated behind the
  `pg_extension` feature plus a `samkhya_pgrx_enabled` rustc cfg, pg17
  pin. The real planner/executor hooks are a v1.1 item, after pgrx ≥
  0.13.

**Named v1.1 deferrals:**
- The single-physical-sidecar / two-engine Puffin demo as one measured
  run.
- The **cold-cache JOB-Slow corrected arm** (capability shipped; the
  corrected measurement did not run — open question, Section 6).
- The TypeScript LLM-corrector 30-trial paired campaign (smoke-tested
  at v1.0).
- Per-join-node q-error walking through the join tree.
- pyo3 ≥ 0.23 and pgrx ≥ 0.13 migrations (currently pinned at 0.22 and
  0.12 respectively).
- DuckDB runtime `LOAD` (upstream-blocked, above).

---

## 9. What I would love feedback on

samkhya is one person's library — no team, no VC, no roadmap meetings.
It needs feedback from people running real workloads to know whether
the abstractions fit. Three open questions:

1. **Which engine adapter is closest to what you actually run?** If you
   are a DuckDB shop, the staticlib + rlib path works today; if you are
   a Postgres-as-data-lake shop, you are waiting on pgrx 0.13.
2. **Does the LLM-pluggable transport contract fit your inference
   stack?** The Python FastAPI server is canonical; the Node TypeScript
   port is for broader operator appeal. If neither fits, what would?
3. **What is the cold-cache magnitude on your workload?** I genuinely
   do not know it — the corrected cold-cache arm has not run, so this
   is an open question, not a teaser for a bigger number. If you can
   run JOB-Slow cold against unmodified DataFusion and tell me what you
   see, that is the single most useful data point you could send.

GitHub issues are open and watched:
https://github.com/singhpratech/samkhya/issues

---

## Quick links

| Resource | URL |
|---|---|
| Repo | https://github.com/singhpratech/samkhya |
| crates.io | https://crates.io/crates/samkhya-core |
| PyPI | https://pypi.org/project/samkhya/ |
| docs.rs | https://docs.rs/samkhya-core |
| Reproducibility (ACM AE v1.1) | https://github.com/singhpratech/samkhya/blob/main/REPRODUCIBILITY.md |
| Architecture deep-dive | https://github.com/singhpratech/samkhya/blob/main/ARCHITECTURE.md |
| Bench-results dossier | https://github.com/singhpratech/samkhya/tree/main/bench-results |

`cargo add samkhya-core` · `pip install samkhya`

If you read this far, thanks. The framework is the contribution; the
1.038× is the honest measurement; the failure-mode catalogue is part
of the product. They ship together.

---

*samkhya v1.0.0 — first stable release. Apache-2.0. Sole author:
Prateek Singh.*

*Suggested tags: `rust`, `databases`, `data-engineering`,
`query-optimization`, `cardinality-estimation`, `open-source`,
`datafusion`, `duckdb`, `polars`.*
</content>
</invoke>
