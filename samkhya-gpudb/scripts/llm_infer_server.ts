// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Prateek Singh
//
// llm_infer_server.ts — TypeScript port of llm_infer_server.py.
//
// Mirrors the wire contract from `samkhya-core::residual::llm`
// (see `samkhya-core/src/residual.rs`):
//
//     POST /infer   Content-Type: application/json
//     {
//         "features":          [<f64>, ...],   // FEATURE_LEN * B values
//         "baseline_estimate": <u64>
//     }
//     -> 200 OK
//     { "estimate": <u64> }
//
//     GET /health
//     -> 200 OK
//     { "ok": true, "backend": "<name>", "model": "<id>" }
//
// Behavioural parity with the FastAPI server is intentional: the Rust
// corrector layer absorbs non-2xx, parse failures, and timeouts as
// `Ok(None)` (fall back to baseline), so the TS port is free to surface
// 4xx on malformed input or 503 on mis-configured backends without
// changing the client behaviour.
//
// Why TypeScript in addition to Python: broader operator appeal. The
// FastAPI server stays the canonical reference (it's what the v1.0
// empirical campaign measured); this port lets Node/TS shops run the
// LLM-pluggable corrector without spinning up a Python venv.
//
// Pluggable backends (selected via SAMKHYA_LLM_BACKEND):
//
//   anthropic (default) — Anthropic Claude via @anthropic-ai/sdk.
//                          Reads ANTHROPIC_API_KEY. Default model
//                          claude-opus-4-7 with claude-sonnet-4-6
//                          fallback semantics matching the Python side.
//   openai              — OpenAI Chat Completions via the `openai`
//                          package. Reads OPENAI_API_KEY. Default
//                          model gpt-4o-mini.
//   local               — Local LLM HTTP endpoint (Ollama / llama.cpp).
//                          Reads SAMKHYA_LLM_LOCAL_URL (default
//                          http://127.0.0.1:11434/api/generate).
//                          Default model llama3.2:1b.
//   dummy               — Returns baseline_estimate unchanged. Used by
//                          the transport-floor benchmark cell so
//                          reviewers without an API key can still
//                          measure the HTTP round-trip cost end-to-end.
//
// Determinism: temperature=0.0, max_tokens=32 baked in. Override via
// SAMKHYA_LLM_TEMPERATURE / SAMKHYA_LLM_MAX_TOKENS.
//
// Logging: one line per request to stderr,
//     [llm] backend=<name> model=<id> latency_ms=<f64> status=ok|parse_err|api_err
// No body content is logged (privacy). The Rust client never logs
// features either — see residual.rs::llm module docs.

import * as http from 'node:http';
import { URL } from 'node:url';

const FEATURE_LEN = 7;
const U64_MAX = (1n << 64n) - 1n;

// SECURITY-REVIEW-2026-05-17.md (H3 + H4 + M1): env-var prompt cap,
// HTTP body cap, and per-request batch cap. Tuned identically to the
// Python server so behaviour is portable across transports.
const PROMPT_MAX_BYTES = 16 * 1024;
const BODY_MAX_BYTES = 8 * 1024 * 1024;
const MAX_INFER_BATCHES = 1024;

const DEFAULT_SYSTEM_PROMPT =
  'You are a cardinality estimator for SQL query optimizers. ' +
  'Given a feature vector describing a join, you reply with a single ' +
  'positive integer that is your best estimate of the row count the ' +
  'join will produce. Output ONLY the integer, no commentary.';

const DEFAULT_USER_PROMPT =
  'Features (7-dim): {features}. ' +
  "Optimizer's baseline guess: {baseline_estimate}. " +
  'Your estimate (integer, single line):';

type BackendName = 'dummy' | 'anthropic' | 'openai' | 'local';

interface ServerState {
  backend: BackendName;
  model: string;
  client: any;
  systemPrompt: string;
  userPrompt: string;
  localUrl: string | null;
  temperature: number;
  maxTokens: number;
  startedAtNs: bigint;
}

const state: ServerState = {
  backend: 'dummy',
  model: 'none',
  client: null,
  systemPrompt: DEFAULT_SYSTEM_PROMPT,
  userPrompt: DEFAULT_USER_PROMPT,
  localUrl: null,
  temperature: 0.0,
  maxTokens: 32,
  startedAtNs: 0n,
};

