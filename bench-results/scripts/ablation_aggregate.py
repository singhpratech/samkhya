#!/usr/bin/env python3
"""ablation_aggregate.py — turn `15_ablation_raw.json` into a measured
per-layer q-error reduction report.

Inputs:  bench-results/15_ablation_raw.json  (output of ablation_runner).
Outputs: prints a markdown-formatted results table to stdout; also writes
         bench-results/15_ablation_summary.json with the structured numbers.

Statistical method:

  - Per-(ablation × query) median q-error is the primary unit.
  - Marginal Δ% per transition Ai-1 → Ai is `(med_i - med_{i-1}) / med_{i-1}`
    computed on a *log10-q-error* scale (so infinite q-errors can be
    handled via a configurable cap; we cap at log10(q)=6, i.e. q=1e6).
  - 95% confidence intervals on the workload-aggregate (median across
    queries) Δ% via BCa bootstrap (Efron & Tibshirani 1993): 10,000
    paired resamples over the per-query median pairs. Acceleration
    constant `a` is estimated by jackknife over queries.
  - Paired Wilcoxon signed-rank test (Wilcoxon 1945) on the per-query
    median log-q-error pairs.
  - Multiple-comparison correction: Benjamini-Hochberg FDR at α=0.05
    across the 4 transitions (Benjamini & Hochberg JRSSB 1995).

Citations carried forward into the receipt and 15_ablation_layers.md.
"""

from __future__ import annotations

import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Tuple


# ---------------------------------------------------------------------------
# Inputs
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "15_ablation_raw.json"
SUMMARY = ROOT / "15_ablation_summary.json"

# When invoked as
#   `python3 ablation_aggregate.py < raw.json > summary.json`
# we read raw records from stdin and emit the structured JSON to stdout
# (the markdown table goes to stderr). When invoked plainly the legacy
# behaviour is preserved (read from `RAW`, write structured JSON to
# `SUMMARY`, markdown table to stdout). Added in Wave-4 so the V2
# re-aggregation can be wired through the receipt's pipeline.

INF_CAP_LOG10 = 6.0   # treat q-error == inf as 1e6 (very large miss)
N_BOOT = 10_000
ALPHA = 0.05


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def load_raw(path: Path) -> List[dict]:
    return json.loads(path.read_text())


def to_log10_qerror(q: float | None) -> float:
    """Map q-error to log10 space, capping infs/None at INF_CAP_LOG10."""
    if q is None or q != q or q == float("inf"):
        return INF_CAP_LOG10
    if q < 1.0:
        return 0.0
    capped = min(q, 10 ** INF_CAP_LOG10)
    return math.log10(capped)


def percentile(sorted_vals: List[float], p: float) -> float:
    if not sorted_vals:
        return float("nan")
    k = (len(sorted_vals) - 1) * p
    f = int(math.floor(k))
    c = int(math.ceil(k))
    if f == c:
        return sorted_vals[f]
    return sorted_vals[f] + (sorted_vals[c] - sorted_vals[f]) * (k - f)


