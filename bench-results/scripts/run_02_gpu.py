#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# bench-results/scripts/run_02_gpu.py
#
# GPU hash throughput driver for samkhya-gpudb batched join-key path.
#
# The kernel under test is the LCG-style 64-bit mix that samkhya-gpudb's
# batched join-key inference path uses to bucket subplan candidate keys
# before scoring (see samkhya-gpudb/scripts/bench-on-4090.py for the
# canonical Python version of the kernel). It is a stand-in for xxhash3
# in this proof-of-life pass; the kernel is bandwidth-bound on the GPU
# and ALU-bound on the CPU, which is the dimension we want to characterize.
#
# Outputs JSON with throughput, latency percentiles, and transfer
# overhead. Consumed by bench-results/02_gpu_hash_throughput.md.

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import time
from dataclasses import asdict, dataclass, field
from typing import Any

LCG_MUL = 2862933555777941757
LCG_ADD = 3037000493
LCG_MASK = 0x7FFFFFFFFFFFFFFF


@dataclass
class RunStats:
    backend: str
    n: int
    threads: int | None
    replicates: int
    mean_s: float
    p50_s: float
    p95_s: float
    p99_s: float
    throughput_gkeys_s_mean: float
    throughput_gkeys_s_ci95: tuple[float, float]
    h2d_s_mean: float | None = None
    d2h_s_mean: float | None = None
    detail: dict[str, Any] = field(default_factory=dict)


def _percentiles(xs: list[float], qs: list[float]) -> list[float]:
    s = sorted(xs)
    out = []
    for q in qs:
        if not s:
            out.append(float("nan"))
            continue
        k = max(0, min(len(s) - 1, int(round(q * (len(s) - 1)))))
        out.append(s[k])
    return out


def _bootstrap_ci(xs: list[float], iters: int = 2000, alpha: float = 0.05) -> tuple[float, float]:
    import random
    rng = random.Random(20260516)
    means = []
    n = len(xs)
    for _ in range(iters):
        s = sum(xs[rng.randrange(n)] for _ in range(n))
        means.append(s / n)
    means.sort()
    lo = means[int(alpha / 2 * iters)]
    hi = means[int((1 - alpha / 2) * iters) - 1]
    return lo, hi


def _bench_cpu_numpy(n: int, threads: int, replicates: int) -> RunStats:
    # Threading must be set BEFORE importing numpy; this function is
    # called only after threads are pinned via env vars.
    import numpy as np

    rng = np.random.default_rng(20260516)
    keys = rng.integers(0, 1 << 62, size=n, dtype=np.int64)

    # Warm-up
    _ = ((keys * LCG_MUL + LCG_ADD) & LCG_MASK).sum()

    times = []
    for _ in range(replicates):
        t0 = time.perf_counter()
        h = (keys * LCG_MUL + LCG_ADD) & LCG_MASK
        _ = int(h.sum())
        times.append(time.perf_counter() - t0)

    p50, p95, p99 = _percentiles(times, [0.5, 0.95, 0.99])
    tput = [n / t / 1e9 for t in times]
    ci = _bootstrap_ci(tput)
    return RunStats(
        backend="cpu_numpy",
        n=n,
        threads=threads,
        replicates=replicates,
        mean_s=statistics.fmean(times),
        p50_s=p50,
        p95_s=p95,
        p99_s=p99,
        throughput_gkeys_s_mean=statistics.fmean(tput),
        throughput_gkeys_s_ci95=ci,
        detail={"numpy_version": np.__version__},
    )


