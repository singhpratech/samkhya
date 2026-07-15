# v1.1.0 Local Release Candidate

This receipt tracks the local final-version candidate. Package metadata is
`1.1.0`, while the changelog remains `[Unreleased]`; no tag, GitHub release, or
registry publication is authorized by this document.

## Scope

- Safe DataFusion pre-join correction path.
- Strict, snapshot-aware Puffin transport through Apache Iceberg.
- Shared HLL/histogram consumption in DataFusion and the DuckDB client API.
- Cross-ecosystem version and artifact checks.

Native DuckDB optimizer injection, PostgreSQL promotion, the full JOB/TPC-H
campaign, and model lifecycle telemetry are explicitly deferred.

## Verification Receipt

| Gate | Result |
| --- | --- |
| Version synchronization across Rust, Python, Node, and fuzz lock | PASS (1.1.0) |
| Core unit/property/doc tests and frozen v1 payloads | PASS (2026-07-14) |
| Iceberg strict/adversarial and feature tests | PASS (2026-07-14) |
| Puffin cross-engine release fixture | PASS (2/2) |
| DataFusion portable-binding focused tests | PASS (4/4) |
| DuckDB no-feature portable consumer tests | PASS (5/5) |
| Workspace format, Clippy, and default tests | PASS |
| Rust 1.85 default-feature compatibility | PASS |
| Semver compatibility against `v1.0.0` | PASS |
| Seven nightly fuzz targets, 60 seconds each | PASS; no crashes or crash artifacts |
| Dependency advisories, bans, licenses, and sources | PASS (`cargo deny`) |
| Python 3.12 wheel install, metadata, and pytest | PASS (10/10) |
| Python 3.9/3.13 wheels | Hosted CI gates configured |
| Python and TypeScript HTTP wire contracts | PASS |
| Publishable Rust package archives | PASS (11/11 compile-verified) |
| SHA-256 manifest and CycloneDX SBOM | Manual hosted workflow configured |

Canonical interoperability command:

```bash
cargo +1.94 test --locked -p samkhya-it \
  --features puffin-cross-engine --test puffin_cross_engine
```

The fixture uses a real Iceberg `TableMetadata`, excludes a stale statistics
file, reads the current sidecar through `FileIO`, rejects a current-snapshot
blob with the wrong sequence number, verifies core↔Apache Puffin in both
directions, maps field ID 17 to DataFusion ordinal 0, and confirms both engines
return unchanged query results.
