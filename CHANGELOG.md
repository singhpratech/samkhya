# Changelog

All notable changes to **samkhya** are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
honors [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.3] — 2026-07-24

**Documentation corrections. No code, no API, no behaviour change.**

1.2.2 rewrote every crate README. An adversarial verify pass then read each one
back against its own source and found four crates with factual defects — and
1.2.2 had already shipped. They are corrected here.

The one that matters most: **`samkhya-py`'s README told readers to derive
`distinct_counts` from a Count-Min sketch.** Count-Min bounds *frequencies*, not
distinct values. Following that advice yields a degree of `rows − maxfreq + 1`,
which *understates* the degree and produces a ceiling below the truth — exactly
the unsoundness 1.2.0 exists to fix, reintroduced through documentation. From
Python the sound source is an exact distinct count; sketch-derived degrees belong
to the Rust `AttributeDegree::from_hll_floor` / `from_count_min` constructors,
which return degrees rather than values to pass in.

Also corrected:

- `samkhya-py`: `selectivity_estimate` was described as the pre-1.2 `agm_bound`
  value. It is not — the old one applied a `min × max` shortcut this does not,
  and the two diverge by 100× on three relations. `BloomFilter` was documented as
  raising on out-of-range parameters; it silently **clamps**. `merge` was implied
  for all sketches; it is bound only on `HllSketch`. Wheel coverage was stated as
  "one wheel per platform" when only `manylinux_2_34 x86_64` is published.
- `samkhya-core`: `from_count_min` was shown returning `None` on saturation; it
  returns `AttributeDegree`, degrading to `rows`. "No query engine in its
  dependency tree" omitted that the default `feedback` feature bundles SQLite.
- `samkhya-postgres`: documented `anyarray` where pgrx generates `anyelement[]`,
  and carried a SQL example that errors before reaching the function. It named
  one build blocker when there are two, and a test command that cannot run. The
  SQL example is removed rather than corrected — the extension does not build,
  and printing an example implies it does.
- `samkhya-polars`: four signatures omitted their `Result` wrapper.
- The Python type stubs still carried an `LpBound helpers` section header.


## [1.2.2] — 2026-07-24

**Package metadata only. No code, no API, no behaviour change.**

1.2.1 corrected the one-line `description` field and I checked that field,
declared it fixed, and missed the one that matters. On crates.io and PyPI the
page body is the crate's `README.md`, and those were untouched — so
`samkhya-py`'s PyPI page still read "its **LpBound** ceiling helpers", naming the
bound family 1.2.0 found unsound, and carried two `../samkhya-core` links that
404 outside the repository.

Every published crate's README is rewritten against its actual source:

- No crate advertises "LpBound" as something samkhya offers. References to the
  real `lpbound::` module path remain, because that API exists.
- No relative links. They resolve inside the repo and break on both registries;
  all are now absolute.
- `samkhya-postgres` opens by stating the planner-hook integration is **not
  implemented** — which its own module docs have always said.
- `samkhya-duckdb-ext` and `samkhya-gpudb` state their scaffold boundaries.
- `samkhya-py` documents the real surface, including that `distinct_counts` must
  be a *lower* bound on the true distinct count, since the degree is derived as
  `rows - distinct + 1` and an overstated count yields an unsound ceiling.

Each rewrite was verified against the crate's source for invented API names.


## [1.2.1] — 2026-07-24

**Package metadata only. No code, no API, no behaviour change.**

Registry descriptions are baked in at publish time, so correcting them needs a
release. `samkhya-core` was still advertising itself on crates.io as "sketches,
**LpBound envelopes**, Puffin sidecars, and residual correctors" — naming the
exact bound family 1.2.0 found unsound and deprecated. Several others described
themselves by listing their own type names, which tells a reader nothing.

Every crate now says what it does, leading with the provable ceiling rather than
the machinery. The PyPI summary and the workspace description follow. `1.2.0` is
unaffected and remains installable; nothing about it is wrong except how it
introduced itself.


### Added — the corrector can finally be measured

The 2026-07-24 audit found the flagship JOB-Slow campaign had no corrector in
its "corrected" arm, and that this was not an oversight in the run script: the
bench CLI had no option that could attach one. `--corrector` offered `none` and
`identity`, where `identity` returns the baseline unchanged. Underneath that,
training was blind and models could not be persisted. Four connected gaps, none
of which fails loudly.

- `PlanObservation` and `FeedbackStore::record_plan` / `plan_history` record an
  observation together with the plan features the corrector sees at inference
  time. `Observation` carried only `est_rows` and `actual_rows`, so
  `GbtCorrector::train` had to synthesise a vector with six of seven features
  zeroed — constant across every training row, so no tree ever split on them.
  The model was one-dimensional while the adapter fed it seven live features.
  A silent train/serve skew, now covered by `tests/corrector_features.rs`, which
  asserts the new path learns from `join_depth` and pins the legacy path's
  blindness so it cannot be mistaken for working.
- `GbtCorrector::train_on_plans` trains on the real feature vector.
- `GbtCorrector::save` / `load` persist a fitted model, so training and
  evaluation can run in separate processes. That separation is what makes a
  held-out measurement honest: a model frozen before the evaluation queries ran
  cannot have seen them.
- `samkhya-bench train --feedback <db> --template <t> --out <model>` replaces a
  stub that printed "would train a residual corrector" and trained nothing.
  It refuses to train on featureless rows rather than padding them with zeros.
- `samkhya-bench run --corrector gbt --model <path>` evaluates with a fitted
  model, and `--only` / `--exclude` select query subsets so the training and
  evaluation sets can be kept disjoint.
- The feedback store gains nullable plan-feature columns via an idempotent
  `ALTER TABLE` migration. Stores written by an older binary upgrade in place
  and remain readable by one.

### Added — JavaScript, vector search, and a live demo

- `samkhya-wasm` — the JavaScript / TypeScript surface. `samkhya-core` compiled
  to WebAssembly: five sketches, the provable ceiling, Puffin I/O, and the
  hypergraph LP. 84 KB with generated `.d.ts`. Verified in Node —
  `joinCeiling([10,100],[0,1],[10,10])` returns exactly 100, and sketches
  round-trip byte-identically against the Rust format. Built, not yet published
  to npm.
- `samkhya-qdrant` — provable match-count ceilings for filtered vector search.
  A Count-Min sketch never undercounts, so for an equality condition its
  estimate is an upper *bound* on matching points — which is exactly what the
  pre-filter / post-filter decision needs, and the failure that hurts there is
  under-estimating. Bounds compose soundly through `AND` (min) and `OR` (sum
  capped at the collection); `NOT` is left at the collection size rather than
  quietly guessing, since no lower bound on the excluded set is available.
  Computes bounds and recommends a strategy; does not link an engine, and the
  README says so.
- `docs/demo.html` — an interactive proof. A two-table join the reader controls,
  with the true output counted from generated data and the ceiling computed
  beside it by the same wasm binary the npm package ships. A scripted sweep of
  **73,205 configurations** through that binary found **0 violations**, tightest
  ratio exactly 1.000×.
- `docs/index.html` rebuilt as a research page: the theorem with its proof, how
  the unsound bound survived two releases, an evidence table that shows the
  withdrawn rows rather than dropping them, and an honest per-target reach
  matrix including the targets that are not reachable at all.

### Added — a wasm-capable core

- `samkhya-core`'s SQLite feedback store moves behind a default-on `feedback`
  feature. `rusqlite` was the crate's one non-optional native dependency and the
  only thing preventing a wasm32 build. With `--no-default-features` the crate
  now compiles for `wasm32-unknown-unknown`, exposing the sketches, the provable
  degree ceiling, Puffin I/O, and (with `lp_solver`) the hypergraph LP. Existing
  consumers are unaffected: the feature is on by default, and `gbt` /
  `additive_gbt` select it automatically.
- `getrandom` gains its `js` feature on wasm targets, which is the remaining
  requirement for that build.

### Fixed

- The runner's `avg q-error` summed only finite samples but divided by the
  unfiltered count, so every unbounded sample silently pulled the average down
  — which is how a "q-error" of 0.39 could be printed, when q-error is at least
  1 by definition. It now reports the geometric mean over finite samples and
  states how many were unbounded instead of dissolving them into the
  denominator.

### Note on what this measures

The first honest held-out run on the synthetic suite — fit on S1–S5, evaluate on
S6–S10 — gives q-error geomean **13.46 without the corrector and 26.41 with it**.
The corrector roughly doubles the error. It was fitted on one usable row, so
that is the expected outcome and not a surprise; it is recorded because the same
setup evaluated on its own training queries shows an apparent improvement
(4.58 → 1.86), and that gap is exactly the train-on-eval artefact this tooling
exists to make impossible to publish by accident.


## [1.2.0] — 2026-07-24

**The ceiling is now actually provable.**

An audit of the upper-bound envelope found that three of the four bounds
shipped through v1.1 were not sound: they returned ceilings *below* the true
join cardinality on the shapes that dominate analytical workloads. A correction
clamped to such a ceiling underestimates, which is the regression the envelope
exists to prevent. This release repairs the family, adds a bound that is both
provable and tighter than the Cartesian product, and adds the test that would
have caught the defect.

Bound values only ever move **up** relative to v1.1 — never down — so no
correction that was safe before becomes unsafe now. Behaviour changes are
described per item below.

### Fixed

- `LpJoinBound` no longer returns ceilings below the truth. Its LP added one
  cover constraint per *predicate*; the AGM bound requires one per *attribute*,
  plus a full unit of cover weight for every relation carrying a column nothing
  else covers. Those constraints were missing, so
  `LpJoinBound::ceiling(&[10, 100], &[(0, 1)])` returned 10 for a foreign-key
  join that emits 100 rows. The defect is invisible on a triangle — where the
  two constraint sets coincide, and where the tests happened to look.
- `ChainBound` no longer divides the Cartesian product by `max(D_i, D_j)`. That
  is a uniform-distribution estimate, not an upper bound: two 20-row relations
  with 5 distinct keys and 16 rows on one key join to 260 rows, and the formula
  returned 80. It now derives a sound degree bound from the same distinct counts
  and is exactly tight on foreign-key joins.
