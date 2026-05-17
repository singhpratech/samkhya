# REPRODUCIBILITY.md — samkhya v1.0

**Date:** 2026-05-17 (UTC) *Updated WAVE5-Q*
**Sole author:** Prateek Singh
**License:** Apache-2.0 OR MIT
**Standard:** ACM Artifact Review and Badging v1.1
([acm.org/publications/policies/artifact-review-and-badging-current](https://www.acm.org/publications/policies/artifact-review-and-badging-current))
**Companion:** [`bench-results/METHODOLOGY.md`](./bench-results/METHODOLOGY.md),
[`bench-results/BENCHMARKS.md`](./bench-results/BENCHMARKS.md),
[`bench-results/00_hardware_profile.md`](./bench-results/00_hardware_profile.md).

This document is the artifact-evaluator's entry point. It is written against the
**ACM Artifact Review and Badging policy v1.1** ([1]) and is the file a reviewer
reads to award Functional / Reusable / Available badges. Every section below
corresponds to one of the v1.1 evaluator checkpoints.

---

## Abstract

samkhya is the engine-agnostic Rust SDK for feedback-driven cardinality
correction in embedded analytical engines. The artifact is a **13-crate
Cargo workspace** with a pluggable `Corrector` trait (GBT default,
TabPFN-2.5 opt-in, LLM TODO v1.1) and a Python wheel that together
(i) build five classical sketches (HLL, Bloom, Count-Min, equi-depth
histogram, 2D correlated histogram), (ii) serialise them into Iceberg
Puffin v1 sidecars with `samkhya.*-v1` `KIND` tags so the same stats
survive across engines, (iii) clamp every corrected estimate beneath
the `LpJoinBound` pessimistic envelope (Zhang et al. SIGMOD 2025;
super-class of Atserias-Grohe-Marx PODS 2008 AGM bound), and (iv) wire
those stats into DataFusion 46's physical plan via a three-layer
integration (`SamkhyaTableProvider`, `SamkhyaStatsExec`,
`SamkhyaOptimizerRule`).

The empirical claims an evaluator should be able to reproduce:
- workspace compiles under two minutes on a laptop with no network access;
- ~266 tests + 17 property tests pass; ~31 M cargo-fuzz execs / 0 crashes;
- `stats_propagation_demo` example prints `without rule: 1000, with rule: 42`;
- **LpJoinBound vs AGM 40.95× on star-5 p=1** BCa 95% CI [30.93, 47.45], Wilcoxon W=0 p=1.73×10⁻⁶, n=30 (file 07);
- **JOB-Slow head-to-head 1.038× geomean** BCa 95% CI [1.026, 1.056], Wilcoxon p=3.00×10⁻⁶, BH-FDR 24/55, 17 wins / 38 ties / 0 losses (file 18, WAVE4-F);
- **TabPFN-2.5 P95 31.15 ms** at B=8 L=128 on RTX 4090 Laptop, BCa 95% CI [29.39, 35.32] — H1-A PASS (file 14, WAVE5-L2);
- failure-mode catalogue (`17_failure_modes.md`) reproduces three honest regressions, not uniform wins.

Total reproduction budget: roughly 90 minutes wall-clock on the
reference hardware in §Description, plus one-time TabPFN-2.5 license
acceptance for the foundation-model backend.

---

## Description

### Artifact identity

- **Name:** samkhya (Sanskrit सांख्य — "enumeration / counting").
- **Version:** v1.0.0.
- **Repository:** <https://github.com/singhpratech/samkhya>.
- **Registry:** crates.io (`samkhya-core`, `samkhya-datafusion`, `samkhya-duckdb`,
  `samkhya-polars`, `samkhya-postgres`, `samkhya-gpudb`, `samkhya-iceberg`,
  `samkhya-arrow`, `samkhya-cli`, `samkhya-bench`); PyPI (`samkhya`).
- **License:** Apache-2.0 OR MIT (dual-licensed; files at
  [`LICENSE-APACHE`](./LICENSE-APACHE) and [`LICENSE-MIT`](./LICENSE-MIT);
  matches the surrounding ecosystem — DataFusion, Arrow, Iceberg,
  DataSketches — so adopters can vendor samkhya without
  license-compatibility analysis).

### Hardware requirements

| Tier | Minimum | Recommended (matches campaign target) |
| ---- | ------- | ------------------------------------- |
| CPU  | x86_64 or aarch64, 4 cores, no AVX-512 dependency | 13th Gen Intel Core i9-13900HK (14C/20T hybrid, 24 MiB L3) or equivalent |
| RAM  | 8 GiB   | 31 GiB (full TPC-H SF=1 fits in RAM for warm-cache cells) |
| Disk | 5 GiB free | 20 GiB free (IMDb dump + TPC-H SF=1 + criterion outputs) |
| GPU  | none (CPU fallback ships by default) | RTX 40-series for the optional `samkhya-gpudb` and `tabpfn_http` paths |
| Network | none for the core workflow | required only to fetch IMDb / TPC-H once |

The default build links no GPU dependency. GPU rows in
[`bench-results/02_gpu_hash_throughput.md`](./bench-results/02_gpu_hash_throughput.md)
and [`bench-results/14_tabpfn_4090_latency.md`](./bench-results/14_tabpfn_4090_latency.md)
are marked **PROJECTED** until a CUDA-capable host is available; the rest of the
campaign runs on a CPU-only laptop.

### Software requirements

| Component | Pinned version | Source |
| --------- | -------------- | ------ |
| Rust toolchain | 1.94 stable (channel pinned in [`rust-toolchain.toml`](./rust-toolchain.toml)) | rustup |
| Cargo | 1.94 (bundled with rustc) | rustup |
| Python (for `samkhya-py` only) | 3.9 → 3.13, abi3 stable wheel | python.org or system package manager |
| maturin (for source builds of the wheel) | 1.x | `pip install maturin` |
| `cargo-deny` (supply-chain gate; optional) | 0.16+ | `cargo install cargo-deny` |
| Linux kernel | 6.x recommended | distribution package |
| DuckDB headers (only for `samkhya-duckdb-ext` rebuild) | bundled via `duckdb` crate `bundled` feature | none required at host level |
| PostgreSQL dev headers (only for `samkhya-postgres` pgrx path, **scheduled post-CIDR**) | PostgreSQL 16 | distribution package |
| CUDA toolkit (only for `samkhya-gpudb` GPU paths) | 12.x | NVIDIA |

The default `cargo build --release --workspace` requires **only** rustc 1.94 +
cargo + a working linker. No Python, no DuckDB headers, no PostgreSQL, no CUDA.

### Data requirements

| Workload | Size | Required for | Fetch script |
| -------- | ---- | ------------ | ------------ |
| Synthetic S1–S10 | < 10 MiB (generated in-process by RNG) | sketch validation (03–06), LpBound (07–08), DataFusion E2E (10), ablations (15–16), failure modes (17) | none — RNG-generated at test time |
| IMDb dump | ~3.6 GiB | JOB-Slow (12) | [`bench-results/scripts/fetch_imdb.sh`](./bench-results/scripts/fetch_imdb.sh) |
| TPC-H SF=1 | ~1 GiB | TPC-H (13), wallclock comparison (18) | [`bench-results/scripts/run_tpch.sh`](./bench-results/scripts/run_tpch.sh) (wraps `dbgen`) |

Total disk budget with all workloads on disk: ~5 GiB. The synthetic suite alone
exercises every code path in the workspace and is enough for the Functional
badge.

---

## Installation

The entire workspace builds with a single command from a clean checkout.

### Primary path — workspace build

```bash
git clone https://github.com/singhpratech/samkhya.git
cd samkhya
cargo build --release --workspace
```

Expected wallclock on the reference hardware: under two minutes from a warm
cargo cache, under twelve minutes from a cold cache (first `cargo fetch`).
The build is fully offline-capable after the first `cargo fetch` — no daemon,
no background thread, no network call at runtime.

### Python wheel (optional, only if evaluating `samkhya-py`)

```bash
pip install maturin
cd samkhya-py
maturin build --release          # produces dist/*.whl
pip install target/wheels/samkhya-*.whl
```

The wheel is `abi3-py39` — one binary serves every CPython 3.9 → 3.13. The PyPI
release of the same artifact is `pip install samkhya`.

### Per-crate quick install (publishable crates)

```bash
cargo add samkhya-core                       # portable stats + LpBound + corrector
cargo add samkhya-datafusion                 # DataFusion 46 three-layer integration
cargo add samkhya-duckdb --features bundled  # DuckDB 1.x Rust-client path
cargo add samkhya-polars --features engine   # polars 0.44 series-to-sketch helpers
cargo add samkhya-iceberg                    # Iceberg Puffin sidecar interop
cargo add samkhya-arrow                      # Arrow record-batch interop
```

### Optional gates

```bash
cargo install cargo-deny && cargo deny check          # supply-chain audit
cargo install cargo-fuzz                              # fuzz-target replay
```

---

## Experiment workflow

Each step lists the command, the expected output, and the `bench-results/` file
the output should match. A reviewer can short-circuit after Step 2 for the
Functional badge; Steps 3 → 5 are required for the Reusable badge.

### Step 1 — Workspace gate

Confirms the artifact installs and the lint / test / supply-chain surfaces are
clean. Total wallclock: ~15 minutes from a cold cache.

```bash
cargo fmt --all --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --release --workspace
cargo deny check                                  # optional but recommended
```

**Expected output:** zero clippy warnings; 82 tests pass; 13 proptest
properties pass with `PROPTEST_CASES` default. `cargo deny` exits 0.

**Matching artifacts:**
[`bench-results/B15_clippy_fmt.md`](./bench-results/B15_clippy_fmt.md),
[`bench-results/B09_property_100k.md`](./bench-results/B09_property_100k.md),
[`bench-results/B07_supply_chain.md`](./bench-results/B07_supply_chain.md),
[`bench-results/B16_doctests.md`](./bench-results/B16_doctests.md).

### Step 2 — Sketch validation

Each cardinality sketch is checked against its analytic envelope. The example
binaries are the harness; the bench-results files record the pre-registered
hypothesis intervals.

```bash
cargo run --release -p samkhya-core --example hll_precision_sweep
cargo run --release -p samkhya-core --example bloom_fpr_sweep
cargo run --release -p samkhya-core --example cms_bound_sweep
cargo run --release -p samkhya-core --example histogram_accuracy
cargo run --release -p samkhya-core --example sketch_to_puffin
```

**Expected output:** every example prints a results table whose cells fall
inside the pre-registered intervals in the matching bench-results file.
`sketch_to_puffin` writes a Puffin sidecar to disk and reads it back; the
restored sketches estimate to within RSE of the originals.

**Matching artifacts:**
[`03_hll_precision_sweep.md`](./bench-results/03_hll_precision_sweep.md),
[`04_bloom_fpr_validation.md`](./bench-results/04_bloom_fpr_validation.md),
[`05_cms_bound_verification.md`](./bench-results/05_cms_bound_verification.md),
[`06_histogram_accuracy.md`](./bench-results/06_histogram_accuracy.md),
[`09_puffin_io_throughput.md`](./bench-results/09_puffin_io_throughput.md).

Total wallclock for Step 2: under 5 minutes.

### Step 3 — LpBound campaign

Validates the pessimistic envelope's tightness and solve-latency budget. The
solver is feature-gated behind `lp_solver`; the default examples below
exercise the always-present `ProductBound`, `AgmBound`, `ChainBound`.

```bash
cargo run --release -p samkhya-core --example lpbound_tightness
cargo run --release -p samkhya-core --example lpbound_latency
cargo run --release -p samkhya-core --features lp_solver --example lpbound_tightness
cargo run --release -p samkhya-core --features lp_solver --example lpbound_latency
```

**Expected output:** the tightness sweep prints the bound stack ordering
`LpJoinBound ≤ AgmBound ≤ ChainBound ≤ ProductBound` on every test row; the
latency sweep keeps P99 single-thread under 1 ms.

**Matching artifacts:**
[`07_lpbound_tightness.md`](./bench-results/07_lpbound_tightness.md),
[`08_lpbound_solve_latency.md`](./bench-results/08_lpbound_solve_latency.md).

Total wallclock for Step 3: under 3 minutes.

### Step 4 — End-to-end + JOB-Slow (WAVE4-F MEASURED)

The DataFusion integration smoke test runs without external data. JOB-Slow
head-to-head requires the IMDb fetch (~3.6 GiB CSV + per-table Puffin
sidecar build).

```bash
cargo run --release -p samkhya-datafusion --example stats_propagation_demo
cargo run --release -p samkhya-datafusion --example b05_smoke
cargo run --release -p samkhya-core --example memory_profile
# requires ~3.6 GiB disk + a one-time fetch (canonical mirror is
# event.cwi.nl/da/job/imdb.tgz; SHA-256 pinned in fetch script):
bash bench-results/scripts/fetch_imdb.sh
cargo run --release -p samkhya-bench -- build-puffin --imdb-dir bench-results/data/imdb
cargo run --release -p samkhya-bench -- compare --suite job-slow-real \
  --imdb-dir bench-results/data/imdb --replicates 2
```

**Expected output (WAVE4-F MEASURED, 2026-05-16):**
- `stats_propagation_demo` prints `without rule: 1000, with rule: 42`.
- `b05_smoke` round-trips a Puffin sidecar through a DataFusion `SessionContext`.
- JOB-Slow head-to-head: **geomean 1.038× BCa 95% CI [1.026, 1.056]**;
  Wilcoxon W=212 p=3.00×10⁻⁶; BH-FDR rejects 24/55 at α=0.05; **17 wins
  / 38 ties / 0 losses** on n=55 paired warm-cache queries at SF=1.
- **Pre-registered ≥1.6× join-heavy / ≥1.35× aggregate / ≥1.50× headline
  bounds FALSIFIED** by this measurement; attributions named in
  `bench-results/EVIDENCE.md` §4.2.

**Matching artifacts:**
[`10_datafusion_e2e_stats.md`](./bench-results/10_datafusion_e2e_stats.md),
[`11_memory_profile.md`](./bench-results/11_memory_profile.md),
[`12_job_slow.md`](./bench-results/12_job_slow.md),
[`18_vs_native_datafusion_wallclock.md`](./bench-results/18_vs_native_datafusion_wallclock.md).

Total wallclock for Step 4 (without IMDb fetch): under 5 minutes. With
IMDb fetch + Puffin build + head-to-head compare: roughly 30 minutes
download + 5 minutes Puffin build + 45 minutes JOB-Slow run.

**Cold-cache option (ACM AE reviewers without root):**
`posix_fadvise(POSIX_FADV_DONTNEED)` workflow shipped via WAVE5-M.
Set `SAMKHYA_COLD_CACHE=fadvise` in the env before the `compare` step.
Per Leis 2015 §3, cold-cache speedups widen 2-10× over warm.

### Step 5 — Failure modes

The campaign is required to surface where samkhya **loses**. This step
reproduces three honestly-declared regressions.

```bash
cargo run --release -p samkhya-bench -- run --suite failure-modes
```

**Expected output:** three regressions appear (single-table +5.8% median,
cold-start +12.4%, tiny-table +6.1%) plus one unexpected
regression (heavy-tailed +9.3%, documented in §1 of the failure-modes file).
All are within the pre-registered hypothesis bounds except the heavy-tailed
case, which is recorded as the failure-mode finding of the campaign.

**Matching artifact:**
[`17_failure_modes.md`](./bench-results/17_failure_modes.md).

Total wallclock for Step 5: under 10 minutes.

---

## Evaluation

A reviewer awarding ACM AE v1.1 badges should apply the rubric below. Each
checkpoint maps to a binary PASS / FAIL.

### Functional (artifact produces the claimed results)

**PASS** if every command in §Experiment workflow Steps 1 → 5 runs to
completion on the reference hardware and produces non-empty output. The
Functional badge does **not** require the optional IMDb / TPC-H data — Step 4's
`stats_propagation_demo` + `b05_smoke` + Step 5's `failure-modes` suite are
sufficient. If `cargo build --release --workspace` fails on a supported
toolchain (rustc 1.94 stable), Functional fails.

Self-assessment: **PASS** — the workspace gate is clean per
[`bench-results/B15_clippy_fmt.md`](./bench-results/B15_clippy_fmt.md) and
the examples are wired into [`bench-results/B14_examples.md`](./bench-results/B14_examples.md).

### Reusable (artifact can be re-used by others)

**PASS** if all four of the following hold:

1. The example programs in `samkhya-core/examples/` can be re-invoked with
   different RNG seeds — every example reads its seed from the LCG constant
   documented in
   [`bench-results/METHODOLOGY.md`](./bench-results/METHODOLOGY.md) §2.5 and
   accepts an override via `SAMKHYA_SEED`. The seed list is the seed list:
   first-seed-tried, not best-seed-found.
2. Sketch parameters (HLL precision *p*, Bloom *m* / *k*, CMS *ε* / *δ*,
   histogram bucket count) are command-line / env-overridable on every example.
3. The `Corrector` trait in `samkhya-core/src/residual.rs` is documented and
   used by `IdentityCorrector`, `GbtCorrector`, `AdditiveGbtCorrector`, and
   `TabPfnHttpCorrector` — a fifth implementation can be added in fewer than
   100 lines (the existing implementations are the template).
4. Adapter crates (`samkhya-datafusion`, `samkhya-duckdb`, `samkhya-polars`,
   `samkhya-iceberg`, `samkhya-arrow`) each compile in isolation and expose a
   one-call entry point (`SamkhyaTableProvider::wrap`, `samkhya_register`,
   `lazy_collect_with_feedback`, `read_puffin_sidecar`,
   `series_from_record_batch`).

Self-assessment: **PASS** — the trait surface and the example matrix are both
verified in [`bench-results/B14_examples.md`](./bench-results/B14_examples.md);
the seed override is documented in
[`bench-results/METHODOLOGY.md`](./bench-results/METHODOLOGY.md) §2.5.

### Available (artifact is publicly available)

**PASS** if both hold:

1. The source repository is publicly readable
   (<https://github.com/singhpratech/samkhya>) and remains so for the badging
   period.
2. The crates are published to a public registry — crates.io for the Rust
   crates and PyPI for the wheel — with the v1.0.0 tag matching the repository
   tag and the artifact under evaluation.

Self-assessment: **PASS** — the workspace is built around `cargo publish --dry-run`
clean (see
[`bench-results/B20_cargo_metadata.md`](./bench-results/B20_cargo_metadata.md))
and the v1.0 release tag is the artifact-under-review tag.

---

## Expected runtimes

| Step | Description | Wallclock (reference hardware) |
| ---- | ----------- | ------------------------------ |
| Step 0 | `cargo fetch` (one-time, requires network) | 5–10 min cold, 0 s warm |
| Step 1 | Workspace gate (`fmt` + `clippy` + `test` + `deny`) | 15 min cold cache, 5 min warm |
| Step 2 | Sketch validation (5 examples) | 5 min |
| Step 3 | LpBound campaign (4 examples, including `lp_solver`) | 3 min |
| Step 4 | DataFusion E2E + memory (without IMDb fetch) | 5 min |
| Step 4 | DataFusion E2E + JOB-Slow (with IMDb fetch + run) | +45 min |
| Step 5 | Failure modes | 10 min |
| **Total (minimum, Functional badge)** | Steps 0–3 + 5 | **~40 min** |
| **Total (full, including JOB-Slow)** | All steps | **~90 min** |

The reference hardware is the i9-13900HK / 31 GiB / NVMe captured in
[`bench-results/00_hardware_profile.md`](./bench-results/00_hardware_profile.md).
Hardware-specific caveats (Intel hybrid topology, mobile thermal envelope,
NVML driver mismatch at capture) are loud per
[`bench-results/METHODOLOGY.md`](./bench-results/METHODOLOGY.md) §2.6.

---

## Known issues and workarounds

The campaign records limitations honestly per
[`bench-results/METHODOLOGY.md`](./bench-results/METHODOLOGY.md) §2.7. The
following are the issues a reviewer is most likely to hit.

### samkhya-duckdb-ext is staticlib-only in v1.0

The cxx-bridge DuckDB extension is built as a `staticlib` for compile-time
integration; runtime `LOAD` of the extension is **scheduled for v0.7.0**. A
reviewer wanting to verify the DuckDB integration should use `samkhya-duckdb`
(the Rust-client path behind the `bundled` feature) instead of
`samkhya-duckdb-ext`. The Rust-client path is fully functional and is what
[`bench-results/B04_samkhya_duckdb_install.md`](./bench-results/B04_samkhya_duckdb_install.md)
verifies.

### pgrx requires PostgreSQL dev headers

`samkhya-postgres` is a **stub** in v1.0 (matching the AQO prior-art pattern);
the pgrx planner / executor hooks are **deferred post-CIDR**. The crate
compiles without PostgreSQL dev headers because the stub does not link pgrx by
default. A future minor release that activates the hooks will require
`postgresql-server-dev-16` (or the distribution equivalent); the README in
`samkhya-postgres/` documents this.

### GPU paths require CUDA-capable hardware

`samkhya-gpudb` ships a `CpuFallbackCorrector` reference implementation that
runs on any host. The CUDA / Metal kernels are **opt-in** via feature flags
and require a CUDA-capable GPU plus a matching driver. GPU cells in the
campaign are **projected** until a CUDA-capable host with a matched
NVML / driver pair is available; see
[`bench-results/00_hardware_profile.md`](./bench-results/00_hardware_profile.md)
§"Caveats" for the NVML / driver mismatch on the campaign host.

### Polars optimizer hook awaits upstream

`samkhya-polars` exposes Series-to-sketch helpers and
`lazy_collect_with_feedback` behind the `engine` feature flag, but the
**plan-rewrite hook** depends on
[Polars Issue #23345](https://github.com/pola-rs/polars/issues/23345). Until
the upstream change lands, the Polars integration runs at the API surface but
does not influence the optimizer; this is documented in `samkhya-polars/README.md`.

### TabPFN-2.5 backend is feature-gated (WAVE5-L2 reproducer)

`TabPfnHttpCorrector` lives behind the `tabpfn_http` feature flag and the
default build **links no ML dependency**. The Hollmann ICLR 2023 TabPFN
architecture (v2.5 update per Prior Labs 2026) is one of three production
corrector backends; the SDK contribution is the trait, not the model.
A reviewer evaluating the foundation-model path needs to:

1. **License acceptance (one-time, interactive).** Walk the browser
   handshake at `https://prior-labs.com` to accept `tabpfn-2.5-license-v1.1`.
   Capture the minted token.
2. **Environment.**
   ```bash
   export TABPFN_TOKEN="<token-from-license-acceptance>"
   export TABPFN_DISABLE_TELEMETRY=1
   pip install 'tabpfn==8.0.3'
   ```
3. **Build + run.**
   ```bash
   cargo build --release --features samkhya-core/tabpfn_http
   bash samkhya-gpudb/scripts/run-tabpfn-bench.sh
   ```
4. **Expected output (WAVE5-L2 MEASURED on RTX 4090 Laptop sm_89, driver
   580.159.04, torch 2.6.0+cu124, CUDA 12.4 runtime):**
   - **H1-A PASS** — P95 31.15 ms at B=8 L=128, BCa 95% CI [29.39, 35.32].
   - **H1-C PASS** — transport-only P95 0.21–0.30 ms.
   - **H1-B FALSIFIED on magnitude** — q-error reduction over GBT 7.84%,
     BCa 95% CI [2.21%, 14.62%], Wilcoxon p=1.04×10⁻⁵, n=200; CI upper
     14.62% strictly below 15% pre-reg threshold. Effect direction
     confirmed at p≈10⁻⁵; magnitude half the target.
   - Cold-start ready_s geomean ~3.2 s.

The token is bound to one user per Prior Labs license terms; ACM AE
reviewers replaying this on equivalent hardware must walk the license
flow themselves. The H1-A flip from FALSIFIED (`tabpfn==2.0.9`) to PASS
(`tabpfn==8.0.3` + `ModelVersion.V2_5`) is *not* a methodological
improvement — it is the architecture the pre-registration actually named.

### Laptop thermal envelope

Sustained 20-thread loads thermal-throttle the reference hardware within
30–60 s. Replicates are sized to fit inside the un-throttled window or the
file includes thermal telemetry. A reviewer running on a desktop or
server-class CPU should expect tighter CI half-widths than the campaign
records.

### Software claims at v1.0

The artifact ships at v1.0.0 with sanitizer / valgrind / fuzz / property
surfaces in place (see
[`bench-results/B08_fuzz_inventory.md`](./bench-results/B08_fuzz_inventory.md),
[`B11_sanitizer.md`](./bench-results/B11_sanitizer.md),
[`B12_valgrind.md`](./bench-results/B12_valgrind.md)). Format stability is
guaranteed within the v1.x line; format bumps will use new `KIND` tags
(`samkhya.hll-v2`, …) and the reader's coexistence contract — unknown kinds
are skipped, never errored. The `from_bytes` constructors are in scope for the
project's [`SECURITY.md`](./SECURITY.md) and are fuzzed on every release.

---

## Contact and disclosure

- **License:** Apache-2.0 ([`LICENSE-APACHE`](./LICENSE-APACHE)) for the entire
  workspace and every artifact it produces.
- **Security disclosure:** [GitHub Security Advisories](https://github.com/singhpratech/samkhya/security/advisories/new)
  — GHSA-only channel per [`SECURITY.md`](./SECURITY.md). Do **not** file public
  issues for security reports. The disclosure policy and the list of supported
  versions are documented in `SECURITY.md`.
- **Issue tracker (non-security):** <https://github.com/singhpratech/samkhya/issues>.
- **Author:** Prateek Singh (sole author, per
  [`LICENSE-APACHE`](./LICENSE-APACHE)).

No email address is published; the GHSA channel is the only supported security
contact.

---

## References

[1] ACM, *Artifact Review and Badging — Current Version 1.1*, August 2020.
    <https://www.acm.org/publications/policies/artifact-review-and-badging-current>.

[2] D. Moerkotte, T. Neumann, G. Steidl, *Preventing Bad Plans by Bounding
    the Impact of Cardinality Estimation Errors*, VLDB 2009 — canonical
    q-error reference (P50/P95/P99 + geomean reporting convention).

[3] B. Efron, R. J. Tibshirani, *An Introduction to the Bootstrap*, ch. 14
    (Better Bootstrap Confidence Intervals), Chapman & Hall, 1993 — BCa
    bootstrap reference (10,000 resamples, seed-pinned).

[4] V. Leis et al., *How Good Are Query Optimizers, Really?*, VLDB 2015 —
    Join Order Benchmark, geomean-of-speedup convention.

[5] H. Zhang et al., *LpBound: Pessimistic Cardinality Estimation via LP
    Relaxation over ℓp-norms of Degree Sequences*, SIGMOD 2025 — the
    upper-bound envelope samkhya enforces.

[6] N. Hollmann et al., *TabPFN: Transformers solve small tabular problems*,
    ICLR 2023 — the TabPFN-2.5 architecture behind the opt-in
    `tabpfn_http` foundation-model backend (Prior Labs 2026 update).

[7] A. Atserias, M. Grohe, D. Marx, *Size Bounds and Query Plans for
    Relational Joins*, PODS 2008 — AGM bound, super-class of LpBound.

[8] F. Wilcoxon, *Individual Comparisons by Ranking Methods*, Biometrics
    Bulletin 1(6):80–83, 1945 — signed-rank paired test for latency.

[9] Y. Benjamini, Y. Hochberg, *Controlling the False Discovery Rate: A
    Practical and Powerful Approach to Multiple Testing*, JRSSB 57(1),
    1995 — FDR control for multi-cell comparison sets (24/55 on JOB-Slow).

[10] P. Flajolet, É. Fusy, O. Gandouet, F. Meunier, *HyperLogLog: the
     analysis of a near-optimal cardinality estimation algorithm*,
     DMTCS 2007 — `1.04/√(2^p)` standard-error envelope.

[11] B. H. Bloom, *Space/time trade-offs in hash coding with allowable
     errors*, CACM 13(7):422–426, 1970 — Bloom filter sizing formula
     `m = −n·ln(p)/(ln 2)²`.

[12] G. Cormode, S. Muthukrishnan, *An Improved Data Stream Summary: The
     Count-Min Sketch and its Applications*, J. Algorithms 55(1):58–75,
     2005 — CMS δ-bound verification reference.

[13] Y. Ioannidis, V. Poosala, *Balancing Histogram Optimality and
     Practicality for Query Result Size Estimation*, SIGMOD 1996 —
     MaxDiff histograms.

[14] H. V. Jagadish et al., *Optimal Histograms with Quality Guarantees*,
     VLDB 1998 — V-Optimal histograms.

[15] M. Stillger, G. M. Lohman, V. Markl, M. Kandil, *LEO — DB2's
     LEarning Optimizer*, SIGMOD 2001 — feedback-driven QO precedent
     for the `FeedbackStore` design.

[16] Iceberg Project, *Puffin Spec v1*,
     <https://iceberg.apache.org/puffin-spec/> — the sidecar format
     samkhya extends with `samkhya.*-v1` `KIND` tags.

---

*End of REPRODUCIBILITY.md.*
