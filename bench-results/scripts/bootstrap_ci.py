#!/usr/bin/env python3
"""Bootstrap 95% confidence intervals (percentile or BCa).

Reference:
    Efron, B. & Tibshirani, R. J. (1993). "An Introduction to the Bootstrap."
    Chapman & Hall/CRC. Chapter 13 (percentile method) and Chapter 14
    ("Better Bootstrap Confidence Intervals", BCa).

Usage
-----
    # JSON array of measurements on stdin
    cat measurements.json | python3 bootstrap_ci.py --method bca

    # JSON array from file
    python3 bootstrap_ci.py --input measurements.json --method bca

    # CSV with a named column
    python3 bootstrap_ci.py --input data.csv --column latency_ms --method bca

Output (stdout, one JSON object)
--------------------------------
    {"point": <theta_hat>, "ci_lo": <2.5%>, "ci_hi": <97.5%>,
     "method": "bca" | "percentile", "n_resamples": <int>,
     "n_obs": <int>, "seed": <int>, "statistic": "mean" | "median" | ...}

Determinism
-----------
    Resample RNG is seeded with the integer passed via ``--seed`` (default 42).
    Identical inputs + identical seed yield byte-identical output.

Pure stdlib — no numpy/scipy dependency.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
import sys
from typing import Callable, List, Sequence, Tuple


# ---------------------------------------------------------------------------
# Statistic dispatch
# ---------------------------------------------------------------------------

def _mean(xs: Sequence[float]) -> float:
    return sum(xs) / len(xs)


def _median(xs: Sequence[float]) -> float:
    s = sorted(xs)
    n = len(s)
    if n % 2 == 1:
        return s[n // 2]
    return 0.5 * (s[n // 2 - 1] + s[n // 2])


def _make_quantile(q: float) -> Callable[[Sequence[float]], float]:
    def _q(xs: Sequence[float]) -> float:
        return _percentile_sorted(sorted(xs), q)
    return _q


STATISTICS: dict[str, Callable[[Sequence[float]], float]] = {
    "mean": _mean,
    "median": _median,
    "p50": _median,
    "p95": _make_quantile(0.95),
    "p99": _make_quantile(0.99),
}


def _percentile_sorted(sorted_vals: Sequence[float], p: float) -> float:
    """Linear-interpolation percentile on an already-sorted sequence.

    ``p`` is a fraction in [0, 1]. Matches numpy's default
    ``linear`` interpolation rule (type 7 in Hyndman & Fan 1996).
    """
    n = len(sorted_vals)
    if n == 0:
        return float("nan")
    if n == 1:
        return float(sorted_vals[0])
    if p <= 0:
        return float(sorted_vals[0])
    if p >= 1:
        return float(sorted_vals[-1])
    pos = p * (n - 1)
    lo = int(math.floor(pos))
    hi = int(math.ceil(pos))
    if lo == hi:
        return float(sorted_vals[lo])
    frac = pos - lo
    return float(sorted_vals[lo] * (1 - frac) + sorted_vals[hi] * frac)


# ---------------------------------------------------------------------------
# Normal CDF / PPF (Beasley-Springer-Moro inverse, erf-based CDF)
# ---------------------------------------------------------------------------

def _norm_cdf(x: float) -> float:
    return 0.5 * (1.0 + math.erf(x / math.sqrt(2.0)))


def _norm_ppf(p: float) -> float:
    """Inverse standard-normal CDF (Beasley-Springer-Moro)."""
    if not (0.0 < p < 1.0):
        if p <= 0.0:
            return -math.inf
        return math.inf
    a = [-3.969683028665376e1, 2.209460984245205e2, -2.759285104469687e2,
         1.383577518672690e2, -3.066479806614716e1, 2.506628277459239]
    b = [-5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2,
         6.680131188771972e1, -1.328068155288572e1]
    c = [-7.784894002430293e-3, -3.223964580411365e-1, -2.400758277161838,
         -2.549732539343734, 4.374664141464968, 2.938163982698783]
    d = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996,
         3.754408661907416]
    plow = 0.02425
    phigh = 1 - plow
    if p < plow:
        q = math.sqrt(-2.0 * math.log(p))
        return (((((c[0]*q + c[1])*q + c[2])*q + c[3])*q + c[4])*q + c[5]) / \
               ((((d[0]*q + d[1])*q + d[2])*q + d[3])*q + 1.0)
    if p <= phigh:
        q = p - 0.5
        r = q * q
        return (((((a[0]*r + a[1])*r + a[2])*r + a[3])*r + a[4])*r + a[5]) * q / \
               (((((b[0]*r + b[1])*r + b[2])*r + b[3])*r + b[4])*r + 1.0)
    q = math.sqrt(-2.0 * math.log(1.0 - p))
    return -(((((c[0]*q + c[1])*q + c[2])*q + c[3])*q + c[4])*q + c[5]) / \
           ((((d[0]*q + d[1])*q + d[2])*q + d[3])*q + 1.0)


# ---------------------------------------------------------------------------
# Bootstrap CI (percentile and BCa)
# ---------------------------------------------------------------------------

def percentile_ci(values: Sequence[float],
                  statistic: Callable[[Sequence[float]], float],
                  n_boot: int,
                  alpha: float,
                  rng: random.Random) -> Tuple[float, float, float]:
    """Percentile bootstrap CI (Efron-Tibshirani 1993, Ch. 13)."""
    n = len(values)
    theta_hat = statistic(values)
    if n < 2:
        return theta_hat, theta_hat, theta_hat
    boot: List[float] = []
    for _ in range(n_boot):
        sample = [values[rng.randrange(n)] for _ in range(n)]
        boot.append(statistic(sample))
    boot.sort()
    lo = _percentile_sorted(boot, alpha / 2)
    hi = _percentile_sorted(boot, 1 - alpha / 2)
    return theta_hat, lo, hi


def bca_ci(values: Sequence[float],
           statistic: Callable[[Sequence[float]], float],
           n_boot: int,
           alpha: float,
           rng: random.Random) -> Tuple[float, float, float]:
    """BCa bootstrap CI (Efron-Tibshirani 1993, Ch. 14).

    Bias correction:
        z_0 = Phi^-1( #{boot < theta_hat} / n_boot )

    Acceleration via jackknife:
        a = sum((jack_mean - x_jack_i)^3)
            / (6 * (sum((jack_mean - x_jack_i)^2))^1.5)

    Adjusted percentile lookup:
        alpha_lo = Phi(z_0 + (z_0 + z_{alpha/2}) / (1 - a (z_0 + z_{alpha/2})))
        alpha_hi = Phi(z_0 + (z_0 + z_{1-alpha/2}) / (1 - a (z_0 + z_{1-alpha/2})))
    """
    n = len(values)
    theta_hat = statistic(values)
    if n < 2:
        return theta_hat, theta_hat, theta_hat

    boot: List[float] = []
    for _ in range(n_boot):
        sample = [values[rng.randrange(n)] for _ in range(n)]
        boot.append(statistic(sample))
    boot.sort()

    # Bias-correction term z_0.
    n_less = sum(1 for b in boot if b < theta_hat)
    if n_less == 0:
        p0 = 0.5 / n_boot
    elif n_less == n_boot:
        p0 = 1.0 - 0.5 / n_boot
    else:
        p0 = n_less / n_boot
    z0 = _norm_ppf(p0)

    # Acceleration via jackknife (Efron-Tibshirani 1993, eq. 14.15).
    jack: List[float] = []
    for i in range(n):
        leave_one = list(values[:i]) + list(values[i + 1:])
        jack.append(statistic(leave_one))
    jack_mean = sum(jack) / n
    num = sum((jack_mean - v) ** 3 for v in jack)
    den = 6.0 * (sum((jack_mean - v) ** 2 for v in jack)) ** 1.5
    a = num / den if den > 0 else 0.0

    z_lo = _norm_ppf(alpha / 2)
    z_hi = _norm_ppf(1 - alpha / 2)

    def _adjust(z: float) -> float:
        denom = 1.0 - a * (z0 + z)
        if denom == 0.0:
            denom = 1e-12
        return _norm_cdf(z0 + (z0 + z) / denom)

    p_lo = _adjust(z_lo)
    p_hi = _adjust(z_hi)
    lo = _percentile_sorted(boot, p_lo)
    hi = _percentile_sorted(boot, p_hi)
    return theta_hat, lo, hi


# ---------------------------------------------------------------------------
# Input loaders
# ---------------------------------------------------------------------------

def _load_json(stream) -> List[float]:
    data = json.load(stream)
    if not isinstance(data, list):
        raise ValueError("JSON input must be an array of numbers")
    out: List[float] = []
    for v in data:
        if isinstance(v, (int, float)) and not isinstance(v, bool):
            out.append(float(v))
        elif v is None:
            continue
        else:
            raise ValueError(f"non-numeric value in JSON array: {v!r}")
    return out


def _load_csv(path: str, column: str) -> List[float]:
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
            try:
                out.append(float(raw))
            except ValueError as exc:
                raise ValueError(
                    f"non-numeric value in column {column!r}: {raw!r}") from exc
    return out


def _load_values(args: argparse.Namespace) -> List[float]:
    if args.input is None:
        return _load_json(sys.stdin)
    if args.input.endswith(".csv"):
        if args.column is None:
            raise SystemExit("--column required for CSV input")
        return _load_csv(args.input, args.column)
    with open(args.input) as fh:
        return _load_json(fh)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Bootstrap 95% confidence intervals (percentile or BCa). "
            "References: Efron & Tibshirani 1993, ch. 13 (percentile) and "
            "ch. 14 (BCa)."
        )
    )
    parser.add_argument("--input", help="path to JSON array or CSV file; "
                        "if omitted, read JSON from stdin")
    parser.add_argument("--column", help="CSV column name (required for CSV)")
    parser.add_argument("--method", choices=["percentile", "bca"],
                        default="percentile",
                        help="CI construction method (default: percentile)")
    parser.add_argument("--statistic", default="mean",
                        choices=sorted(STATISTICS.keys()),
                        help="point statistic to bootstrap (default: mean)")
    parser.add_argument("--n-resamples", type=int, default=10000,
                        help="number of bootstrap resamples (default: 10000)")
    parser.add_argument("--alpha", type=float, default=0.05,
                        help="two-sided alpha (default: 0.05 → 95%% CI)")
    parser.add_argument("--seed", type=int, default=42,
                        help="resample RNG seed (default: 42)")
    args = parser.parse_args()

    values = _load_values(args)
    if not values:
        raise SystemExit("no numeric observations supplied")

    statistic = STATISTICS[args.statistic]
    rng = random.Random(args.seed)

    if args.method == "bca":
        point, lo, hi = bca_ci(values, statistic, args.n_resamples,
                               args.alpha, rng)
    else:
        point, lo, hi = percentile_ci(values, statistic, args.n_resamples,
                                      args.alpha, rng)

    json.dump({
        "point": point,
        "ci_lo": lo,
        "ci_hi": hi,
        "method": args.method,
        "statistic": args.statistic,
        "n_resamples": args.n_resamples,
        "n_obs": len(values),
        "alpha": args.alpha,
        "seed": args.seed,
        "citation": (
            "Efron & Tibshirani 1993, An Introduction to the Bootstrap, "
            + ("Chapter 14 (BCa)" if args.method == "bca"
               else "Chapter 13 (percentile)")
        ),
    }, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
