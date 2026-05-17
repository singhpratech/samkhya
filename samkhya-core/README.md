# samkhya-core

[![crates.io](https://img.shields.io/crates/v/samkhya-core.svg)](https://crates.io/crates/samkhya-core)
[![docs.rs](https://docs.rs/samkhya-core/badge.svg)](https://docs.rs/samkhya-core)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

The foundational crate of the [samkhya](https://github.com/singhpratech/samkhya)
project — portable, feedback-driven cardinality correction for embedded
analytical engines.

`samkhya-core` is engine-agnostic. It contains every primitive that the
per-engine adapters (`samkhya-datafusion`, `samkhya-duckdb-ext`,
`samkhya-polars`, `samkhya-postgres`, `samkhya-py`) build on top of: the four
foundational sketches, the 2D correlated histogram, the Puffin sidecar
reader/writer, the LpBound envelope family, the feedback store, and the
residual corrector trait. Nothing here links to a specific query engine.

## What this crate provides

```
samkhya-core
├── sketch
│   ├── HllSketch                    HyperLogLog (distinct count)
│   ├── BloomFilter                  membership
│   ├── CountMinSketch               point frequency
│   ├── EquiDepthHistogram           1D range
│   └── CorrelatedHistogram2D        2D joint distribution
├── puffin
│   ├── PuffinReader / PuffinWriter  Iceberg Puffin v1 sidecars
│   └── KIND tags                    samkhya.hll-v1, .bloom-v1, .cms-v1, ...
├── lpbound
│   ├── ProductBound                 coarse n_1 * n_2 * ... ceiling
│   ├── AgmBound                     AGM fractional edge cover
│   ├── ChainBound                   chain-join specialisation
│   └── LpJoinBound                  LP-derived (feature `lp_solver`)
├── feedback
│   ├── FeedbackStore                SQLite-backed observation log
│   └── TemplateHash                 query-template fingerprint
└── corrector
    ├── IdentityCorrector            no-op (passes the clamped ceiling)
    ├── GbtCorrector                 gradient-boosted residual
    ├── AdditiveGbtCorrector         per-template additive residual
    └── (trait Corrector)            implement your own
```

## Quick start

```rust
use samkhya_core::sketch::HllSketch;

let mut hll = HllSketch::new(14);
for i in 0..10_000u64 {
    hll.add(&i.to_le_bytes());
}
let estimate = hll.estimate();
assert!((estimate as i64 - 10_000).abs() < 200); // ~0.5% rel err at p=14

let bytes = hll.to_bytes();
let restored = HllSketch::from_bytes(&bytes).unwrap();
assert_eq!(restored.estimate(), estimate);
```

A larger end-to-end example — building four sketches, writing them to a
Puffin sidecar, and reading them back — is at
[`examples/sketch_to_puffin.rs`](examples/sketch_to_puffin.rs).

## Cardinality envelope

Every corrector output in samkhya is clamped above by a provable ceiling
derived from sketch-level statistics. The four envelopes form a strict
ordering on tightness:

```
LpJoinBound  <=  AgmBound  <=  ChainBound  <=  ProductBound
(tightest)                                     (loosest)
```

`LpJoinBound` requires the `lp_solver` feature (pulls in `good_lp` +
`microlp`). The default build ships `ProductBound`/`AgmBound`/`ChainBound`
without any LP dependency.

## Feature flags

| flag                | default | what it adds                                          |
| ------------------- | ------- | ----------------------------------------------------- |
| `lp_solver`         | off     | `LpJoinBound` via `good_lp` + `microlp`               |
| `gbt`               | on      | `GbtCorrector` via `gbdt`                             |
| `tabpfn_http`       | off     | `TabPfnHttpCorrector` (foundation-model HTTP backend) |
| `iceberg_compat`    | on      | Puffin sidecar reader strictness for Iceberg payloads |

Disabling `gbt` removes the only ML dep; pure-sketch deployments can do that.

## Safety / format stability

All `from_bytes` constructors take untrusted input and are in-scope for the
project's SECURITY.md. They are fuzzed (`cargo fuzz`) on every release and
must never panic on adversarial bytes — they return `Err` instead.

Sketch payload codecs and the Puffin KIND tags are pinned at v1 for the
v1.x line. Format bumps will use new `kind`s (`samkhya.hll-v2`, …) and the
reader's coexistence contract: unknown kinds are skipped, never errored.

## Integration

`samkhya-core` is the only crate the engine adapters depend on. If you're
embedding samkhya into a new engine, start by depending on this crate and
mirroring the integration pattern in `samkhya-datafusion`.

## License

Apache-2.0. Sole author: Prateek Singh.