function logLine(backend: string, model: string, latencyMs: number, status: string): void {
  process.stderr.write(
    `[llm] backend=${backend} model=${model} latency_ms=${latencyMs.toFixed(3)} status=${status}\n`,
  );
}

function parseFirstInteger(text: string): bigint | null {
  if (!text) return null;
  const cleaned = text.replace(/,/g, '');
  const m = cleaned.match(/\d+/);
  if (!m) return null;
  try {
    const v = BigInt(m[0]);
    if (v < 0n) return null;
    if (v > U64_MAX) return U64_MAX;
    return v;
  } catch {
    return null;
  }
}

function renderUserPrompt(features: number[], baselineEstimate: bigint): string {
  const head = features.slice(0, FEATURE_LEN);
  const featureStr = JSON.stringify(head.map((x) => Number(x)));
  return state.userPrompt
    .replace('{features}', featureStr)
    .replace('{baseline_estimate}', baselineEstimate.toString());
}

// ---------------------------------------------------------------------------
// Backend adapters. Each returns [estimate, raw_reply].
// ---------------------------------------------------------------------------

type BackendResult = [bigint | null, string];

async function backendDummy(_prompt: string, baseline: bigint): Promise<BackendResult> {
  return [baseline, baseline.toString()];
}

async function backendAnthropic(userPrompt: string, baseline: bigint): Promise<BackendResult> {
  const client = state.client;
  const model = state.model;
  try {
    const msg = await client.messages.create({
      model,
      max_tokens: state.maxTokens,
      temperature: state.temperature,
      system: state.systemPrompt,
      messages: [{ role: 'user', content: userPrompt }],
    });
    const textBlocks: string[] = (msg.content ?? [])
      .filter((b: any) => b?.type === 'text')
      .map((b: any) => b.text as string);
    const raw = textBlocks[0] ?? '';
    const parsed = parseFirstInteger(raw);
    return [parsed ?? baseline, raw];
  } catch (exc) {
    // See SECURITY-REVIEW-2026-05-17.md (C3): full exception detail is
    // logged locally for operator diagnosis, but only the constructor
    // name goes on the wire so credentials / stack traces / upstream
    // API error bodies cannot be probed through malformed requests.
    process.stderr.write(`[llm] anthropic api_err: ${String(exc)}\n`);
    logLine('anthropic', model, 0.0, 'api_err');
    const name = (exc as { constructor?: { name?: string } })?.constructor?.name ?? 'Error';
    return [baseline, `<api_err: ${name}>`];
  }
}

async function backendOpenai(userPrompt: string, baseline: bigint): Promise<BackendResult> {
  const client = state.client;
  const model = state.model;
  try {
    const resp = await client.chat.completions.create({
      model,
      max_tokens: state.maxTokens,
      temperature: state.temperature,
      messages: [
        { role: 'system', content: state.systemPrompt },
        { role: 'user', content: userPrompt },
      ],
    });
    const raw = resp.choices?.[0]?.message?.content ?? '';
    const parsed = parseFirstInteger(raw);
    return [parsed ?? baseline, raw];
  } catch (exc) {
    process.stderr.write(`[llm] openai api_err: ${String(exc)}\n`);
    logLine('openai', model, 0.0, 'api_err');
    const name = (exc as { constructor?: { name?: string } })?.constructor?.name ?? 'Error';
    return [baseline, `<api_err: ${name}>`];
  }
}

