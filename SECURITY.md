# Security policy

samkhya is **pre-1.0 software**. The public API and on-disk formats (Puffin
sidecar layout, sketch payload codecs, SQLite feedback-store schema) may
still change in backwards-incompatible ways before v1.0. The supply-chain
guarantees in this document apply regardless of API stability.

Sole author and security contact: **Prateek Singh**
(`via GitHub Security Advisories on the singhpratech/samkhya repository`).

## Supported versions

samkhya follows the standard "current minor + previous minor" support
window during the pre-1.0 phase. Older releases receive no further
security backports — operators on those lines must upgrade.

| Version  | Supported          |
| -------- | ------------------ |
| 0.9.x    | yes (current)      |
| 0.8.x    | yes (previous)     |
| < 0.8.0  | no                 |

After v1.0 ships, this table will switch to the standard semver-stable
window: the current major + the previous major's last minor.

## Reporting a vulnerability

**Do not open a public GitHub issue for security reports.** Use one of the
following private channels:

1. **Preferred: GitHub Security Advisories** on
   [`singhpratech/samkhya`](https://github.com/singhpratech/samkhya). From
   the repository page → Security tab → Advisories → "Report a
   vulnerability". This is the canonical channel and the one wired into
   the release process.

2. **Fallback: email** `via GitHub Security Advisories on the singhpratech/samkhya repository` with subject prefix
   `[samkhya security]`. Plain text is fine; PGP is available on request.

Initial acknowledgement target: **3 business days**. Triage and remediation
plan: **14 business days** from acknowledgement.

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
  (e.g. an issue in `gbdt`, `good_lp`, or `rusqlite` that surfaces through
  samkhya) requires more time, the embargo may extend beyond 90 days. In
  that case we publish a security advisory at the original 90-day mark
  describing the *shape* of the issue and the affected version range,
  with full technical details deferred until the upstream fix lands.

A CVE will be requested for any vulnerability rated **medium or higher**
on the CVSS v3.1 scale.

## Scope

In-scope:

* All five published crates: `samkhya-core`, `samkhya-datafusion`,
  `samkhya-duckdb`, `samkhya-py`, `samkhya-bench`.
* The on-disk formats consumed by the above: Puffin sidecar layout,
  sketch payload codecs (`HllSketch::from_bytes`, `BloomFilter::from_bytes`,
  `CountMinSketch::from_bytes`, `EquiDepthHistogram::from_bytes`,
  `CorrelatedHistogram2D::from_bytes`), and the SQLite feedback-store
  schema. Panics on adversarial input to any of these are in scope.
* The build pipeline (CI workflows, `deny.toml`, `Cargo.lock`).

Out-of-scope:

* Issues that require the operator to deliberately misuse the API (e.g.
  passing trusted but malformed bytes through a path that's documented as
  trusted-input-only).
* Performance / DoS issues that don't violate a stated bound — samkhya's
  contract is correctness, not real-time performance. A bench regression
  is not a vulnerability.

## Operator obligations (pre-1.0)

samkhya's safety story is built around the LpBound clamp: every corrector
output is bounded above by a provable ceiling derived from sketch-level
statistics, and *that ceiling is what gates regression behavior*. Two
operator-side validation points are non-negotiable at the pre-1.0 stage:

1. **Validate the clamped ceiling against your own SLAs.** The default
   `Corrector` clamp uses the coarse `ProductBound` envelope; v0.5.0 will
   ship the real LP-derived `LpJoinBound`. Until then, operators running
   samkhya in production must benchmark the clamped output on a
   representative query mix and confirm the worst-case row-count estimate
   is within their planner's SLA. samkhya provides the mechanism; the
   operator's workload defines the threshold.
2. **Re-run the bench harness after every minor upgrade.**
   `cargo run -p samkhya-bench --release -- compare --suite synthetic` is
   the floor; operators should add their own engine-specific suite. A
   minor-version bump that holds the API surface stable may still change
   the *numeric* behavior of the corrector if a sketch precision or
   bound-construction detail moves.

These obligations relax at v1.0 once the API and the bound construction
are both frozen and the kill-criteria gate (ROADMAP §11) has been passed.

## Acknowledgements

Reporters who follow this policy in good faith will be credited in the
release notes and the published advisory, unless they request anonymity.

---

License: Apache-2.0. Sole author: Prateek Singh.
