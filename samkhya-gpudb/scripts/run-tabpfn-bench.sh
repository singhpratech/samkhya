#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
#
# run-tabpfn-bench.sh — drive the WAVE-5L latency campaign for file 14
# (TabPFN inference latency on RTX 4090).
#
# Steps:
#   1. Probe the local CUDA/NVIDIA environment (probe-cuda.sh).
#   2. Launch tabpfn_infer_server.py in the background, capture PID.
#   3. Wait for /health to return ok:true (≤ 120s).
#   4. Run the Rust client (tabpfn_latency) against the wire contract.
#   5. Tear the server down on exit.
#
# Output:
#   bench-results/14_tabpfn_raw.json         (Rust client output)
#   bench-results/wave5l_raw/server.log      (server stdout/stderr)
#   bench-results/wave5l_raw/probe.log       (CUDA probe output)
#
# Environment overrides:
#   TABPFN_PORT     (default 8765)
#   TABPFN_DEVICE   (default cuda)

set -u

# Resolve repo root from this script's location so the script is
# safe to invoke from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
RAW_DIR="${ROOT}/bench-results/wave5l_raw"
mkdir -p "${RAW_DIR}"

PORT="${TABPFN_PORT:-8765}"
DEVICE="${TABPFN_DEVICE:-cuda}"
URL="http://127.0.0.1:${PORT}/infer"
HEALTH_URL="http://127.0.0.1:${PORT}/health"
PY="${ROOT}/samkhya-py/.venv-acceptance/bin/python3"

banner() { printf '\n=== %s ===\n' "$1"; }

banner "probe CUDA"
bash "${SCRIPT_DIR}/probe-cuda.sh" > "${RAW_DIR}/probe.log" 2>&1 || {
    echo "(probe-cuda.sh failed; continuing — probe output captured)"
}
tail -20 "${RAW_DIR}/probe.log" || true

banner "starting TabPFN inference server (port=${PORT} device=${DEVICE})"
TABPFN_PORT="${PORT}" TABPFN_DEVICE="${DEVICE}" \
    "${PY}" "${SCRIPT_DIR}/tabpfn_infer_server.py" \
    > "${RAW_DIR}/server.log" 2>&1 &
SERVER_PID=$!
echo "server PID=${SERVER_PID}"

# Tear down server on every exit path.
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

banner "waiting for /health (timeout 120s)"
READY=""
START_TS=$(date +%s)
for _ in $(seq 1 120); do
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

banner "running Rust client (cargo run --release -p samkhya-bench --bin tabpfn_latency)"
cd "${ROOT}"
cargo run --release -p samkhya-bench --bin tabpfn_latency -- \
    --batch-sizes "1,4,8,16,32,64,128" \
    --seq-len 128 \
    --trials 30 \
    --warmup 5 \
    --url "${URL}" \
    --json-out "${ROOT}/bench-results/14_tabpfn_raw.json"
CLIENT_RC=$?

banner "client exit=${CLIENT_RC}"
echo "raw JSON written to ${ROOT}/bench-results/14_tabpfn_raw.json"
echo "server log: ${RAW_DIR}/server.log"

# Record server warm time + final health snapshot
{
    echo "server_warm_secs=${WARM_SECS}"
    echo "client_rc=${CLIENT_RC}"
    curl -s "${HEALTH_URL}" || true
    echo
} > "${RAW_DIR}/run.summary.txt"

exit "${CLIENT_RC}"
