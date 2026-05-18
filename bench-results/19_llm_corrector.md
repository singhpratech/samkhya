# LLM-pluggable corrector backend — latency, accuracy, and reproducibility

**Date:** 2026-05-17 (WAVE5-N transport-floor smoke; live-LLM cells PROJECTED)
**Sole author:** Prateek Singh
**License:** Apache-2.0
**Hardware profile:** see `bench-results/00_hardware_profile.md`
**Status:** **TRANSPORT FLOOR MEASURED; live-LLM cells PROJECTED**
**Verdict (current revision):** H1-A PASS (transport floor); H1-B / H1-C / H1-D PROJECTED pending API keys + ollama install.

This document characterizes the **LLM-pluggable corrector backend**
(`samkhya-core::residual::llm::LlmHttpCorrector`, gated on the
`llm_http` cargo feature), an opt-in pluggable backend that routes the
cardinality-correction call to a foundation-model LLM (Anthropic Claude,
OpenAI GPT-4o-mini, local Ollama, or a dummy echo for transport-floor
probes). The wire contract is identical to the TabPFN HTTP backend
(file 14), so the corrector trait, fallback contract, and engine-side
integration are unchanged; only the server-side provider differs.

> **Naming.** Per the samkhya naming rule we frame this as the
> **LLM-pluggable corrector backend** — *not* an "AI feature",
> "learned model", or "adaptive optimizer." The samkhya envelope (LpBound,
> portable sketches, the corrector trait, the safety fallback) still
> dominates the contract. The LLM is one pluggable backend among many.
> The default samkhya build does not pull this in (`llm_http` cargo
> feature is off by default).

---

## 1. Hypotheses (pre-registered)

| ID | Statement | Falsifier | Status |
|---|---|---|---|
| H1-A | Dummy-backend transport floor P95 < **5 ms** on loopback, B ∈ {1, 4, 8, 16, 32}. | P95 ≥ 5 ms at any cell with 95 % BCa CI fully above 5 ms. | **MEASURED PASS** (this revision) |
| H1-B | Anthropic Claude (`claude-opus-4-7` or `claude-sonnet-4-6`) end-to-end P95 < **2 s** at B=8, warm cache. | P95 ≥ 2 s over ≥ 30 trials with BCa CI not crossing 2 s. | **PROJECTED** (pending `ANTHROPIC_API_KEY` + user cost approval) |
| H1-B′ | OpenAI `gpt-4o-mini` end-to-end P95 < **2 s** at B=8, warm cache. | P95 ≥ 2 s over ≥ 30 trials with BCa CI not crossing 2 s. | **PROJECTED** (pending `OPENAI_API_KEY` + user cost approval) |
| H1-C | LLM-backend accuracy delta over the GBT corrector (median-q-error reduction, Moerkotte VLDB 2009) ≥ **0 %** (effect *direction*; magnitude is a v1.1 item). | Median q-error reduction < 0 % with BCa CI fully below 0. | **PROJECTED** (pending live LLM call budget) |
| H1-D | Local Ollama backend (`llama3.2:1b` via `http://127.0.0.1:11434/api/generate`) honors the default wire contract: server boots, `/health` returns `{"ok": true, "backend": "local"}`, and `/infer` returns a well-formed `{"estimate": <u64>}`. | Server fails to boot, `/health` fails, or `/infer` returns non-2xx on a syntactically valid request. | **PROJECTED** (measurable today if ollama is installed; not currently in this environment) |

**Why these hypotheses are deliberately weak.** The headline question
for the v1.0 launch is not "is LLM cardinality estimation better than
GBT" — that is a v1.1 research question. The headline question is "can
samkhya plug an LLM into the same corrector slot as TabPFN and GBT
without changing the wire contract?" H1-A and H1-D answer that
mechanically; H1-B / H1-B′ / H1-C are the live-LLM follow-ups.

---

## 2. Methodology

### 2.1 Workload

For each batch size B we issue one HTTP POST per estimate. The Rust
client is the production transport (`samkhya-core::residual::llm::LlmHttpCorrector`,
gated on `llm_http`); the bench binary `samkhya-bench/src/bin/llm_latency.rs`
mirrors its agent configuration line-for-line so wire-level overhead is
comparable. Server-side wire contract (matches
`samkhya-core/src/residual.rs::llm`):