def median(vals: List[float]) -> float:
    if not vals:
        return float("nan")
    s = sorted(vals)
    n = len(s)
    if n % 2 == 1:
        return s[n // 2]
    return 0.5 * (s[n // 2 - 1] + s[n // 2])


# ---------------------------------------------------------------------------
# BCa bootstrap (Efron & Tibshirani 1993, §14.3)
# ---------------------------------------------------------------------------

def bca_ci(values: List[float], statistic, n_boot: int, alpha: float, rng) -> Tuple[float, float, float]:
    """Return (point estimate, lower CI, upper CI) for the supplied
    statistic on `values` using a BCa bootstrap with `n_boot` resamples.

    `statistic` takes a list of floats and returns a float.
    """
    n = len(values)
    if n < 2:
        v = statistic(values) if values else float("nan")
        return v, v, v

    theta_hat = statistic(values)

    boot = []
    for _ in range(n_boot):
        sample = [values[rng.randrange(n)] for _ in range(n)]
        boot.append(statistic(sample))
    boot.sort()

    # Bias-correction term z0.
    n_less = sum(1 for b in boot if b < theta_hat)
    if n_less == 0:
        p0 = 0.5 / n_boot
    elif n_less == n_boot:
        p0 = 1 - 0.5 / n_boot
    else:
        p0 = n_less / n_boot
    z0 = _norm_ppf(p0)

    # Acceleration `a` via jackknife.
    jack = []
    for i in range(n):
        leave_one = values[:i] + values[i + 1:]
        jack.append(statistic(leave_one))
    jack_mean = sum(jack) / n
    num = sum((jack_mean - v) ** 3 for v in jack)
    den = 6 * (sum((jack_mean - v) ** 2 for v in jack)) ** 1.5
    a = num / den if den > 0 else 0.0

    z_lo = _norm_ppf(alpha / 2)
    z_hi = _norm_ppf(1 - alpha / 2)
    # Adjusted percentiles.
    def adjust(z):
        denom = 1 - a * (z0 + z)
        if denom == 0:
            denom = 1e-12
        return _norm_cdf(z0 + (z0 + z) / denom)

    p_lo = adjust(z_lo)
    p_hi = adjust(z_hi)
    lo = percentile(boot, p_lo)
    hi = percentile(boot, p_hi)
    return theta_hat, lo, hi


def _norm_ppf(p: float) -> float:
    """Inverse standard-normal CDF via the Beasley-Springer-Moro algorithm
    (good to ~1e-9 over (1e-12, 1-1e-12))."""
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
        q = math.sqrt(-2 * math.log(p))
        return (((((c[0]*q + c[1])*q + c[2])*q + c[3])*q + c[4])*q + c[5]) / \
               ((((d[0]*q + d[1])*q + d[2])*q + d[3])*q + 1)
    if p <= phigh:
        q = p - 0.5
        r = q*q
        return (((((a[0]*r + a[1])*r + a[2])*r + a[3])*r + a[4])*r + a[5])*q / \
               (((((b[0]*r + b[1])*r + b[2])*r + b[3])*r + b[4])*r + 1)
    q = math.sqrt(-2 * math.log(1 - p))
    return -(((((c[0]*q + c[1])*q + c[2])*q + c[3])*q + c[4])*q + c[5]) / \
           ((((d[0]*q + d[1])*q + d[2])*q + d[3])*q + 1)


def _norm_cdf(x: float) -> float:
    return 0.5 * (1 + math.erf(x / math.sqrt(2)))


# ---------------------------------------------------------------------------
# Wilcoxon signed-rank (paired)
# ---------------------------------------------------------------------------

def wilcoxon_signed_rank(diffs: List[float]) -> Tuple[float, float]:
    """Two-sided p-value via normal approximation. Returns (W, p).

    `diffs` are the paired differences (b - a). Zeros are dropped (Pratt-style
    omission would keep them; the Wilcoxon 1945 original drops them, which
    matches `scipy.stats.wilcoxon(method='approx', zero_method='wilcox')`).
    """
    diffs = [d for d in diffs if d != 0]
    n = len(diffs)
    if n == 0:
        return 0.0, 1.0
    abs_diffs = sorted([(abs(d), 1 if d > 0 else -1) for d in diffs])
    # Assign ranks with average-rank ties.
    ranks = [0.0] * n
    i = 0
    while i < n:
        j = i
        while j + 1 < n and abs_diffs[j + 1][0] == abs_diffs[i][0]:
            j += 1
        avg = 0.5 * (i + j) + 1  # ranks are 1-indexed; mid-rank
        for k in range(i, j + 1):
            ranks[k] = avg
        i = j + 1
    w_plus = sum(r for r, (_, sign) in zip(ranks, abs_diffs) if sign > 0)
    w_minus = sum(r for r, (_, sign) in zip(ranks, abs_diffs) if sign < 0)
    w = min(w_plus, w_minus)
    mu = n * (n + 1) / 4.0
    sigma = math.sqrt(n * (n + 1) * (2 * n + 1) / 24.0)
    if sigma == 0:
        return w, 1.0
    z = (w - mu) / sigma
    p = 2 * (1 - _norm_cdf(abs(z)))
    return w, max(min(p, 1.0), 0.0)


# ---------------------------------------------------------------------------
# Benjamini-Hochberg FDR (Benjamini & Hochberg JRSSB 1995, §3)
# ---------------------------------------------------------------------------

def benjamini_hochberg(pvals: List[float], alpha: float) -> List[bool]:
    """Returns one boolean per input p-value: True if rejected (significant)
    under BH at level alpha."""
    m = len(pvals)
    indexed = sorted(enumerate(pvals), key=lambda t: t[1])
    rejected = [False] * m
    max_k = -1
    for rank, (_, p) in enumerate(indexed, start=1):
        if p <= rank / m * alpha:
            max_k = rank
    if max_k >= 0:
        for rank, (orig_idx, _) in enumerate(indexed, start=1):
            if rank <= max_k:
                rejected[orig_idx] = True
    return rejected


# ---------------------------------------------------------------------------
# Aggregation
# ---------------------------------------------------------------------------

def per_query_medians(records: List[dict]) -> Dict[str, Dict[str, float]]:
    """Returns {ablation: {query: median_log10_qerror}}."""
    bucket: Dict[str, Dict[str, List[float]]] = defaultdict(lambda: defaultdict(list))
    for r in records:
        bucket[r["ablation"]][r["query"]].append(to_log10_qerror(r["q_error"]))
    out: Dict[str, Dict[str, float]] = {}
    for ab, by_q in bucket.items():
        out[ab] = {q: median(vs) for q, vs in by_q.items()}
    return out


def inf_counts(records: List[dict]) -> Dict[str, int]:
    c: Dict[str, int] = defaultdict(int)
    for r in records:
        if r["q_error"] is None or (isinstance(r["q_error"], float) and r["q_error"] != r["q_error"]):
            c[r["ablation"]] += 1
    return dict(c)


def main(argv: List[str]) -> int:
    # Wave-4: detect stdin-piped invocation. The runner script for the
    # V2 retrain is
    #   python3 ablation_aggregate.py < raw_v2.json > summary_v2.json
    # which we honour by routing structured JSON to stdout and the
    # markdown table to stderr. When stdin is a TTY (normal interactive
    # use) we fall back to the legacy disk-path behaviour.
    stdin_piped = not sys.stdin.isatty()
    if stdin_piped:
        raw = json.loads(sys.stdin.read())
    else:
        raw = load_raw(RAW)
    medians = per_query_medians(raw)
    infs = inf_counts(raw)

    abls = ["A0", "A1", "A2", "A3", "A4"]
    queries = sorted({r["query"] for r in raw})

    # ------------------------------------------------------------------
    # Per-ablation aggregate: median q-error (back-transformed from log10).
    # ------------------------------------------------------------------
    import random
    rng = random.Random(20260516)

    headline_rows = []
    for ab in abls:
        if ab not in medians:
            continue
        log_vals = [medians[ab][q] for q in queries if q in medians[ab]]
        # Bootstrap the workload-median.
        point, lo, hi = bca_ci(log_vals, lambda xs: median(xs), N_BOOT, ALPHA, rng)
        q_point = 10 ** point
        q_lo = 10 ** lo
        q_hi = 10 ** hi
        # P95 q-error: extract per-record q-errors (capped) per ablation.
        per_rec_logq = [to_log10_qerror(r["q_error"]) for r in raw if r["ablation"] == ab]
        per_rec_logq.sort()
        p95 = 10 ** percentile(per_rec_logq, 0.95)
        headline_rows.append({
            "ablation": ab,
            "median_qerror": q_point,
            "ci_low": q_lo,
            "ci_high": q_hi,
            "p95_qerror": p95,
            "inf_count": infs.get(ab, 0),
            "n_finite_records": sum(1 for r in raw if r["ablation"] == ab and r["q_error"] is not None),
        })

    # ------------------------------------------------------------------
    # Marginal Δ% per transition (Ai-1 → Ai), with BCa CI on Δ in log10 space.
    # Also: Wilcoxon paired test on the per-query log10 deltas; BH-FDR.
    # ------------------------------------------------------------------
    transitions = []
    pvals_for_bh = []
    for i in range(1, len(abls)):
        ab_prev, ab_curr = abls[i - 1], abls[i]
        if ab_prev not in medians or ab_curr not in medians:
            continue
        # Paired per-query log10 deltas (curr - prev). Negative = improvement.
        deltas = []
        rel_deltas = []
        for q in queries:
            lp = medians[ab_prev].get(q)
            lc = medians[ab_curr].get(q)
            if lp is None or lc is None:
                continue
            deltas.append(lc - lp)
            if lp != 0:
                # Relative % change of q-error: (q_curr/q_prev - 1)*100 in linear space.
                rel = (10 ** lc) / (10 ** lp) - 1.0
            else:
                rel = (10 ** lc) - 1.0
            rel_deltas.append(rel * 100.0)
        point_log, lo_log, hi_log = bca_ci(
            deltas, lambda xs: statistics.mean(xs), N_BOOT, ALPHA, rng
        )
        # Convert the log10 delta back to a percent change in linear q-error.
        pct_point = (10 ** point_log - 1.0) * 100.0
        pct_lo = (10 ** lo_log - 1.0) * 100.0
        pct_hi = (10 ** hi_log - 1.0) * 100.0
        w_stat, p_val = wilcoxon_signed_rank(deltas)
        pvals_for_bh.append(p_val)
        transitions.append({
            "from": ab_prev,
            "to": ab_curr,
            "delta_pct_median_qerror": pct_point,
            "delta_pct_ci_low": pct_lo,
            "delta_pct_ci_high": pct_hi,
            "wilcoxon_W": w_stat,
            "wilcoxon_p": p_val,
        })

    bh_rejected = benjamini_hochberg(pvals_for_bh, ALPHA)
    for t, r in zip(transitions, bh_rejected):
        t["bh_significant_alpha_0.05"] = bool(r)

    summary = {
        "raw_records": len(raw),
        "ablations": abls,
        "n_queries": len(queries),
        "n_replicates_per_cell": (len(raw) // (len(abls) * len(queries))) if queries else 0,
        "inf_cap_log10": INF_CAP_LOG10,
        "n_bootstrap": N_BOOT,
        "alpha": ALPHA,
        "headline": headline_rows,
        "transitions": transitions,
        "citations": [
            "Efron & Tibshirani 1993 — An Introduction to the Bootstrap (BCa).",
            "Wilcoxon 1945 — Individual comparisons by ranking methods (signed-rank test).",
            "Benjamini & Hochberg 1995 — Controlling the FDR: a practical and powerful approach.",
            "Moerkotte, Neumann, Steidl 2009 — Preventing bad plans by bounding q-error.",
        ],
    }
    # Output routing:
    #  - stdin-piped invocation (Wave-4): summary JSON → stdout,
    #    markdown table → stderr.
    #  - interactive invocation (legacy): summary JSON → SUMMARY file,
    #    markdown table → stdout.
    md_lines: List[str] = []
    md_lines.append(f"# 15_ablation_layers summary (MEASURED, synthetic suite)")
    md_lines.append(f"# raw_records={summary['raw_records']}, queries={summary['n_queries']}, reps/cell={summary['n_replicates_per_cell']}, n_boot={N_BOOT}")
    md_lines.append("")
    md_lines.append("| Config | Median q-error | 95% BCa CI | P95 q-error | Inf-cases (capped) |")
    md_lines.append("|--------|----------------|------------|-------------|--------------------|")
    for row in headline_rows:
        md_lines.append(f"| {row['ablation']} | {row['median_qerror']:.3f} | "
              f"[{row['ci_low']:.3f}, {row['ci_high']:.3f}] | {row['p95_qerror']:.2f} | {row['inf_count']} |")
    md_lines.append("")
    md_lines.append("| Transition | Δ% median q-error | 95% BCa CI | Wilcoxon p | BH-FDR sig (α=0.05) |")
    md_lines.append("|------------|-------------------|------------|------------|---------------------|")
    for t in transitions:
        sig = "yes" if t["bh_significant_alpha_0.05"] else "no"
        md_lines.append(f"| {t['from']} → {t['to']} | {t['delta_pct_median_qerror']:+.1f}% | "
              f"[{t['delta_pct_ci_low']:+.1f}%, {t['delta_pct_ci_high']:+.1f}%] | "
              f"{t['wilcoxon_p']:.3g} | {sig} |")

    if stdin_piped:
        # Wave-4 mode: JSON to stdout, table to stderr.
        sys.stdout.write(json.dumps(summary, indent=2))
        sys.stdout.write("\n")
        for line in md_lines:
            print(line, file=sys.stderr)
    else:
        SUMMARY.write_text(json.dumps(summary, indent=2))
        for line in md_lines:
            print(line)
        print()
        print(f"Wrote structured summary: {SUMMARY.relative_to(ROOT.parent)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
