# Security policy

samkhya is **v1.0+ software**. The public API and on-disk formats (Puffin
sidecar layout, sketch payload codecs, SQLite feedback-store schema) are
covered by semver. Breaking changes require a major-version bump and the
deprecation window in `docs/SEMVER.md`. The supply-chain guarantees in
this document apply across all supported lines.

Sole author and security contact: **Prateek Singh** (via GitHub Security
Advisories on the `singhpratech/samkhya` repository — see the
"Reporting a vulnerability" section below). **GHSA-only intake** — do not
contact the maintainer by email for security issues.

## Supported versions

samkhya follows the standard semver "current major + previous major's
last minor" support window. Older releases receive no further security
backports — operators on those lines must upgrade.

| Version  | Supported          |
| -------- | ------------------ |
| 1.0.x    | yes (current)      |
| < 1.0.0  | no                 |

The previous-major row will populate once v2.0 ships.

## Reporting a vulnerability — GHSA only

**Do not open a public GitHub issue for security reports, and do not
contact the maintainer by email.** This project uses a **GitHub Security
Advisory (GHSA) only** disclosure channel. The canonical URL pattern is:

    https://github.com/singhpratech/samkhya/security/advisories/new

From the repository page → **Security** tab → **Advisories** → **Report
a vulnerability**. This is the only supported intake channel; it is the
one wired into the release process and the only channel for which an
acknowledgement SLA is committed.

Initial acknowledgement target: **3 business days**. Triage and
remediation plan: **14 business days** from acknowledgement.

## Embargo policy

**Standard embargo: 90 days** from acknowledgement to public disclosure.
This matches the convention used by the broader Rust ecosystem (RustSec
advisory database, cargo / crates.io security team) and gives downstream
embedders (samkhya-datafusion adapter users, samkhya-duckdb extension
users, samkhya-py wheel consumers) time to roll out fixes.

The embargo is **negotiable** in either direction:

* **Shorter** — when the vulnerability is already being actively
  exploited, or when the reporter has a public-talk deadline they've
  cleared with us, the embargo may be reduced (often to 7-14 days).
* **Longer** — when coordinated disclosure with an upstream dependency
  (e.g. an issue in `gbdt`, `good_lp`, or `rusqlite` that surfaces
  through samkhya) requires more time, the embargo may extend beyond 90
  days. In that case we publish an advisory at the original 90-day mark
  describing the *shape* of the issue and the affected version range,
  with full technical details deferred until the upstream fix lands.

A CVE will be requested for any vulnerability rated **medium or higher**
on the CVSS v3.1 scale.

## Scope

In-scope:

* All 12 publishable workspace crates (the 13th, `samkhya-it`, is
  `publish = false` integration-test harness):
  `samkhya-core`, `samkhya-cli`, `samkhya-arrow`, `samkhya-bench`,
  `samkhya-datafusion`, `samkhya-duckdb`, `samkhya-duckdb-ext`,
  `samkhya-polars`, `samkhya-iceberg`, `samkhya-postgres`, `samkhya-gpudb`,
  `samkhya-py`.
* The on-disk formats: Puffin sidecar layout and every sketch payload
  codec (`HllSketch::from_bytes`, `BloomFilter::from_bytes`,
  `CountMinSketch::from_bytes`, `EquiDepthHistogram::from_bytes`,
  `CorrelatedHistogram2D::from_bytes`) plus the SQLite feedback-store
  schema. **Every `from_bytes` constructor is adversarial-input scope:**
  any panic, OOB read, allocator-DoS, or silent corruption on attacker
  -supplied bytes is a security bug.
* The build pipeline (CI workflows, `deny.toml`, `Cargo.lock`).

Out-of-scope:

* Bugs in **transitive dependencies**. RUSTSEC advisories suppressed in
  `deny.toml` are documented per the project's advisory-triage
  policy: every ignore entry carries
  an unreachability argument *and* a retire-condition (the upstream PR,
  the version we bump to, or the audit task that retires the ignore).
  Operators should re-run `cargo deny check advisories` after every
  pull.
* The **v1.0 `samkhya-duckdb-ext` static-link path** (the C++ extension
  glue) is out of scope for this policy revision; it ships behind a
  feature flag and will get its own threat model when the static-link
  story stabilises in v1.0.
* **Every corrector backend equally — operator-chosen.** The `Corrector`
  trait surface (GBT default, AdditiveGBT opt-in, TabPFN-2.5 opt-in
  via `tabpfn_http`, LLM TODO v1.1) is opt-in and operator-controlled.
  Network behaviour, credential handling (including `TABPFN_TOKEN`),
  telemetry disablement (`TABPFN_DISABLE_TELEMETRY=1`), and the choice of
  inference endpoint are operator concerns. samkhya commits only that
  every non-default backend is gated behind an explicit Cargo feature
  flag and is off in the default build. **Operator obligation:**
  corrector-backend selection is an operator decision; the framework
  does not pick for you.