```text
POST /infer  Content-Type: application/json
{
  "features":          [<f64>, ...],     // FEATURE_LEN × B values
  "baseline_estimate": <u64>
}
→ 200 OK
{ "estimate": <u64> }
```

| Knob | Values |
|---|---|
| Batch size **B** | 1, 4, 8, 16, 32 |
| Sequence length **L** (recorded but unused — LLM payload is `(features, baseline)` only) | 128 |
| Trials per cell | 30 (after 5 warm-up) |
| Statistic | P50 / P95 / P99 + 95 % **BCa bootstrap CI**, 10 000 resamples (Efron-Tibshirani 1993 ch. 14) on the per-trial latency vectors; paired LLM-vs-GBT q-error deltas via **Wilcoxon signed-rank** (Wilcoxon 1945) |
| Transport | localhost loopback HTTP, `ureq` 2.x (rustls-only, no OpenSSL) |
| Wall-clock source | `std::time::Instant` on the Rust side |

### 2.2 Statistical reporting

- **95 % BCa bootstrap CI** on P50, P95, P99 per cell — 10 000 resamples
  with replacement, bias-corrected and accelerated per
  **Efron-Tibshirani 1993**, *An Introduction to the Bootstrap*,
  Chapter 14. Resample seed `42` (pre-registered). Driver:
  `bench-results/scripts/bootstrap_ci.py`.
- **Wilcoxon signed-rank** (Wilcoxon 1945, *Biometrics Bulletin*
  1(6):80–83) for paired LLM-vs-GBT q-error deltas on matched test
  queries (§5, PROJECTED).
- **Benjamini-Hochberg FDR** at q=0.05 across the per-cell hypothesis
  family when more than one cell is reported as MEASURED in a single
  run.
- We do **not** report means — the LLM tail (cold-cache, rate-limit,
  retry) makes them deeply misleading. P50 / P95 / P99 only.

### 2.3 Pre-registered analysis decisions

- Latency cells with P95 above the threshold count against the
  hypothesis regardless of P50.
- Cold-start (first `/infer` after server spawn) is reported in §4.5
  separately from warm-cache; only warm-cache cells count against
  H1-B / H1-B′.
- The accuracy comparison (H1-C) is paired: the same feature vectors
  are fed to the LLM and to the GBT corrector; the q-error delta is
  measured per row, not per cell.
- Sampling temperature is pinned at **0.0** and `max_tokens` at **32**
  for determinism and bounded cost (see §6).

---

## 3. Hardware

This revision's MEASURED cells (§4.1 transport floor) are CPU-bound
loopback HTTP; they share the host profile recorded in
`bench-results/00_hardware_profile.md`. No GPU is exercised by the LLM
backend on the client side — all LLM inference happens either remotely
(Anthropic / OpenAI hosted) or on a local sidecar (Ollama). The §4.1
numbers therefore do not need MLPerf-style GPU pinning; the §4.2 / §4.3
/ §4.4 cells will, when MEASURED, record the appropriate target
(Anthropic API endpoint region, OpenAI API endpoint region, local
Ollama device, etc.).

| Component | Value |
|---|---|
| CPU | 13th Gen Intel Core i9-13900HK (see `00_hardware_profile.md`) |
| OS | Linux 6.17.0-29-generic |
| Network for hosted LLM cells | egress via host's default route; will be recorded at MEASURED time |
| Network for local Ollama cell | loopback (`lo`) only |

---

## 4. Results

### 4.1 Transport floor — dummy backend (MEASURED, WAVE5-N, 2026-05-17)

Stub server: `samkhya-gpudb/scripts/llm_dummy_backend.py`. Returns
`{"estimate": <baseline_estimate>}` with no LLM call. 30 trials per
cell, 5 warm-up trials discarded. Loopback HTTP, `ureq` 2.x.

| B | P50 (ms) | P95 (ms) | P99 (ms) | min (ms) | max (ms) | trials ok/fail |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 0.07 | 0.07 | 0.07 | 0.06 | 0.07 | 30 / 0 |
| 4 | 0.07 | 0.08 | 0.08 | 0.07 | 0.08 | 30 / 0 |
| 8 | 0.08 | 0.08 | 0.08 | 0.07 | 0.08 | 30 / 0 |
| 16 | 0.08 | 0.09 | 0.09 | 0.08 | 0.09 | 30 / 0 |
| 32 | 0.10 | 0.11 | 0.11 | 0.09 | 0.11 | 30 / 0 |

