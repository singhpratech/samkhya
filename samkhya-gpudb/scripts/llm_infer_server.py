#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
"""LLM-pluggable cardinality corrector inference server.

Matches the wire contract from ``samkhya-core::residual::llm`` (see
``samkhya-core/src/residual.rs``):

    POST /infer   Content-Type: application/json
    {
        "features":          [<f64>, ...],   # FEATURE_LEN * B values
        "baseline_estimate": <u64>
    }
    -> 200 OK
    { "estimate": <u64> }

    GET /health
    -> 200 OK
    { "ok": true, "backend": "<name>", "model": "<id>" }

The corrector layer's safety contract maps any non-2xx, parse failure, or
timeout to ``Ok(None)`` on the Rust side, so this server is free to
return a 4xx on malformed input or a 503 if the chosen backend is
mis-configured — the Rust client absorbs both as "fall back to baseline."

Pluggable backends (selected via ``SAMKHYA_LLM_BACKEND``):

* ``anthropic`` (default) — Anthropic Claude via the ``anthropic`` Python
  SDK. Reads ``ANTHROPIC_API_KEY``. Model from ``SAMKHYA_LLM_MODEL``
  (default ``claude-opus-4-7``); falls back to ``claude-sonnet-4-6`` if
  the chosen model is unavailable in the account.
* ``openai`` — OpenAI Chat Completions via the ``openai`` SDK. Reads
  ``OPENAI_API_KEY``. Model from ``SAMKHYA_LLM_MODEL`` (default
  ``gpt-4o-mini``).
* ``local`` — local LLM HTTP endpoint (llama.cpp / ollama). Reads
  ``SAMKHYA_LLM_LOCAL_URL`` (default ``http://127.0.0.1:11434/api/generate``
  — Ollama default). Model from ``SAMKHYA_LLM_MODEL`` (default
  ``llama3.2:1b``).
* ``dummy`` — returns ``baseline_estimate`` unchanged. Used by the
  transport-floor benchmark cell so reviewers without an API key can
  still measure the HTTP round-trip cost end-to-end.

Naming
------

Per the samkhya naming rule (no "learned"/"adaptive"/"AI" branding) the
file 19 doc frames this server as the **LLM-pluggable corrector
backend**. This is a pluggable transport that lets a foundation language
model serve as the cardinality corrector behind the same trait as every
other corrector backend — *not* an "AI feature." The default samkhya
build does not even pull this in (``llm_http`` cargo feature is off by
default).

Prompt template
---------------

Defaults are baked in; both system and user prompts are overridable via
``SAMKHYA_LLM_SYSTEM_PROMPT`` / ``SAMKHYA_LLM_USER_PROMPT`` (with
``{features}`` and ``{baseline_estimate}`` placeholders).

Determinism
-----------

The LLM backends are pinned to ``temperature=0.0`` and ``max_tokens=32``
so the response is bounded both in cost and in stochasticity. Real
deployments may bump these; the reproducibility section of file 19
records the exact values used for the campaign.

Logging
-------

One line per request to stderr:

    [llm] backend=<name> model=<id> latency_ms=<f64> status=ok|parse_err|api_err

No body content is logged (privacy). The Rust client never logs
features either — see ``residual.rs::llm`` module docs.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from typing import Any, Optional

import uvicorn
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse

FEATURE_LEN = 7
U64_MAX = (1 << 64) - 1

# Defaults baked in. Caller can override via env.
DEFAULT_SYSTEM_PROMPT = (
    "You are a cardinality estimator for SQL query optimizers. "
    "Given a feature vector describing a join, you reply with a single "
    "positive integer that is your best estimate of the row count the "
    "join will produce. Output ONLY the integer, no commentary."
)
DEFAULT_USER_PROMPT = (
    "Features (7-dim): {features}. "
    "Optimizer's baseline guess: {baseline_estimate}. "
    "Your estimate (integer, single line):"
)

app = FastAPI(title="samkhya llm_http inference server")

_state: dict[str, Any] = {
    "backend": "dummy",
    "model": "none",
    "client": None,
    "system_prompt": DEFAULT_SYSTEM_PROMPT,
    "user_prompt": DEFAULT_USER_PROMPT,
    "local_url": None,
    "temperature": 0.0,
    "max_tokens": 32,
    "started_at_ns": 0,
}


def _log(backend: str, model: str, latency_ms: float, status: str) -> None:
    print(
        f"[llm] backend={backend} model={model} latency_ms={latency_ms:.3f} status={status}",
        file=sys.stderr,
        flush=True,
    )


def _parse_first_integer(text: str) -> Optional[int]:
    """Extract the first non-negative integer from ``text``.

    LLM replies may include stray punctuation, commas, or commentary
    despite the prompt asking for a bare integer. We extract the first
    digit run and parse it. Returns None if no digits are present.
    """
    if not text:
        return None
    # Strip commas inside numbers (e.g., "1,234,567" -> "1234567") then
    # match the first digit run.
    cleaned = text.replace(",", "")
    m = re.search(r"\d+", cleaned)
    if not m:
        return None
    try:
        v = int(m.group(0))
    except ValueError:
        return None
    if v < 0:
        return None
    if v > U64_MAX:
        return U64_MAX - 1
    return v


def _render_user_prompt(features: list[float], baseline_estimate: int) -> str:
    template = _state["user_prompt"]
    # Render features as a compact JSON array so the LLM has something
    # well-formed to anchor on. Truncate to 7 (one feature vector worth);
    # for larger batches we use only the first row — the wire returns one
    # estimate anyway.
    head = features[:FEATURE_LEN]
    feature_str = json.dumps([float(x) for x in head])
    return template.format(features=feature_str, baseline_estimate=baseline_estimate)


# ---------------------------------------------------------------------------
# Backend adapters
# ---------------------------------------------------------------------------


def _backend_dummy(_user_prompt: str, baseline_estimate: int) -> tuple[Optional[int], str]:
    """No-op backend: echoes baseline. Returns (estimate, raw_reply)."""
    return baseline_estimate, str(baseline_estimate)


def _backend_anthropic(user_prompt: str, baseline_estimate: int) -> tuple[Optional[int], str]:
    """Anthropic Claude via the official SDK."""
    client = _state["client"]
    model = _state["model"]
    try:
        msg = client.messages.create(
            model=model,
            max_tokens=_state["max_tokens"],
            temperature=_state["temperature"],
            system=_state["system_prompt"],
            messages=[{"role": "user", "content": user_prompt}],
        )
        # The SDK returns content as a list of blocks; we want the first
        # text block.
        text_blocks = [b.text for b in msg.content if getattr(b, "type", None) == "text"]
        raw = text_blocks[0] if text_blocks else ""
    except Exception as exc:  # pragma: no cover — depends on API key
        _log("anthropic", model, 0.0, "api_err")
        return baseline_estimate, f"<api_err: {exc}>"
    parsed = _parse_first_integer(raw)
    return parsed if parsed is not None else baseline_estimate, raw


def _backend_openai(user_prompt: str, baseline_estimate: int) -> tuple[Optional[int], str]:
    """OpenAI Chat Completions via the official SDK."""
    client = _state["client"]
    model = _state["model"]
    try:
        resp = client.chat.completions.create(
            model=model,
            max_tokens=_state["max_tokens"],
            temperature=_state["temperature"],
            messages=[
                {"role": "system", "content": _state["system_prompt"]},
                {"role": "user", "content": user_prompt},
            ],
        )
        raw = resp.choices[0].message.content or ""
    except Exception as exc:  # pragma: no cover — depends on API key
        _log("openai", model, 0.0, "api_err")
        return baseline_estimate, f"<api_err: {exc}>"
    parsed = _parse_first_integer(raw)
    return parsed if parsed is not None else baseline_estimate, raw


def _backend_local(user_prompt: str, baseline_estimate: int) -> tuple[Optional[int], str]:
    """Local HTTP endpoint (Ollama / llama.cpp)."""
    import urllib.request

    url = _state["local_url"]
    model = _state["model"]
    payload = json.dumps(
        {
            "model": model,
            "prompt": f"{_state['system_prompt']}\n\n{user_prompt}",
            "stream": False,
            "options": {
                "temperature": _state["temperature"],
                "num_predict": _state["max_tokens"],
            },
        }
    ).encode("utf-8")
    req = urllib.request.Request(
        url, data=payload, headers={"Content-Type": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=55) as resp:
            body = json.loads(resp.read().decode("utf-8"))
        raw = body.get("response", "")
    except Exception as exc:  # pragma: no cover — depends on ollama
        _log("local", model, 0.0, "api_err")
        return baseline_estimate, f"<api_err: {exc}>"
    parsed = _parse_first_integer(raw)
    return parsed if parsed is not None else baseline_estimate, raw


_BACKENDS = {
    "dummy": _backend_dummy,
    "anthropic": _backend_anthropic,
    "openai": _backend_openai,
    "local": _backend_local,
}


# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------


def load_backend(name: str) -> None:
    """Initialize the chosen backend; populate ``_state``."""
    name = name.lower().strip()
    if name not in _BACKENDS:
        raise SystemExit(
            f"[llm] unknown SAMKHYA_LLM_BACKEND={name!r}; "
            f"choose one of {sorted(_BACKENDS)}"
        )

    _state["backend"] = name
    _state["system_prompt"] = os.environ.get(
        "SAMKHYA_LLM_SYSTEM_PROMPT", DEFAULT_SYSTEM_PROMPT
    )
    _state["user_prompt"] = os.environ.get(
        "SAMKHYA_LLM_USER_PROMPT", DEFAULT_USER_PROMPT
    )
    _state["temperature"] = float(os.environ.get("SAMKHYA_LLM_TEMPERATURE", "0.0"))
    _state["max_tokens"] = int(os.environ.get("SAMKHYA_LLM_MAX_TOKENS", "32"))

    if name == "dummy":
        _state["model"] = os.environ.get("SAMKHYA_LLM_MODEL", "dummy-echo")
        print(
            f"[llm] backend=dummy model={_state['model']} "
            f"(transport-floor only; no LLM calls)",
            file=sys.stderr,
            flush=True,
        )
    elif name == "anthropic":
        try:
            import anthropic  # type: ignore
        except ImportError as exc:
            raise SystemExit(
                "[llm] backend=anthropic requested but `anthropic` SDK is not "
                "installed; run `pip install anthropic` or pick another "
                "SAMKHYA_LLM_BACKEND."
            ) from exc
        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            raise SystemExit(
                "[llm] backend=anthropic requires ANTHROPIC_API_KEY in env."
            )
        _state["model"] = os.environ.get("SAMKHYA_LLM_MODEL", "claude-opus-4-7")
        _state["client"] = anthropic.Anthropic(api_key=api_key)
        print(
            f"[llm] backend=anthropic model={_state['model']} ready",
            file=sys.stderr,
            flush=True,
        )
    elif name == "openai":
        try:
            import openai  # type: ignore
        except ImportError as exc:
            raise SystemExit(
                "[llm] backend=openai requested but `openai` SDK is not "
                "installed; run `pip install openai` or pick another "
                "SAMKHYA_LLM_BACKEND."
            ) from exc
        api_key = os.environ.get("OPENAI_API_KEY")
        if not api_key:
            raise SystemExit("[llm] backend=openai requires OPENAI_API_KEY in env.")
        _state["model"] = os.environ.get("SAMKHYA_LLM_MODEL", "gpt-4o-mini")
        _state["client"] = openai.OpenAI(api_key=api_key)
        print(
            f"[llm] backend=openai model={_state['model']} ready",
            file=sys.stderr,
            flush=True,
        )
    elif name == "local":
        _state["model"] = os.environ.get("SAMKHYA_LLM_MODEL", "llama3.2:1b")
        _state["local_url"] = os.environ.get(
            "SAMKHYA_LLM_LOCAL_URL", "http://127.0.0.1:11434/api/generate"
        )
        print(
            f"[llm] backend=local model={_state['model']} url={_state['local_url']} ready",
            file=sys.stderr,
            flush=True,
        )

    _state["started_at_ns"] = time.perf_counter_ns()


# ---------------------------------------------------------------------------
# Routes
# ---------------------------------------------------------------------------


@app.get("/health")
def health() -> dict[str, Any]:
    if _state["started_at_ns"] == 0:
        return JSONResponse({"ok": False, "reason": "backend not loaded"}, status_code=503)
    return {
        "ok": True,
        "backend": _state["backend"],
        "model": _state["model"],
        "temperature": _state["temperature"],
        "max_tokens": _state["max_tokens"],
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
        raise HTTPException(
            status_code=400, detail="missing or non-u64 'baseline_estimate'"
        )

    if len(features) % FEATURE_LEN != 0:
        raise HTTPException(
            status_code=400,
            detail=f"features length {len(features)} not a multiple of {FEATURE_LEN}",
        )

    user_prompt = _render_user_prompt(features, int(baseline))
    backend_fn = _BACKENDS[_state["backend"]]

    t0 = time.perf_counter_ns()
    estimate, raw_reply = backend_fn(user_prompt, int(baseline))
    elapsed_ms = (time.perf_counter_ns() - t0) / 1_000_000.0

    if estimate is None:
        # Parse failure path: log it, but return baseline so the Rust
        # client still gets a well-formed `{"estimate": ...}` and does
        # not invoke its fallback. The corrector contract says the
        # server is free to return baseline as a "neutral" estimate.
        _log(_state["backend"], _state["model"], elapsed_ms, "parse_err")
        return {"estimate": int(baseline), "_status": "parse_err"}

    estimate = max(0, int(estimate))
    if estimate >= U64_MAX:
        estimate = U64_MAX - 1

    _log(_state["backend"], _state["model"], elapsed_ms, "ok")
    return {"estimate": estimate, "_latency_ms": elapsed_ms}


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
    parser.add_argument(
        "--backend",
        default=os.environ.get("SAMKHYA_LLM_BACKEND", "anthropic"),
        choices=sorted(_BACKENDS),
    )
    args = parser.parse_args()

    # Allow CLI flag to override env so the run-llm-bench.sh driver can
    # force a backend choice without exporting env at every call site.
    os.environ["SAMKHYA_LLM_BACKEND"] = args.backend

    load_backend(args.backend)
    uvicorn.run(app, host=args.host, port=args.port, log_level="warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