async function backendLocal(userPrompt: string, baseline: bigint): Promise<BackendResult> {
  const url = state.localUrl!;
  const model = state.model;
  // SECURITY-REVIEW-2026-05-17.md (M5): we want the overall request
  // bounded AND the read phase bounded per-chunk so a hung upstream
  // doesn't burn the full overall budget waiting on a stalled socket.
  //  - HARD_DEADLINE_MS is the absolute ceiling.
  //  - IDLE_DEADLINE_MS resets on every chunk we receive; if the
  //    upstream goes silent for longer than this we abort early.
  const HARD_DEADLINE_MS = 55_000;
  const IDLE_DEADLINE_MS = 15_000;
  const controller = new AbortController();
  const hard = setTimeout(() => controller.abort(), HARD_DEADLINE_MS);
  let idle = setTimeout(() => controller.abort(), IDLE_DEADLINE_MS);
  const bumpIdle = () => {
    clearTimeout(idle);
    idle = setTimeout(() => controller.abort(), IDLE_DEADLINE_MS);
  };
  try {
    const resp = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        model,
        prompt: `${state.systemPrompt}\n\n${userPrompt}`,
        stream: false,
        options: {
          temperature: state.temperature,
          num_predict: state.maxTokens,
        },
      }),
      signal: controller.signal,
    });
    bumpIdle();
    if (!resp.ok) {
      clearTimeout(hard);
      clearTimeout(idle);
      logLine('local', model, 0.0, 'api_err');
      return [baseline, `<api_err: HTTP ${resp.status}>`];
    }
    // Read the body in chunks so the idle timer can fire if the
    // upstream stalls mid-response. Cap the total read at BODY_MAX_BYTES
    // so a malicious local backend can't echo back a multi-GB body.
    const reader = resp.body?.getReader();
    let raw = '';
    if (reader) {
      const decoder = new TextDecoder('utf-8');
      let total = 0;
      while (true) {
        const { done, value } = await reader.read();
        bumpIdle();
        if (done) break;
        if (value) {
          total += value.byteLength;
          if (total > BODY_MAX_BYTES) {
            try {
              await reader.cancel();
            } catch {
              /* ignore */
            }
            clearTimeout(hard);
            clearTimeout(idle);
            logLine('local', model, 0.0, 'api_err');
            return [baseline, '<api_err: BodyTooLarge>'];
          }
          raw += decoder.decode(value, { stream: true });
        }
      }
      raw += decoder.decode();
    } else {
      raw = await resp.text();
    }
    clearTimeout(hard);
    clearTimeout(idle);
    let body: { response?: string };
    try {
      body = JSON.parse(raw) as { response?: string };
    } catch {
      logLine('local', model, 0.0, 'api_err');
      return [baseline, '<api_err: SyntaxError>'];
    }
    const text = body.response ?? '';
    const parsed = parseFirstInteger(text);
    return [parsed ?? baseline, text];
  } catch (exc) {
    clearTimeout(hard);
    clearTimeout(idle);
    process.stderr.write(`[llm] local api_err: ${String(exc)}\n`);
    logLine('local', model, 0.0, 'api_err');
    const name = (exc as { constructor?: { name?: string } })?.constructor?.name ?? 'Error';
    return [baseline, `<api_err: ${name}>`];
  }
}

const BACKENDS: Record<BackendName, (p: string, b: bigint) => Promise<BackendResult>> = {
  dummy: backendDummy,
  anthropic: backendAnthropic,
  openai: backendOpenai,
  local: backendLocal,
};

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

function loadPromptEnv(varName: string, defaultValue: string): string {
  // See SECURITY-REVIEW-2026-05-17.md (H3): reject prompt overrides
  // larger than PROMPT_MAX_BYTES so a shared-env adversary cannot OOM
  // the server or amplify upstream API cost.
  const val = process.env[varName];
  if (val === undefined) return defaultValue;
  if (Buffer.byteLength(val, 'utf8') > PROMPT_MAX_BYTES) {
    process.stderr.write(`[llm] ${varName} exceeds ${PROMPT_MAX_BYTES} bytes; refusing to start.\n`);
    process.exit(3);
  }
  return val;
}