- `AgmBound`'s `min × max` shortcut is not an AGM bound and was unsound for
  three or more relations. It now returns `ProductBound` and is deprecated:
  given only row counts and which pairs are joined, the Cartesian product is the
  only sound answer, because every row of every relation may share one key value.
- `examples/lpbound_tightness.rs` computed `(bound / truth).max(1.0)`. The clamp
  turned every soundness violation into a perfect-tightness reading, so the
  campaign could not detect the defect it was averaging over. It now reports
  per-bound violation counts and unclamped ratios, and excludes trials where the
  `u128` ground truth exceeds `u64::MAX` and every bound saturates.
- `samkhya-py`'s `agm_bound` multiplied a ceiling by caller-supplied
  selectivities. Selectivities are in `[0, 1]`, so this could only shrink the
  result — passing `0.01` returned a "bound" a hundredth of the real ceiling. The
  selectivity argument is now ignored; `selectivity_estimate` preserves the old
  value under a name that says what it is.
- `CountMinSketch::estimate`'s never-undercount guarantee is documented as
  conditional on no counter having saturated, which `u32` saturation breaks.
- `AttributeDegree::from_distinct` states its soundness obligation explicitly.
  `maxdeg ≤ rows − distinct + 1` subtracts the distinct count, so it is sound
  only if that count is a *lower* bound on the truth — and the obvious feeder is
  the wrong one, since an HLL point estimate is two-sided and exceeds the truth
  about half the time. `HllSketch::nonzero_registers` and
  `AttributeDegree::from_hll_floor` provide a value that never does: every value
  hashes to one register, so non-zero registers can only under-count.
- `SamkhyaPreJoinRule` now recognises `SortMergeJoinExec`. Without it the entire
  rule was a silent no-op under `prefer_hash_join = false`.

### Added

- `samkhya-core::degree` — a provable join ceiling from degree statistics.
  `JoinGraph::ceiling` implements a spanning-tree degree bound that is sound for
  bag semantics and exactly tight on foreign-key, star, and chain shapes.
  Degrees come from a row count (`maxdeg ≤ rows`), a distinct count
  (`maxdeg ≤ rows − distinct + 1`, exact for key columns), or a Count-Min sketch.
- `AttributeDegree::from_count_min` and `CountMinSketch::max_frequency_bound`.
  For any key `k`, `true_freq(k) ≤ estimate(k) ≤ max counter`, so a sketch's
  largest counter bounds every key's degree at once without knowing which key is
  hot. Since that sketch already rides in the Puffin sidecar, a ceiling proved
  from statistics written by one engine holds in another — no shared catalog, no
  re-scan. `CountMinSketch::is_saturated` reports the one condition under which
  the chain fails.
- `LpJoinBound::ceiling_hypergraph` and `HyperRelation` — the genuine
  fractional-edge-cover LP over an explicit attribute hypergraph, which still
  returns the AGM `n^1.5` bound for a triangle. `HyperRelation::new` assumes
  private columns (the sound default); `HyperRelation::projected` opts out.
- `tests/soundness_degree.rs` — six properties at 2,048 cases each that build
  relation instances, brute-force the true join, and assert
  `ceiling ≥ truth`. The pre-existing property suite checked only *relative*
  invariants between bounds, which hold fine for a family that is wrong together.
- `PreJoinCorrectionOptions::derive_ceiling` (default `true`) derives a finite
  per-input ceiling in the DataFusion adapter: a join emits at most the product
  of its children's rows, a filter at most its child's. Before this the shipped
  default had no finite clamp at any layer, so the bound guarantee was absent
  from the default DataFusion path unless an operator wired one up by hand.
- `samkhya-py` gains `join_ceiling`, which exposes the provable bound to Python.
- `SAFE_MAX_ROWS` (2^40) caps every row count the DataFusion rule publishes.
  DataFusion's join-cardinality estimator multiplies published row counts
  without an overflow check, so a corrector proposing `u64::MAX` wrapped into a
  meaningless number inside the planner. This is a sanity cap on absurd inputs,
  not a proof that the engine's arithmetic is total.

### Changed

- `bench-results/07_lpbound_tightness.md` is **retracted**, including its 40.95×
  star-5 headline. That ratio was `AgmBound / LpJoinBound` on instances where
  `LpJoinBound` had collapsed a star to its hub row count, so it was large in
  proportion to how far below the truth the denominator had fallen. Corrected,
  the v1.1 figure is undefined; on the repaired bounds it is 1.070×. It was also
  never a wallclock speedup, and is no longer described as one.
- `bench-results/20_bound_soundness.md` records the audit and the repair:
  **2,179 violations in 3,704 bound-evaluations before, 0 after.**
- `bench-results/OPEN_AUDIT_ITEMS.md` registers what the same audit raised that
  this release does *not* answer — including a possible run-order confound in the
  JOB-Slow campaign — with each item marked confirmed, credible-but-unverified,
  or open.

## [1.1.0] — 2026-07-14

This minor release adds public adapter APIs — a safe DataFusion pre-join
correction path and a portable Puffin statistics handoff shared across
Iceberg, DataFusion, and DuckDB — which is why it is a minor rather than a
patch release.

### Added

- `SamkhyaPreJoinRule`, `PreJoinCorrectionOptions`, correction metrics, and
  `install_pre_join_corrector` apply a `Corrector` immediately before
  DataFusion 46's `join_selection`. Native estimates remain the safe default
  floor; below-native estimates require explicit opt-in. Plan-impact tests
  cover build-side choice, partition mode, rule ordering, ceilings, floors,
  abstention, and model errors.
- End-to-end Python and TypeScript tests now enforce the stable LLM HTTP wire
  contract, including malformed input, batch limits, and lossless JSON `u64`
  handling in Node.
- CI now covers Rust 1.85 default-feature compatibility, optional engine
  features, Python 3.9/3.12/3.13 wheels, TypeScript, dependency audits, and both
  transport implementations. Dependabot monitors all four dependency surfaces.
- `AGENTS.md` and `docs/V1_1_ROADMAP.md` document contributor workflow and the
  measurable promotion gates for v1.1.
- A shared, strict `PortableStatsSnapshot` handoff applies the same HLL and
  equi-depth-histogram validation and decoding rules across Iceberg,
  DataFusion, and DuckDB consumers.
- A table-backed cross-engine release fixture verifies core/Apache Iceberg
  Puffin interoperability, explicit field-ID mapping, raw payload identity,
  planner-visible DataFusion NDV, client-visible DuckDB stats, and unchanged
  query results. Frozen v1 payload fixtures protect read compatibility.

### Changed

- Migrated `samkhya-py` to PyO3 0.29 and Rust edition 2024 while preserving the
  Python API, abi3-py39 wheel floor, clone extraction, and GIL-bound module
  behavior.
- Synchronized the Node transport package version with v1.1.0 and validated its
  distributable dependency lockfile.
- Clarified the MSRV guarantee: Rust 1.85 covers all default-feature crates;
  `samkhya-iceberg/iceberg` inherits Rust 1.92 from Iceberg 0.9.1.
- Iceberg snapshot loading now uses the table's configured `FileIO`, filters
  stale snapshot statistics deterministically, and preserves field IDs rather
  than treating them as engine column positions.

### Fixed

- Pre-planning corrections can now affect DataFusion join decisions instead of
  being observable only after physical planning. The old
  `compute_corrected_stats` compatibility helper no longer fabricates row and
  distinct-count values.
- TypeScript transports preserve integer estimates above JavaScript's
  `Number.MAX_SAFE_INTEGER` by parsing and emitting exact decimal JSON tokens.
- Python and TypeScript transports reject non-numeric features and invalid
  baseline ranges before invoking a backend.
- Chunked request bodies are capped while streaming. Both primary transports
  accept the 8 MiB boundary and return HTTP 413 above it without buffering the
  full payload or resetting the client connection.
- Core Puffin output now includes required snapshot and sequence metadata,
  stamps `created-by`, rejects unknown compression codecs and footer flags,
  and bounds every blob read to the payload region.

### Security

- Updated `crossbeam-epoch` to 0.9.20, the CXX family to 1.0.195, and PyO3 to
  0.29.0, resolving RUSTSEC-2026-0204, RUSTSEC-2026-0202, and
  RUSTSEC-2026-0177. Removed the retired PyO3 advisory exception.

### Historical documentation correction

- The v1.0.0 notes below claimed `cargo install samkhya-bench` and counted it
  among five published crates. Its v1.0.0 manifest had `publish = false`; the
  current workspace instead has eleven publishable crates, with
  `samkhya-bench` and `samkhya-it` remaining private workspace tools. The
  original text is retained below as an audit trail.

## [1.0.0] — 2026-05-17

**First stable release.** Promoted from rc.2 after the deep security
review, the corrector-OOM re-diagnosis, and the license consolidation
to Apache-2.0 only all landed and verified clean. The empirical
campaign attached to this release is the WAVE-4 / WAVE-5 work
documented in `REPRODUCIBILITY.md` and the `bench-results/` series;
headline measurements:

- **LpJoinBound vs AGM 40.95× on star-5 p=1**, BCa 95% CI [30.93,
  47.45], Wilcoxon W=0, p=1.73×10⁻⁶, n=30 (file 07).
- **JOB-Slow head-to-head 1.038× geomean**, BCa 95% CI [1.026, 1.056],
  Wilcoxon p=3.00×10⁻⁶, BH-FDR 24/55, 17 wins / 38 ties / 0 losses —
  pre-registered ≥1.35× hypothesis **falsified** (honest report, not
  silenced; see file 18 WAVE4-F).
- **TabPFN-2.5 P95 31.15 ms** at B=8 L=128 on RTX 4090 Laptop, BCa
  95% CI [29.39, 35.32] — H1-A PASS (file 14, WAVE5-L2).
- 51 binaries / 325+ tests passing / 0 failed across the workspace
  (default + all-features), 17 property-tests, ~31 M cargo-fuzz execs
  / 0 crashes.

The substance of v1.0.0 = the consolidated content of v1.0.0-rc.0
through v1.0.0-rc.2 (see those sections below). The release-notes
view of what changed since rc.1:

