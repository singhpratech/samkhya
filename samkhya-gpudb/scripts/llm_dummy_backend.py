#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
"""Standalone dummy LLM backend for transport-floor measurement.

Used by the file 19 transport-floor cell so the LLM-pluggable corrector
backend can be exercised end-to-end without an API key. The server in
``llm_infer_server.py`` already supports a ``dummy`` backend internally;
this file is a *standalone* HTTP shim that speaks the same wire contract
without depending on FastAPI, useful when a reviewer wants to reproduce
the floor with only the stdlib.

Wire contract (matches ``samkhya-core::residual::llm``)::

    POST /infer  Content-Type: application/json
    { "features": [...], "baseline_estimate": <u64> }
    -> 200 OK
    { "estimate": <u64> }

Strategy
--------

The dummy returns the baseline estimate unchanged. This is the *neutral*
estimate from the cardinality-corrector contract's perspective — no
information is added by the corrector, so the engine behaves as if no
corrector were installed. The cost we measure with the dummy is the
*HTTP transport floor* of the LLM backend: serialize features → POST →
deserialize response. Real LLM backends stack on top of this floor.

Run
---

    python3 samkhya-gpudb/scripts/llm_dummy_backend.py --port 8766

then drive it with the same Rust client (``llm_latency`` binary) that
hits the production server.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

FEATURE_LEN = 7
U64_MAX = (1 << 64) - 1

# SECURITY-REVIEW-2026-05-17.md (H4 + M1).
BODY_MAX_BYTES = 8 * 1024 * 1024
MAX_INFER_BATCHES = 1024


class Handler(BaseHTTPRequestHandler):
    """One-shot HTTP handler — minimal correctness, maximal stability."""

    server_version = "samkhya-llm-dummy/1.0"

    def do_GET(self) -> None:  # noqa: N802 — BaseHTTPRequestHandler API
        if self.path != "/health":
            self.send_error(404, "not found")
            return
        body = json.dumps(
            {"ok": True, "backend": "dummy", "model": "dummy-echo"}
        ).encode()
        self._respond(200, body)

    def do_POST(self) -> None:  # noqa: N802 — BaseHTTPRequestHandler API
        if self.path != "/infer":
            self.send_error(404, "not found")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self.send_error(400, "invalid Content-Length")
            return
        if length > BODY_MAX_BYTES:
            self.send_error(413, f"body {length} exceeds {BODY_MAX_BYTES}")
            return
        try:
            raw = self.rfile.read(length)
            body = json.loads(raw)
        except Exception as exc:
            # See SECURITY-REVIEW-2026-05-17.md (C3): log full detail to
            # stderr but echo only the exception class on the wire.
            print(f"[llm-dummy] /infer parse err: {exc!r}", file=sys.stderr, flush=True)
            self.send_error(400, f"bad json: {type(exc).__name__}")
            return

        features = body.get("features")
        baseline = body.get("baseline_estimate")
        if not isinstance(features, list) or not features:
            self.send_error(400, "missing features")
            return
        if not isinstance(baseline, int) or baseline < 0:
            self.send_error(400, "missing baseline_estimate")
            return
        if len(features) % FEATURE_LEN != 0:
            self.send_error(
                400, f"features length {len(features)} not a multiple of {FEATURE_LEN}"
            )
            return
        batches = len(features) // FEATURE_LEN
        if batches > MAX_INFER_BATCHES:
            self.send_error(413, f"features batch count {batches} exceeds {MAX_INFER_BATCHES}")
            return

        estimate = max(0, min(int(baseline), U64_MAX - 1))
        reply = json.dumps({"estimate": estimate}).encode()
        self._respond(200, reply)

    def log_message(self, *_args: object, **_kwargs: object) -> None:
        # Suppress per-request access logs to stdout — we want clean
        # bench output. The Rust client times its own round-trip.
        pass

    def _respond(self, code: int, body: bytes) -> None:
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        try:
            self.wfile.write(body)
            self.wfile.flush()
        except BrokenPipeError:
            pass


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--host", default=os.environ.get("SAMKHYA_LLM_HOST", "127.0.0.1")
    )
    parser.add_argument(
        "--port",
        type=int,
        default=int(os.environ.get("SAMKHYA_LLM_PORT", "8766")),
    )
    args = parser.parse_args()
    # SECURITY-REVIEW-2026-05-17.md (H1): the dummy backend is harmless
    # (it doesn't reach upstream LLMs) but the warning still helps the
    # operator notice that an unauthenticated server is exposed.
    if args.host not in ("127.0.0.1", "::1", "localhost"):
        banner = "=" * 70
        print(
            f"\n{banner}\n"
            f"[WARN] samkhya LLM dummy server bound to non-loopback ({args.host}).\n"
            f"[WARN] This server has NO authentication. Ensure network isolation\n"
            f"[WARN] before exposing the address.\n"
            f"{banner}\n",
            file=sys.stderr,
            flush=True,
        )
    print(
        f"[llm-dummy] listening on http://{args.host}:{args.port} "
        f"(GET /health, POST /infer)",
        file=sys.stderr,
        flush=True,
    )
    HTTPServer((args.host, args.port), Handler).serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