* Issues that require the operator to deliberately misuse the API (e.g.
  passing trusted but malformed bytes through a path that's documented
  as trusted-input-only).
* Performance / DoS issues that don't violate a stated bound — samkhya's
  contract is correctness, not real-time performance. A bench regression
  is not a vulnerability.

## Adversarial-input invariants

Every `from_bytes` constructor performs **structural-invariant
validation after deserialisation**, not just byte-length checks. The
current invariant set:

* **HllSketch** — rejects 16-byte all-zero payloads; validates the
  register width and precision header before allocating the register
  array.
* **EquiDepthHistogram** — rejects 4 MiB all-zero payloads; validates
  bucket-count monotonicity and bin-edge ordering before accepting the
  histogram.
* **BloomFilter** — rejects byte vectors whose length does not match
  `ceil(num_bits / 8)`; validates `num_hashes > 0` and that the bit
  layout matches the stored `num_bits`.
* **CountMinSketch** — validates `depth × width == counter array
  length` and rejects depth-zero or width-zero payloads.
* **CorrelatedHistogram2D** — validates the 2-D bucket grid against the
  declared row × column counts; rejects mismatched bin-edge arrays.

These checks were tightened in the path from v0.4 to v1.0 — the
goal is that *no* attacker-supplied byte sequence reaches the sketch
internals without first being structurally validated.

## Allocator-DoS guard

The `BloomFilter::try_new` constructor caps requested sizing at
`MAX_NUM_BITS = 2^33` bits (~1 GiB) and returns a typed error rather
than allocating. The pre-cap formula `num_bits = -n · ln(fp) / ln(2)^2`
is unbounded as `fp_rate → 0`: an attacker-controlled `fp_rate ≈ 0` on
the previous API could drive multi-EiB allocation attempts. The cap
fails closed (returns `Err`) rather than silently clamping, so callers
get a clear signal that their parameters are out of range.

## Fuzz coverage (continuous)

samkhya ships **7 cargo-fuzz targets** that exercise every adversarial
-input surface:

* `puffin_reader` — Puffin sidecar parser
* `sketch_decoder` — generic sketch payload dispatch
* `fuzz_hll_parse` — `HllSketch::from_bytes`
* `fuzz_bloom_parse` — `BloomFilter::from_bytes`
* `fuzz_cms_parse` — `CountMinSketch::from_bytes`
* `fuzz_equidepth_parse` — `EquiDepthHistogram::from_bytes`
* `fuzz_correlated_parse` — `CorrelatedHistogram2D::from_bytes`

Per the H01 fortress run (bench-results/H01_samkhya_core_fortress.md):
**60 s × 7 targets = ~31.4 M total executions, 0 crashes, 0 OOMs**.
This is the floor, not the ceiling.

**CI nightly obligation.** A scheduled CI workflow runs every fuzz
target for at least 60 s on every nightly build; a new crash artefact
is a release blocker. The 60 s budget is the *minimum* — the nightly
job also runs an extended 30-minute pass on a rotating target so that
each target gets ≥3 h of cumulative fuzz time per week.

## Sanitizer coverage (continuous)

Per B11 (bench-results/B11_sanitizer.md):

* **ASAN** — clean on all `samkhya-core` lib tests.
* **MIRI** — green on the deterministic test subset (sketch codecs,
  Puffin reader, LpBound construction).
* **LSan** — clean (no leaks in lib tests).
* **MSan** — green on the subset that builds with MSan-instrumented
  std.

**CI nightly obligation.** The sanitizer matrix runs on the same
nightly schedule as the fuzz job. A regression in any of the four
sanitizers is a release blocker.

## Operator obligations

samkhya's safety story is built around the LpBound clamp: every
corrector output is bounded above by a provable ceiling derived from
sketch-level statistics, and *that ceiling is what gates regression
behavior*. Two operator-side validation points:

1. **Validate the clamped ceiling against your own SLAs.** The default
   `Corrector` clamp uses `LpJoinBound` (shipped v1.0; the coarse
   `ProductBound` / `AgmBound` / `ChainBound` triple remains the
   solver-failure fallback). Operators running samkhya in production
   should benchmark the clamped output on a representative query mix
   and confirm the worst-case row-count estimate is within their
   planner's SLA. samkhya provides the mechanism; the operator's
   workload defines the threshold.
2. **Re-run the bench harness after every minor upgrade.**
   `cargo run -p samkhya-bench --release -- compare --suite synthetic`
   is the floor; operators should add their own engine-specific suite.
   A minor-version bump that holds the API surface stable may still
   change the *numeric* behavior of the corrector if a sketch precision
   or bound-construction detail moves.
3. **Choose your corrector backend.** GBT default is the safe production
   choice. TabPFN-2.5 requires `TABPFN_TOKEN` license acceptance and is
   an opt-in research evaluator. LLM backend slot is forward-pointing
   to v1.1.

## Acknowledgements

Reporters who follow this policy in good faith will be credited in the
release notes and the published advisory, unless they request
anonymity.

---

License: Apache-2.0. Sole author: Prateek Singh.