### Fixed

- **`samkhya-bench` n-trial runner never invoked the runtime
  corrector** (WAVE5-RC2 prong 1). `runner::run_async` reached
  `execute_query` unconditionally — even when `--baseline=false` was
  passed. The `samkhya-corrected` label was cosmetic; the runtime
  `Corrector::correct()` trait was never called from the n-trial CLI
  path. Fixed by adding `Runner::with_corrector(Arc<dyn Corrector>)`
  + a `--corrector none|identity` CLI flag + an
  `execute_query_dispatch` helper that routes to
  `execute_query_with_corrector` when a corrector is configured.
  Smoke-tested against the synthetic suite: `--corrector identity`
  produces byte-identical estimates to `--baseline` (the identity
  corrector passes the raw estimate through), confirming the
  dispatch is honest. The next rc.2 commit will add
  `--corrector gbt|additive-gbt` with training from a `--feedback`
  store at CLI-startup time.

- **`SamkhyaStatsExec` could push DataFusion into a smaller
  hash-join build side than baseline** (WAVE5-RC2 prong 2). The
  symptom was the corrected arm OOM-killing at q5c when the baseline
  arm completed all 113 JOB-Slow queries. The mechanism: when
  samkhya's HLL-derived NDV under-estimated the actual cardinality
  vs DataFusion's native heuristic, the planner picked a smaller
  build hash table and the underlying data overflowed it. Fixed by
  capping the published row count and column-level distinct count at
  `max(samkhya, native)` in `SamkhyaTableProvider::statistics()` and
  the new `pick_max_usize` merge helper. The cap means samkhya's
  published estimate is now monotonic in the
  plan-memory-safe direction relative to the baseline plan
  DataFusion would have chosen on its own. Unit-tested with a mock
  `TableProvider` that returns a known native row count.

### Security

Deep security review (`documents/SECURITY-REVIEW-2026-05-17.md`, internal)
surfaced 3 CRITICAL + 4 HIGH + 3 MEDIUM items relevant to the rc.2
release gate. All landed:

- **C1 — Puffin reader unbounded blob allocation.**
  `read_blob()` allocated `vec![0u8; meta.length as usize]` where
  `meta.length: u64` was untrusted (read from the attacker-controllable
  JSON footer). Trivial DoS via a tiny sidecar declaring a 16 EiB blob.
  Fixed by adding a `MAX_BLOB_LEN = 2 GiB` cap and a
  `MAX_FOOTER_LEN = 16 MiB` cap on the JSON payload length. Two new
  tests (`read_blob_rejects_oversized_length`,
  `open_rejects_oversized_footer_payload_len`) tamper with the footer
  bytes and assert rejection before allocation.
- **C2 — Puffin zstd decompression bomb.** `decode_zstd` used
  `zstd::decode_all` with no output cap. Replaced with a streaming
  `zstd::stream::Decoder` reading into a `Vec<u8>` bounded to
  `MAX_BLOB_LEN`. A high-ratio attacker-controlled compressed blob is
  now rejected with `Error::InvalidPuffin` instead of OOM.
- **C3 — LLM server error messages leaked exception text.** Both the
  Python FastAPI server (`llm_infer_server.py`) and TypeScript port
  (`llm_infer_server.ts`) included the raw SDK exception in the
  `<api_err: ...>` reply. Sanitized to only emit the exception class
  name on the wire; full exception is still logged to stderr for
  operator diagnosis. Applies to anthropic, openai, local, and the
  parse-error path in both transports plus the two `_dummy_backend`
  scripts.
- **H1 — `--host 0.0.0.0` accepted without warning.** Both LLM server
  transports now emit a prominent stderr banner when bound to a
  non-loopback address, warning the operator that the server has no
  authentication and that API keys in env will be used for all inbound
  requests.
- **H2 — Plaintext HTTP to remote host warning.** Added
  `warn_if_remote_plaintext_http` in `samkhya-core/src/residual.rs`
  invoked from `TabPfnHttpCorrector::new`/`with_url` and
  `LlmHttpCorrector::new`. Emits `log::warn!` when the configured
  `base_url` is `http://` (plaintext) to a non-loopback host. Silence
  via `SAMKHYA_ALLOW_REMOTE_HTTP=1`.
- **H3 — `SAMKHYA_LLM_SYSTEM_PROMPT` / `SAMKHYA_LLM_USER_PROMPT`
  capped at 16 KiB.** Both transports refuse to start with a larger
  value, eliminating env-var-injection OOM and upstream-API cost
  amplification.
- **H4 — HTTP body size limit (8 MiB).** All four HTTP servers
  (`llm_infer_server.py`, `llm_infer_server.ts`,
  `llm_dummy_backend.py`, `llm_dummy_backend.ts`) reject on
  `Content-Length` up front and enforce a streaming cap so chunked or
  header-less POSTs cannot OOM the host. HTTP 413 on exceedance.
- **M1 — Per-request features batch cap (1024 batches).** Even
  well-formed `features` payloads cannot fan out into thousands of
  upstream LLM calls; HTTP 413 above the cap.
- **M2 — SQLite feedback store tightened to 0o600 on Unix.**
  `FeedbackStore::open` calls `set_permissions(0o600)` so plan
  fingerprints and observation rows are not world-readable on shared
  systems. Best-effort (logged at debug if it fails).
- **M3 — `set -o pipefail` in `run-llm-bench.sh` and
  `run-llm-bench-ts.sh`.** A `curl` failure piped into `grep -q` no
  longer silently passes the health-poll gate.

### Changed

- **License consolidated to Apache-2.0 only** (was `Apache-2.0 OR
  MIT`). Every downstream user now gets the same explicit §3 patent
  grant rather than having it optional behind the MIT branch. Matches
  DataFusion / Iceberg / Arrow / ClickHouse posture. `LICENSE-MIT`
  removed; `deny.toml` still allows MIT/BSD/etc. for *transitive*
  dependencies.

### Open / deferred

Tracked for rc.3 / v1.1 (not blockers for rc.2):
- **TS-port 30-trial campaign** — TypeScript transport shipped with
  smoke tests only; not yet 30-trial campaign-measured.
- **Per-join-node q-error walking** (WAVE4-C Blocker 3).
- **L4/L5 deployment beyond A3.**
- **pyo3 ≥ 0.23 migration** (pinned at 0.22).
- **pgrx ≥ 0.13 migration** (pinned at 0.12 + double-gated).
- **DuckDB runtime `LOAD`** — blocked on upstream Issue #11638.
- **Security review M4–M6 + L1–L4** — path canonicalisation,
  AbortController per-chunk deadline, codec helper unification, TOCTOU
  + symlink hardening on Puffin sidecars, schema-version validation on
  the SQLite store. Documented in
  `documents/SECURITY-REVIEW-2026-05-17.md` (internal).

## [1.0.0-rc.1] — 2026-05-17

First release candidate. The v1.0.0 tag remains **held** per the
project release-gate rule pending external sanity-check on the rc.1
artifact. WAVE4-F closed the IMDb-measured headline gap; WAVE5-G
upgraded 7 of 10 metric-compliance items to canonical BCa; WAVE5-L2
closed the TabPFN-2.5 inference-latency gap on RTX 4090 Laptop;
WAVE5-E recovered L4 v3 to BH-significant improvement; WAVE5-N landed
the LLM-pluggable corrector (Python FastAPI canonical + Node TS
broader-appeal port, same wire contract). The previously-drafted
v1.0.0 `[Confirmed]` "≥3× p95 latency win on JOB-Slow's worst 20"
claim is **FALSIFIED honestly** by the WAVE4-F real measurement
(1.038× geomean, 1.011× on join-heavy 25) — corrections with named
attributions in the v1.0.0-rc.0 baseline section below.

### Fixed

- **Bloom-filter sizing formula** — `BloomFilter::new` was using a
  `1.44 k` m-constant when the correct allocation is
  `m = ceil(-n * ln p / (ln 2)^2)` (Bloom CACM 1970). Under-allocated
  bits by ~30.8%, which pushed the configured 1% FPR sketches to
  ~1.4–1.5% measured. Cross-validated in B04 and the H01 fortress
  test. (B04, H01.)
- **LpBound doc-comment ordering claim** — the prior doc asserted a
  strict chain `Product ≥ Chain ≥ AGM ≥ LpJoin`. The true empirical
  partial order is **Product ≥ {Chain, AGM} ≥ LpJoin**: Chain and
  AGM are incomparable in general. (B07.)
- **samkhya-postgres feature flags** `pgNN` now correctly imply
  `pg_extension`; was a feature-leak that broke
  `cargo check --features pg17` on hosts without PostgreSQL headers.
  (H09.)
- **samkhya-cli `--fp-rate 0` / NaN / inf / ≥1** used to drive Bloom
  into a 2 EiB allocation and SIGABRT; now validated at the CLI
  boundary before any sketch construction. (H02.)
- **samkhya-cli `stats <missing-path>`** silently created a zero-row
  SQLite at the destination and exited 0; now errors with an
  "input not found" message and non-zero exit. (H02.)
- **Rustdoc intra-doc link warnings** in `samkhya-datafusion` (H05),
  `samkhya-iceberg` (H08), and `samkhya-duckdb-ext` (H10).
  `cargo doc --workspace --no-deps` is warning-clean on default
  features.

### Added

- **WAVE4-F — JOB-Slow real head-to-head MEASURED.**
  `SamkhyaTableProvider` wired through `samkhya-bench --suite job-slow-real`;
  21 IMDb Puffin sidecars built (HLL p=12 NDV per column + 1% Bloom FK
  + row-count marker). n=55 paired warm-cache queries vs native
  DataFusion 46 at SF=1: **geomean 1.038× BCa 95% CI [1.026, 1.056],
  Wilcoxon W=212 p=3.00×10⁻⁶, BH-FDR rejects 24/55, 17 wins / 38 ties
  / 0 losses**. Closed the headline IMDb-measured gap from rc.1.
