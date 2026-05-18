#!/usr/bin/env python3
"""aggregate_ablation_16.py — emit matched-pair vectors for file 16
(`bench-results/16_ablation_calibration_size.md`), then compute paired
BCa CIs and Wilcoxon W / p per ablation pair.

This is the WAVE5-H closure for the file 16 pipeline blocker:
`bench-results/scripts/ablation_aggregate.py` produces per-ablation
summary CIs but never persists matched-pair vectors keyed by (query,
seed, replicate). Without paired vectors the downstream Wilcoxon
signed-rank test cannot run, so file 16's "Wilcoxon p-value pending"
cells stayed pending across WAVE5G.

This script:

  1. Loads `bench-results/15_ablation_raw_v3.json` (or any path passed
     as the first positional argument).
  2. Walks every (query, seed, replicate) tuple and emits one matched-
     pair record per (ablation_a, ablation_b) combination, carrying the
     paired (a_i, b_i) q-error and latency_ms values.
  3. Computes 95% paired BCa CIs (Efron-Tibshirani 1993, Ch. 14) on the
     log-ratio `ln(b/a)` per ablation pair.
  4. Computes the two-sided Wilcoxon signed-rank test (Wilcoxon 1945)
     on the same paired vector.
  5. Applies Benjamini-Hochberg FDR (Benjamini-Hochberg 1995) at α=0.05
     across the pair grid.
  6. Writes `bench-results/16_ablation_matched_pairs.json` (the paired
     vectors) and prints a markdown table to stdout.

Citations:
  - Efron, B. & Tibshirani, R. J. (1993). *An Introduction to the
    Bootstrap*. Chapman & Hall / CRC. Chapter 14.
  - Wilcoxon, F. (1945). "Individual Comparisons by Ranking Methods."
    *Biometrics Bulletin* 1(6):80–83.
  - Benjamini, Y. & Hochberg, Y. (1995). "Controlling the False
    Discovery Rate: A Practical and Powerful Approach to Multiple
    Testing." *J. Royal Stat. Soc. Series B* 57(1):289-300.

Usage:
    python3 aggregate_ablation_16.py                       # default raw_v3
    python3 aggregate_ablation_16.py 15_ablation_raw.json  # explicit path
"""

from __future__ import annotations

import json
import math
import random
import sys
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# Reuse the stats primitives from ablation_aggregate.py.
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import ablation_aggregate as aa  # type: ignore  # noqa: E402

ROOT = HERE.parent
DEFAULT_RAW = ROOT / "15_ablation_raw_v3.json"
PAIRS_OUT = ROOT / "16_ablation_matched_pairs.json"

N_BOOT = 10_000
ALPHA = 0.05
INF_CAP_LOG10 = 6.0  # match ablation_aggregate.py


def log_qerror(q: Optional[float]) -> float:
    """Map a q-error to log10 space with infinity-capping."""
    return aa.to_log10_qerror(q)


def load_raw(path: Path) -> List[dict]:
    return json.loads(path.read_text())


def group_by_key(records: List[dict]) -> Dict[Tuple[str, int, int], Dict[str, dict]]:
    """Return {(query, replicate, seed) -> {ablation -> record}}."""
    out: Dict[Tuple[str, int, int], Dict[str, dict]] = defaultdict(dict)
    for r in records:
        key = (r["query"], int(r["replicate"]), int(r["seed"]))
        out[key][r["ablation"]] = r
    return out


def pair_vectors(records: List[dict]) -> Dict[Tuple[str, str], List[dict]]:
    """For every (ablation_a, ablation_b) pair, emit a list of matched
    records: {query, replicate, seed, a_qerror, b_qerror, a_log10q,
    b_log10q, a_latency_ms, b_latency_ms, log_ratio}.
    """
    grouped = group_by_key(records)
    ablations = sorted({r["ablation"] for r in records})
    out: Dict[Tuple[str, str], List[dict]] = {}
    for i, ab_a in enumerate(ablations):
        for ab_b in ablations[i + 1:]:
            cells = []
            for (query, replicate, seed), abls in grouped.items():
                if ab_a not in abls or ab_b not in abls:
                    continue
                a = abls[ab_a]
                b = abls[ab_b]
                a_log = log_qerror(a.get("q_error"))
                b_log = log_qerror(b.get("q_error"))
                cells.append({
                    "query": query,
                    "replicate": replicate,
                    "seed": seed,
                    "a_qerror": a.get("q_error"),
                    "b_qerror": b.get("q_error"),
                    "a_log10q": a_log,
                    "b_log10q": b_log,
                    "a_latency_ms": a.get("latency_ms"),
                    "b_latency_ms": b.get("latency_ms"),
                    # log_ratio in *natural* log to match the Leis 2015 +
                    # bootstrap_ci.py convention; equivalent to log10
                    # multiplied by ln(10).
                    "log_ratio_natural": (b_log - a_log) * math.log(10.0),
                })
            out[(ab_a, ab_b)] = cells
    return out


