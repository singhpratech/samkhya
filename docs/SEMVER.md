# samkhya — Semantic Versioning Policy

samkhya follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)
once v1.0.0 has shipped. The version number is `MAJOR.MINOR.PATCH`:

- **MAJOR** — breaking changes to anything in the *Public Surface* table below.
- **MINOR** — backwards-compatible additions to the Public Surface; bug
  fixes that change observable behavior in a way reviewers might call
  a soft break (e.g., a sketch's reported false-positive rate moves
  closer to the published Bloom CACM 1970 formula) ship in a MINOR.
- **PATCH** — bug fixes that don't change observable behavior; doc
  improvements; internal refactors; test additions; performance work
  that does not alter the wire format or trait surface.

The leading `v` is part of the **git tag** only (`v1.0.0`); inside
`Cargo.toml`, `package.json`, and the published crate, the bare
version string (`1.0.0`) is used.

---

## Pre-1.0 versioning (where we are now)

Until the first `v1.0.0` tag, samkhya is in the **pre-release**
iteration phase. Per Semver 2.0.0 §4, the public API SHOULD NOT be
considered stable before 1.0.0; in practice we treat each pre-1.0
release as a candidate that may break any surface between minor bumps
in order to land the right v1.0 shape. Pre-1.0 tags ship as
`v0.MAJOR.MINOR` (e.g., `v0.4.0`) with no patch component.

