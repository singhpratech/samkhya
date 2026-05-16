# samkhya

> सांख्य — "enumeration / counting"

Portable cardinality correction for embedded analytical engines (DataFusion, DuckDB, Polars, gpudb).

## What it is

A Rust crate that gives embedded analytical optimizers accurate cardinality estimates via:

- **Portable stats** — Iceberg Puffin sidecars, classical sketches (HLL/Theta/KLL/CMS/Bloom/t-digest), multi-column correlated histograms
- **Feedback-driven correction** — Bao/AutoSteer pattern: observe (plan, estimate, actual), inject corrections as hints; fall back to native plan
- **Pessimistic safety envelope** — LpBound-style provable upper bound (SIGMOD 2025 Best Paper, no ML). Correction can never exceed it; cold start = native plan or better, never worse
- **GPU batch inference** (optional) — score thousands of subplan candidates in one CUDA/Metal kernel launch via gpudb integration
- **TabPFN-as-pluggable-backend** — foundation-model interface from day one; if TabPFN-style table models win, samkhya becomes the infrastructure

Not a learned cardinality estimator. The ML layer is opt-in. Framing intentionally avoids "learned" / "adaptive" / "AI" — see `samkhya.md` §3.

## Status

**v0.0.1 (2026-05-16)** — initial scaffolding release. Full architectural skeleton in place, all 6 samkhya-core modules wired with minimal correct implementations, 31 tests passing workspace-wide, CI green. See [CHANGELOG.md](CHANGELOG.md) for the full notes.

What works:
- HLL + Bloom sketches with portable serde codec
- Iceberg Puffin reader/writer (round-trips sketches through sidecars)
- SQLite-backed feedback recorder
- LpBound envelope (`ProductBound` + coarse `AgmBound` + clamp helpers)
- `Corrector` trait with identity passthrough
- DataFusion 46 `OptimizerRule` (observe-only, cold-start-safe)
- PyO3 bindings (HllSketch / BloomFilter / ColumnStats)
- clap CLI scaffold for JOB-Slow / TPC-H / STATS-CEB benchmarks
- GitHub Actions CI + rustfmt + clippy `-D warnings`

What's pending (see `samkhya.md` §4 90-day MVP plan):
- Full LpBound LP solver (currently a coarse AGM approximation)
- Real DataFusion stats injection (currently placeholder)
- Residual GBT / TabPFN backends
- JOB-Slow baseline-vs-corrected runner + week-13 GO/NO-GO gate
- DuckDB cxx extension

Tracking toward:
- CIDR 2027 submission (deadline 2026-08-04)
- Week-13 GO/NO-GO gate against JOB-Slow worst queries — need ≥3× p95 latency win

## Workspace

| Crate | Purpose |
|---|---|
| `samkhya-core` | Sketches, Puffin I/O, feedback recorder, LpBound envelope, residual model |
| `samkhya-datafusion` | DataFusion `OptimizerRule` adapter — the first integration |
| `samkhya-duckdb` | DuckDB extension (Rust → C++ via cxx) — Query-farm/datasketches pattern |
| `samkhya-py` | Python bindings via PyO3 |
| `samkhya-bench` | Benchmark harness: JOB-Slow + TPC-H + STATS-CEB |

## Reading

See `samkhya.md` for the full project bootstrap doc: research findings from a 5-agent parallel sweep, architecture, 90-day MVP plan, publication strategy, and ~40-entry annotated bibliography.

## License

Apache-2.0. Matches DataFusion, Arrow, DataSketches.
