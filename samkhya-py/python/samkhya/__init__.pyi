"""Type stubs for the ``samkhya`` package.

These declarations describe the public surface re-exported from the
native PyO3 extension at ``samkhya._native``. They are kept in sync with
``src/lib.rs`` by hand; if the Rust signatures change, update this file
to match.
"""

from __future__ import annotations

from typing import List, Tuple, Type, TypeVar

__version__: str

class SamkhyaError(Exception):
    """Recoverable error raised by the samkhya core (invalid sketch
    parameters, serialization failures, etc.)."""

# -- Sketches ----------------------------------------------------------------

_H = TypeVar("_H", bound="HllSketch")
_B = TypeVar("_B", bound="BloomFilter")
_C = TypeVar("_C", bound="CountMinSketch")
_E = TypeVar("_E", bound="EquiDepthHistogram")

class HllSketch:
    """HyperLogLog cardinality sketch."""

    precision: int

    def __init__(self, p: int) -> None: ...
    def add(self, item: bytes) -> None: ...
    def estimate(self) -> float: ...
    def merge(self, other: "HllSketch") -> None: ...
    def to_bytes(self) -> bytes: ...
    @classmethod
    def from_bytes(cls: Type[_H], data: bytes) -> _H: ...
    def __repr__(self) -> str: ...

class BloomFilter:
    """Bloom filter sized for `n_items` at the given false-positive rate."""

    num_bits: int
    num_hashes: int

    def __init__(self, n_items: int, fp_rate: float) -> None: ...
    def add(self, item: bytes) -> None: ...
    def contains(self, item: bytes) -> bool: ...
    def to_bytes(self) -> bytes: ...
    @classmethod
    def from_bytes(cls: Type[_B], data: bytes) -> _B: ...
    def __repr__(self) -> str: ...

class CountMinSketch:
    """Count-Min Sketch for frequency estimation of skewed values."""

    width: int
    depth: int
    total: int

    def __init__(self, width: int, depth: int) -> None: ...
    def add(self, item: bytes, count: int) -> None: ...
    def estimate(self, item: bytes) -> int: ...
    def to_bytes(self) -> bytes: ...
    @classmethod
    def from_bytes(cls: Type[_C], data: bytes) -> _C: ...
    def __repr__(self) -> str: ...

class EquiDepthHistogram:
    """Equi-depth histogram constructed from explicit (boundaries, counts)."""

    total: int
    buckets: int

    def __init__(self, boundaries: List[float], counts: List[int]) -> None: ...
    def range_estimate(self, low: float, high: float) -> int: ...
    def to_bytes(self) -> bytes: ...
    @classmethod
    def from_bytes(cls: Type[_E], data: bytes) -> _E: ...
    def __repr__(self) -> str: ...

# -- LpBound helpers ---------------------------------------------------------

def product_bound(card_estimates: List[float]) -> float:
    """Trivial Cartesian-product upper bound: ``prod(card_estimates)``."""

def agm_bound(
    joins: List[Tuple[int, int, float]],
    card_estimates: List[float],
) -> float:
    """Selectivity-weighted AGM upper bound for an equi-join graph.

    Each entry in ``joins`` is ``(left_idx, right_idx, predicate_selectivity)``
    where the indices reference positions in ``card_estimates``.
    """

def samkhya_version() -> str:
    """Return the version string of the underlying samkhya crate."""