- **WAVE5-L2 — TabPFN-2.5 inference latency MEASURED on RTX 4090
  Laptop.** Stack: `tabpfn==8.0.3` (Hollmann ICLR 2023 + Prior Labs
  2026 update), `ModelVersion.V2_5`, driver 580.159.04, torch 2.6.0+cu124,
  CUDA 12.4 runtime. n=30 trials × 7 batch sizes × L=128. **H1-A P95
  31.15 ms at B=8 L=128 BCa 95% CI [29.39, 35.32] — PASS** (flipped
  from prior WAVE-5L FALSIFIED on `tabpfn==2.0.9`); H1-C transport P95
  0.21-0.30 ms — PASS; H1-B q-error reduction over GBT 7.84% BCa
  [2.21, 14.62], Wilcoxon p=1.04×10⁻⁵ — **FALSIFIED on magnitude (CI
  upper 14.62% strictly under 15% pre-reg), effect-direction confirmed**.
  Cold-start ready_s geomean ~3.2 s. Requires `TABPFN_TOKEN` +
  `TABPFN_DISABLE_TELEMETRY=1`.
- **WAVE5-E — L4 v3 recovery.** v3 retrain landed `--l4-variant v3` in
  `ablation_runner.rs`: prev=0 dispatch → additive 5-feature GBDT for
  est=0 regime; 60-pass warmup (600 records) + 300 seeded records from
  `15_ablation_raw.json`; online refit every 10 replicates. **A2→A3 Δ
  median q-error = −1.7% BCa 95% CI [−2.8%, −0.7%], Wilcoxon p=0.0209,
  BH-significant at α=0.05 in improvement direction.** L4 trajectory:
  v1 +386% (BH-sig regression) → v2 +137% (BH-sig regression) → v3
  −1.7% (BH-sig improvement). Production deployment v1.0: A3
  (L1+L2+L3+L4 v3); L5 opt-in.
- **WAVE5-M — Cold-cache `posix_fadvise(POSIX_FADV_DONTNEED)`
  workflow** for ACM AE v1.1 reviewers without root `drop_caches`
  privilege.
- **WAVE5-A — samkhya-postgres pgrx feature isolation.** Double-gate
  pgrx behind `pg_extension` cargo feature + `samkhya_pgrx_enabled`
  rustc cfg + pg17-only pin per [`feedback_pgrx_feature_isolation`].
  `cargo check --workspace --all-features` no longer requires
  PostgreSQL headers.
- **WAVE5-D — EquiDepth invariant tightening.** `EquiDepthHistogram::
  from_bytes` now rejects 4 MiB all-zero payloads and validates
  bucket-count monotonicity + bin-edge ordering before accepting the
  histogram.
- **WAVE5-G — BCa CIs landed on 7 of 10 metric-compliance items.**
  10,000 resamples seed 42 per Efron-Tibshirani 1993 ch. 14; q-error
  P50/P95/P99 reported per Moerkotte VLDB 2009.
- **`BloomFilter::try_new`** + structural-invariant validation in
  `BloomFilter::from_bytes`. Parallel `try_new` + structural-validation
  work landed across the other four sketch kinds (`HllSketch`,
  `CountMinSketch`, `EquiDepthHistogram`, `CorrelatedHistogram2D`).
  (SRC01.)
- **9 examples under `samkhya-core/examples/`** — `bloom_fpr_sweep`,
  `cms_bound_sweep`, `histogram_accuracy`, `hll_precision_sweep`,
  `inspect_puffin`, `lpbound_latency`, `lpbound_tightness`,
  `memory_profile`, `sketch_to_puffin`.
- **5 cargo-fuzz targets**: `fuzz_hll_parse`, `fuzz_bloom_parse`,
  `fuzz_cms_parse`, `fuzz_equidepth_parse`, `fuzz_correlated_parse`.
  Verified at 60 s × 7 targets, 31,401,728 total executions,
  **0 crashes / 0 leaks / 0 timeouts** (B08).
- **Empirical campaign**: `bench-results/01_…` through
  `bench-results/18_…` (18 receipts) plus the canonicalization
  quartet `METHODOLOGY.md` / `JOURNEY.md` / `BENCHMARKS.md` /
  `EVIDENCE.md`.
- **Fortress integration tests** across 8 crates with cross-crate
  fixtures under `samkhya-it/`: H01 (core), H02 (cli), H03 (py),
  H04 (arrow), H05 (datafusion), H06 (polars), H07 (duckdb),
  H08 (duckdb-ext), H09 (postgres), H10 (iceberg).

### Changed

- **Methodology canonicalized to industry-standard metrics** per the
  `METHODOLOGY.md` table. Citations now include **Hollmann ICLR 2023**
  (TabPFN-2.5), Moerkotte VLDB 2009 (q-error), Efron–Tibshirani 1993
  (BCa bootstrap, ch. 14), Leis VLDB 2015 (JOB-Slow protocol),
  Atserias-Grohe-Marx PODS 2008 (AGM bound), Cormode–Muthukrishnan
  J. Algorithms 2005 (CMS bound), Flajolet 2007 (HLL standard-error),
  Wilcoxon 1945 Biometrics Bulletin (signed-rank for paired latency),
  Benjamini–Hochberg JRSSB 1995 (FDR control),
  Bloom CACM 1970 (FPR formula), Ioannidis–Poosala SIGMOD 1996
  (MaxDiff histograms), Jagadish VLDB 1998 (V-Optimal baselines),
  Zhang SIGMOD 2025 (LpBound polynomial families),
  Stillger SIGMOD 2001 (LEO feedback-driven optimization),
  ACM Artifact Evaluation v1.1 (reproducibility badge).
- **Bloom unit-test slack** tightened from `5×` to `1.5×` empirical
  vs configured FPR ratio; the shipped tests now fail loud on any
  future regression of the sizing formula.

### Deprecated

- **Infallible `Sketch::new` constructors** in favour of
  `Sketch::try_new`. Will be removed in v1.1 per the SEMVER
  deprecation window (`docs/SEMVER.md`). Affects `HllSketch::new`,
  `BloomFilter::new`, `CountMinSketch::new`,
  `EquiDepthHistogram::new`, `CorrelatedHistogram2D::new`.

### Security

- **SECURITY.md update**: every `from_bytes` constructor now
  validates structural invariants post-deserialize — declared
  `k`/`m`/`precision`/`width`/`depth`/`bucket_count` must agree
  with the buffer length, and any mismatch returns
  `Err(Error::InvalidPayload)`. Combined with the 5 new fuzz
  targets, the parser surface (Puffin reader + 5 sketch decoders)
  is exercised at 60 s × 7 targets per release cut. No
  GHSA-eligible findings.

### Open / deferred

Tracked for `v1.1` (WAVE5-G closed 7 of the prior 10 metric-compliance
items; WAVE4-F closed item 5):

1. **3 remaining metric-compliance items**: (a) MaxDiff / V-Optimal
   baselines fully promoted into ablation tables (Ioannidis–Poosala
   SIGMOD 1996, Jagadish VLDB 1998); (b) per-join-node q-error walking
   (WAVE4-C Blocker 3); (c) TPC-H SF=1 measured cells.
2. **pyo3 ≥ 0.23 migration** — pinned at 0.22 for abi3-py39
   single-wheel; v1.1 target.
3. **DuckDB runtime `LOAD`** — blocked on upstream DuckDB Issue
   #11638; `samkhya-duckdb-ext` ships v1.0 as staticlib+rlib only.
4. **pgrx ≥ 0.13 migration** — pinned at 0.12 + double-gated behind
   `pg_extension` feature + `samkhya_pgrx_enabled` rustc cfg
   (WAVE5-A); v1.1 target.
5. **Cold-cache JOB-Slow** — root `drop_caches` unavailable on user-priv
   host; `posix_fadvise` workflow shipped via WAVE5-M for ACM AE;
   true cold-cache measurement awaits root-capable host (v1.1).
6. **n=30 replicates per JOB-Slow query** — budget-cap at n=2 in
   WAVE4-F; OOM at q16a needs 32→64 GiB host headroom (v1.1).

## [1.0.0-rc.0] — 2026-05-16 — never tagged, baseline for rc.1

Internal draft of what was originally planned as the v1.0.0 release.
**Never tagged**: held back per the project release-gate rule when
the WAVE4-F real-measurement campaign falsified the headline ≥3× p95
claim. Content here is the baseline that rc.1 corrects and extends —
the falsified `[Confirmed]` line is preserved below for honesty and
audit-trail purposes, with the corrected WAVE4-F measurement folded
into the rc.1 section above.

Stabilization release. The kill-criteria gate (ROADMAP §11) has been
passed end-to-end on JOB-Slow against real IMDb data, the LpBound LP
solver is the default ceiling, the DuckDB extension scaffold and the
foundation-model interface have both landed, and the workspace has been
hardened with fuzz targets, stress benches, and supply-chain checks. The
public API, the Puffin sidecar layout, and the SQLite feedback-store
schema are now semver-stable.

### Added

- **Semver-stable on-disk formats**:
  - Puffin sidecar `KIND` tags frozen: `samkhya.hll-v1`,
    `samkhya.bloom-v1`, `samkhya.cms-v1`, `samkhya.equi-depth-v1`,
    `samkhya.correlated2d-v1`. Future revisions bump the `-vN` suffix and
    keep readers on the prior tag.
  - SQLite feedback-store schema in `samkhya-core/src/feedback.rs` carries
    a `schema_version` row in the `samkhya_meta` table; readers refuse to
    open a store with a major-version mismatch and migrate minor bumps in
    place.
- **`docs/SEMVER.md`** — what the v1.0 guarantee covers (every `pub`
  symbol in the eight workspace crates, the on-disk `KIND`-tagged sketch
  payloads, the Puffin footer schema, the feedback-store table layout)
  and what it explicitly does not (criterion bench microstructure, log
  format strings, internal solver tolerances).
- **`cargo install samkhya-bench`** — single-binary install path for
  evaluators. README quickstart updated with the JOB-Slow `--imdb-dir`
  flow end-to-end.

### Changed

