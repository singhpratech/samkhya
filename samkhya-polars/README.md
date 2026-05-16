# samkhya-polars

Polars adapter for [samkhya](../).

## Status

Scaffold. Polars currently has no public optimizer-rule extension API
(see [pola-rs/polars#23345](https://github.com/pola-rs/polars/issues/23345)),
so this crate declares the integration surface and will gain real wiring
once that upstream gap closes.

## Planned integration patterns

1. **Stats sidecar consumer** — load a Polars `DataFrame` together with
   a Puffin sidecar built via `samkhya-core::puffin`; expose helpers
   that inspect a `LazyFrame` plan and return corrected cardinality
   hints for downstream consumers.
2. **Feedback wrapper** — wrap `LazyFrame::collect()` to capture
   estimated vs actual row counts and persist `Observation`s to a
   `samkhya_core::feedback::FeedbackStore`.
3. **Sketch-from-`Series` builder** — pure-Rust helpers that build HLL
   / Bloom / Count-Min / EquiDepthHistogram sketches directly from a
   `polars::Series`, then round-trip via the `samkhya_core::sketches::Sketch`
   trait.

## Why this crate exists today

The samkhya architecture diagram in `samkhya.md` §3 lists Polars
alongside DataFusion, DuckDB, and gpudb as integration targets. Even as
a stub, this crate:

- Reserves the workspace member slot and crate name on crates.io.
- Lets downstream users add `samkhya-polars` to their Cargo.toml today
  and pick up the real integration once it lands.
- Mirrors the cross-engine namespace pattern used by other Arrow tools.

## License

Apache-2.0, inherited from the workspace.