async function loadBackend(name: string): Promise<void> {
  const n = name.toLowerCase().trim() as BackendName;
  if (!(n in BACKENDS)) {
    process.stderr.write(
      `[llm] unknown SAMKHYA_LLM_BACKEND=${name}; choose one of ${Object.keys(BACKENDS).sort().join(',')}\n`,
    );
    process.exit(2);
  }

  state.backend = n;
  state.systemPrompt = loadPromptEnv('SAMKHYA_LLM_SYSTEM_PROMPT', DEFAULT_SYSTEM_PROMPT);
  state.userPrompt = loadPromptEnv('SAMKHYA_LLM_USER_PROMPT', DEFAULT_USER_PROMPT);
  state.temperature = Number(process.env.SAMKHYA_LLM_TEMPERATURE ?? '0.0');
  state.maxTokens = Number(process.env.SAMKHYA_LLM_MAX_TOKENS ?? '32');

  if (n === 'dummy') {
    state.model = process.env.SAMKHYA_LLM_MODEL ?? 'dummy-echo';
    process.stderr.write(
      `[llm] backend=dummy model=${state.model} (transport-floor only; no LLM calls)\n`,
    );
  } else if (n === 'anthropic') {
    let mod: any;
    try {
      // @ts-expect-error optional peer dep — resolved at runtime when the
      // operator installs `@anthropic-ai/sdk`; TS cannot see it during
      // `tsc --noEmit` because it's a `peerDependency`, not a direct dep.
      mod = await import('@anthropic-ai/sdk');
    } catch {
      process.stderr.write(
        '[llm] backend=anthropic requested but @anthropic-ai/sdk is not installed; ' +
          'run `npm install @anthropic-ai/sdk` or pick another SAMKHYA_LLM_BACKEND.\n',
      );
      process.exit(3);
    }
    const apiKey = process.env.ANTHROPIC_API_KEY;
    if (!apiKey) {
      process.stderr.write('[llm] backend=anthropic requires ANTHROPIC_API_KEY in env.\n');
      process.exit(3);
    }
    state.model = process.env.SAMKHYA_LLM_MODEL ?? 'claude-opus-4-7';
    const Anthropic = mod.default ?? mod.Anthropic ?? mod;
    state.client = new Anthropic({ apiKey });
    process.stderr.write(`[llm] backend=anthropic model=${state.model} ready\n`);
  } else if (n === 'openai') {
    let mod: any;
    try {
      // @ts-expect-error optional peer dep — resolved at runtime when the
      // operator installs `openai`; TS cannot see it during `tsc --noEmit`
      // because it's a `peerDependency`, not a direct dep.
      mod = await import('openai');
    } catch {
      process.stderr.write(
        '[llm] backend=openai requested but the `openai` package is not installed; ' +
          'run `npm install openai` or pick another SAMKHYA_LLM_BACKEND.\n',
      );
      process.exit(3);
    }
    const apiKey = process.env.OPENAI_API_KEY;
    if (!apiKey) {
      process.stderr.write('[llm] backend=openai requires OPENAI_API_KEY in env.\n');
      process.exit(3);
    }
    state.model = process.env.SAMKHYA_LLM_MODEL ?? 'gpt-4o-mini';
    const OpenAI = mod.default ?? mod.OpenAI ?? mod;
    state.client = new OpenAI({ apiKey });
    process.stderr.write(`[llm] backend=openai model=${state.model} ready\n`);
  } else if (n === 'local') {
    state.model = process.env.SAMKHYA_LLM_MODEL ?? 'llama3.2:1b';
    state.localUrl =
      process.env.SAMKHYA_LLM_LOCAL_URL ?? 'http://127.0.0.1:11434/api/generate';
    process.stderr.write(
      `[llm] backend=local model=${state.model} url=${state.localUrl} ready\n`,
    );
  }

  state.startedAtNs = process.hrtime.bigint();
}

// ---------------------------------------------------------------------------
// HTTP routes
// ---------------------------------------------------------------------------

function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
  const buf = Buffer.from(JSON.stringify(body));
  res.writeHead(status, {
    'Content-Type': 'application/json',
    'Content-Length': buf.byteLength,
  });
  res.end(buf);
}

function parseU64Baseline(rawJson: string, body: any): bigint | null {
  if (body === null || typeof body !== 'object' || Array.isArray(body)) return null;
  if (typeof body.baseline_estimate !== 'number' || !Number.isInteger(body.baseline_estimate)) {
    return null;
  }
  // JSON.parse rounds integers above 2^53. Recover the one wire field from
  // its original decimal token and reject duplicate/nested keys.
  const matches = [...rawJson.matchAll(/"baseline_estimate"\s*:\s*(0|[1-9]\d*)(?![\d.eE])/g)];
  if (matches.length !== 1) return null;
  try {
    const value = BigInt(matches[0][1]);
    return value <= U64_MAX ? value : null;
  } catch {
    return null;
  }
}

