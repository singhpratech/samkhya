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

async function readBody(req: http.IncomingMessage): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks)));
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
  const bodyBytes = await readBody(req);
  let body: any;
  try {
    body = JSON.parse(bodyBytes.toString('utf8'));
  } catch (exc) {
    sendJson(res, 400, { detail: `invalid json: ${String(exc)}` });
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

function main(): void {
  const { host, port } = parseArgs(process.argv.slice(2));
  state.host = host;
  state.port = port;
  state.startedAtNs = process.hrtime.bigint();

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