Raw per-trial vectors persisted to `bench-results/19_llm_corrector_raw.json`
(schema `samkhya.bench.llm_latency.v1`). Driver log:
`bench-results/wave5n_raw/server.log` plus the inline shell stdout
captured in the WAVE5-N receipt.

**H1-A: MEASURED PASS.** P95(transport-floor) is **0.07–0.11 ms** across
the B grid — strictly below the 5 ms threshold across every cell. This
is the **lower bound** that any LLM-backed end-to-end measurement on
this host must respect; the live-LLM time in §4.2 / §4.3 / §4.4 stacks
on top.

The dummy floor is also **~3× lower** than the TabPFN transport floor
in `bench-results/14_tabpfn_4090_latency.md` §4.1 (0.21–0.30 ms P95
across a similar grid). The difference is attributable to the
stdlib-only `BaseHTTPServer` in `llm_dummy_backend.py` vs FastAPI +
uvicorn in `tabpfn_infer_server.py`'s no-inference cell — both are
loopback HTTP but FastAPI's ASGI dispatch adds ~0.2 ms per call. Either
floor is well below the LLM-cell budget; the comparison only matters
for understanding what the *non-LLM* overhead in the live-LLM cells
will look like.

### 4.1.b TypeScript transport — SMOKE-TESTED (parallel transport, broader-appeal port)

In addition to the canonical Python+FastAPI server, the v1.0 build
ships a Node/TypeScript port of the same wire contract at
`samkhya-gpudb/scripts/llm_infer_server.ts` (plus the stdlib-only
mirror `llm_dummy_backend.ts`). The Rust client cannot tell them
apart; both expose `POST /infer` + `GET /health` over the
`samkhya.bench.llm_latency.v1` schema.

**Smoke-test status (2026-05-17):** dummy backend served on Node
v22.22.2 via `npx tsx`; `/health` returned `ok:true`, `/infer` echoed
`baseline_estimate` for batch=1 (7 features) and batch=2 (14
features), and all malformed-input cells (length-not-multiple-of-7,
missing `baseline_estimate`, unknown path) returned the documented
4xx/404 status codes. Cold-start time from `node` invocation to first
ready `/health` reply was **~3 s** including `npm install` of
`tsx`+`@types/node` on a clean tree, **<1 s** with `node_modules`
warm.