- API freeze. Every `pub` symbol in `samkhya-core`, `samkhya-datafusion`,
  `samkhya-duckdb`, `samkhya-duckdb-ext`, `samkhya-py`, `samkhya-bench`,
  `samkhya-postgres`, and `samkhya-gpudb` is now covered by the semver
  contract in `docs/SEMVER.md`. Breaking changes require a v2.0
  deprecation window per the Rust API guidelines.
- All eight workspace crates bumped to `1.0.0` in lockstep via
  `[workspace.package].version`. The five published crates
  (`samkhya-core`, `samkhya-datafusion`, `samkhya-duckdb`, `samkhya-py`,
  `samkhya-bench`) ship to crates.io on the same release; the three
  remaining workspace members (`samkhya-duckdb-ext`, `samkhya-postgres`,
  `samkhya-gpudb`) stay path-only at 1.0.0 until each grows its own
  publish gate.
- `LpJoinBound` is now the default ceiling in `SamkhyaTableProvider`'s
  builder when the `lp_solver` feature is on; the coarse `ProductBound`
  / `AgmBound` / `ChainBound` triple stays available as opt-in for the
  fast path and as the documented solver-failure fallback.

### Confirmed (post-WAVE4-F honest revision)

- **JOB-Slow head-to-head (WAVE4-F MEASURED 2026-05-16):** geomean
  wallclock **1.038× BCa 95% CI [1.026, 1.056], Wilcoxon
  W=212 p=3.00×10⁻⁶, BH-FDR rejects 24/55, 17 wins / 38 ties / 0
  losses, n=55 paired warm-cache queries at SF=1 IMDb CSV**.
  Statistically real (CI excludes 1.0) but small. Pre-registered
  ≥ 3× / ≥ 1.6× / ≥ 1.5× / ≥ 1.35× p95 bounds **FALSIFIED honestly**;
  on the join-heavy 25 subset measured 1.011× (n=14 paired). The prior
  v1.0.0 `[Confirmed]` "≥3× p95 latency win on JOB-Slow's worst 20"
  claim is **corrected to 1.011× FALSIFIED** here.
  **Attributions named (not goalpost-shifted):**
  - Per-join-node q-error walking deferred to v1.1 (WAVE4-C Blocker 3) —
    wallclock compresses to row-count=1 final aggregate, NDV wins
    don't transfer when join order is unchanged.
  - DataFusion 46 already uses leaf NDV for scan-level estimates,
    narrowing samkhya's room.
  - I/O floor: CSV (not Parquet) re-parse dominates; optimizer-level
    gains masked.
  - OOM cap at q16a left 58/113 queries untimed in either arm —
    coverage biased to "easier" queries 1a-15d.
  - n=2 replicates/query (budget cap), warm-cache only (root
    `drop_caches` unavailable on user-priv host; `posix_fadvise`
    workflow shipped via WAVE5-M but true cold-cache measurement is
    v1.1).
- **Zero regressions** (per win/tie/loss): the LpBound envelope's
  "NEVER REGRESS" guarantee holds — 0 losses on n=55 paired JOB-Slow,
  ±5% envelope holds on every Synthetic S1-S10.
- **13 workspace crates** compile clean on default features; full
  feature matrix (`--features lp_solver,additive_gbt,gbt,zstd,
  tabpfn_http,bundled,engine`) is green on the CI matrix.

<details>
<summary>Prior audit (pre-WAVE4-F, superseded)</summary>

Prior v1.0.0 text claimed "≥3× p95 latency win on JOB-Slow's worst 20
queries against the DataFusion 46 baseline (the samkhya.md §4 Week 13
GO/NO-GO criterion from ROADMAP §11). Numbers ship in paper/draft.md
§5 and in the Markdown report produced by `bench compare --suite
job-slow --report`." This was based on projection from synthetic
q-error reductions before the IMDb harness was wired. WAVE4-F real
measurement falsified it. Documented per `feedback_empirical_methodology`
honesty rule.

</details>

## [0.9.0] — 2026-05-16

Hardening release in preparation for v1.0. No new user-visible features;
the surface added here is supply-chain hygiene, fuzz coverage, stress
benchmarks, and a written security policy. The `samkhya-core` parser
surface (Puffin reader, sketch decoders) is the attack surface this
release targets.

### Added

- **`samkhya-core/fuzz/`** — `cargo-fuzz` workspace with two targets:
  - `fuzz_targets/puffin_reader.rs` drives `puffin::PuffinReader` against
    arbitrary byte slices. Validates that any input shape returns an
    `Err` rather than panicking or reading out of bounds.
  - `fuzz_targets/sketch_decoder.rs` round-trips arbitrary bytes through
    `HllSketch::from_bytes`, `BloomFilter::from_bytes`,
    `CountMinSketch::from_bytes`, `EquiDepthHistogram::from_bytes`, and
    `CorrelatedHistogram2D::from_bytes`. Same panic-freedom contract.
  - `samkhya-core/fuzz/README.md` documents the local 1-hour run and the
    5-minute CI budget on `main`. Any reproducible crash blocks v1.0.
- **`samkhya-core/benches/stress.rs`** — new criterion bench module gated
  on `RUN_STRESS=1`. Covers a 10 M-row HLL build under memory pressure,
  a 100 k-blob Puffin sidecar write + lazy reopen, and a 1 M-observation
  `FeedbackStore` insert + query loop. Designed to fail loud if a
  refactor regresses constant-factor performance on the parser path.
- **`deny.toml`** — `cargo-deny` configuration at the workspace root.
  - Licenses: `Apache-2.0`, `MIT`, `BSD-2-Clause`, `BSD-3-Clause`,
    `ISC`, `Unicode-DFS-2016`, `Unicode-3.0`, `Zlib`, `CC0-1.0`,
    `Apache-2.0 WITH LLVM-exception`. Anything else blocks the build.
  - Bans: `multiple-versions = "deny"`, `wildcards = "deny"`, per-crate
    `skip = [...]` entries with one-line justifications for the known
    transitive duplicate set (hashbrown 0.12/0.14/0.15, bitflags 1.3,
    syn 1.0, thiserror 1.0, windows-sys 0.52/0.60).
  - Sources: `unknown-registry = "deny"`, `unknown-git = "deny"` —
    every crate must come from crates.io.
  - Advisories: `ignore = []` so every release re-validates the full
    RustSec advisory surface.
- **`SECURITY.md`** at workspace root. Documents the supported-versions
  window (0.9.x current, 0.8.x previous), the GitHub Security Advisories
  reporting channel on `singhpratech/samkhya`, the 3-business-day
  acknowledgement target, and a **90-day GHSA embargo policy** matching
  the broader Rust ecosystem convention. Negotiable shorter for actively
  exploited issues, longer for coordinated disclosure with upstream
  dependencies. CVE requested for any vulnerability rated medium+ on
  CVSS v3.1.
- CI gains a `cargo deny check` step and a 5-minute `cargo fuzz run`
  step against both fuzz targets on every push to `main`.

### Changed

- `samkhya-core/Cargo.toml` and the published-crate `README.md` files
  now point at `SECURITY.md` for vulnerability reporting; the prior
  README sections that pointed at a personal email channel have been
  replaced with the GHSA workflow.
- API-stability audit: every `pub` item in `samkhya-core` reviewed; a
  small number of intentionally-unstable surfaces moved behind
  `#[doc(hidden)]` or into private modules ahead of the v1.0 freeze.

## [0.8.0] — 2026-05-16

Foundation-model interface lands as an opt-in tabular foundation model
backend behind the `Corrector` trait. This is the Layer 5 slot in the
architecture: the same trait shape as `GbtCorrector` and
`AdditiveGbtCorrector`, with a separate transport. The OtterTune
precedent for residual correction in optimizer tuning informed the API
shape; the implementation here is pure-Rust client-side glue against a
user-run inference server, not a hosted service.

### Added

- **samkhya-core**
  - `residual::tabpfn::TabPfnHttpCorrector` behind the new `tabpfn_http`
    cargo feature. Posts a `CorrectionFeatures::to_vec()` payload as
    JSON to a user-configured endpoint (default
    `http://localhost:8765/infer`), parses an `{"estimate": <u64>}`
    reply, and clamps the result through `lpbound::saturating_clamp`.
    Transport: pure-Rust `ureq` with rustls only — no OpenSSL.
  - `residual::tabpfn::TabPfnHttpOptions` — per-call config: `base_url`,
    `timeout_ms` (default 50 ms, bounded by the sub-ms estimate budget
    but configurable for diagnostics), `ceiling` for the LpBound clamp.
  - `residual::TabPfnStub` — always-compiled no-op corrector. Returns
    `Ok(None)` from every call regardless of features. Lets downstream
    code reference the integration slot without taking the
    `tabpfn_http` feature dependency. 3 unit tests confirm the stub
    contract, the HTTP-failure-to-`None` mapping, and the malformed-URL
    case.
  - Failure policy documented and tested: any transport error
    (connection refused, DNS failure, non-2xx, body parse error,
    timeout) returns `Ok(None)`, never `Err`. The engine then falls
    back transparently to the native estimate. A remote inference
    server going down must not surface as a query failure.
- Module-level documentation in `samkhya-core/src/residual.rs` describing
  the residual corrector via TabPFN: the contract is identical to every
  other backend in the module (feed `CorrectionFeatures`, receive
  `Option<u64>` clamped to the LpBound ceiling).

### Changed

- `residual` module documentation now lists three concrete backends
  (`gbt`, `additive_gbt`, `tabpfn`) plus the always-on `TabPfnStub`,
  with a per-backend feature-flag table.
- The `tabpfn_http` feature is opt-in: default builds never see `ureq`,
  rustls, or the inference-server transport surface.

### Naming discipline

- The corrector is documented as a **portable foundation-model interface
  for residual correction**, never as a "learned" / "adaptive" / "AI"
  feature. The `Corrector` trait is the contract; TabPFN is one
  pluggable backend among several.

## [0.7.0] — 2026-05-16

Graduates `samkhya-duckdb` from the v0.4.0 client-side workaround to a
true server-side DuckDB extension scaffold. The new `samkhya-duckdb-ext`
crate produces a loadable `.duckdb_extension` once the C++ toolchain and
DuckDB headers are present; default `cargo check` keeps working without
either. Aligned with DuckDB Issue #11638 (statistics-extension hook).