When v1.0.0 is tagged (gated by explicit maintainer signal — see the
[release gate](#release-gate) section), the Public Surface becomes
locked: subsequent breaks bump MAJOR, not minor.

---

## Public Surface (the contract semver guards)

Everything below is in-scope for the semver contract. Anything *not*
listed is internal and may change between any two patch releases
without notice.

| Category | Item | Why it's in-scope |
|---|---|---|
| **Rust trait surfaces** | `samkhya_core::residual::Corrector`, `samkhya_core::residual::CorrectionFeatures` (field names + arity), `samkhya_core::sketches::Sketch` | These are the integration seams every downstream engine plugs into. |
| **Crate roots — re-exports** | The set of types re-exported at the root of every `samkhya-*` crate (the names downstream `use samkhya_core::Foo;` lines actually resolve to). | A removed re-export breaks downstream builds. |
| **Cargo features** | The set of feature names, the default-features value, and what each feature gates. | Renaming a feature breaks every downstream `Cargo.toml`. |
| **Puffin sidecar wire format** | Magic bytes, header layout, sketch-payload encoding, per-sketch `serialize`/`from_bytes` contract. | Sidecars are written once and read many; old readers must keep working with new sidecars within the same MAJOR. |
| **LLM inference server wire contract** | `POST /infer` request/response schema (field names, types, ranges), `GET /health` schema. Both Python (`llm_infer_server.py`) and TypeScript (`llm_infer_server.ts`) ports. | Engines depend on this; the contract is what makes the corrector pluggable. |
| **TabPFN inference server wire contract** | `POST /infer` + `GET /health` for `samkhya-gpudb/scripts/tabpfn_infer_server.py`. | Same reason as LLM. |
| **CLI argument surface** | `samkhya-cli` flag names, `samkhya-bench` flag names, exit-code semantics. | Reproducer scripts in the wild hard-code these. |
| **Sketch on-disk encoding** | `HllSketch::from_bytes`, `BloomFilter::from_bytes`, `CountMinSketch::from_bytes`, `EquiDepthHistogram::from_bytes`, `CorrelatedHistogram2D::from_bytes` — bytes written by version N must decode under version N+PATCH within the same MAJOR. | Sketches are persisted into Puffin sidecars and replayed; cross-version readability is a hard requirement. |
| **Environment variables** | `SAMKHYA_LLM_BACKEND`, `SAMKHYA_LLM_MODEL`, `SAMKHYA_LLM_TEMPERATURE`, `SAMKHYA_LLM_MAX_TOKENS`, `SAMKHYA_LLM_HOST`, `SAMKHYA_LLM_PORT`, `SAMKHYA_LLM_LOCAL_URL`, `TABPFN_HOST`, `TABPFN_PORT`, `TABPFN_DEVICE`, `TABPFN_SUPPORT`. | Operator-facing knobs; renames break running deployments. |
| **MSRV (Minimum Supported Rust Version)** | The minimum `rustc` version every crate compiles with. | Bumping MSRV is a MINOR change with a [release-notes call-out](https://blog.rust-lang.org/category/community.html); a *drop* of MSRV support is a MAJOR. |

---

## Out of scope (free to change in any release)

- **Empirical numbers in `bench-results/*.md`** — measurements are
  reproducible artifacts of a specific hardware + dataset; they
  evolve with each campaign and are not semver-stable.
- **Internal struct field layouts** that are not `pub` at a crate
  root.
- **Internal helper modules** (anything inside a `mod foo;` that
  isn't re-exported).
- **Test fixtures and dev-dependency versions.**
- **Documentation text** (the wire contract itself is stable; the
  prose explaining it is not).
- **The CSV → Parquet converter binary** (`samkhya-bench/src/csv_to_parquet.rs`)
  — bench-internal tooling.
- **Internal corrector implementations** like `GbtCorrector`'s
  feature engineering or training procedure — what *is* stable is
  the `Corrector` trait; the implementation is free to change.

---

## What constitutes a "break" inside the Public Surface

For Rust types and traits, a *break* is anything Cargo would flag as
a semver violation under [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks):

- Removing a `pub` item.
- Renaming a `pub` item without a `#[deprecated]` re-export bridge.
- Adding a required generic parameter or required method to a `pub` trait.
- Tightening a function signature in any direction the caller can't satisfy.
- Changing the variant set of a `pub` enum that is not marked `#[non_exhaustive]`.

For wire protocols, a *break* is anything that would cause a
correct-by-previous-spec client to receive a non-2xx response or
parse failure that previously succeeded.

---

## MSRV policy

samkhya supports the latest stable Rust at v1.0.0 release time, and
the two prior `1.x` rustc lines (the "N, N-1, N-2" window). Bumping
the floor of that window is a **MINOR** change. Dropping a still-in-window
stable line is a **MAJOR** change. The MSRV is recorded in the workspace
`Cargo.toml`'s `rust-version` field and in `samkhya-core`'s README.

---

## Release gate

samkhya's release process gates every published version on **explicit
maintainer signal**, regardless of what semver would mechanically
permit. The release tag is never created automatically by CI; the
maintainer signs the tag manually. There is no `--auto-release`
mode in this project's CI, and there will not be one in v1.x.

This is intentional. Per-release human review is the last empirical
gate before a number is locked into the supply chain.

---

## Yanking

A published crate version that ships a wire-contract regression, a
build break on a MSRV target, or a correctness bug in a sketch
encoder may be **yanked** from crates.io. Yanked versions remain
downloadable for pinned consumers but are excluded from default
resolver semver selection. Every yank is accompanied by a follow-up
PATCH release that fixes the underlying issue, and an entry in
`CHANGELOG.md` documenting the yank and the fix.

---

## Pre-release suffix

Pre-release builds use the standard Semver pre-release identifier
syntax:

- `1.0.0-rc.1`, `1.0.0-rc.2`, ... — release candidates before v1.0.0.
- `1.x.0-alpha.N`, `1.x.0-beta.N` — feature previews after v1.0.0.

Pre-release identifiers sort lower than the corresponding release
(`1.0.0-rc.1` < `1.0.0`), as Semver 2.0.0 §11 specifies. Cargo's
default resolver excludes pre-releases unless the dependency
explicitly opts in (`version = "1.0.0-rc.1"`); we rely on this.

---

## Build metadata

samkhya does not use Semver build-metadata identifiers (the `+abc`
suffix). Every published crate version has a deterministic content
addressed by its `MAJOR.MINOR.PATCH[-pre]` string alone.

---

## Reference

- [Semver 2.0.0 specification](https://semver.org/spec/v2.0.0.html)
- [Cargo's semver compatibility chapter](https://doc.rust-lang.org/cargo/reference/semver.html)
- [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks)
- [Rust release-train](https://forge.rust-lang.org/release/process.html)
