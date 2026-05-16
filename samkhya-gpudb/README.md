# samkhya-gpudb

gpudb adapter for [samkhya](../) — the GPU batch-inference layer of
the architecture.

## Role

This crate is **Layer 4** of the samkhya design (see
[`ARCHITECTURE.md`](../ARCHITECTURE.md) §3 and
[`samkhya.md`](../samkhya.md) §3). It pairs samkhya's residual
corrector with [gpudb](https://github.com/singhpratech/gpudb) —
Prateek's GPU-accelerated DuckDB extension targeting CUDA and Apple
Silicon Metal — so the corrector can score thousands of subplan
candidates in a single kernel launch instead of serializing them
through CPU inference.

Subplan enumeration is embarrassingly parallel: each candidate is an
independent forward pass through a sub-MB model. CPU inference walks
them one at a time; a GPU kernel batches the lot. **No published
cardinality-estimation system targets batch GPU inference of the
correction model itself** — that is the differentiator vs. TiCard and
the rest of the embedded-engine CE work.

## Status

Scaffold. The batch trait surface and a CPU fallback live here today;
the real CUDA and Metal kernels are post-MVP and land behind the
`cuda` and `metal` cargo features.

## Opt-in posture

GPU support is **strictly opt-in**:

- Default builds of samkhya never link CUDA or Metal.
- Default builds of this crate never link CUDA or Metal.
- `samkhya-gpudb` is a separate workspace member, not a transitive
  dependency of `samkhya-core`.
- The `cuda` and `metal` cargo features are placeholders today and a
  no-op on the public API; enabling them will pull in the future
  kernel backends without changing the trait surface.

Engines and downstream applications that have no GPU available can
depend on `samkhya-gpudb`, use the `CpuFallbackCorrector`, and switch
to a GPU backend later by flipping a feature flag.

## Planned integration

1. **Batch-score subplan candidates.** During plan enumeration, gpudb's
   optimizer accumulates a vector of `CorrectionFeatures` (one per
   candidate) and calls `GpuCorrector::batch_score` once per planning
   round. The CUDA / Metal backend dispatches a single kernel.
2. **Reuse the residual model.** The trained residual model (GBT or
   PFN-style, sub-MB footprint) is uploaded to device memory once per
   query and reused across all subplan batches in that query.
3. **LpBound clamp on-device.** The per-row LpBound ceiling travels
   with the feature vector and is applied inside the kernel, so the
   safety contract is preserved without a host round-trip.

## License

Apache-2.0, inherited from the workspace.
