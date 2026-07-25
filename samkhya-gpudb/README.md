# samkhya-gpudb

A batch-scoring trait for samkhya's cardinality corrector, plus a CPU
implementation of it. **This is a scaffold.** There is no GPU code in this
crate — no CUDA, no Metal, no device kernels, no GPU runtime linked under
any feature. What ships is one trait (`GpuCorrector`) and one implementation
(`CpuFallbackCorrector`) that returns each row's baseline estimate
unchanged. The name records an intended direction, not a capability.

## What is here

`GpuCorrector` is the batch sibling of the single-row
`samkhya_core::residual::Corrector`. Its one method is
`fn batch_score(&self, features: &[CorrectionFeatures]) -> Result<Vec<u64>>`,
and the returned `Vec<u64>` has the same length and order as the input
slice. A kernel backend would score the whole slice in one dispatch; that is
the only reason the batch form exists. `CpuFallbackCorrector` is a
zero-sized struct that echoes each row's `baseline_estimate`, matching
`samkhya_core::residual::IdentityCorrector`. It keeps the trait exercised in
CI and gives an engine something that compiles. It corrects nothing.

## Install

```toml
[dependencies]
samkhya-gpudb = "1.2"
samkhya-core = "1.2"
```

## Example

```rust
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_core::residual::CorrectionFeatures;
use samkhya_gpudb::{CpuFallbackCorrector, GpuCorrector};

let corrector = CpuFallbackCorrector::new();
let batch = vec![
    CorrectionFeatures { baseline_estimate: 10,  ..Default::default() },
    CorrectionFeatures { baseline_estimate: 250, ..Default::default() },
];
let scored = corrector.batch_score(&batch).expect("batch_score");
assert_eq!(scored, vec![10, 250]);   // the fallback echoes baselines

// The ceiling is the caller's job. 10 orders to 100 line items over 10
// distinct keys: the join provably cannot exceed 100 rows.
const KEY: u32 = 0;
let orders = JoinRelation::new(10)
    .with_degree(KEY, AttributeDegree::from_distinct(10, 10));
let lineitem = JoinRelation::new(100)
    .with_degree(KEY, AttributeDegree::from_distinct(100, 10));
let graph = JoinGraph::new(vec![orders, lineitem]).with_edge(0, 1, KEY);
let ceiling = graph.ceiling();
assert_eq!(ceiling, 100);
let clamped: Vec<u64> = scored.iter().map(|&e| e.min(ceiling)).collect();
assert_eq!(clamped, vec![10, 100]);
```

## The ceiling is not applied here

samkhya's central guarantee is a provable join-cardinality ceiling: a
corrected estimate is held under a number the join cannot exceed.
`batch_score` does not enforce it and cannot — `CorrectionFeatures` carries
no ceiling field. Compute the ceiling from `samkhya_core::degree::JoinGraph`
and clamp the results yourself, as above. Versions of this file through 1.1
described `batch_score` as returning clamped estimates. It never did, and
the bound family they named was found unsound by the 2026-07-24 audit.

## Feature flags

`cuda` and `metal` are off by default and **gate no code today** — enabling
either compiles exactly the same bytes. They are reserved names so a kernel
backend can arrive as a feature flip, not a breaking change.

## The `scripts/` directory

The repository copy of this crate carries the server side of two opt-in
`samkhya-core` features. These are standalone programs; the Rust code here
neither launches nor depends on them.

| Script | Serves | Runtime |
| --- | --- | --- |
| `llm_infer_server.py` | `llm_http` | Python 3.10+, FastAPI, uvicorn |
| `llm_infer_server.ts` | `llm_http` | Node 18+, `node:http` |
| `tabpfn_infer_server.py` | `tabpfn_http` | Python, TabPFN |

Both LLM servers expose `POST /infer` and `GET /health` and speak the same
JSON: `{"features": [f64...], "baseline_estimate": u64}` in,
`{"estimate": u64}` out. `SAMKHYA_LLM_BACKEND` picks `anthropic`, `openai`,
`local` (Ollama / llama.cpp), or `dummy`, which returns the baseline
unchanged so transport cost can be measured without an API key. The bench
wrappers bind to `127.0.0.1` and default to port 8766 for Python and 8767
for Node, so both can run at once. A non-2xx, timeout, or unparseable
response means "no correction" to the Rust client, so a dead server degrades
to the baseline instead of failing the query. From a checkout of
<https://github.com/singhpratech/samkhya>,
`bash samkhya-gpudb/scripts/run-llm-bench.sh --backend dummy` starts the
Python server and benchmarks against it.

## Scope

- No kernel, no device code, no GPU dependency, under any feature.
- Nothing in the workspace calls `GpuCorrector`; no engine integration
  exists, and this file will not imply one until it does.
- `CpuFallbackCorrector` is an identity function, not a corrector to deploy.
- The `scripts/` servers are reference implementations for local
  benchmarking, not production services.

Apache-2.0. Sole author: Prateek Singh.