function sendExactEstimate(
  res: http.ServerResponse,
  estimate: bigint,
  metadata: Record<string, unknown> = {},
): void {
  const encodedMetadata = JSON.stringify(metadata);
  const suffix = encodedMetadata === '{}' ? '' : `,${encodedMetadata.slice(1, -1)}`;
  const buf = Buffer.from(`{"estimate":${estimate.toString()}${suffix}}`);
  res.writeHead(200, {
    'Content-Type': 'application/json',
    'Content-Length': buf.byteLength,
  });
  res.end(buf);
}

async function readBody(req: http.IncomingMessage, maxBytes: number): Promise<Buffer | null> {
  // SECURITY-REVIEW-2026-05-17.md (H4): refuse upfront on
  // Content-Length and also enforce a streaming cap so chunked /
  // missing-header requests cannot OOM us. Returns null when the body
  // exceeds the cap; the caller must respond 413.
  const declared = req.headers['content-length'];
  if (declared !== undefined) {
    const n = Number(declared);
    if (Number.isFinite(n) && n > maxBytes) {
      req.resume();
      return null;
    }
  }
  return new Promise<Buffer | null>((resolve, reject) => {
    const chunks: Buffer[] = [];
    let total = 0;
    let aborted = false;
    req.on('data', (c: Buffer) => {
      if (aborted) return;
      total += c.byteLength;
      if (total > maxBytes) {
        aborted = true;
        chunks.length = 0;
        resolve(null);
        return;
      }
      chunks.push(c);
    });
    req.on('end', () => {
      if (!aborted) resolve(Buffer.concat(chunks));
    });
    req.on('error', reject);
  });
}

async function handleInfer(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
  const bodyBytes = await readBody(req, BODY_MAX_BYTES);
  if (bodyBytes === null) {
    sendJson(res, 413, { detail: `request body exceeds ${BODY_MAX_BYTES}` });
    return;
  }
  let body: any;
  const rawJson = bodyBytes.toString('utf8');
  try {
    body = JSON.parse(rawJson);
  } catch (exc) {
    // See SECURITY-REVIEW-2026-05-17.md (C3): full parse detail to
    // stderr, only the exception class on the wire.
    process.stderr.write(`[llm] /infer parse err: ${String(exc)}\n`);
    const name = (exc as { constructor?: { name?: string } })?.constructor?.name ?? 'Error';
    sendJson(res, 400, { detail: `invalid json: ${name}` });
    return;
  }

  if (body === null || typeof body !== 'object' || Array.isArray(body)) {
    sendJson(res, 400, { detail: 'json body must be an object' });
    return;
  }

  const features = body.features;
  const baseline = parseU64Baseline(rawJson, body);
  if (!Array.isArray(features) || features.length === 0) {
    sendJson(res, 400, { detail: "missing or empty 'features'" });
    return;
  }
  if (!features.every((value: unknown) => typeof value === 'number' && Number.isFinite(value))) {
    sendJson(res, 400, { detail: "'features' must contain only finite numbers" });
    return;
  }
  if (baseline === null) {
    sendJson(res, 400, { detail: "missing or non-u64 'baseline_estimate'" });
    return;
  }
  if (features.length % FEATURE_LEN !== 0) {
    sendJson(res, 400, {
      detail: `features length ${features.length} not a multiple of ${FEATURE_LEN}`,
    });
    return;
  }
  // SECURITY-REVIEW-2026-05-17.md (M1): per-request batch cap.
  const batches = features.length / FEATURE_LEN;
  if (batches > MAX_INFER_BATCHES) {
    sendJson(res, 413, {
      detail: `features batch count ${batches} exceeds ${MAX_INFER_BATCHES}`,
    });
    return;
  }

  const userPrompt = renderUserPrompt(features as number[], baseline);
  const backendFn = BACKENDS[state.backend];

  const t0 = process.hrtime.bigint();
  const [estimateRaw] = await backendFn(userPrompt, baseline);
  const elapsedMs = Number(process.hrtime.bigint() - t0) / 1_000_000;

  if (estimateRaw === null) {
    logLine(state.backend, state.model, elapsedMs, 'parse_err');
    sendExactEstimate(res, baseline, { _status: 'parse_err' });
    return;
  }

  let estimate = estimateRaw < 0n ? 0n : estimateRaw;
  if (estimate > U64_MAX) estimate = U64_MAX;

  logLine(state.backend, state.model, elapsedMs, 'ok');
  // Emit the BigInt as a raw JSON number token. JSON.stringify cannot encode
  // BigInt, and converting through Number would corrupt values above 2^53.
  sendExactEstimate(res, estimate, { _latency_ms: elapsedMs });
}

