#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: Prateek Singh
#
# bench-on-4090.py — empirical benchmark of the samkhya Python wheel on
# the local machine, with optional GPU-side hash-aggregation timing via
# PyTorch CUDA when the stack is available.
#
# Designed for the workstation tier (RTX 4090 / Ada Lovelace, sm_89) but
# runs on any host that has the wheel installed; GPU steps degrade to
# `SKIPPED: <reason>` instead of crashing the script.
#
# Workload definitions (kept stable so future re-runs are comparable):
#   - HLL workload     : 1000 sketches x 10k random u64 keys (10M inserts)
#   - Bloom workload   : 1000 filters  x 10k random u64 keys, FPR=0.01
#   - CMS workload     : 1000 sketches x 10k random u64 keys + counts
#
# Output is machine-readable: a JSON report on stdout plus a human
# table. Re-runnable: `python3 samkhya-gpudb/scripts/bench-on-4090.py`.
#
# No PII: nothing is logged about the host beyond CPU/GPU model + memory.

from __future__ import annotations

import json
import os
import random
import sys
import time
from dataclasses import asdict, dataclass, field
from typing import Any


# ---- Workload sizing -------------------------------------------------------

NUM_SKETCHES = 1000
KEYS_PER_SKETCH = 10_000
TOTAL_INSERTS = NUM_SKETCHES * KEYS_PER_SKETCH  # 10_000_000

HLL_PRECISION = 14         # ~16 KiB per sketch, ~0.81% relative error
BLOOM_FP_RATE = 0.01
CMS_WIDTH = 2048
CMS_DEPTH = 5

# GPU proof-of-life workload: 10M random u64s, sum-of-hashes on device.
GPU_N_ELEMS = 10_000_000


# ---- Result container ------------------------------------------------------

@dataclass
class StageResult:
    name: str
    status: str                       # "OK" | "FAILED" | "SKIPPED"
    wall_clock_s: float | None = None
    throughput_per_s: float | None = None
    detail: dict[str, Any] = field(default_factory=dict)
    error: str | None = None


# ---- Helpers ---------------------------------------------------------------

def _make_keys(n: int, seed: int) -> list[bytes]:
    """Generate `n` 8-byte keys deterministically (no PII; pure RNG)."""
    rng = random.Random(seed)
    # 64-bit unsigned ints serialized as 8-byte little-endian.
    return [rng.getrandbits(64).to_bytes(8, "little") for _ in range(n)]


