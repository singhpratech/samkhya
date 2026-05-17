#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
"""
TabPFN inference server matching the wire contract from
``samkhya-core::residual::tabpfn`` (see ``samkhya-core/src/residual.rs``
lines 575-595).

Wire contract
-------------
POST /infer   Content-Type: application/json
{
    "features":          [<f64>, ...],   # FEATURE_LEN * B values
    "baseline_estimate": <u64>
}
-> 200 OK
{ "estimate": <u64> }

GET /health
-> 200 OK {"ok": true, "device": ..., "warm_us": ...}

The corrector layer expects ``Ok(None)`` on any non-2xx, so we are free
to 4xx malformed requests; the Rust client absorbs them.

Per-row inference is performed by treating each 7-dim feature vector as
a test instance: at startup we synthesize a small in-context support set
(8-row "soft prior") from a uniform random regression over the feature
columns. The support set is held constant for the lifetime of the
server so the cost we measure is *inference-only*, not fit + inference.
This matches TabPFN's intended deployment shape (Hollmann et al.,
"TabPFN: A Transformer That Solves Small Tabular Classification
Problems in a Second", ICLR 2023; the 2.5 line extends this to
regression via the same in-context posterior shape).

The model classifier prediction is mapped to a cardinality estimate by
treating the baseline_estimate as a pivot and emitting
``baseline_estimate * exp(prediction)`` (capped at u64::MAX-1). For the
latency-focused harness this is sufficient: the cost we are measuring
is the forward pass, not the calibration of the predicted ratio.

Environment
-----------
TABPFN_HOST    (default 127.0.0.1)
TABPFN_PORT    (default 8765)
TABPFN_DEVICE  (default cuda; falls back to cpu if cuda unavailable)
TABPFN_SUPPORT (default 8 — rows in the in-context support set)

Citations
---------
Hollmann, N., Müller, S., Eggensperger, K., & Hutter, F. (2023).
    "TabPFN: A Transformer That Solves Small Tabular Classification
    Problems in a Second." ICLR 2023.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from typing import Any, List

import numpy as np
import torch
import uvicorn
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse

FEATURE_LEN = 7
U64_MAX = (1 << 64) - 1

app = FastAPI(title="samkhya tabpfn_http inference server")

# Module-level state. Populated in ``load_model`` at startup.
_state: dict[str, Any] = {
    "model": None,
    "device": "cpu",
    "support_X": None,
    "support_y": None,
    "warm_us": None,
    "support_rows": 0,
    "started_at_ns": 0,
}


def _build_support_set(n_rows: int, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """Synthesize a deterministic in-context support set.

    The support set is held constant across all requests for this server
    process. It is intentionally tiny (8 rows by default) so the
    latency we report reflects TabPFN's *in-context inference* cost on
    a realistic small-support regime, matching the regime described in
    the TabPFN-2.5 paper (Hollmann et al., 2023) where in-context
    learning is the load-bearing mechanism.
    """
    X = rng.standard_normal((n_rows, FEATURE_LEN)).astype(np.float32)
    # log-cardinality target in roughly [-2, 2] — three modes mimicking the
    # multi-modal q-error distribution we see on correlated joins.
    centres = np.array([-1.5, 0.2, 1.4], dtype=np.float32)
    mode_pick = rng.integers(0, len(centres), size=n_rows)
    y = centres[mode_pick] + rng.standard_normal(n_rows).astype(np.float32) * 0.3
    return X, y


def load_model(device: str, support_rows: int) -> None:
    """Load TabPFN once, push to ``device``, warm it with a dummy call.

    We use ``fit_mode='fit_with_cache'`` and ``n_estimators=1`` so the
    in-context trainset representation is cached after ``fit()`` and the
    per-request cost is one forward pass instead of an ensemble. This is
    the lowest-latency single-prompt path TabPFN exposes; it matches the
    "online inference" deployment regime described in
    Hollmann et al. (ICLR 2023), §3.2.
    """
    print(f"[server] importing tabpfn", file=sys.stderr, flush=True)
    from tabpfn import TabPFNRegressor
    from tabpfn.model_loading import ModelVersion

    rng = np.random.default_rng(0xDEADBEEFCAFEBABE & ((1 << 64) - 1))
    support_X, support_y = _build_support_set(support_rows, rng)

    print(
        f"[server] constructing TabPFNRegressor v2.5 (device={device}, "
        f"support_rows={support_rows}, fit_mode=fit_with_cache, n_estimators=1)",
        file=sys.stderr,
        flush=True,
    )
    reg = TabPFNRegressor.create_default_for_version(
        ModelVersion.V2_5,
        device=device,
        ignore_pretraining_limits=True,
        fit_mode="fit_with_cache",
        n_estimators=1,
    )
    reg.fit(support_X, support_y)

    # Warm with several batch=1 forward passes to populate any lazy
    # kernel JIT / autotune caches.
    probe_X = rng.standard_normal((1, FEATURE_LEN)).astype(np.float32)
    for _ in range(5):
        _ = reg.predict(probe_X)
    if device == "cuda" and torch.cuda.is_available():
        torch.cuda.synchronize()
    t0 = time.perf_counter_ns()
    _ = reg.predict(probe_X)
    if device == "cuda" and torch.cuda.is_available():
        torch.cuda.synchronize()
    warm_us = (time.perf_counter_ns() - t0) / 1_000.0

    _state["model"] = reg
    _state["device"] = device
    _state["support_X"] = support_X
    _state["support_y"] = support_y
    _state["support_rows"] = support_rows
    _state["warm_us"] = warm_us
    _state["started_at_ns"] = time.perf_counter_ns()

    if device == "cuda" and torch.cuda.is_available():
        free_mib = torch.cuda.mem_get_info()[0] / (1 << 20)
        total_mib = torch.cuda.mem_get_info()[1] / (1 << 20)
        print(
            f"[server] model loaded; device={device}; "
            f"GPU free={free_mib:.0f}/{total_mib:.0f} MiB; "
            f"warm_pass={warm_us:.1f} us",
            file=sys.stderr,
            flush=True,
        )
    else:
        print(
            f"[server] model loaded; device={device}; warm_pass={warm_us:.1f} us",
            file=sys.stderr,
            flush=True,
        )


@app.get("/health")
def health() -> dict[str, Any]:
    if _state["model"] is None:
        return JSONResponse({"ok": False, "reason": "model not loaded"}, status_code=503)
    return {
        "ok": True,
        "device": _state["device"],
        "support_rows": _state["support_rows"],
        "warm_us": _state["warm_us"],
    }


@app.post("/infer")
async def infer(request: Request) -> dict[str, Any]:
    body_bytes = await request.body()
    try:
        body = json.loads(body_bytes)
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"invalid json: {exc!s}")

    features = body.get("features")
    baseline = body.get("baseline_estimate")
    if not isinstance(features, list) or not features:
        raise HTTPException(status_code=400, detail="missing or empty 'features'")
    if not isinstance(baseline, int) or baseline < 0:
        raise HTTPException(status_code=400, detail="missing or non-u64 'baseline_estimate'")

    n = len(features)
    if n % FEATURE_LEN != 0:
        raise HTTPException(
            status_code=400,
            detail=f"features length {n} not a multiple of {FEATURE_LEN}",
        )
    batch = n // FEATURE_LEN

    try:
        X = np.asarray(features, dtype=np.float32).reshape(batch, FEATURE_LEN)
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"feature reshape failed: {exc!s}")

    model = _state["model"]
    if model is None:
        raise HTTPException(status_code=503, detail="model not loaded")

    # Forward pass.
    preds = model.predict(X)  # ndarray shape (batch,)
    # Reduce across the batch to a single scalar; the wire returns one u64
    # per request. We use the *first* prediction as the headline estimate
    # so the per-request cost grows linearly in batch (which is what we
    # want to measure), and we *touch* every prediction (sum) so the
    # forward pass isn't elided.
    sentinel = float(np.sum(preds))  # forces materialization
    headline = float(preds[0])
    # Map the predicted log-ratio to an absolute estimate using the
    # baseline as pivot. Saturate to u64-1 on overflow.
    if not np.isfinite(headline):
        return {"estimate": int(baseline)}
    scale = float(np.exp(headline))
    raw = max(0.0, float(baseline) * scale)
    if raw >= float(U64_MAX):
        estimate = U64_MAX - 1
    else:
        estimate = int(raw)

    # Touch sentinel to ensure preds isn't optimized out — embed in a
    # nominal field so the client sees identical wire shape.
    return {"estimate": estimate, "_pred_sentinel": sentinel}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=os.environ.get("TABPFN_HOST", "127.0.0.1"))
    parser.add_argument(
        "--port", type=int, default=int(os.environ.get("TABPFN_PORT", "8765"))
    )
    parser.add_argument(
        "--device", default=os.environ.get("TABPFN_DEVICE", "cuda")
    )
    parser.add_argument(
        "--support-rows",
        type=int,
        default=int(os.environ.get("TABPFN_SUPPORT", "8")),
    )
    args = parser.parse_args()

    device = args.device
    if device == "cuda" and not torch.cuda.is_available():
        print("[server] cuda requested but not available; falling back to cpu",
              file=sys.stderr, flush=True)
        device = "cpu"

    load_model(device, args.support_rows)
    uvicorn.run(app, host=args.host, port=args.port, log_level="warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
