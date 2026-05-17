// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Prateek Singh
//
// llm_dummy_backend.ts — zero-dep TypeScript mirror of
// llm_dummy_backend.py. Runs on bare Node 18+; no @anthropic-ai/sdk
// or openai required. Used by the file 19 transport-floor cell so a
// reviewer with only Node installed can replay the round-trip cost
// measurement end-to-end.
//
// Wire contract is identical to llm_infer_server.ts (POST /infer +
// GET /health). The "model" returns baseline_estimate verbatim — the
// number we measure is HTTP round-trip + JSON parse + Node event-loop
// scheduling, nothing else.

import * as http from 'node:http';
import { URL } from 'node:url';

const FEATURE_LEN = 7;
// SECURITY-REVIEW-2026-05-17.md (H4 + M1).
const BODY_MAX_BYTES = 8 * 1024 * 1024;
const MAX_INFER_BATCHES = 1024;

interface DummyState {
  startedAtNs: bigint;
  port: number;
  host: string;
}

const state: DummyState = {
  startedAtNs: 0n,
  port: 8766,
  host: '127.0.0.1',
};

function sendJson(res: http.ServerResponse, status: number, body: unknown): void {
  const buf = Buffer.from(JSON.stringify(body));
  res.writeHead(status, {
    'Content-Type': 'application/json',
    'Content-Length': buf.byteLength,
  });
  res.end(buf);
}

async function readBody(req: http.IncomingMessage, maxBytes: number): Promise<Buffer | null> {
  // SECURITY-REVIEW-2026-05-17.md (H4): same body-size guard as the
  // primary llm_infer_server.ts. Returns null on overflow.
  const declared = req.headers['content-length'];
  if (declared !== undefined) {
    const n = Number(declared);
    if (Number.isFinite(n) && n > maxBytes) return null;
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
        req.destroy();
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

function handleHealth(_req: http.IncomingMessage, res: http.ServerResponse): void {
  sendJson(res, 200, {
    ok: true,
    backend: 'dummy',
    model: 'dummy-echo-ts',
    temperature: 0.0,
    max_tokens: 32,
  });
}

async function handleInfer(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
  const bodyBytes = await readBody(req, BODY_MAX_BYTES);
  if (bodyBytes === null) {
    sendJson(res, 413, { detail: `request body exceeds ${BODY_MAX_BYTES}` });
    return;
  }
  let body: any;
  try {
    body = JSON.parse(bodyBytes.toString('utf8'));
  } catch (exc) {
    // See SECURITY-REVIEW-2026-05-17.md (C3): log full detail to stderr
    // but echo only the exception class on the wire.
    process.stderr.write(`[llm-dummy] /infer parse err: ${String(exc)}\n`);
    const name = (exc as { constructor?: { name?: string } })?.constructor?.name ?? 'Error';
    sendJson(res, 400, { detail: `invalid json: ${name}` });
    return;
  }
  const features = body.features;
  const baseline = body.baseline_estimate;
  if (!Array.isArray(features) || features.length === 0) {
    sendJson(res, 400, { detail: "missing or empty 'features'" });
    return;
  }
  if (typeof baseline !== 'number' || !Number.isInteger(baseline) || baseline < 0) {
    sendJson(res, 400, { detail: "missing or non-u64 'baseline_estimate'" });
    return;
  }
  if (features.length % FEATURE_LEN !== 0) {
    sendJson(res, 400, {
      detail: `features length ${features.length} not a multiple of ${FEATURE_LEN}`,
    });
    return;
  }
  const batches = features.length / FEATURE_LEN;
  if (batches > MAX_INFER_BATCHES) {
    sendJson(res, 413, {
      detail: `features batch count ${batches} exceeds ${MAX_INFER_BATCHES}`,
    });
    return;
  }
  sendJson(res, 200, { estimate: baseline });
}

function parseArgs(argv: string[]): { host: string; port: number } {
  let host = process.env.SAMKHYA_LLM_HOST ?? '127.0.0.1';
  let port = Number(process.env.SAMKHYA_LLM_PORT ?? '8766');
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--host') host = argv[++i];
    else if (a === '--port') port = Number(argv[++i]);
    else if (a === '-h' || a === '--help') {
      process.stdout.write('usage: node llm_dummy_backend.js [--host H] [--port P]\n');
      process.exit(0);
    } else {
      process.stderr.write(`unknown arg: ${a}\n`);
      process.exit(2);
    }
  }
  return { host, port };
}

const LOOPBACK_HOSTS = new Set(['127.0.0.1', '::1', 'localhost']);

function main(): void {
  const { host, port } = parseArgs(process.argv.slice(2));
  state.host = host;
  state.port = port;
  state.startedAtNs = process.hrtime.bigint();

  // SECURITY-REVIEW-2026-05-17.md (H1): warn on non-loopback bind.
  if (!LOOPBACK_HOSTS.has(host)) {
    const banner = '='.repeat(70);
    process.stderr.write(
      `\n${banner}\n` +
        `[WARN] samkhya LLM dummy server bound to non-loopback (${host}).\n` +
        `[WARN] This server has NO authentication. Ensure network isolation\n` +
        `[WARN] before exposing the address.\n` +
        `${banner}\n`,
    );
  }

  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', `http://${req.headers.host ?? host}`);
    if (req.method === 'GET' && url.pathname === '/health') {
      handleHealth(req, res);
      return;
    }
    if (req.method === 'POST' && url.pathname === '/infer') {
      handleInfer(req, res).catch((exc) => {
        process.stderr.write(`[llm-dummy] /infer crashed: ${String(exc)}\n`);
        sendJson(res, 500, { detail: 'internal error' });
      });
      return;
    }
    sendJson(res, 404, { detail: 'not found' });
  });

  server.listen(port, host, () => {
    process.stderr.write(`[llm-dummy] listening on http://${host}:${port}\n`);
  });

  const shutdown = (sig: string) => {
    process.stderr.write(`[llm-dummy] received ${sig}; shutting down\n`);
    server.close(() => process.exit(0));
    setTimeout(() => process.exit(1), 5_000).unref();
  };
  process.on('SIGINT', () => shutdown('SIGINT'));
  process.on('SIGTERM', () => shutdown('SIGTERM'));
}

main();