def _bench_samkhya() -> list[StageResult]:
    out: list[StageResult] = []

    try:
        import samkhya
    except Exception as e:  # pragma: no cover
        out.append(StageResult(
            name="samkhya_import", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))
        return out

    out.append(StageResult(
        name="samkhya_import", status="OK",
        detail={"version": samkhya.samkhya_version()},
    ))

    # Pre-generate one shared key pool so all three sketches see the
    # same data and timings reflect sketch work, not RNG cost.
    print(f"[bench] generating {TOTAL_INSERTS:,} keys (seed=20260516)...",
          file=sys.stderr, flush=True)
    t0 = time.perf_counter()
    keys = _make_keys(TOTAL_INSERTS, seed=20260516)
    t1 = time.perf_counter()
    print(f"[bench] key generation: {t1 - t0:.2f}s",
          file=sys.stderr, flush=True)

    # -------------------- HLL --------------------
    try:
        from samkhya import HllSketch
        t0 = time.perf_counter()
        for s in range(NUM_SKETCHES):
            h = HllSketch(HLL_PRECISION)
            base = s * KEYS_PER_SKETCH
            for k in keys[base:base + KEYS_PER_SKETCH]:
                h.add(k)
        elapsed = time.perf_counter() - t0
        out.append(StageResult(
            name="hll_build", status="OK",
            wall_clock_s=elapsed,
            throughput_per_s=TOTAL_INSERTS / elapsed,
            detail={
                "sketches": NUM_SKETCHES,
                "keys_per_sketch": KEYS_PER_SKETCH,
                "precision": HLL_PRECISION,
                "last_estimate": h.estimate(),
            },
        ))
    except Exception as e:
        out.append(StageResult(
            name="hll_build", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))

    # -------------------- Bloom --------------------
    try:
        from samkhya import BloomFilter
        t0 = time.perf_counter()
        for s in range(NUM_SKETCHES):
            b = BloomFilter(KEYS_PER_SKETCH, BLOOM_FP_RATE)
            base = s * KEYS_PER_SKETCH
            for k in keys[base:base + KEYS_PER_SKETCH]:
                b.add(k)
        elapsed = time.perf_counter() - t0
        out.append(StageResult(
            name="bloom_build", status="OK",
            wall_clock_s=elapsed,
            throughput_per_s=TOTAL_INSERTS / elapsed,
            detail={
                "filters": NUM_SKETCHES,
                "keys_per_filter": KEYS_PER_SKETCH,
                "fp_rate": BLOOM_FP_RATE,
                "last_num_bits": b.num_bits,
                "last_num_hashes": b.num_hashes,
            },
        ))
    except Exception as e:
        out.append(StageResult(
            name="bloom_build", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))

    # -------------------- CMS --------------------
    try:
        from samkhya import CountMinSketch
        t0 = time.perf_counter()
        for s in range(NUM_SKETCHES):
            c = CountMinSketch(CMS_WIDTH, CMS_DEPTH)
            base = s * KEYS_PER_SKETCH
            for k in keys[base:base + KEYS_PER_SKETCH]:
                c.add(k, 1)
        elapsed = time.perf_counter() - t0
        out.append(StageResult(
            name="cms_build", status="OK",
            wall_clock_s=elapsed,
            throughput_per_s=TOTAL_INSERTS / elapsed,
            detail={
                "sketches": NUM_SKETCHES,
                "keys_per_sketch": KEYS_PER_SKETCH,
                "width": CMS_WIDTH,
                "depth": CMS_DEPTH,
                "last_total": c.total,
            },
        ))
    except Exception as e:
        out.append(StageResult(
            name="cms_build", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))

    return out


def _bench_gpu() -> list[StageResult]:
    """Optional GPU proof-of-life. Always degrades to SKIPPED on failure."""
    out: list[StageResult] = []

    # --- torch detection ---
    try:
        import torch  # type: ignore
    except Exception as e:
        out.append(StageResult(
            name="torch_import", status="SKIPPED",
            error=f"{type(e).__name__}: {e}",
        ))
        return out

    out.append(StageResult(
        name="torch_import", status="OK",
        detail={"version": torch.__version__},
    ))

    try:
        cuda_ok = torch.cuda.is_available()
    except Exception as e:
        out.append(StageResult(
            name="torch_cuda_init", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))
        return out

    if not cuda_ok:
        out.append(StageResult(
            name="torch_cuda_init", status="FAILED",
            error="torch.cuda.is_available() == False",
        ))
        return out

    try:
        props = torch.cuda.get_device_properties(0)
        out.append(StageResult(
            name="torch_cuda_init", status="OK",
            detail={
                "device_name": torch.cuda.get_device_name(0),
                "total_memory_gb": round(props.total_memory / 1024 ** 3, 2),
                "multi_processor_count": props.multi_processor_count,
                "major": props.major,
                "minor": props.minor,
                "compute_capability": f"sm_{props.major}{props.minor}",
            },
        ))
    except Exception as e:
        out.append(StageResult(
            name="torch_cuda_init", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))
        return out

    # --- GPU hash-sum proof-of-life over 10M u64s ---
    try:
        device = torch.device("cuda:0")

        # generate on CPU (deterministic) then move to GPU
        g = torch.Generator(device="cpu").manual_seed(20260516)
        cpu_u64 = torch.randint(
            low=0, high=(1 << 62), size=(GPU_N_ELEMS,),
            dtype=torch.int64, generator=g,
        )

        # CPU baseline for the same hash-sum workload
        t0 = time.perf_counter()
        cpu_hash = (
            (cpu_u64 * 2862933555777941757 + 3037000493) & 0x7FFFFFFFFFFFFFFF
        )
        cpu_sum = int(cpu_hash.sum().item())
        cpu_elapsed = time.perf_counter() - t0
        out.append(StageResult(
            name="cpu_hashsum_10m", status="OK",
            wall_clock_s=cpu_elapsed,
            throughput_per_s=GPU_N_ELEMS / cpu_elapsed,
            detail={"elements": GPU_N_ELEMS, "hash_sum": cpu_sum},
        ))

        # GPU version
        torch.cuda.synchronize()
        gpu_u64 = cpu_u64.to(device, non_blocking=False)
        torch.cuda.synchronize()
        t0 = time.perf_counter()
        gpu_hash = (
            (gpu_u64 * 2862933555777941757 + 3037000493) & 0x7FFFFFFFFFFFFFFF
        )
        gpu_sum_t = gpu_hash.sum()
        torch.cuda.synchronize()
        gpu_elapsed = time.perf_counter() - t0
        gpu_sum = int(gpu_sum_t.item())

        out.append(StageResult(
            name="gpu_hashsum_10m", status="OK",
            wall_clock_s=gpu_elapsed,
            throughput_per_s=GPU_N_ELEMS / gpu_elapsed,
            detail={
                "elements": GPU_N_ELEMS,
                "hash_sum": gpu_sum,
                "matches_cpu": gpu_sum == cpu_sum,
                "speedup_vs_cpu": cpu_elapsed / gpu_elapsed if gpu_elapsed > 0 else None,
            },
        ))
    except Exception as e:
        out.append(StageResult(
            name="gpu_hashsum_10m", status="FAILED",
            error=f"{type(e).__name__}: {e}",
        ))

    # --- cupy (informational) ---
    try:
        import cupy  # type: ignore
        out.append(StageResult(
            name="cupy_import", status="OK",
            detail={"version": cupy.__version__},
        ))
    except Exception as e:
        out.append(StageResult(
            name="cupy_import", status="SKIPPED",
            error=f"{type(e).__name__}: {e}",
        ))

    return out