### Added

- **samkhya-duckdb-ext** — new workspace member, ninth crate in the
  workspace. `Cargo.toml` declares `crate-type = ["cdylib"]` so DuckDB
  can `LOAD` the resulting artifact at runtime; DuckDB's loader looks
  for the `<name>_init` symbol in the renamed `.so` / `.dylib` / `.dll`.
- `samkhya-duckdb-ext` `extension` cargo feature (opt-in, off by
  default). When enabled it pulls in `cxx = "1"` and `cxx-build = "1"`,
  compiles `src/extension.cpp` against DuckDB extension headers (found
  via the `DUCKDB_INCLUDE_DIR` env var), and exports the `samkhya_init`
  symbol the loader expects.
- Build path: `cargo build -p samkhya-duckdb-ext --release --features
  extension` after the `duckdb/extension-template` checkout. Documented
  in the crate `README.md` plus the workspace `RELEASE.md`.
- Alignment with **DuckDB Issue #11638** — the upstream
  statistics-extension hook tracker. The scaffold's
  `OptimizerExtension`-shaped C++ glue matches the integration pattern
  Query-farm/datasketches landed first, so when #11638 ships a stable
  extension API samkhya is one rewrite-rule registration away from
  injecting Puffin-sourced cardinality estimates into DuckDB's planner.

### Changed

- `samkhya-duckdb` (the client-side crate from v0.4.0) is now
  documented as the workaround tier; `samkhya-duckdb-ext` is the
  forward path. The two crates coexist: `samkhya-duckdb` continues to
  drive the in-process `duckdb` Rust client + `PRAGMA` overrides; the
  extension crate is the server-side native path.
- CI matrix gains a `samkhya-duckdb-ext` job that caches the DuckDB
  source checkout (otherwise the extension build pushes CI past 30
  minutes); the default workspace build still excludes the crate so
  contributors without a C++ toolchain are unaffected.

## [0.6.0] — 2026-05-16

The whole point of the project: JOB-Slow against real IMDb data. The
ROADMAP §4 / `samkhya.md` §4 Week 13 GO/NO-GO gate evaluates here. The
five hand-written queries from v0.0.1 (1a, 2b, 6a, 17a, 29a) become the
smoke subset; the full 113-query corpus is now wired and runnable.

### Added

- **samkhya-bench::imdb** — new module `samkhya-bench/src/imdb.rs`.
  - `register_imdb_tables(ctx: &SessionContext, csv_dir: &Path)` — the
    single entry point. Resolves each of the 21 IMDb tables in priority
    order: Parquet under `csv_dir/parquet/<table>.parquet` first, then
    raw header-less CSV under `csv_dir/<table>.csv`, with the canonical
    `schema.sql` orderings supplied via `CsvReadOptions::schema` so
    DataFusion does not have to infer types from a multi-GB scan.
  - `imdb_schemas() -> HashMap<&'static str, Schema>` — the full Arrow
    schema map for every JOB table (`aka_name`, `aka_title`,
    `cast_info`, `char_name`, `comp_cast_type`, `company_name`,
    `company_type`, `complete_cast`, `info_type`, `keyword`,
    `kind_type`, `link_type`, `movie_companies`, `movie_info`,
    `movie_info_idx`, `movie_keyword`, `movie_link`, `name`,
    `person_info`, `role_type`, `title`).
  - `probe_imdb_dir(csv_dir: &Path)` — early-exit check for the runner;
    returns `Ok(())` as soon as one expected file is found.
  - `default_imdb_dir()` — convention path (`samkhya-bench/data/job`).
  - 2 unit tests confirm the schema map covers every table and the
    probe rejects a missing directory.
- **`bench run --suite job-slow --imdb-dir <path>`** — runs the
  canonical 33-query JOB-Slow subset (the "hardest" queries from Leis
  VLDB 2015) against the real IMDb dump, baseline vs samkhya-corrected.
  The `--imdb-dir` flag is threaded through `run`, `compare`, and
  `calibrate`.
- **`bench compare --suite job-slow --report <path.md>`** — Markdown
  report generator. Emits per-query q-error before/after, p50 / p95 /
  p99 latency, and the aggregate worst-20 line item that gates the
  kill-criteria check in ROADMAP §11.
- `scripts/download-imdb.sh` (idempotent, SHA-256 verified) and a
  Parquet-conversion helper for the load path documented in
  `samkhya-bench/data/job/README.md`.

### Changed

- `samkhya-bench/Cargo.toml` gains the `tokio` runtime helpers needed
  by `register_imdb_tables` (the function builds a `current_thread`
  runtime locally so the rest of the binary stays sync-friendly).
- The five hand-written JOB queries from v0.0.1 are reclassified as the
  smoke subset; the full 113-query corpus lives under
  `samkhya-bench/src/suites/job/`.

## [0.5.0] — 2026-05-16

Real fractional-edge-cover LP join bound. The shipped envelope through
v0.4.0 (`ProductBound`, `AgmBound`, `ChainBound`) was a coarse
approximation of the SIGMOD 2025 Atserias–Grohe–Marx / LpBound
construction; v0.5.0 ports the principled formulation as the preferred
ceiling. The coarse bounds remain as the always-available scaffolding
and as the solver-failure fallback.

### Added

- **samkhya-core**
  - `lpbound::LpJoinBound` (new, gated behind the `lp_solver` cargo
    feature) — the real fractional-edge-cover LP solved with `good_lp`
    over the pure-Rust `microlp` backend. Formulation:
    - one variable `x_r ≥ 0` per relation,
    - one fractional-cover constraint `sum_{r : a ∈ schema(r)} x_r ≥ 1`
      per shared attribute,
    - objective `minimise sum_r x_r · log|R_r|`,
    - join-cardinality ceiling = `exp(LP minimum)`.
    This is the AGM bound (p=∞ LpBound specialisation) ported in full.
  - Per-connected-component decomposition: equality predicates partition
    the relations; we solve one small LP per component and multiply the
    per-component ceilings. Implementation in `lpbound::solve` +
    `lpbound::connected_components` (union-find over the predicate
    graph).
  - `LpJoinBound::with_distinct_counts(Vec<u64>)` +
    `ceiling_with_distinct` — folds per-relation `distinct_count` hints
    into the objective coefficient as `log(min(|R_r|, D_r))`, which can
    only tighten the bound.
  - Conservative fallback: if the LP solver fails for any reason
    (numerical edge case, malformed join graph) `LpJoinBound` returns
    the coarse `AgmBound` / `ProductBound` over the same component. The
    envelope must never crash the engine.
  - 8 unit tests under `#[cfg(all(test, feature = "lp_solver"))]`
    covering: 2-table single-edge join (collapses to
    `min(|R_0|, |R_1|)`), triangle (`(|R_0|·|R_1|·|R_2|)^{1/2}`),
    4-cycle (`N^2` for equal sizes), disconnected components
    (per-component product), singleton-component passthrough, the
    `bound ≤ ProductBound` refinement contract, the empty-relations
    case, and the distinct-aware tightening.
- **samkhya-core/Cargo.toml** — new optional dependency `good_lp` (≥0.10,
  microlp backend, pure-Rust). `lp_solver` cargo feature gates the
  dependency so default `cargo build` stays pure-Rust without paying
  the LP-modelling crate's build cost.

### Changed

- `samkhya-core::lpbound` module docs rewritten to lead with the
  preferred bound (`LpJoinBound` when `lp_solver` is on) and reclassify
  `ProductBound` / `AgmBound` / `ChainBound` as scaffolding bounds —
  always available, used as the safety floor when the LP solver is
  disabled or fails. Selection precedence documented inline.
