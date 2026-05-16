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

**v0.3.0 (2026-05-16)** — end-to-end feedback loop with measurable q-error reduction.
`cargo run -p samkhya-bench -- calibrate --suite synthetic` collects observations,
trains a GBT residual corrector, re-runs with corrected estimates, and reports
the q-error improvement. 82 tests pass workspace-wide, clippy `-D warnings` clean,
4 release tags (v0.0.1 → v0.3.0). See [CHANGELOG.md](CHANGELOG.md) for full notes.

What works today:

- Four foundational sketches with portable serde codec: HLL (distinct), Bloom (membership), Count-Min (frequency), EquiDepthHistogram (range)
- Iceberg Puffin reader/writer with optional zstd compression
- SQLite-backed feedback recorder with q-error computation
- LpBound envelope: `ProductBound`, `AgmBound`, `ChainBound` (frequency-moment chain bound), plus clamp helpers
- Residual corrector trait: `IdentityCorrector` baseline; `GbtCorrector` (gbdt-rs) behind the `gbt` feature
- DataFusion 46 integration with three-layer stats injection (`SamkhyaTableProvider` + `SamkhyaStatsExec` + `SamkhyaOptimizerRule` for both logical and physical passes) — proven via the `stats_propagation_demo` example
- PyO3 bindings (HllSketch / BloomFilter / ColumnStats)
- `samkhya-bench` clap CLI: `list-queries`, `run`, `compare` (baseline vs samkhya side-by-side), `report`, `train`, `calibrate` (full feedback loop: collect → train GBT → re-run with correction)
- 10-query synthetic suite covering single-filter through 4-table joins with correlated predicates
- GitHub Actions CI + rustfmt + clippy `-D warnings` + criterion microbenches + 13 proptest properties

Demonstrated gap the project targets:

```
$ cargo run -p samkhya-bench -- run --suite synthetic
query     estimated       actual    q-error         ms
--------------------------------------------------------
S1             2000         3925       1.96       3.49
S2                0          300        inf       8.09
S3                0         6924        inf       8.81
S4                0          761        inf      33.01
S5                0         5223        inf      14.28
```

DataFusion 46 estimates 0 rows for the multi-join queries that actually return
300–6924 rows. This is the embedded-engine cardinality estimation gap.

What's still pending (see `samkhya.md` §4 90-day MVP plan):

- Full LpBound LP solver (currently a coarse AGM approximation)
- DataFusion stats propagation through the planner (the wrapper is correctly
  shaped but DF 46 doesn't consume `TableProvider::statistics()` in mainline)
- DuckDB cxx extension
- TabPFN-style foundation-model corrector backend
- JOB-Slow / TPC-H runs against real datasets

Tracking toward:

- CIDR 2027 submission (deadline 2026-08-04) — abstract drafted in `paper/`
- Week-13 GO/NO-GO gate against JOB-Slow worst queries — need ≥3× p95 latency win

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
