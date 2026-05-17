#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
#
# run-llm-bench.sh — drive the file 19 LLM-pluggable corrector backend
# latency campaign.
#
# Steps:
#   1. Validate the requested backend's prerequisites:
#        dummy     — no prereq (transport-floor cell)
#        anthropic — ANTHROPIC_API_KEY must be set
#        openai    — OPENAI_API_KEY must be set
#        local     — SAMKHYA_LLM_LOCAL_URL must answer (default Ollama at 11434)
#   2. Launch the inference server in the background, capture PID.
#   3. Wait for /health to return ok:true (≤ 30s for dummy, ≤ 120s for live LLM).
#   4. Run the Rust client (llm_latency) against the wire contract.
#   5. Tear the server down on exit.
#
# Output:
#   bench-results/19_llm_corrector_raw.json   (Rust client output)
#   bench-results/wave5n_raw/server.log       (server stdout/stderr)
#   bench-results/wave5n_raw/run.summary.txt  (driver summary)
#
# Usage:
#   bash samkhya-gpudb/scripts/run-llm-bench.sh --backend dummy
#   bash samkhya-gpudb/scripts/run-llm-bench.sh --backend anthropic
#   bash samkhya-gpudb/scripts/run-llm-bench.sh --backend openai
#   bash samkhya-gpudb/scripts/run-llm-bench.sh --backend local
#
# Environment overrides:
#   SAMKHYA_LLM_PORT    (default 8766; distinct from TabPFN's 8765)
#   SAMKHYA_LLM_MODEL   (per-backend default; see llm_infer_server.py)
#   SAMKHYA_LLM_LOCAL_URL (default http://127.0.0.1:11434/api/generate)
#
# Naming: per the samkhya naming rule we frame this as
# "LLM-pluggable corrector backend", not an "AI" or "learned" feature.
#
# Sibling transport: a Node/TypeScript port of this server lives at
# samkhya-gpudb/scripts/llm_infer_server.ts (driver:
# samkhya-gpudb/scripts/run-llm-bench-ts.sh, default port 8767). Same
# wire contract; pick whichever runtime fits the host.

set -u

# Resolve repo root from this script's location so the script is safe
# to invoke from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
RAW_DIR="${ROOT}/bench-results/wave5n_raw"
mkdir -p "${RAW_DIR}"

BACKEND="dummy"
PORT="${SAMKHYA_LLM_PORT:-8766}"
PY="${ROOT}/samkhya-py/.venv-acceptance/bin/python3"
JSON_OUT="${ROOT}/bench-results/19_llm_corrector_raw.json"
TIMEOUT_MS=60000

while [ $# -gt 0 ]; do
    case "$1" in
        --backend)
            BACKEND="$2"; shift 2;;
        --port)
            PORT="$2"; shift 2;;
        --json-out)
            JSON_OUT="$2"; shift 2;;
        --timeout-ms)
            TIMEOUT_MS="$2"; shift 2;;
        -h|--help)
            grep -E '^# ' "$0" | sed 's/^# //'; exit 0;;
        *)
            echo "unknown flag: $1" >&2; exit 2;;
    esac
done

URL="http://127.0.0.1:${PORT}/infer"
HEALTH_URL="http://127.0.0.1:${PORT}/health"

banner() { printf '\n=== %s ===\n' "$1"; }

# ---------- preflight ----------

banner "preflight backend=${BACKEND}"
case "${BACKEND}" in
    dummy)
        echo "transport-floor probe — no API key required";;
    anthropic)
        if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
            echo "ERROR: backend=anthropic requires ANTHROPIC_API_KEY" >&2
            exit 3
        fi
        echo "ANTHROPIC_API_KEY present (length=${#ANTHROPIC_API_KEY})";;
    openai)
        if [ -z "${OPENAI_API_KEY:-}" ]; then
            echo "ERROR: backend=openai requires OPENAI_API_KEY" >&2
            exit 3
        fi
        echo "OPENAI_API_KEY present (length=${#OPENAI_API_KEY})";;
    local)
        LOCAL_URL="${SAMKHYA_LLM_LOCAL_URL:-http://127.0.0.1:11434/api/generate}"
        # Probe the local endpoint for liveness via a minimal GET to its base.
        BASE=$(echo "${LOCAL_URL}" | sed -E 's#^(https?://[^/]+).*#\1#')
        if ! curl -sf -m 2 "${BASE}" -o /dev/null 2>/dev/null \
             && ! curl -sf -m 2 "${BASE}/api/tags" -o /dev/null 2>/dev/null; then
            echo "WARNING: local backend at ${LOCAL_URL} does not respond; \
the server may still bind but every /infer call will return baseline."
        fi;;
    *)
        echo "ERROR: unknown backend=${BACKEND}" >&2; exit 2;;
