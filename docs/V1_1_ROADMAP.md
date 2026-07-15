# v1.1 Release Scope

## Release Goal

v1.1 promotes a safe DataFusion planning path and a production-tested portable
statistics handoff. It preserves v1.0 public APIs and payload codecs while
making one Puffin sidecar consumable through Iceberg, DataFusion, and DuckDB.

## Must-Ship Outcomes

### Production DataFusion Path

- Install `SamkhyaPreJoinRule` immediately before DataFusion's
  `join_selection` rule.
- Keep native estimates as the default floor and expose correction metrics.
- Prove rule ordering, plan impact, ceiling/floor behavior, model-error
  fallback, and unchanged query results.

Status: implemented and covered by `pre_join_corrector.rs`.

### Portable-Statistics Golden Path

- Write HLL and histogram blobs to one snapshot-aware Puffin sidecar.
- Discover only the current file from real Iceberg table metadata and read it
  through Iceberg's configured `FileIO`.
- Bind Iceberg field IDs to DataFusion ordinals explicitly; expose the same
  validated HLL/histogram to the DuckDB client API.
- Reject corrupt known payloads, duplicates, schema/snapshot/sequence
  mismatches, and explicit future schema versions; skip unknown kinds.
- Preserve frozen v1 payload compatibility and verify core↔Apache Iceberg
  reader/writer interoperability.

Status: implemented; release gate is
`cargo +1.94 test -p samkhya-it --features puffin-cross-engine --test puffin_cross_engine`.

### Release Engineering

- Keep MSRV, optional-feature, Python-wheel, TypeScript, wire-contract, and
  supply-chain jobs green.
- Enforce Rust/Python/Node/fuzz-lock version consistency.
- Build package and wheel archives from the tested commit, generate checksums
  and an SBOM, and smoke-test installed artifacts before publishing.

Status: automated CI gates are present; registry publication and tagging remain
manual, deliberate release-owner actions.

## Deferred Beyond v1.1

- The complete JOB-Slow and TPC-H paired empirical campaign.
- Versioned model artifacts, plan-node observation revisions, and shadow/apply
  lifecycle telemetry.
- Native DuckDB optimizer hooks, pending a stable upstream surface.
- PostgreSQL extension promotion and GPU-kernel claims.

These are not v1.1 deployment claims. They should be scheduled independently
and promoted only with their own machine-readable evidence.
