#!/usr/bin/env bash
# bench-results/scripts/run_02_gpu.sh
# Reproducibility driver for bench-results/02_gpu_hash_throughput.md.
#
# Runs the LCG-style batched join-key hash workload on:
#   - CPU single-thread (NumPy)
#   - CPU 8-thread and 20-thread (NumPy + OMP_NUM_THREADS)
#   - GPU (PyTorch CUDA)
# at workloads of 1M, 10M, 100M u64 keys, 30 replicates each.
#
# Reports throughput (G keys/s), P50/P95/P99 latency, host-to-device
# transfer overhead, and the break-even point versus the 20-thread CPU
# baseline.
#
# Expected environment:
#   - Linux + NVIDIA driver + CUDA 12.x
#   - python venv with torch (cu124) and numpy installed
#
# On the development laptop (RTX 4090 Laptop GPU) the default venv is
# samkhya-py/.venv-acceptance/. On a desktop 4090 box, point VENV_PY at
# a venv with `pip install torch --index-url https://download.pytorch.org/whl/cu124 numpy`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. && pwd)"
VENV_PY="${VENV_PY:-$ROOT/samkhya-py/.venv-acceptance/bin/python3}"
OUT_JSON="${OUT_JSON:-$ROOT/bench-results/02_gpu_hash_throughput.json}"

if [[ ! -x "$VENV_PY" ]]; then
  echo "error: VENV_PY not executable: $VENV_PY" >&2
  echo "set VENV_PY to a python3 with torch+cuda and numpy installed" >&2
  exit 2
fi

echo "[run_02] using python: $VENV_PY"
echo "[run_02] gpu detection:"
nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader || true

exec "$VENV_PY" "$ROOT/bench-results/scripts/run_02_gpu.py" \
  --replicates "${REPLICATES:-30}" \
  --sizes "${SIZES:-1000000,10000000,100000000}" \
  --output "$OUT_JSON"
