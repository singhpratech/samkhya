#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
#
# run-bench-4090.sh — re-runnable launcher for bench-on-4090.py.
#
# Activates the samkhya-py acceptance venv (built by
# `samkhya-py/scripts/acceptance.sh`) and runs the benchmark. If the
# venv is missing it prints a clear remediation hint instead of failing
# silently. Future contributors:
#
#     bash samkhya-gpudb/scripts/run-bench-4090.sh
#
# Optional GPU stack (PyTorch CUDA) is detected at runtime and skipped
# cleanly when absent.

set -euo pipefail

# Resolve repo root from this script's location so the launcher works
# from any cwd (no `/home/...` hardcoded paths).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
VENV="${REPO_ROOT}/samkhya-py/.venv-acceptance"
BENCH="${SCRIPT_DIR}/bench-on-4090.py"

if [[ ! -d "${VENV}" ]]; then
    echo "ERROR: acceptance venv not found at <repo>/samkhya-py/.venv-acceptance" >&2
    echo "Bootstrap it first with:" >&2
    echo "    cd samkhya-py && bash scripts/acceptance.sh" >&2
    exit 2
fi

if [[ ! -x "${VENV}/bin/python3" ]]; then
    echo "ERROR: ${VENV}/bin/python3 is missing or not executable" >&2
    exit 2
fi

if [[ ! -f "${BENCH}" ]]; then
    echo "ERROR: bench-on-4090.py not found next to this launcher" >&2
    exit 2
fi

# Run the benchmark inside the acceptance venv (samkhya wheel already
# installed there by acceptance.sh). PyTorch / CuPy are optional; the
# benchmark degrades gracefully when they are absent.
exec "${VENV}/bin/python3" "${BENCH}" "$@"