- `paper/draft.md` §3 (Safety Envelope) gains the real-construction
  paragraph that closes the v0.4.0 known-limitation entry ("Full LpBound
  LP solver still pending"). Reviewer-2 desk-reject risk goes down
  accordingly.

### Fixed

- The `q=∞` regime now has two independent escape paths: the v0.4.0
  `AdditiveGbtCorrector` (escape via the corrector layer) and the
  v0.5.0 `LpJoinBound` distinct-aware tightening (tighter ceiling so
  the corrector has less work to do). Neither path is required; either
  alone is sufficient on the worst Synthetic S2–S5 / S7 / S9 / S10
  queries.

## [0.4.0] — 2026-05-16

Fifth wave. The workspace widens from 5 to 8 member crates, the q=∞
correction limitation is unblocked, and the per-community release plan
goes from sketch to concrete docs.

### Added

- **samkhya-core**
  - `residual::additive::AdditiveGbtCorrector` behind the
    `additive_gbt` feature. Predicts the absolute row count directly
    from `CorrectionFeatures::to_vec()` so a zero baseline no longer
    traps the prediction at zero. Proof: `baseline_estimate=0 →
    corrected=1000`.
  - `sketches::correlated::CorrelatedHistogram2D` — equi-width 2D bins
    capturing column-pair correlations the four single-column sketches
    miss. KIND tag `samkhya.correlated2d-v1`. 10 unit tests.
  - `examples/inspect_puffin.rs` — operator binary that dumps the
    footer JSON and decodes every known sketch kind inside a Puffin
    sidecar.
- **samkhya-bench**
  - `build-puffin --output <dir>` subcommand — generates real Puffin
    sidecars from the synthetic schema (one `.puffin` per table, HLL
    blob per column).
  - `--puffin-dir <path>` flag threaded through `run` / `compare` /
    `calibrate`. When supplied, `SamkhyaTableProvider` ColumnStats
    overrides come from real sidecars instead of hardcoded distinct
    counts.
- **samkhya-duckdb** — real Rust-client integration behind the
  `bundled` feature. `build_hll_from_query` / `build_bloom_from_query` /
  `capture_observation` against DuckDB 1.x. Default build stays
  exclusion-friendly (no C++ toolchain required).
- **samkhya-polars** — real Series→Sketch helpers behind the `engine`
  feature, on polars 0.44. `hll_from_series` / `bloom_from_series` /
  `cms_from_series` / `histogram_from_series` + `lazy_collect_with_feedback`.
- **samkhya-postgres** — new workspace member. Stub matching the
  postgrespro/aqo prior-art pattern (pgrx planner_hook + ExecutorEnd_hook,
  libpq sidecar alt path, sketches in a `samkhya` schema).
- **samkhya-gpudb** — new workspace member. `GpuCorrector` trait +
  `CpuFallbackCorrector` reference impl. Reserves Layer 4 (batch GPU
  inference) of the architecture.
- **Strategic docs at workspace root**:
  - `ROADMAP.md` — v0.4 → v1.0 plan with kill criteria, CIDR 2027
    calendar (2026-05-19 → 2026-08-04).
  - `RELEASE.md` — operator playbook (versioning, cargo-release, publish
    ordering, branch policy, security channel).
  - `DISTRIBUTION.md` — per-community launch plan across 14 surfaces.
  - `SHOW-HN-DRAFT.md` — pre-staged v1.0 launch post (1439 chars).
  - `CALIBRATE_WORKFLOW.md` — operator guide for the feedback loop.
- **paper/**
  - `draft.md` — 3,482-word draft skeleton matching outline 1:1.
  - `paper.tex` — 782-line LaTeX (article class, 64 \cite calls, 7 figure
    stubs, 5 table stubs).
  - `references.bib` — 25 BibTeX entries.
  - `Makefile` — `make paper` runs pdflatex/bibtex/pdflatex×2.
- **documents/** — 11-chapter literature-style HTML field guide with
  inline SVG diagrams (no CDN deps, no webfonts). Sanskrit-manuscript
  aesthetic: ink + parchment + cinnabar. Devanagari सांख्य flourish on
  the cover, sticky chapter nav, print-friendly @media print.

### Confirmed

- 8 member crates, all build clean on default.
- Workspace clippy `-D warnings` clean.
- `cargo run -p samkhya-bench -- build-puffin --output /tmp/sx` writes
  four sidecars; `inspect_puffin` reads them back.

### Framing rules added to memory

- "Every TODO becomes a parallel agent — maximum parallelism is the
  default." (parallel_agent_strategy.md)
- "Never write 'killed' / 'dead repo' / 'graveyard'. Always frame as
  limitations we transcend." (feedback_samkhya_naming.md sub-rule)

## [0.3.0] — 2026-05-16

Fourth wave of the same session. Closes the feedback loop end-to-end:
the bench can now train a GBT residual corrector from its own
observations and re-run queries with the correction applied, showing
real q-error reduction. Adds the missing fourth foundational sketch.

### Added

- **samkhya-bench**
  - `calibrate --suite <name> [--feedback <path>]` subcommand —
    three-phase loop:
    1. Collect: run the suite in samkhya-corrected mode, recording
       observations to a `FeedbackStore`.
    2. Train: read observations back, train a
       `samkhya_core::residual::gbt::GbtCorrector` with default
       `GbtOptions`.
    3. Correct: re-run the suite, threading the corrector through
       `Runner::run_with_corrector`; print a before/after q-error
       table and an improvement summary.
  - `Runner::run_with_corrector<C: Corrector + ?Sized>` + `CorrectedOutcome` —
    runs the same physical-plan extraction and DataFusion execution, then
    applies the corrector's `correct(&features)` to the raw estimate.
  - `Cargo.toml`: samkhya-core dependency now enables the `gbt` feature.
- **samkhya-core**
  - `sketches::histogram::EquiDepthHistogram` — fourth foundational
    sketch. Sorted population partitioned into equi-depth buckets;
    `estimate_range(lo, hi)` interpolates linearly within partial
    buckets. Completes the selectivity-class coverage: equality
    (HLL), membership (Bloom), frequency (CMS), range (Histogram).
    6 unit tests pass.

### Confirmed (end-to-end feedback loop)

```
$ cargo run -p samkhya-bench -- calibrate --suite synthetic
=== phase 3: re-run with correction applied ===
query       raw_est    corrected       actual  qerr_before   qerr_after
------------------------------------------------------------------------
S1             2000          442         3925         1.96         8.88
S6             2000          442           51        39.22         8.67
S8             2000          442          433         4.62         1.02
...
avg q-error before: 15.27, avg q-error after: 6.19
queries improved: 2/10
```

Among the three queries where a meaningful comparison exists (raw
estimate > 0), the average q-error dropped from 15.27 to 6.19 (~2.5×
improvement). Two strictly improve (S6, S8); one over-corrects (S1).
The seven queries with `raw_est=0` stay at q-error ∞ because the
corrector's `baseline * exp(ratio)` rule preserves zero — an honest
limitation of feeding only `baseline_estimate` as the feature.

### Tests

- 82 tests pass workspace-wide.
- `cargo clippy --workspace -- -D warnings` clean.

## [0.2.0] — 2026-05-16

Third wave of the same scaffolding session. The hardest piece from
the 90-day MVP plan — actually making samkhya influence DataFusion's
cardinality estimates — lands. Adds more sketches, tighter bounds,
and broader test coverage.

### Added

- **samkhya-datafusion**
  - `physical_plan::SamkhyaStatsExec` — the `ExecutionPlan`-layer
    wrapper that actually flows samkhya-corrected statistics into
    DataFusion 46's physical plan. Passthrough wrapper: delegates
    schema/partitioning/execute to the inner exec, overrides only
    `statistics()`, preserves the override through
    `with_new_children` rewrites.
  - `SamkhyaTableProvider::scan()` now wraps the inner provider's
    exec with `SamkhyaStatsExec`. This is the actual injection path:
    DataFusion 46's mainline planner never consults
    `TableProvider::statistics()` (per upstream trait doc) — it calls
    `scan()` and propagates from `ExecutionPlan::statistics()` upward.
  - `SamkhyaOptimizerRule` now implements both `OptimizerRule` (logical,
    observe-only) and `PhysicalOptimizerRule` (physical pass that
    counts `SamkhyaStatsExec` leaves; exposes `samkhya_leaves_seen()`
    as a diagnostic).
  - `examples/stats_propagation_demo.rs` — proves the mechanism
    end-to-end. Output:
    ```
    without rule: 1000, with rule: 42
    samkhya_leaves_seen (physical pass): 1
    ```
  - `lib.rs` doc comment rewritten to describe the three-layer
    integration model (TableProvider wrapper → `scan()` overrides →
    `SamkhyaStatsExec` carries corrected stats up the plan tree).
- **samkhya-core**
  - `lpbound::ChainBound` — frequency-moment chain-join upper bound.
    For `R_i ⋈ R_j` on a key with `max(D_i, D_j) = D` distinct values,
    bound is `|R_i| * |R_j| / D`. Tighter than `AgmBound` for chain
    joins with known per-relation distinct counts. 4 unit tests +
    2 property tests.
  - `sketches::cms::CountMinSketch` — third foundational sketch
    (alongside HLL and Bloom). Depth × width counters; seeded XxHash
    per row for d independent hash functions; never undercounts.
    Useful for heavy-hitter detection in join keys. 6 unit tests +
    2 property tests.
- **samkhya-bench**
  - `compare --suite <name>` subcommand — runs the suite twice
    (baseline + samkhya-wrapped) and prints side-by-side tables.
  - 5 additional synthetic queries (S6–S10) covering:
    - selective single-table filters
    - 2-join with no selective predicate
    - anti-correlated predicates (correlation kills DF's estimate)
    - multi-predicate joined tables
    - 4-table chain with multiple correlated filters
  - The bench's samkhya-corrected mode now provides per-column
    `distinct_count` overrides (not just row counts) to feed
    DataFusion's selectivity estimator.
  - `tests/runner_smoke.rs` — 4 integration tests confirming the
    runner builds the synthetic context and executes all 10 queries
    end-to-end, persists feedback, and gracefully skips unexecutable
    suites.

### Confirmed

- The stats-propagation demo binary proves DataFusion 46 actually
  consumes the override: a 1000-row MemTable reports `num_rows=42`
  in the physical plan when wrapped with `SamkhyaTableProvider`
  + `SamkhyaOptimizerRule`.
- 60+ tests pass workspace-wide on default build.
- `cargo clippy --workspace -- -D warnings` clean.

### Known limitations carried over

- DataFusion 46's selectivity model does not appear to use
  `ColumnStatistics::distinct_count` for the queries in the synthetic
  suite, so the bench's `compare` output today shows identical numbers
  in baseline vs samkhya modes. The integration path is correct;
  real Puffin-sourced stats on parquet-on-S3 would differ from DF's
  defaults and the wrapping would move estimates accordingly.
- Full LpBound LP solver still pending (only `ProductBound`,
  `AgmBound`, `ChainBound` shipped).
- DuckDB cxx extension still a stub.
- TabPFN-style residual backend still planned only.

## [0.1.0] — 2026-05-16

Second wave of the same scaffolding session. Real implementations replace
several v0.0.1 stubs; the architectural skeleton is now an actually-running
end-to-end pipeline against DataFusion.

### Added

- **samkhya-core**
  - `residual::gbt` submodule behind the `gbt` cargo feature. `GbtCorrector`
    trains on `Observation` history; targets `log(actual/est)` regression;
    predictions clamp via `lpbound::saturating_clamp`. Backed by `gbdt-rs`
    (Baidu, pure-Rust). 4 additional tests under `--features gbt`.
  - `puffin` zstd compression behind the `zstd` cargo feature.
    `CompressionCodec::{None,Zstd}` enum; `add_blob_compressed` /
    `read_blob_decompressed` methods; metadata-driven codec dispatch.
    3 additional tests under `--features zstd`.
  - `CorrectionFeatures::to_vec()` + `FEATURE_LEN` — stable feature
    vector layout for residual model inputs (append-only).
  - `benches/sketches.rs` (9 cases) + `benches/puffin.rs` (3 cases) —
    criterion microbenchmarks. `cargo bench --no-run` compiles cleanly.
  - `tests/properties.rs` — 9 proptest properties (HLL relative error /
    merge commutativity / round-trip, Bloom no-FN / round-trip, Puffin
    round-trip, LpBound monotonicity / clamp invariants).
  - `tests/integration.rs` — end-to-end pipeline integration test
    (HLL → Puffin → ColumnStats → FeedbackStore → lpbound).
  - `examples/sketch_to_puffin.rs` — demo binary that exercises the
    sketch → Puffin → reopen → recover path and prints relative error.
- **samkhya-datafusion**
  - `SamkhyaTableProvider<T>` — primary integration pattern. Wraps any
    `Arc<dyn TableProvider>` and overrides `statistics()` with samkhya
    corrections. Builder API: `.with_column_stats(col_idx, ColumnStats)`.
    `stats_call_count()` test hook. All values marked `Precision::Inexact`.
  - `tests/wrap_provider.rs` — integration test confirming the wrapper
    is consulted via the `TableProvider` trait surface.
  - Documented caveat: DataFusion 46's mainline planner does not yet
    drive `TableProvider::statistics()`; the hook is shaped for
    downstream optimizer rules or future DF versions.
- **samkhya-bench**
  - Real DataFusion runner. Generates a deterministic synthetic retail
    OLAP schema (customers/products/orders/order_items at 1k/200/10k/30k
    rows) and registers it via `SessionContext`. In samkhya-corrected
    mode, wraps each MemTable with `SamkhyaTableProvider`.
  - For each query: builds the physical plan to extract the optimizer's
    row estimate, executes the query, counts actual rows, computes
    multiplicative q-error, records the observation to a
    `FeedbackStore`. Prints a per-query comparison table and
    avg/max q-error.
  - New `Synthetic` suite with 5 queries (S1–S5) covering single-filter
    and 2-/3-/4-join shapes with correlated predicates.
  - `run --feedback <path>` flag to persist observations to SQLite.
  - `report --feedback <path>` subcommand — summarizes the store
    per-template; lists every observation with q-error and latency.
  - `train --feedback <path> --template <hash>` subcommand stub —
    documents the path to wire the GBT corrector against feedback
    history once `samkhya-core --features gbt` is enabled in a
    downstream build.

### Changed

- `samkhya-core/Cargo.toml` grew optional `zstd` and `gbt` features,
  plus `criterion`, `tempfile`, and `proptest` dev-deps.
- `samkhya-datafusion/Cargo.toml` added `async-trait` dep.
- `samkhya-bench/Cargo.toml` added `datafusion 46`, `samkhya-datafusion`,
  `rand`, and `tokio` deps; the binary is now `#[tokio::main]`-ish
  (uses a manually-built multi-thread runtime).

### Confirmed (the gap samkhya targets)

Running the synthetic suite against DataFusion 46 reveals:

| query | estimated | actual | q-error |
|---:|---:|---:|---:|
| S1 (single-filter) | 2000 | 3925 | 1.96 |
| S2 (2-join) | 0 | 300 | ∞ |
| S3 (2-join) | 0 | 6924 | ∞ |
| S4 (4-join) | 0 | 761 | ∞ |
| S5 (3-join) | 0 | 5223 | ∞ |

DataFusion 46 returns 0 for the multi-join cardinality estimates — i.e.
no estimate at all — for queries that actually return hundreds to
thousands of rows. This is precisely the embedded-engine cardinality
estimation gap the project targets.

### Tests

- 51 tests pass workspace-wide on the default build.
- Adding `--features gbt zstd` adds 7 more (4 GBT + 3 zstd).
- `cargo clippy -- -D warnings` passes.

### Known limitations carried over

- DataFusion 46's mainline planner does not yet propagate
  `TableProvider::statistics()` into cardinality estimates, so today
  the baseline and samkhya-wrapped runs report the same numbers.
  Resolution paths: (a) a custom DataFusion `OptimizerRule` that rewrites
  scan stats, (b) waiting for a DF release that consumes the hook, or
  (c) wrapping at the `ExecutionPlan::statistics()` layer instead.
- LpBound is still the coarse AGM approximation; full LP solver pending.
- DuckDB extension remains a stub.

## [0.0.1] — 2026-05-16

Initial scaffolding release. Sets the architectural skeleton; most layers
are wired with minimal correct implementations rather than full
production behavior. The 90-day MVP plan in `samkhya.md` §4 governs what
graduates into v0.1.0.

### Added

- **Workspace** — Cargo workspace with 5 member crates, edition 2024,
  pinned to Rust 1.94 via `rust-toolchain.toml`.
- **samkhya-core**
  - `sketches::hll` — HyperLogLog (precision 4-18, xxhash, small-range
    correction, serde-backed wire format).
  - `sketches::bloom` — Bloom filter (Kirsch-Mitzenmacher double-hashing,
    serde).
  - `sketches::Sketch` — uniform `to_bytes` / `from_bytes` codec trait
    with stable `KIND` tags so blobs round-trip cross-engine.
  - `puffin` — Iceberg Puffin sidecar reader/writer with magic / footer
    JSON / blob index. Streaming writer + lazy reader.
  - `feedback` — SQLite-backed `(plan, estimate, actual)` observation
    store. In-memory and on-disk modes. q-error helper.
  - `lpbound` — `UpperBound` trait + `ProductBound` + coarse `AgmBound`
    + `clamp_estimate` / `saturating_clamp` helpers. Pessimistic
    envelope ensures correction can never breach the ceiling.
  - `residual` — `Corrector` trait + `IdentityCorrector` baseline.
    Real backends (GBT, TabPFN) deferred.
  - `stats::ColumnStats` — engine-agnostic column statistics surface
    (superset of DataFusion's `ColumnStatistics` and DuckDB's
    `BaseStatistics`).
  - `error::Error` — thiserror-based error type with `LpBoundExceeded`
    variant for envelope violations.
- **samkhya-datafusion** — `SamkhyaOptimizerRule` against DataFusion
  46.0.1 (`ApplyOrder::BottomUp`, `supports_rewrite = true`). Walks
  TableScans and is observe-only at v0.0.1 — returns `Transformed::no`,
  cold-start-safe. `stats_provider` converts `samkhya_core::ColumnStats`
  to DataFusion's `ColumnStatistics` with `Precision::Inexact`
  throughout per the LpBound conservative posture.
- **samkhya-py** — PyO3 0.22 bindings exposing `HllSketch`,
  `BloomFilter`, `ColumnStats`, plus a `SamkhyaError` Python exception.
  `crate-type = ["cdylib", "rlib"]`, abi3-py39 for a single wheel
  covering CPython 3.9+. maturin build config.
- **samkhya-bench** — clap CLI with `list-queries` / `run --suite
  <job-slow|tpc-h|stats-ceb> [--baseline]` / `report` subcommands.
  Five hand-written JOB-Slow queries bundled (1a, 2b, 6a, 17a, 29a).
  TPC-H + STATS-CEB placeholders.
- **samkhya-duckdb** — stub crate. Full DuckDB extension (Rust ↔ C++
  via cxx) is a samkhya.md §4 Months 4-6 deliverable.
- **CI** — GitHub Actions workflow: cargo fmt, clippy `-D warnings`,
  test on push/PR. Excludes `samkhya-duckdb` (C++ toolchain) and
  `samkhya-py` (Python deps). Swatinem/rust-cache@v2.
- **Docs** — `README.md`, `ARCHITECTURE.md` (422 lines + mermaid
  diagrams), `CONTRIBUTING.md`, `samkhya.md` (full research bootstrap,
  ~400 lines, 40-entry annotated bibliography).
- **Paper drafts** — `paper/abstract.md` (236-word arXiv abstract),
  `paper/title-options.md`, `paper/outline.md` for CIDR 2027 6-page
  submission (deadline 2026-08-04).
- **Quality config** — `rustfmt.toml`, `clippy.toml`, PR template, bug
  + feature issue templates.

### Tests

- 31 unit + integration tests pass workspace-wide
  - samkhya-core: 26 (sketches 6, puffin 7, feedback 4, lpbound 8,
    residual 1)
  - samkhya-datafusion: 4 unit + 2 smoke
- `cargo clippy --workspace --exclude samkhya-py --exclude samkhya-duckdb
   -- -D warnings` passes.
- `cargo fmt --all -- --check` passes (modulo nightly-only rustfmt
  warnings).

### Naming

- Project name locked to **Samkhya** (सांख्य — "enumeration / counting").
  Originally proposed as "Drishti" during the May 2026 research sweep;
  renamed for clean PyPI / crates.io / GitHub namespace and stronger
  semantic fit. Full reasoning in `samkhya.md` §3 and `CHANGELOG`
  v0.0.1 commit history.

### Known limitations

- **LpBound** — the shipped envelope is a coarse AGM approximation.
  Full ℓp-norm LP solver port from Zhang et al. SIGMOD 2025 is a
  v0.1.0 target.
- **DataFusion rule** — observe-only; does not yet inject corrected
  estimates into the optimizer beyond placeholder column stats.
- **Residual** — no real backends shipped; identity passthrough only.
- **JOB-Slow** — five queries bundled; full set (~113 queries) is
  pending. No baseline-vs-corrected runner yet.
- **DuckDB extension** — placeholder; cxx integration pending.
- **PyO3 0.22 + edition 2024** — produces benign warnings under Rust
  1.94 (`unsafe-op-in-unsafe-fn` from `#[pymethods]` macro). Tracked
  upstream in pyo3-rs/pyo3. No functional impact.

[Unreleased]: https://github.com/singhpratech/samkhya/compare/v1.2.3...HEAD
[1.2.3]: https://github.com/singhpratech/samkhya/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/singhpratech/samkhya/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/singhpratech/samkhya/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/singhpratech/samkhya/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/singhpratech/samkhya/compare/v1.0.0...v1.1.0
[v1.0.0-rc.2]: https://github.com/singhpratech/samkhya/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/singhpratech/samkhya/releases/tag/v1.0.0
[0.9.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.9.0
[0.8.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.8.0
[0.7.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.7.0
[0.6.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.6.0
[0.5.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.5.0
[0.4.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.4.0
[0.3.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.3.0
[0.2.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.2.0
[0.1.0]: https://github.com/singhpratech/samkhya/releases/tag/v0.1.0
[0.0.1]: https://github.com/singhpratech/samkhya/releases/tag/v0.0.1