def _bench_gpu_torch(n: int, replicates: int) -> RunStats:
    import torch
    assert torch.cuda.is_available(), "CUDA not available"

    device = torch.device("cuda:0")
    g_cpu = torch.Generator(device="cpu").manual_seed(20260516)
    cpu_u64 = torch.randint(0, 1 << 62, (n,), dtype=torch.int64, generator=g_cpu)

    # Pre-allocate device buffer
    torch.cuda.synchronize()
    gpu_u64 = cpu_u64.to(device, non_blocking=False)
    torch.cuda.synchronize()

    # Warm-up
    for _ in range(3):
        h = (gpu_u64 * LCG_MUL + LCG_ADD) & LCG_MASK
        _ = h.sum()
        torch.cuda.synchronize()

    # Kernel-only timing (no transfer)
    kernel_times = []
    for _ in range(replicates):
        torch.cuda.synchronize()
        t0 = time.perf_counter()
        h = (gpu_u64 * LCG_MUL + LCG_ADD) & LCG_MASK
        s = h.sum()
        torch.cuda.synchronize()
        kernel_times.append(time.perf_counter() - t0)

    # H2D and D2H timing (5 replicates)
    h2d_times = []
    d2h_times = []
    for _ in range(min(5, replicates)):
        torch.cuda.synchronize()
        t0 = time.perf_counter()
        gpu_tmp = cpu_u64.to(device, non_blocking=False)
        torch.cuda.synchronize()
        h2d_times.append(time.perf_counter() - t0)

        torch.cuda.synchronize()
        t0 = time.perf_counter()
        _ = gpu_tmp.to("cpu", non_blocking=False)
        torch.cuda.synchronize()
        d2h_times.append(time.perf_counter() - t0)
        del gpu_tmp

    p50, p95, p99 = _percentiles(kernel_times, [0.5, 0.95, 0.99])
    tput = [n / t / 1e9 for t in kernel_times]
    ci = _bootstrap_ci(tput)
    props = torch.cuda.get_device_properties(0)
    return RunStats(
        backend="gpu_torch",
        n=n,
        threads=None,
        replicates=replicates,
        mean_s=statistics.fmean(kernel_times),
        p50_s=p50,
        p95_s=p95,
        p99_s=p99,
        throughput_gkeys_s_mean=statistics.fmean(tput),
        throughput_gkeys_s_ci95=ci,
        h2d_s_mean=statistics.fmean(h2d_times),
        d2h_s_mean=statistics.fmean(d2h_times),
        detail={
            "device_name": torch.cuda.get_device_name(0),
            "compute_capability": f"sm_{props.major}{props.minor}",
            "total_memory_gb": round(props.total_memory / 1024 ** 3, 2),
            "torch_version": torch.__version__,
        },
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sizes", default="1000000,10000000,100000000")
    ap.add_argument("--replicates", type=int, default=30)
    ap.add_argument("--output", required=True)
    ap.add_argument("--backend", default="all", choices=["cpu", "gpu", "all"])
    ap.add_argument("--cpu-threads", default=None,
                    help="if set, only run CPU at this thread count")
    args = ap.parse_args()

    sizes = [int(s) for s in args.sizes.split(",")]
    threads_env = args.cpu_threads or os.environ.get("OMP_NUM_THREADS")
    threads_int = int(threads_env) if threads_env else None

    all_results: list[RunStats] = []

    if args.backend in ("cpu", "all"):
        for n in sizes:
            print(f"[run_02] cpu_numpy n={n:,} threads={threads_int}",
                  file=sys.stderr, flush=True)
            try:
                r = _bench_cpu_numpy(n, threads_int or 0, args.replicates)
                all_results.append(r)
            except Exception as e:
                print(f"  FAILED: {type(e).__name__}: {e}", file=sys.stderr)

    if args.backend in ("gpu", "all"):
        try:
            import torch
            if torch.cuda.is_available():
                for n in sizes:
                    print(f"[run_02] gpu_torch n={n:,}",
                          file=sys.stderr, flush=True)
                    try:
                        r = _bench_gpu_torch(n, args.replicates)
                        all_results.append(r)
                    except Exception as e:
                        print(f"  FAILED: {type(e).__name__}: {e}",
                              file=sys.stderr)
            else:
                print("[run_02] CUDA not available; skipping GPU",
                      file=sys.stderr)
        except ImportError:
            print("[run_02] torch not installed; skipping GPU", file=sys.stderr)

    report = {
        "schema": "bench-results/02_gpu_hash_throughput.v1",
        "results": [asdict(r) for r in all_results],
    }
    with open(args.output, "w") as f:
        json.dump(report, f, indent=2, default=str)
    print(f"[run_02] wrote {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
