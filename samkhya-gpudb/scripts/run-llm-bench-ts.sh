#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
#
# run-llm-bench-ts.sh — TypeScript-side driver for the file 19
# LLM-pluggable corrector latency campaign. Mirrors run-llm-bench.sh
# but launches the Node/TS server instead of the FastAPI server.
#
# Steps:
#   1. Validate the requested backend's prerequisites (same matrix as
#      the Python driver).
#   2. Launch the TS inference server in the background, capture PID.
#   3. Wait for /health to return ok:true (≤ 30s for dummy, ≤ 120s for
#      live LLM).
#   4. Run the Rust client (llm_latency) against the wire contract.
#   5. Tear the server down on exit.
#
# The Rust client doesn't know — and shouldn't need to know — whether
# the server on the other end is Python or TypeScript. The wire
# contract is identical; only the boot path differs.
#
# Output:
#   bench-results/19_llm_corrector_ts_raw.json   (Rust client output)
#   bench-results/wave5n_ts_raw/server.log       (server stdout/stderr)
#   bench-results/wave5n_ts_raw/run.summary.txt  (driver summary)
#
# Usage:
#   bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend dummy
#   bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend anthropic
#   bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend openai
#   bash samkhya-gpudb/scripts/run-llm-bench-ts.sh --backend local
#
# Environment overrides:
#   SAMKHYA_LLM_PORT      (default 8767; distinct from Python's 8766
#                          so both can run side-by-side)
#   SAMKHYA_LLM_MODEL     (per-backend default; see llm_infer_server.ts)
#   SAMKHYA_LLM_LOCAL_URL (default http://127.0.0.1:11434/api/generate)
#   SAMKHYA_NODE          (override node binary; default $(command -v node))
#   SAMKHYA_USE_TSX       (default 1 — run TS directly via tsx; set 0 to
#                          require a `npm run build` first and use dist/)

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
RAW_DIR="${ROOT}/bench-results/wave5n_ts_raw"
mkdir -p "${RAW_DIR}"

BACKEND="dummy"
PORT="${SAMKHYA_LLM_PORT:-8767}"
NODE_BIN="${SAMKHYA_NODE:-$(command -v node || true)}"
USE_TSX="${SAMKHYA_USE_TSX:-1}"
JSON_OUT="${ROOT}/bench-results/19_llm_corrector_ts_raw.json"
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

if [ -z "${NODE_BIN}" ] || [ ! -x "${NODE_BIN}" ]; then
    echo "ERROR: node not found on PATH; install Node 18+ or set SAMKHYA_NODE" >&2
    exit 4
fi
NODE_VER=$("${NODE_BIN}" --version)
echo "node binary: ${NODE_BIN} (${NODE_VER})"

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

cd "${SCRIPT_DIR}"

SERVER_ENTRY=""
LAUNCH_PREFIX=()

if [ "${USE_TSX}" = "1" ]; then
    # Run TS sources directly via tsx (zero-build dev path).
    if [ ! -d "${SCRIPT_DIR}/node_modules" ]; then
        echo "node_modules not present in ${SCRIPT_DIR}; running 'npm install'..."
        ( cd "${SCRIPT_DIR}" && npm install --silent ) || {
            echo "ERROR: npm install failed; pre-build with 'npm run build' or set SAMKHYA_USE_TSX=0" >&2
            exit 5
        }
    fi
    if [ "${BACKEND}" = "dummy" ]; then
        SERVER_ENTRY="${SCRIPT_DIR}/llm_dummy_backend.ts"
    else
        SERVER_ENTRY="${SCRIPT_DIR}/llm_infer_server.ts"
    fi
    LAUNCH_PREFIX=("${SCRIPT_DIR}/node_modules/.bin/tsx")
else
    # Require pre-built JS in dist/.
    DIST_DIR="${SCRIPT_DIR}/dist"
    if [ "${BACKEND}" = "dummy" ]; then
        SERVER_ENTRY="${DIST_DIR}/llm_dummy_backend.js"
    else
        SERVER_ENTRY="${DIST_DIR}/llm_infer_server.js"
    fi
    if [ ! -f "${SERVER_ENTRY}" ]; then
        echo "ERROR: ${SERVER_ENTRY} missing; run 'npm run build' first or set SAMKHYA_USE_TSX=1" >&2
        exit 5
    fi
    LAUNCH_PREFIX=("${NODE_BIN}")
fi

banner "starting TS LLM inference server (backend=${BACKEND} port=${PORT})"
SAMKHYA_LLM_BACKEND="${BACKEND}" \
    SAMKHYA_LLM_PORT="${PORT}" \
    "${LAUNCH_PREFIX[@]}" "${SERVER_ENTRY}" \
    --backend "${BACKEND}" --port "${PORT}" --host 127.0.0.1 \
    > "${RAW_DIR}/server.log" 2>&1 &
SERVER_PID=$!
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

{
    echo "backend=${BACKEND}"
    echo "transport=typescript+node"
    echo "node_version=${NODE_VER}"
    echo "use_tsx=${USE_TSX}"
    echo "server_warm_secs=${WARM_SECS}"
    echo "client_rc=${CLIENT_RC}"
    curl -s "${HEALTH_URL}" || true
    echo
} > "${RAW_DIR}/run.summary.txt"

exit "${CLIENT_RC}"