def paired_bca_and_wilcoxon(
    diffs: List[float], rng: random.Random
) -> Tuple[float, float, float, float, float]:
    """Return (point, ci_lo, ci_hi, wilcoxon_W, wilcoxon_p) on the
    paired log-ratio differences."""
    if not diffs:
        return float("nan"), float("nan"), float("nan"), 0.0, 1.0
    point, lo, hi = aa.bca_ci(diffs, lambda xs: sum(xs) / len(xs), N_BOOT, ALPHA, rng)
    w, p = aa.wilcoxon_signed_rank(diffs)
    return point, lo, hi, w, p


def main(argv: List[str]) -> int:
    raw_path = Path(argv[1]) if len(argv) > 1 else DEFAULT_RAW
    raw_path = raw_path.resolve()
    if not raw_path.exists():
        print(f"error: raw input file not found: {raw_path}", file=sys.stderr)
        return 1
    raw = load_raw(raw_path)

    pairs = pair_vectors(raw)
    rng = random.Random(42)  # canonical bootstrap seed

    summary: Dict[str, dict] = {
        "raw_input": str(raw_path),
        "n_records": len(raw),
        "n_bootstrap": N_BOOT,
        "alpha": ALPHA,
        "bootstrap_seed": 42,
        "citations": [
            "Efron & Tibshirani 1993 — An Introduction to the Bootstrap (BCa).",
            "Wilcoxon 1945 — Individual Comparisons by Ranking Methods.",
            "Benjamini & Hochberg 1995 — Controlling the FDR.",
            "Moerkotte, Neumann, Steidl 2009 — Preventing bad plans by bounding q-error.",
        ],
        "pairs": [],
    }

    pair_pvals: List[float] = []
    for (ab_a, ab_b), cells in pairs.items():
        diffs = [c["log_ratio_natural"] for c in cells]
        point, lo, hi, w, p = paired_bca_and_wilcoxon(diffs, rng)
        pair_pvals.append(p)
        # Back-transform to multiplicative q-error ratio.
        pt_ratio = math.exp(point) if point == point else float("nan")
        lo_ratio = math.exp(lo) if lo == lo else float("nan")
        hi_ratio = math.exp(hi) if hi == hi else float("nan")
        summary["pairs"].append({
            "a": ab_a,
            "b": ab_b,
            "n_pairs": len(cells),
            "mean_log_ratio_natural": point,
            "log_ratio_ci_lo": lo,
            "log_ratio_ci_hi": hi,
            "qerror_ratio_b_over_a_point": pt_ratio,
            "qerror_ratio_b_over_a_ci_lo": lo_ratio,
            "qerror_ratio_b_over_a_ci_hi": hi_ratio,
            "wilcoxon_W": w,
            "wilcoxon_p": p,
            "matched_pairs": cells,
        })

    # BH FDR.
    bh_rejected = aa.benjamini_hochberg(pair_pvals, ALPHA)
    for entry, sig in zip(summary["pairs"], bh_rejected):
        entry["bh_significant_alpha_0.05"] = bool(sig)

    PAIRS_OUT.write_text(json.dumps(summary, indent=2))

    # Markdown table to stdout.
    print(f"# aggregate_ablation_16 — matched-pair output (input: "
          f"{summary['raw_input']}, n_pairs={N_BOOT} resamples)")
    print()
    print("| Pair (a → b) | n | b/a q-error ratio | 95% BCa CI | Wilcoxon p | BH α=0.05 sig |")
    print("|---|---:|---:|---|---:|---:|")
    for entry in summary["pairs"]:
        sig = "yes" if entry.get("bh_significant_alpha_0.05") else "no"
        print(
            f"| {entry['a']} → {entry['b']} | {entry['n_pairs']} | "
            f"{entry['qerror_ratio_b_over_a_point']:.3f} | "
            f"[{entry['qerror_ratio_b_over_a_ci_lo']:.3f}, "
            f"{entry['qerror_ratio_b_over_a_ci_hi']:.3f}] | "
            f"{entry['wilcoxon_p']:.3g} | {sig} |"
        )
    print()
    print(f"# matched-pair vectors persisted to {PAIRS_OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