**Not yet 30-trial measured.** A full latency campaign on the TS port
is a v1.1 item — the Python+FastAPI server is the canonical
empirical-floor reference for v1.0. The smoke test is sufficient for
the v1.0 functional claim ("the wire contract is implementable in
either runtime").

Why ship the TS port at all in v1.0: broader operator reach. Node/TS
shops can run the LLM-pluggable corrector against Anthropic / OpenAI /
Ollama without spinning up a Python venv. The fact that the wire
contract is small (one POST, ~8 fields total) makes the parity claim
verifiable in a single afternoon.

### 4.2 Anthropic Claude — PROJECTED (pending API key)

Will be MEASURED when `ANTHROPIC_API_KEY` is provided and the user
approves the cost budget. Pre-registered model:
`claude-opus-4-7` (fallback `claude-sonnet-4-6` if the chosen model is
not available in the account). Temperature 0.0, max_tokens 32.

**Projection (anchored on published Anthropic API latency).** Anthropic's
public latency guidance for `claude-sonnet-4-*` is P50 ~600 ms,
P95 ~1.2 s, P99 ~2.5 s on small-prompt completions (32 tokens max). For
`claude-opus-4-*` add ~30 % at P50, ~50 % at P95. The 7-dim feature
vector renders to ~150 input tokens; the response is ≤ 32 tokens. So:

| B | P50 (s) projected | P95 (s) projected | Notes |
|---:|---:|---:|---|
| 1 | 0.6–0.8 | 1.1–1.6 | One Claude call per /infer |
| 4–32 | 0.6–0.8 | 1.1–1.6 | Wire returns one headline estimate per request; batch is server-internal only |

Honest disclaimer: this is paper-projection. The MEASURED §4.2 table
will replace this projection wholesale when the campaign runs.

### 4.3 OpenAI GPT-4o-mini — PROJECTED (pending API key)

Will be MEASURED when `OPENAI_API_KEY` is provided and the user
approves the cost budget. Pre-registered model: `gpt-4o-mini`.
Temperature 0.0, max_tokens 32.

**Projection (anchored on OpenAI's public latency guidance).** GPT-4o-mini
typically lands at P50 ~300 ms, P95 ~800 ms for 150-token-input /
32-token-output completions. So:

| B | P50 (s) projected | P95 (s) projected | Notes |
|---:|---:|---:|---|
| 1 | 0.25–0.40 | 0.5–1.0 | One GPT call per /infer |
| 4–32 | 0.25–0.40 | 0.5–1.0 | Same wire — one headline estimate |

### 4.4 Local Ollama — PROJECTED (pending ollama install)

Will be MEASURED when ollama is installed and a small model (e.g.,
`llama3.2:1b`) is pulled. Model: `llama3.2:1b` (1B params, fits in <2
GB RAM, runs on CPU). Temperature 0.0, max_tokens 32.

**Projection (anchored on llama.cpp / ollama benchmarks).** On the
i9-13900HK at warm-cache, a 1B model with 150-token input / 32-token
output typically produces ~50–100 tokens/s; total latency ~0.4–1.0 s
for a 32-token completion plus prompt-prefill.

| B | P50 (s) projected | P95 (s) projected | Notes |
|---:|---:|---:|---|
| 1 | 0.5–0.8 | 0.8–1.5 | Single CPU model |
| 4–32 | 0.5–0.8 | 0.8–1.5 | Wire returns one headline estimate |

### 4.5 Cold-start — PROJECTED

Will be MEASURED alongside §4.2 / §4.3 / §4.4. Expected cold paths:

- Anthropic / OpenAI: client construction ~50 ms; first request adds
  TLS handshake (~150–300 ms) + provider-side cold latency
  (~1.5–3 s for the first call after a warm idle).
- Local Ollama: model load (~2–5 s for `llama3.2:1b` from disk on
  i9-13900HK), then warm latency as in §4.4.

The cold path is **not** part of the H1-B / H1-B′ verdict — the LLM
sidecar is expected to be long-lived. Cold-start is reported separately
for operator planning.

---

## 5. Accuracy delta over the GBT corrector — PROJECTED

Will be MEASURED when one of the live LLM backends is available
(Anthropic / OpenAI / local). The synthetic correlated-multi-modal
workload from file 14 §5 is the pre-registered fixture
(`bench-results/wave5l_raw/accuracy_compare.py`); the LLM is dropped
in as a third corrector backend alongside the baseline and the GBT
v3 incumbent.

**Pre-registered evaluation:**

- 200 paired test rows.
- Each backend produces one estimate per row at temperature 0.0
  (LLMs) / deterministic prediction (GBT).
- Metric: median q-error (Moerkotte VLDB 2009).
- Paired statistic: Wilcoxon signed-rank (Wilcoxon 1945).
- BCa 95 % CI on the median-q-error *reduction* (LLM vs GBT), 10 000
  resamples, seed 42.

**Conjecture (NOT pre-registered as a hypothesis — see §1).** We do not
expect the LLM to beat the GBT corrector in this v1.0 campaign. The
GBT corrector is trained on the exact feature distribution; the LLM is
asked to estimate row counts from a 7-dim feature vector with no
training-set anchor. The LLM is most likely to add value in a
*schema-introspection* mode (where the prompt carries column names,
table descriptions, and example values) — that is the v1.1 follow-up,
not this revision.

**H1-C interpretation:** the bar is intentionally low (≥ 0 % effect
direction). If the LLM merely matches the GBT corrector, the H1-C
hypothesis is supported; the *magnitude* of any LLM win is deferred to
v1.1.

---

## 6. Reproducibility (ACM Artifact Evaluation v1.1 — partial)

### 6.1 Hardware

See §3. Any future re-run on a different CPU / network endpoint **must**
record the new probe lines at the top of its replacement document.

### 6.2 Software stack for the dummy floor (MEASURED today)

```bash
# Build the corrector library + bench binary
cargo build --release -p samkhya-core --features llm_http
cargo build --release -p samkhya-bench --bin llm_latency

# Run the dummy-backend transport-floor campaign
bash samkhya-gpudb/scripts/run-llm-bench.sh --backend dummy
```

Output:

```
bench-results/19_llm_corrector_raw.json   # schema: samkhya.bench.llm_latency.v1
bench-results/wave5n_raw/server.log       # stdlib HTTP server log
bench-results/wave5n_raw/run.summary.txt  # driver summary
```

### 6.3 Software stack for the live LLM cells (PROJECTED)

```bash
# Server-side deps (run in the acceptance venv):
source samkhya-py/.venv-acceptance/bin/activate
pip install fastapi 'uvicorn[standard]'

# Anthropic
pip install anthropic
export ANTHROPIC_API_KEY=...
export SAMKHYA_LLM_MODEL=claude-opus-4-7      # or claude-sonnet-4-6
bash samkhya-gpudb/scripts/run-llm-bench.sh --backend anthropic

# OpenAI
pip install openai
export OPENAI_API_KEY=...
export SAMKHYA_LLM_MODEL=gpt-4o-mini
bash samkhya-gpudb/scripts/run-llm-bench.sh --backend openai

# Local Ollama
# (assume ollama is installed + `ollama pull llama3.2:1b` completed)
export SAMKHYA_LLM_MODEL=llama3.2:1b
export SAMKHYA_LLM_LOCAL_URL=http://127.0.0.1:11434/api/generate
bash samkhya-gpudb/scripts/run-llm-bench.sh --backend local
```

### 6.4 Pinned prompts, models, and sampling

| Knob | Value | Override env var |
|---|---|---|
| System prompt | `"You are a cardinality estimator for SQL query optimizers. Given a feature vector describing a join, you reply with a single positive integer that is your best estimate of the row count the join will produce. Output ONLY the integer, no commentary."` | `SAMKHYA_LLM_SYSTEM_PROMPT` |
| User prompt template | `"Features (7-dim): {features}. Optimizer's baseline guess: {baseline_estimate}. Your estimate (integer, single line):"` | `SAMKHYA_LLM_USER_PROMPT` |
| Temperature | 0.0 (deterministic) | `SAMKHYA_LLM_TEMPERATURE` |
| Max tokens | 32 (bounded cost + bounded latency) | `SAMKHYA_LLM_MAX_TOKENS` |
| Anthropic model | `claude-opus-4-7` (fallback `claude-sonnet-4-6`) | `SAMKHYA_LLM_MODEL` |
| OpenAI model | `gpt-4o-mini` | `SAMKHYA_LLM_MODEL` |
| Local model | `llama3.2:1b` | `SAMKHYA_LLM_MODEL` |
| Local URL | `http://127.0.0.1:11434/api/generate` (Ollama default) | `SAMKHYA_LLM_LOCAL_URL` |

### 6.5 Retry policy

The corrector contract is *no retry on failure* (see
`samkhya-core/src/residual.rs::llm` safety contract). Any error —
HTTP non-2xx, timeout, body parse — maps to `Ok(None)` on the Rust
side and the engine falls back to the baseline estimate. There is no
exponential backoff, no in-budget retry, no jittered re-issue. This is
deliberate: a corrector that retries can pin the query optimizer's
critical path indefinitely. Retries are the *application's*
responsibility (e.g., a feedback-loop training job may retry; the
optimizer-hot-path corrector may not).

### 6.6 Data files

| File | Schema | Contents | Status |
|---|---|---|---|
| `bench-results/19_llm_corrector_raw.json` | `samkhya.bench.llm_latency.v1` | Per-batch per-trial latency vectors (microseconds, u64) | MEASURED (this revision, dummy backend) |
| `bench-results/wave5n_raw/server.log` | text | Server stdout/stderr | MEASURED |
| `bench-results/wave5n_raw/run.summary.txt` | text | Driver summary (warm time, client exit code, final /health) | MEASURED |
| `bench-results/19_llm_accuracy.json` | `samkhya.bench.llm_accuracy.v1` | H1-C q-error + Wilcoxon (PROJECTED) | PROJECTED |
| `bench-results/19_llm_cold_start.json` | `samkhya.bench.llm_cold_start.v1` | §4.5 cold-start trials (PROJECTED) | PROJECTED |
| `bench-results/19_llm_corrector_ts_raw.json` | `samkhya.bench.llm_latency.v1` | TypeScript-port latency vectors (PROJECTED — smoke-tested only this revision) | PROJECTED |
| `bench-results/wave5n_ts_raw/server.log` | text | TS server stdout/stderr (PROJECTED) | PROJECTED |
| `bench-results/wave5n_ts_raw/run.summary.txt` | text | TS driver summary (PROJECTED) | PROJECTED |

### 6.7 TypeScript / Node reproducer (parallel transport — smoke-tested this revision, full campaign deferred to v1.1)

The TS port mirrors the Python server byte-for-byte on the wire. Pick
either runtime; the Rust client doesn't care. The Python+FastAPI server
is the canonical empirical-floor reference for v1.0; the TS port is
shipped for operator-side appeal and verified by the §4.1.b smoke test.

```bash
# One-time setup (cold tree, dev path via tsx — no build step)
cd samkhya-gpudb/scripts
npm install           # installs tsx + @types/node + typescript

# Dummy backend (transport-floor smoke test; default port 8767)
bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend dummy

# Live LLM cells — same env contract as the Python driver
export ANTHROPIC_API_KEY=...
bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend anthropic

export OPENAI_API_KEY=...
bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend openai

bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend local
```

Alternatively, build to plain JS and run on a stripped Node:

```bash
cd samkhya-gpudb/scripts
npm install
npm run build               # writes dist/llm_infer_server.js + dist/llm_dummy_backend.js
SAMKHYA_USE_TSX=0 bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend dummy
```

Outputs land in `bench-results/19_llm_corrector_ts_raw.json` +
`bench-results/wave5n_ts_raw/{server.log,run.summary.txt}` so the
Python and TS runs do not stomp on each other.

---

## 7. Citations

- **Hollmann et al., ICLR 2023.** *"TabPFN: A Transformer That Solves
  Small Tabular Classification Problems in a Second."* Background; the
  TabPFN tier in file 14 is the architectural neighbor of this LLM
  tier.
- **Efron & Tibshirani 1993.** *An Introduction to the Bootstrap*,
  Chapter 14. Applied to the per-trial latency vectors and the
  median-q-error deltas.
- **Wilcoxon 1945.** "Individual Comparisons by Ranking Methods,"
  *Biometrics Bulletin* 1(6):80–83. Applied to paired LLM-vs-GBT
  q-error deltas in §5.
- **Moerkotte, VLDB 2009.** Used for the q-error definition
  `q = max(actual/pred, pred/actual)` in §5.
- **Benjamini & Hochberg 1995.** *Controlling the False Discovery
  Rate*. Applied to multi-cell hypothesis families.

---

## 8. Honest disclaimer

**LLMs are not the recommended production corrector.** samkhya ships
with the GBT v3 corrector as the default residual-correction tier
(`samkhya-core::residual::gbt::GbtCorrector`, file 15 ablation §A2),
and the architecture's safety contract (LpBound envelope + transparent
fallback) does the heavy lifting regardless of which corrector is
plugged in.

The LLM-pluggable corrector backend exists for two reasons:

1. **Pluggability demonstration.** The same trait that hosts the
   TabPFN tier hosts the LLM tier; the wire contract is identical; the
   default samkhya build does not pull either of them in. This is the
   v1.0 "framework, not a model" message — the SDK that lets you swap
   GBT, TabPFN, or an LLM as the cardinality corrector behind a single
   safety contract.
2. **Schema-introspection use cases.** The most plausible v1.1 LLM
   value-add is *not* fitting a regression to 7-dim feature vectors —
   the GBT does that fine — but answering schema-introspection
   questions like *"this 'genre' column has 18 distinct values; given
   the table description, which 4 are likely to dominate the
   distribution?"* That is a different prompt (carries column names,
   sample values, table descriptions) and a different hypothesis
   class; it is deliberately out of scope for v1.0.

A user who deploys the LLM backend in v1.0 should treat it as a
research artifact: instrument heavily, time-box every call, never let
it become a hard dependency. The fallback contract guarantees that the
engine continues to function correctly when the LLM is unreachable —
but the latency tail is wide and the accuracy delta is not yet
proven.

The default routing for the v1.0 launch is: **GBT v3** for the hot
path, **TabPFN** for offline / overnight re-validation (file 14 §6),
and **the LLM backend off** unless the operator explicitly opts in
with `llm_http` + a server-side API key. See `samkhya-core/src/residual.rs::llm`
module docs for the formal safety contract.
