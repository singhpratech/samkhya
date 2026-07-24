"""Type stubs for the ``samkhya`` package.

These declarations describe the public surface re-exported from the
native PyO3 extension at ``samkhya._native``. They are kept in sync with
``src/lib.rs`` by hand; if the Rust signatures change, update this file
to match.
"""

from __future__ import annotations

from typing import List, Optional, Tuple, Type, TypeVar

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
    """Provable upper bound for an equi-join graph.

    Each entry in ``joins`` is ``(left_idx, right_idx, predicate_selectivity)``
    where the indices reference positions in ``card_estimates``.

    Changed in 1.2.0: the selectivity field is ignored. Multiplying a ceiling
    by a selectivity in ``[0, 1]`` can only shrink it, which destroys the
    upper-bound property. Use :func:`selectivity_estimate` for the old value
    (an estimate, not a bound) or :func:`join_ceiling` for a bound that is
    provable *and* tighter than the Cartesian product.
    """

def selectivity_estimate(
    joins: List[Tuple[int, int, float]],
    card_estimates: List[float],
) -> float:
    """System-R-style selectivity-weighted cardinality *estimate*.

    The pre-1.2 behaviour of :func:`agm_bound`, under a name that says what it
    is. This can land below the true cardinality; never clamp a corrector to it.
    """

def join_ceiling(
    joins: List[Tuple[int, int]],
    card_estimates: List[float],
    distinct_counts: Optional[List[float]] = None,
) -> float:
    """Provable join ceiling from row counts and distinct-value counts.

    ``distinct_counts`` gives the number of distinct join-key values per
    relation; entries that are zero, missing, or larger than the row count
    degrade safely to "no degree information" rather than an unsound value.

    On a foreign-key join of 10 orders to 100 line items over 10 distinct keys
    this returns exactly 100, where the Cartesian product returns 1000.
    """

def samkhya_version() -> str:
    """Return the version string of the underlying samkhya crate."""