esac

# ---------- server ----------

banner "starting LLM inference server (backend=${BACKEND} port=${PORT})"
if [ -x "${PY}" ]; then
    SERVER_PY="${PY}"
else
    SERVER_PY="$(command -v python3)"
    echo "(acceptance venv not found at ${PY}; falling back to ${SERVER_PY})"
fi

# For the dummy backend we can use either the FastAPI server (requires
# uvicorn/fastapi) or the stdlib-only standalone shim. Prefer the
# standalone shim for the dummy path so reviewers with no Python deps
# at all can still run the floor probe.
if [ "${BACKEND}" = "dummy" ]; then
    "${SERVER_PY}" "${SCRIPT_DIR}/llm_dummy_backend.py" \
        --host 127.0.0.1 --port "${PORT}" \
        > "${RAW_DIR}/server.log" 2>&1 &
    SERVER_PID=$!
else
    SAMKHYA_LLM_BACKEND="${BACKEND}" \
        SAMKHYA_LLM_PORT="${PORT}" \
        "${SERVER_PY}" "${SCRIPT_DIR}/llm_infer_server.py" \
        --backend "${BACKEND}" --port "${PORT}" --host 127.0.0.1 \
        > "${RAW_DIR}/server.log" 2>&1 &
    SERVER_PID=$!
fi
echo "server PID=${SERVER_PID}"

cleanup() {
    if kill -0 "${SERVER_PID}" 2>/dev/null; then
        echo "stopping server PID=${SERVER_PID}"
        kill "${SERVER_PID}" 2>/dev/null || true
        for _ in 1 2 3 4 5; do
            if ! kill -0 "${SERVER_PID}" 2>/dev/null; then break; fi
            sleep 1
        done
        if kill -0 "${SERVER_PID}" 2>/dev/null; then
            echo "force-killing server PID=${SERVER_PID}"
            kill -9 "${SERVER_PID}" 2>/dev/null || true
        fi
    fi
}
trap cleanup EXIT INT TERM

# Wait for /health. Dummy starts in <1s; live LLM backends boot in ~5s
# (anthropic SDK import + client construction).
if [ "${BACKEND}" = "dummy" ]; then
    HEALTH_TIMEOUT=30
else
    HEALTH_TIMEOUT=120
fi

banner "waiting for /health (timeout ${HEALTH_TIMEOUT}s)"
READY=""
START_TS=$(date +%s)
for _ in $(seq 1 "${HEALTH_TIMEOUT}"); do
    if curl -s -m 2 "${HEALTH_URL}" 2>/dev/null | grep -q '"ok": *true'; then
        READY="yes"; break
    fi
    sleep 1
done
WARM_TS=$(date +%s)
WARM_SECS=$((WARM_TS - START_TS))
if [ -z "${READY}" ]; then
    echo "TIMEOUT waiting for server /health"
    tail -50 "${RAW_DIR}/server.log"
    exit 2
fi
echo "server ready after ${WARM_SECS}s"
echo "health response:"
curl -s "${HEALTH_URL}"; echo

# ---------- client ----------

banner "running Rust client (cargo run --release -p samkhya-bench --bin llm_latency)"
cd "${ROOT}"
# llm_latency adapts batch sizes for the LLM cell; default 1,4,8,16,32
# (slim because LLM round-trips are ~10x slower than TabPFN).
cargo run --release -p samkhya-bench --bin llm_latency -- \
    --batch-sizes "1,4,8,16,32" \
    --seq-len 128 \
    --trials 30 \
    --warmup 5 \
    --url "${URL}" \
    --timeout-ms "${TIMEOUT_MS}" \
    --llm-backend "${BACKEND}" \
    --json-out "${JSON_OUT}"
CLIENT_RC=$?

banner "client exit=${CLIENT_RC}"
echo "raw JSON written to ${JSON_OUT}"
echo "server log: ${RAW_DIR}/server.log"

# Record warm time + final health snapshot.
{
    echo "backend=${BACKEND}"
    echo "server_warm_secs=${WARM_SECS}"
    echo "client_rc=${CLIENT_RC}"
    curl -s "${HEALTH_URL}" || true
    echo
} > "${RAW_DIR}/run.summary.txt"

exit "${CLIENT_RC}"
