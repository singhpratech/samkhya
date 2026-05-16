#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
#
# probe-cuda.sh — print a portable, honest snapshot of the local NVIDIA /
# CUDA environment for the samkhya-gpudb crate. Re-runnable from a fresh
# checkout: `bash samkhya-gpudb/scripts/probe-cuda.sh`.
#
# Captures (best-effort, every step tolerant of failure):
#   - nvidia-smi GPU table
#   - nvcc release / build banner
#   - kernel-side NVIDIA driver version (/proc/driver/nvidia/version)
#   - PCI-level enumeration (lspci) for cross-checking the GPU model
#   - CUDA compute-capability hint when nvidia-smi works
#
# No PII: prints only tool output. No hostnames, no user paths.

set -u

banner() { printf '\n=== %s ===\n' "$1"; }

banner "nvidia-smi (CSV)"
if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=name,memory.total,driver_version,cuda_version,compute_cap \
        --format=csv,noheader 2>&1 || echo "(nvidia-smi CSV query FAILED)"
else
    echo "(nvidia-smi: not on PATH)"
fi

banner "nvidia-smi (default)"
if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi 2>&1 | head -20 || true
fi

banner "nvcc --version"
if command -v nvcc >/dev/null 2>&1; then
    nvcc --version 2>&1 | head -4
else
    echo "(nvcc: not on PATH)"
fi

banner "kernel-side driver (/proc/driver/nvidia/version)"
if [[ -r /proc/driver/nvidia/version ]]; then
    head -5 /proc/driver/nvidia/version
else
    echo "(/proc/driver/nvidia/version absent — no NVIDIA kernel module loaded?)"
fi

banner "PCI enumeration (lspci -nn)"
if command -v lspci >/dev/null 2>&1; then
    lspci -nn | grep -i nvidia || echo "(no NVIDIA devices on PCI bus)"
else
    echo "(lspci: not on PATH)"
fi

banner "loaded NVIDIA kernel modules (lsmod)"
if command -v lsmod >/dev/null 2>&1; then
    lsmod | grep -i nvidia | head -10 || echo "(no nvidia modules loaded)"
fi

banner "CUDA runtime libraries on linker path"
ldconfig -p 2>/dev/null | grep -E 'libcuda|libnvidia-ml|libcudart' | head -10 \
    || echo "(none found via ldconfig)"

banner "summary"
echo "If nvidia-smi failed with NVML 'Driver/library version mismatch',"
echo "the kernel module (/proc/driver/nvidia/version) and the userspace"
echo "library (libnvidia-ml.so) are on different driver releases. Reboot"
echo "into the matching driver, or reinstall the userspace package."
echo "Probe complete."
