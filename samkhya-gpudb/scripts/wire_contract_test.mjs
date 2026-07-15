// SPDX-License-Identifier: Apache-2.0
// End-to-end contract test shared by the standalone and primary TypeScript
// dummy transports. Uses only Node built-ins so CI never needs provider keys.

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import * as http from 'node:http';
import net from 'node:net';

const HOST = '127.0.0.1';

async function availablePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, HOST, resolve);
  });
  const address = server.address();
  assert(address && typeof address === 'object');
  const { port } = address;
  await new Promise((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
  return port;
}

async function request(baseUrl, path, init = {}) {
  const response = await fetch(`${baseUrl}${path}`, {
    ...init,
    signal: AbortSignal.timeout(2_000),
  });
  const raw = await response.text();
  const body = JSON.parse(raw);
  return { response, body, raw };
}

async function chunkedBody(baseUrl, byteLength) {
  const endpoint = new URL('/infer', baseUrl);
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        hostname: endpoint.hostname,
        port: endpoint.port,
        path: endpoint.pathname,
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'transfer-encoding': 'chunked',
        },
      },
      (res) => {
        let raw = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => { raw += chunk; });
        res.on('end', () => resolve({ status: res.statusCode, raw }));
      },
    );
    req.setTimeout(5_000, () => req.destroy(new Error('chunked request timed out')));
    req.on('error', reject);
    let remaining = byteLength;
    while (remaining > 0) {
      const size = Math.min(remaining, 1024 * 1024);
      req.write(Buffer.alloc(size, 0x20));
      remaining -= size;
    }
    req.end();
  });
}

async function waitUntilReady(baseUrl, child, diagnostics) {
  for (let attempt = 0; attempt < 80; attempt += 1) {
    if (child.exitCode !== null) {
      throw new Error(`server exited with ${child.exitCode}: ${diagnostics()}`);
    }
    try {
      const { response } = await request(baseUrl, '/health');
      if (response.status === 200) return;
    } catch {
      // The listener may not have bound yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`server did not become ready: ${diagnostics()}`);
}

async function stop(child) {
  if (child.exitCode !== null) return;
  child.kill('SIGTERM');
  await Promise.race([
    new Promise((resolve) => child.once('exit', resolve)),
    new Promise((_, reject) => setTimeout(() => reject(new Error('server did not stop')), 5_000)),
  ]);
}

async function exercise(label, script, extraArgs = []) {
  const port = await availablePort();
  const baseUrl = `http://${HOST}:${port}`;
  const child = spawn(process.execPath, [script, '--host', HOST, '--port', String(port), ...extraArgs], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '';
  child.stdout.on('data', (chunk) => { output += chunk.toString(); });
  child.stderr.on('data', (chunk) => { output += chunk.toString(); });

  try {
    await waitUntilReady(baseUrl, child, () => output);

    const health = await request(baseUrl, '/health');
    assert.equal(health.response.status, 200);
    assert.equal(health.body.ok, true);
    assert.equal(health.body.backend, 'dummy');

    const valid = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ features: [1, 2, 3, 4, 5, 6, 7], baseline_estimate: 42 }),
    });
    assert.equal(valid.response.status, 200);
    assert.equal(valid.body.estimate, 42);
    if (valid.body._latency_ms !== undefined) {
      assert.equal(typeof valid.body._latency_ms, 'number');
      assert(valid.body._latency_ms >= 0);
    }

    const wrongWidth = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ features: [1, 2], baseline_estimate: 42 }),
    });
    assert.equal(wrongWidth.response.status, 400);

    const nonNumericFeature = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ features: [1, 2, 3, 4, 5, 6, '7'], baseline_estimate: 42 }),
    });
    assert.equal(nonNumericFeature.response.status, 400);

    const largeInteger = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"features":[1,2,3,4,5,6,7],"baseline_estimate":9007199254740993}',
    });
    assert.equal(largeInteger.response.status, 200);
    assert.match(largeInteger.raw, /"estimate":9007199254740993(?:[,}])/);

    const maxInteger = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"features":[1,2,3,4,5,6,7],"baseline_estimate":18446744073709551615}',
    });
    assert.equal(maxInteger.response.status, 200);
    assert.match(maxInteger.raw, /"estimate":18446744073709551615(?:[,}])/);

    const aboveMaxInteger = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"features":[1,2,3,4,5,6,7],"baseline_estimate":18446744073709551616}',
    });
    assert.equal(aboveMaxInteger.response.status, 400);

    const invalidJson = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{',
    });
    assert.equal(invalidJson.response.status, 400);

    for (const nonObject of [null, [], 42]) {
      const response = await request(baseUrl, '/infer', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(nonObject),
      });
      assert.equal(response.response.status, 400);
    }

    const oversizedBatch = await request(baseUrl, '/infer', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ features: Array(7 * 1025).fill(0), baseline_estimate: 1 }),
    });
    assert.equal(oversizedBatch.response.status, 413);

    const boundaryBody = await chunkedBody(baseUrl, 8 * 1024 * 1024);
    assert.equal(boundaryBody.status, 400, boundaryBody.raw);

    const oversizedBody = await chunkedBody(baseUrl, 8 * 1024 * 1024 + 1);
    assert.equal(oversizedBody.status, 413, oversizedBody.raw);

    const missing = await request(baseUrl, '/missing');
    assert.equal(missing.response.status, 404);
  } finally {
    await stop(child);
  }

  process.stdout.write(`wire contract ok: ${label}\n`);
}

await exercise('standalone dummy', 'dist/llm_dummy_backend.js');
await exercise('primary dummy backend', 'dist/llm_infer_server.js', ['--backend', 'dummy']);
