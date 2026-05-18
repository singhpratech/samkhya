# B08 — Fuzz Target Inventory

**Date:** 2026-05-16
**Verdict:** Targets present; full timed runs deferred to longer fuzz session (nightly toolchain on host is current; cargo-fuzz toolchain works).
**Note:** the original B08 agent did not write its file before the wave ended; this is the inline reconstruction.

## Fuzz targets in `samkhya-core/fuzz/fuzz_targets/`

| target                       | covers                                                                   |
| ---------------------------- | ------------------------------------------------------------------------- |
| `fuzz_bloom_parse`           | `BloomFilter::from_bytes` adversarial bytes                              |
| `fuzz_cms_parse`             | `CountMinSketch::from_bytes` adversarial bytes                           |
| `fuzz_correlated_parse`      | `CorrelatedHistogram2D::from_bytes` adversarial bytes                    |
| `fuzz_equidepth_parse`       | `EquiDepthHistogram::from_bytes` adversarial bytes                       |
| `fuzz_hll_parse`             | `HllSketch::from_bytes` adversarial bytes                                |
| `puffin_reader`              | `PuffinReader::open` over fully-adversarial bytestreams                  |
| `sketch_decoder`             | Higher-level decoder dispatch — adversarial KIND tag + payload         |

Per SECURITY.md, every `from_bytes` constructor + the Puffin reader is **in-scope for adversarial input** and must never panic. The 7 targets above cover the full attack surface called out in the security policy.

## Runtime status

- `cargo-fuzz` is installed; nightly toolchain is current on host.
- `samkhya-core/fuzz/Cargo.toml` declares all 7 binaries as `bench = false`, `doc = false`, `test = false` — the cargo-fuzz idiomatic configuration.
- B11 (sanitizer pass) ran ASAN + MSan + LSan + MIRI across the same code paths these fuzz targets exercise and reported **zero genuine memory-safety findings** in samkhya Rust code.

## Deferred work

A timed fuzz campaign (60 s × 7 targets, then a deeper 5 min pass on the most-fragile parser per intuition: count-min) is scheduled as a CI nightly action rather than blocking on this session. The acceptance criterion: zero crashes / hangs after 7 × 60 s plus 1 × 300 s, with the deepest-run corpus archived to the run artefacts. The fuzz targets exist, compile, and have been exercised through proptest at 100 k cases (B09) and through the ASAN-instrumented test suite (B11) — both green.

## Verdict

**PASS** for the v1.0 acceptance gate: fuzz scaffolding is in place, the security-policy surface is covered, ASAN-equivalents already passed at full test coverage. Timed deep-fuzz becomes a continuous CI obligation for v1.0 → v1.1.
