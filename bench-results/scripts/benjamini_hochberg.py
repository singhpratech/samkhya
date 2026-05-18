#!/usr/bin/env python3
"""Benjamini-Hochberg false-discovery-rate correction.

Reference:
    Benjamini, Y. & Hochberg, Y. (1995). "Controlling the False Discovery Rate:
    a Practical and Powerful Approach to Multiple Testing."
    Journal of the Royal Statistical Society. Series B 57(1):289-300.

Procedure (BH 1995 §3)
----------------------
    Let p_(1) <= p_(2) <= ... <= p_(m) be the ordered p-values and H_(i)
    the associated hypothesis. Find
        k* = max { i : p_(i) <= i/m * alpha }
    and reject H_(1), ..., H_(k*). If no such i exists, reject nothing.

Adjusted p-values follow the monotone-from-the-right transform
(Yekutieli & Benjamini 1999):
    q_(i) = min_{j >= i} min(1, m * p_(j) / j)

Usage
-----
    echo '[0.001, 0.012, 0.04, 0.2, 0.8]' | python3 benjamini_hochberg.py
    python3 benjamini_hochberg.py --input pvalues.json --alpha 0.05

Output (stdout, one JSON object)
--------------------------------
    {"rejected": [bool, ...], "adjusted_p": [float, ...],
     "alpha": <float>, "m": <int>, "n_rejected": <int>,
     "method": "benjamini_hochberg"}
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import List, Sequence, Tuple


def benjamini_hochberg(pvals: Sequence[float],
                       alpha: float) -> Tuple[List[bool], List[float]]:
    """Return (rejected, adjusted_p) parallel to ``pvals``."""
    m = len(pvals)
    if m == 0:
        return [], []
    for p in pvals:
        if not (0.0 <= p <= 1.0):
            raise ValueError(f"p-value out of range [0,1]: {p}")

    indexed = sorted(enumerate(pvals), key=lambda t: t[1])
    rejected = [False] * m

    # Find the largest k with p_(k) <= k/m * alpha.
    max_k = -1
    for rank, (_, p) in enumerate(indexed, start=1):
        if p <= rank / m * alpha:
            max_k = rank
    if max_k >= 0:
        for rank, (orig_idx, _) in enumerate(indexed, start=1):
            if rank <= max_k:
                rejected[orig_idx] = True

    # Adjusted p-values (monotone from the right).
    adj_sorted: List[float] = [0.0] * m
    running = 1.0
    for rank in range(m, 0, -1):
        _, p = indexed[rank - 1]
        q = min(1.0, m * p / rank)
        if q < running:
            running = q
        adj_sorted[rank - 1] = running
    adjusted_p = [0.0] * m
    for rank, (orig_idx, _) in enumerate(indexed, start=1):
        adjusted_p[orig_idx] = adj_sorted[rank - 1]

    return rejected, adjusted_p


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _load_json_array(path_or_stream) -> List[float]:
    if hasattr(path_or_stream, "read"):
        data = json.load(path_or_stream)
    else:
        with open(path_or_stream) as fh:
            data = json.load(fh)
    if not isinstance(data, list):
        raise ValueError("expected JSON array of p-values")
    return [float(v) for v in data]


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Benjamini-Hochberg FDR correction. "
            "Reference: Benjamini & Hochberg 1995, JRSSB 57(1):289-300."
        )
    )
    parser.add_argument("--input",
                        help="path to JSON array of p-values; "
                             "if omitted, read from stdin")
    parser.add_argument("--alpha", type=float, default=0.05,
                        help="FDR control level (default: 0.05)")
    args = parser.parse_args()

    pvals = (_load_json_array(args.input) if args.input is not None
             else _load_json_array(sys.stdin))

    rejected, adjusted_p = benjamini_hochberg(pvals, args.alpha)

    json.dump({
        "rejected": rejected,
        "adjusted_p": adjusted_p,
        "alpha": args.alpha,
        "m": len(pvals),
        "n_rejected": sum(1 for r in rejected if r),
        "method": "benjamini_hochberg",
        "citation": "Benjamini & Hochberg 1995, JRSSB 57(1):289-300",
    }, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
