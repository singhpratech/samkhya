"""samkhya — portable, feedback-driven cardinality correction.

This package is a thin Python facade over the ``samkhya-core`` Rust crate.
The native extension (``samkhya._samkhya``) is built by maturin and the
public surface is re-exported here so that ``import samkhya`` is the only
entry point users need.

The exported types and functions cover the four classical sketches
samkhya ships (HLL, Bloom, Count-Min, equi-depth histogram) plus the
LpBound-style ceiling helpers (``product_bound``, ``agm_bound``) that
keep corrected estimates honest.
"""

from __future__ import annotations

# The PyO3 module is compiled as ``samkhya`` itself (see ``[lib] name`` in
# Cargo.toml and ``module-name`` in pyproject.toml's ``[tool.maturin]``).
# Maturin places it on the Python search path as the package's compiled
# component, so we import its symbols by re-exporting from the package
# namespace once maturin has installed it.
from samkhya._native import (  # type: ignore[attr-defined]
    BloomFilter,
    CountMinSketch,
    EquiDepthHistogram,
    HllSketch,
    SamkhyaError,
    agm_bound,
    product_bound,
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
    "product_bound",
    "samkhya_version",
]

__version__: str = samkhya_version()
