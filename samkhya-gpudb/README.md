# samkhya-gpudb

[![crates.io](https://img.shields.io/crates/v/samkhya-gpudb.svg)](https://crates.io/crates/samkhya-gpudb)
[![docs.rs](https://docs.rs/samkhya-gpudb/badge.svg)](https://docs.rs/samkhya-gpudb)
[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/singhpratech/samkhya/blob/main/LICENSE)

GPU batch-inference adapter for samkhya. Defines the `GpuCorrector` trait —
the batch sibling of `samkhya_core::residual::Corrector` — and ships a CPU
fallback so downstream code can exercise the surface today without a CUDA
toolkit or a Metal framework on the build host.

Part of the [samkhya](https://github.com/singhpratech/samkhya) project —
portable, feedback-driven cardinality correction for embedded analytical
engines.

## What this crate provides

- **`GpuCorrector`** — the batch-scoring trait. Implementations score a slice
  of `CorrectionFeatures` in a single call; real GPU backends dispatch one
  CUDA / Metal kernel launch over the entire batch. The contract mirrors the
  single-row `Corrector` trait in `samkhya-core` but in batch form: the
  output `Vec<u64>` is parallel to the input slice and uses LpBound-clamped
  row-count estimates.
- **`CpuFallbackCorrector`** — a zero-sized struct that walks the batch
  sequentially and echoes each row's `baseline_estimate`, matching the
  behavior of `samkhya_core::residual::IdentityCorrector`. Useful for
  keeping the trait honest in CI, for local development without a GPU, and
  as a safe default for engines that opt into the batch surface before
  wiring up real kernels.

## Why batch GPU inference

Subplan enumeration is embarrassingly parallel: each candidate is an
independent forward pass through a sub-MB residual model (GBT or PFN-style).
CPU inference walks candidates one at a time; a single GPU kernel batches
the lot. The companion [gpudb](https://github.com/singhpratech/gpudb)
project targets CUDA and Apple Silicon Metal, and this crate is the
integration surface between samkhya's residual corrector and that batch
path.

## Quick start

```rust
use samkhya_core::residual::CorrectionFeatures;
use samkhya_gpudb::{CpuFallbackCorrector, GpuCorrector};

let corrector = CpuFallbackCorrector::new();
let batch = vec![
    CorrectionFeatures { baseline_estimate: 10,    ..Default::default() },
    CorrectionFeatures { baseline_estimate: 250,   ..Default::default() },
    CorrectionFeatures { baseline_estimate: 9_999, ..Default::default() },
];

let scored = corrector.batch_score(&batch)?;
assert_eq!(scored, vec![10, 250, 9_999]);
# Ok::<(), samkhya_core::Error>(())
```

## LLM-pluggable corrector — server transports

The `samkhya-core` `llm_http` feature speaks a small JSON wire contract
to an external inference server. `samkhya-gpudb/scripts/` ships **two
reference implementations** of that server, both behind the same wire
contract — pick whichever fits your environment:

| Transport | Entry point | Runtime | When to pick |
|---|---|---|---|
| **Python (FastAPI)** — canonical | `llm_infer_server.py` | Python 3.10+, FastAPI, uvicorn | The reference build the v1.0 empirical campaign measured. Pick this if you already have a Python venv. |
| **TypeScript (Node)** — broader appeal | `llm_infer_server.ts` | Node 18+, zero deps for transport (uses `node:http`), optional `@anthropic-ai/sdk` / `openai` peers | Pick this if your team is Node/TS shop and you don't want a Python venv. Same wire contract; the Rust client doesn't notice the swap. |

Both servers expose `POST /infer` + `GET /health` and accept the same
four backends — `anthropic` / `openai` / `local` (Ollama) /
`dummy` — selected via `SAMKHYA_LLM_BACKEND`. They use disjoint default
ports (`8766` for Python, `8767` for TypeScript) so they can run side
by side.

```bash
# Python (canonical)
bash samkhya-gpudb/scripts/run-llm-bench.sh --backend dummy

# TypeScript (broader appeal)
cd samkhya-gpudb/scripts && npm install         # one-time
bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend dummy
```

Per the samkhya naming rule, the LLM transport is framed as a
**pluggable corrector backend** — *not* an "AI feature." The default
samkhya build does not link either server in; the `llm_http` cargo
feature is off by default.

## Feature flags

- `cuda` (off by default) — placeholder for the CUDA kernel backend. Default
  builds never link a CUDA runtime; turning this on will pull in the kernel
  scoring path without changing the public trait surface.
- `metal` (off by default) — placeholder for the Apple Silicon Metal kernel
  backend. Default builds never link a Metal framework.

GPU is **strictly opt-in**. With neither feature enabled, this crate
compiles on any host with a Rust toolchain — no GPU drivers, no system C++
build required. Engines without a GPU can depend on `samkhya-gpudb`, use the
`CpuFallbackCorrector`, and switch to a real backend later by flipping a
feature flag.

## Planned wiring

1. **Batch-score subplan candidates.** During plan enumeration, gpudb's
   optimizer accumulates a vector of `CorrectionFeatures` (one per
   candidate) and calls `GpuCorrector::batch_score` once per planning round.
   The CUDA / Metal backend dispatches a single kernel.
2. **Reuse the residual model.** The trained residual model (sub-MB
   footprint) is uploaded to device memory once per query and reused across
   all subplan batches in that query.
3. **LpBound clamp on-device.** The per-row LpBound ceiling travels with the
   feature vector and is applied inside the kernel, so the safety contract
   is preserved without a host round-trip.

## Integration

The companion [gpudb](https://github.com/singhpratech/gpudb) engine is the
intended primary consumer. Engines that want to enumerate thousands of
subplans against a sub-MB residual model — without serializing each through
CPU inference — depend on this crate to keep the batch interface stable
while the kernel backends evolve behind cargo features.

## License

Apache-2.0. Sole author: Prateek Singh.