def _print_table(results: list[StageResult]) -> None:
    print()
    print(f"{'stage':<24} {'status':<8} {'wall (s)':>10} {'throughput/s':>16}")
    print("-" * 64)
    for r in results:
        wall = f"{r.wall_clock_s:.3f}" if r.wall_clock_s is not None else "-"
        tput = (
            f"{r.throughput_per_s:,.0f}"
            if r.throughput_per_s is not None else "-"
        )
        print(f"{r.name:<24} {r.status:<8} {wall:>10} {tput:>16}")
        if r.error:
            print(f"  error: {r.error}")
    print()


def main() -> int:
    sketch_results = _bench_samkhya()
    gpu_results = _bench_gpu()
    all_results = sketch_results + gpu_results

    _print_table(all_results)

    # Stable JSON for downstream report generation
    report = {
        "workloads": {
            "hll": {
                "sketches": NUM_SKETCHES,
                "keys_per_sketch": KEYS_PER_SKETCH,
                "precision": HLL_PRECISION,
            },
            "bloom": {
                "filters": NUM_SKETCHES,
                "keys_per_filter": KEYS_PER_SKETCH,
                "fp_rate": BLOOM_FP_RATE,
            },
            "cms": {
                "sketches": NUM_SKETCHES,
                "keys_per_sketch": KEYS_PER_SKETCH,
                "width": CMS_WIDTH,
                "depth": CMS_DEPTH,
            },
            "gpu_hashsum": {"elements": GPU_N_ELEMS},
        },
        "python": {
            "version": sys.version.split()[0],
            "executable": os.path.basename(sys.executable),
        },
        "results": [asdict(r) for r in all_results],
    }
    print(json.dumps(report, indent=2, sort_keys=True))

    # Exit 0 only if at least one samkhya sketch ran. GPU steps are
    # allowed to SKIP / FAIL without failing the script.
    sketch_ok = any(
        r.status == "OK" and r.name in {"hll_build", "bloom_build", "cms_build"}
        for r in sketch_results
    )
    return 0 if sketch_ok else 2


if __name__ == "__main__":
    sys.exit(main())
