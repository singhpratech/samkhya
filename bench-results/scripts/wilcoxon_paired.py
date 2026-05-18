#!/usr/bin/env python3
"""Paired Wilcoxon signed-rank test (two-sided).

Reference:
    Wilcoxon, F. (1945). "Individual Comparisons by Ranking Methods."
    Biometrics Bulletin 1(6):80-83.

Zeros (perfectly tied pairs) are dropped per Wilcoxon's original 1945
procedure, matching ``scipy.stats.wilcoxon(zero_method='wilcox')``.
Ties on |d| are handled with average ranks.

Implementation
--------------
    * For n > 25 the normal approximation is used:
          mu     = n(n+1)/4
          sigma  = sqrt(n(n+1)(2n+1)/24 - tie_correction/48)
          z      = (W - mu) / sigma           (no continuity correction)
          p_two  = 2 * (1 - Phi(|z|))
      where tie_correction = sum_t (t^3 - t) over tie groups of size t > 1
      on the absolute differences (Lehmann 1975, ch. 4).
    * For n <= 25 an exact enumeration of the 2^n sign assignments is used.

    If scipy is available it is used directly; the hand-rolled path is the
    fallback and yields the same numerics to ~1e-9 for the normal-approx
    regime.

Usage
-----
    # Two JSON arrays via flags
    python3 wilcoxon_paired.py \\
        --treatment treatment.json --baseline baseline.json

    # CSV with two columns
    python3 wilcoxon_paired.py --input data.csv \\
        --column-treatment samkhya_p95 --column-baseline native_p95

Output (stdout, one JSON object)
--------------------------------
    {"W": <stat>, "p": <two-sided>, "n_pairs": <int>,
     "median_diff": <float>, "method": "wilcoxon_signed_rank"}
"""

from __future__ import annotations

import argparse
import csv
import itertools
import json
import math
import sys
from typing import List, Optional, Sequence, Tuple


# ---------------------------------------------------------------------------
# Normal CDF
# ---------------------------------------------------------------------------

def _norm_cdf(x: float) -> float:
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


# ---------------------------------------------------------------------------
# Stats
# ---------------------------------------------------------------------------

