"""samkhya — portable, feedback-driven cardinality correction.

This package is a thin Python facade over the ``samkhya-core`` Rust crate.
The native extension (``samkhya._native``) is built by maturin and the
public surface is re-exported here so that ``import samkhya`` is the only
entry point users need.

The exported types and functions cover the four classical sketches
samkhya ships (HLL, Bloom, Count-Min, equi-depth histogram) plus the
Provable join-ceiling helpers (``join_ceiling``, ``product_bound``,
``agm_bound``) and the selectivity ``selectivity_estimate`` heuristic, that
keep corrected estimates honest.
"""

from __future__ import annotations

# The PyO3 module is compiled as ``samkhya._native`` (see ``[lib] name`` in
# Cargo.toml and ``module-name`` in pyproject.toml's ``[tool.maturin]``).
# Re-export its symbols here so users only need the package namespace.
from samkhya._native import (  # type: ignore[attr-defined]
    BloomFilter,
    CountMinSketch,
    EquiDepthHistogram,
    HllSketch,
    SamkhyaError,
    agm_bound,
    join_ceiling,
    product_bound,
    selectivity_estimate,
    samkhya_version,
)

__all__ = [
    "BloomFilter",
    "CountMinSketch",
    "EquiDepthHistogram",
    "HllSketch",
    "SamkhyaError",
    "__version__",
    "agm_bound",
    "join_ceiling",
    "product_bound",
    "selectivity_estimate",
    "samkhya_version",
]

__version__: str = samkhya_version()