function handleHealth(_req: http.IncomingMessage, res: http.ServerResponse): void {
  if (state.startedAtNs === 0n) {
    sendJson(res, 503, { ok: false, reason: 'backend not loaded' });
    return;
  }
  sendJson(res, 200, {
    ok: true,
    backend: state.backend,
    model: state.model,
    temperature: state.temperature,
    max_tokens: state.maxTokens,
  });
}

function makeServer(): http.Server {
  return http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', `http://${req.headers.host ?? '127.0.0.1'}`);
    if (req.method === 'GET' && url.pathname === '/health') {
      handleHealth(req, res);
      return;
    }
    if (req.method === 'POST' && url.pathname === '/infer') {
      handleInfer(req, res).catch((exc) => {
        process.stderr.write(`[llm] /infer crashed: ${String(exc)}\n`);
        sendJson(res, 500, { detail: 'internal error' });
      });
      return;
    }
    sendJson(res, 404, { detail: 'not found' });
  });
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

function parseArgs(argv: string[]): { host: string; port: number; backend: string } {
  let host = process.env.SAMKHYA_LLM_HOST ?? '127.0.0.1';
  let port = Number(process.env.SAMKHYA_LLM_PORT ?? '8766');
  let backend = process.env.SAMKHYA_LLM_BACKEND ?? 'anthropic';
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--host') host = argv[++i];
    else if (a === '--port') port = Number(argv[++i]);
    else if (a === '--backend') backend = argv[++i];
    else if (a === '-h' || a === '--help') {
      process.stdout.write(
        'usage: node llm_infer_server.js [--host H] [--port P] [--backend dummy|anthropic|openai|local]\n',
      );
      process.exit(0);
    } else {
      process.stderr.write(`unknown arg: ${a}\n`);
      process.exit(2);
    }
  }
  return { host, port, backend };
}

const LOOPBACK_HOSTS = new Set(['127.0.0.1', '::1', 'localhost']);

function warnIfNonLoopbackBind(host: string): void {
  if (LOOPBACK_HOSTS.has(host)) return;
  const banner = '='.repeat(70);
  process.stderr.write(
    `\n${banner}\n` +
      `[WARN] samkhya LLM server bound to a non-loopback address (${host}).\n` +
      `[WARN] This server has NO authentication. API keys (anthropic /\n` +
      `[WARN] openai) in your environment will be used for ALL inbound\n` +
      `[WARN] requests. Ensure network isolation (firewall, VPN, or\n` +
      `[WARN] reverse-proxy with auth) before exposing this address.\n` +
      `${banner}\n`,
  );
}

async function main(): Promise<void> {
  const { host, port, backend } = parseArgs(process.argv.slice(2));
  process.env.SAMKHYA_LLM_BACKEND = backend;
  // SECURITY-REVIEW-2026-05-17.md (H1): warn before bind so the
  // message appears even if loadBackend fails fast.
  warnIfNonLoopbackBind(host);
  await loadBackend(backend);

  const server = makeServer();
  server.listen(port, host, () => {
    process.stderr.write(`[llm] listening on http://${host}:${port}\n`);
  });

  const shutdown = (sig: string) => {
    process.stderr.write(`[llm] received ${sig}; shutting down\n`);
    server.close(() => process.exit(0));
    setTimeout(() => process.exit(1), 5_000).unref();
  };
  process.on('SIGINT', () => shutdown('SIGINT'));
  process.on('SIGTERM', () => shutdown('SIGTERM'));
}

main().catch((exc) => {
  process.stderr.write(`[llm] fatal: ${String(exc)}\n`);
  process.exit(1);
});