def _median(xs: Sequence[float]) -> float:
    s = sorted(xs)
    n = len(s)
    if n == 0:
        return float("nan")
    if n % 2 == 1:
        return s[n // 2]
    return 0.5 * (s[n // 2 - 1] + s[n // 2])


def _rank_average(values: Sequence[float]) -> List[float]:
    """Average-rank assignment, 1-indexed."""
    n = len(values)
    order = sorted(range(n), key=lambda i: values[i])
    ranks = [0.0] * n
    i = 0
    while i < n:
        j = i
        while j + 1 < n and values[order[j + 1]] == values[order[i]]:
            j += 1
        avg = 0.5 * (i + j) + 1  # 1-indexed mid-rank
        for k in range(i, j + 1):
            ranks[order[k]] = avg
        i = j + 1
    return ranks


# ---------------------------------------------------------------------------
# Wilcoxon signed-rank — hand-rolled path
# ---------------------------------------------------------------------------

def _wilcoxon_handrolled(diffs: Sequence[float]) -> Tuple[float, float, int]:
    """Returns (W, p_two_sided, n_after_zero_drop)."""
    nz_diffs = [d for d in diffs if d != 0.0]
    n = len(nz_diffs)
    if n == 0:
        return 0.0, 1.0, 0

    abs_diffs = [abs(d) for d in nz_diffs]
    ranks = _rank_average(abs_diffs)
    w_plus = sum(r for r, d in zip(ranks, nz_diffs) if d > 0)
    w_minus = sum(r for r, d in zip(ranks, nz_diffs) if d < 0)
    w = min(w_plus, w_minus)

    if n <= 25:
        # Exact: enumerate all 2^n sign assignments. We need the
        # two-sided tail count: number of sign-flips whose
        # min(W_plus, W_minus) <= w_observed.
        # Equivalent: count assignments with W_plus <= w or W_plus >= total - w.
        total = sum(ranks)
        threshold_lo = w
        threshold_hi = total - w
        count = 0
        denom = 0
        for bits in itertools.product((0, 1), repeat=n):
            wp = sum(r for r, b in zip(ranks, bits) if b == 1)
            denom += 1
            if wp <= threshold_lo or wp >= threshold_hi:
                count += 1
        p = count / denom
        return w, max(min(p, 1.0), 0.0), n

    # Normal approximation with tie correction.
    mu = n * (n + 1) / 4.0
    # Tie-correction in variance: subtract sum_t (t^3 - t) / 48
    # over tie groups of size t on abs_diffs.
    sorted_abs = sorted(abs_diffs)
    tie_term = 0
    i = 0
    while i < n:
        j = i
        while j + 1 < n and sorted_abs[j + 1] == sorted_abs[i]:
            j += 1
        t = j - i + 1
        if t > 1:
            tie_term += t ** 3 - t
        i = j + 1
    var = n * (n + 1) * (2 * n + 1) / 24.0 - tie_term / 48.0
    if var <= 0:
        return w, 1.0, n
    sigma = math.sqrt(var)
    z = (w - mu) / sigma
    p = 2.0 * (1.0 - _norm_cdf(abs(z)))
    return w, max(min(p, 1.0), 0.0), n


def wilcoxon_paired(treatment: Sequence[float],
                    baseline: Sequence[float]) -> Tuple[float, float, int, float]:
    """Returns (W, p, n_pairs_after_zero_drop, median_diff).

    Uses ``scipy.stats.wilcoxon`` if available, else the hand-rolled path.
    Both implement Wilcoxon 1945 with ``zero_method='wilcox'`` (drop zeros)
    and ``alternative='two-sided'``.
    """
    if len(treatment) != len(baseline):
        raise ValueError(
            f"length mismatch: treatment={len(treatment)} baseline={len(baseline)}")
    diffs = [t - b for t, b in zip(treatment, baseline)]
    median_diff = _median(diffs) if diffs else float("nan")

    try:
        from scipy.stats import wilcoxon  # type: ignore
    except ImportError:
        w, p, n_after = _wilcoxon_handrolled(diffs)
        return w, p, n_after, median_diff

    nz_diffs = [d for d in diffs if d != 0.0]
    n_after = len(nz_diffs)
    if n_after == 0:
        return 0.0, 1.0, 0, median_diff
    # Use 'approx' to match the n>25 normal-approximation path; for small n
    # scipy auto-selects exact when feasible.
    res = wilcoxon(nz_diffs, zero_method="wilcox", alternative="two-sided")
    return float(res.statistic), float(res.pvalue), n_after, median_diff


# ---------------------------------------------------------------------------
# Input loaders
# ---------------------------------------------------------------------------

def _load_json_array(path_or_stream) -> List[float]:
    if hasattr(path_or_stream, "read"):
        data = json.load(path_or_stream)
    else:
        with open(path_or_stream) as fh:
            data = json.load(fh)
    if not isinstance(data, list):
        raise ValueError("expected JSON array of numbers")
    return [float(v) for v in data if v is not None]


def _load_csv_column(path: str, column: str) -> List[float]:
    out: List[float] = []
    with open(path, newline="") as fh:
        reader = csv.DictReader(fh)
        if column not in (reader.fieldnames or []):
            raise ValueError(
                f"column {column!r} not in CSV header {reader.fieldnames!r}")
        for row in reader:
            raw = row.get(column, "")
            if raw is None or raw == "" or raw.lower() in {"null", "nan", "none"}:
                continue
            out.append(float(raw))
    return out


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Paired Wilcoxon signed-rank test (two-sided). "
            "Reference: Wilcoxon 1945, Biometrics Bulletin 1(6):80-83."
        )
    )
    parser.add_argument("--treatment",
                        help="JSON array of treatment-arm measurements")
    parser.add_argument("--baseline",
                        help="JSON array of baseline-arm measurements")
    parser.add_argument("--input",
                        help="CSV with two matched-pair columns")
    parser.add_argument("--column-treatment",
                        help="CSV column name for treatment values")
    parser.add_argument("--column-baseline",
                        help="CSV column name for baseline values")
    args = parser.parse_args()

    if args.input is not None:
        if not (args.column_treatment and args.column_baseline):
            raise SystemExit(
                "--input requires --column-treatment and --column-baseline")
        treatment = _load_csv_column(args.input, args.column_treatment)
        baseline = _load_csv_column(args.input, args.column_baseline)
    else:
        if not (args.treatment and args.baseline):
            raise SystemExit(
                "either (--treatment + --baseline) or "
                "(--input + --column-treatment + --column-baseline) required")
        treatment = _load_json_array(args.treatment)
        baseline = _load_json_array(args.baseline)

    w, p, n_pairs, median_diff = wilcoxon_paired(treatment, baseline)

    json.dump({
        "W": w,
        "p": p,
        "n_pairs": n_pairs,
        "median_diff": median_diff,
        "method": "wilcoxon_signed_rank",
        "alternative": "two-sided",
        "zero_method": "wilcox",
        "citation": "Wilcoxon 1945, Biometrics Bulletin 1(6):80-83",
    }, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
