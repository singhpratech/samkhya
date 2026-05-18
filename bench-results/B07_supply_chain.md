# B07 — Supply Chain (cargo audit + cargo deny)

**Date:** 2026-05-16
**Verdict:** PASS (all 4 cargo-deny sections green; all real advisories triaged + documented)
**Note:** the original B07 agent never wrote its file; this is the inline reconstruction run.

## Tool versions

| tool         | version |
| ------------ | ------- |
| cargo-audit  | 0.22.1  |
| cargo-deny   | (latest stable via `cargo install --locked cargo-deny`) |

## Advisories surfaced

| ID                  | Crate          | Severity     | Path                                          | Disposition |
| ------------------- | -------------- | ------------ | --------------------------------------------- | ----------- |
| RUSTSEC-2021-0127   | serde_cbor 0.11.2 | unmaintained | pgrx 0.12.9 → samkhya-postgres            | **ignored** — transitive via pgrx; not reached from samkhya code; retires when pgrx ≥ 0.13. |
| RUSTSEC-2024-0379   | fast-float 0.2.0  | unsound      | polars 0.44.2 → samkhya-polars            | **ignored** — transitive; samkhya code never invokes polars CSV parser; retires on polars → fast-float2 migration. |
| RUSTSEC-2025-0003   | fast-float 0.2.0  | vuln (segfault) | same as RUSTSEC-2024-0379                | **ignored** — same path / retire condition. |
| RUSTSEC-2024-0436   | paste 1.0.15      | unmaintained | datafusion 46 → samkhya-datafusion        | **ignored** — transitive; no samkhya code uses `paste!`; retires when DF moves to pastey. |
| RUSTSEC-2025-0020   | pyo3 0.22.x       | vuln (PyString::from_object buffer overflow) | samkhya-py | **ignored** — verified via grep that samkhya-py uses neither `PyString` nor `from_object`; vulnerable code path unreachable; retires when samkhya-py v1.1 bumps to pyo3 0.24.1+. |
| RUSTSEC-2025-0141   | bincode 1.3.3     | unmaintained | samkhya-core (direct)                       | **ignored** — upstream team declares 1.3.3 a complete release; samkhya uses bincode only to (de)serialise its own sketch payload bytes that it itself wrote and validates against the Puffin KIND tag on read; no untrusted-input deserialization path. v1.1 evaluates postcard / rkyv / bitcode alternatives. |

Each ignore is recorded in `deny.toml` with a multi-line justification block explaining the path, the reason the vulnerable code path is unreachable, and the upstream condition that retires the ignore.

## cargo deny — final state

```
advisories ok, bans ok, licenses ok, sources ok
```

All 4 sections clear after:

1. Path-only dep `samkhya-datafusion = { path = "../samkhya-datafusion" }` in samkhya-bench was tightened with `version = "1.0"` (path-only deps register as wildcards under deny).
2. Duplicate-version skips added for cpufeatures, getrandom (0.2 / 0.3), itertools (0.10 / 0.13), r-efi, twox-hash, wit-bindgen (0.51 / 0.57) — every entry justified inline.

## License inventory

cargo-deny's licenses check walks the entire transitive graph; the allowlist is the standard Apache-2.0 / MIT / BSD-2/3 / ISC / Unicode / Zlib / CC0 set used by rustc / DataFusion / Servo. No copyleft, no unknown license appears in the graph.

## Tree health

| metric                                        | value      |
| --------------------------------------------- | ---------- |
| total transitive deps (default features)       | 673        |
| unique direct deps in workspace                | ~40        |
| publishable crates                            | 11 of 13   |

## Verdict

**PASS** — the v1.0 supply-chain surface is clean under reviewer-grade automated checks. Every advisory in the active graph either has no reachable vulnerable code path or is in upstream-only territory waiting on a release the samkhya team does not control. Every ignore is annotated with its retire condition so the next maintainer (or the same one, six months later) can re-evaluate without re-reading the original audit run.
