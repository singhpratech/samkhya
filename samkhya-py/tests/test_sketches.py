"""Smoke tests for the samkhya Python bindings.

Each test exercises one public class or function end-to-end (construct,
mutate, query, round-trip) so a broken build fails loudly. The Rust core
has its own exhaustive test suite; these are integration probes for the
PyO3 surface.
"""

from __future__ import annotations

import importlib.metadata

import pytest
import samkhya


def test_public_module_contract() -> None:
    """The facade must expose version metadata and its dedicated error type."""
    assert samkhya.__version__ == samkhya.samkhya_version()
    assert issubclass(samkhya.SamkhyaError, Exception)
    with pytest.raises(samkhya.SamkhyaError):
        samkhya.HllSketch(3)


def test_installed_wheel_version_matches_native_module() -> None:
    """Wheel metadata, Python facade, and Cargo-derived native version must agree."""
    assert importlib.metadata.version("samkhya") == samkhya.__version__
    assert samkhya.__version__ == samkhya.samkhya_version()


def test_serializers_return_builtin_bytes() -> None:
    """Every sketch serializer must return Python's built-in bytes type."""
    sketches = [
        samkhya.HllSketch(14),
        samkhya.BloomFilter(10, 0.01),
        samkhya.CountMinSketch(16, 2),
        samkhya.EquiDepthHistogram([0.0, 1.0, 2.0], [1, 1]),
    ]
    assert all(type(sketch.to_bytes()) is bytes for sketch in sketches)


def test_hll_estimates_within_relative_error() -> None:
    """HLL estimate should be within ~3% of the true cardinality."""
    hll = samkhya.HllSketch(14)
    n = 10_000
    for i in range(n):
        hll.add(str(i).encode())
    est = hll.estimate()
    rel_err = abs(est - n) / n
    assert rel_err < 0.03, f"relative error {rel_err:.4f} exceeds 3%"


def test_hll_round_trip_and_merge() -> None:
    """Serialize/deserialize must produce identical estimates; merge must union."""
    hll = samkhya.HllSketch(14)
    for i in range(5_000):
        hll.add(str(i).encode())

    # round-trip
    data = hll.to_bytes()
    hll2 = samkhya.HllSketch.from_bytes(data)
    assert hll.estimate() == hll2.estimate()

    # merge: add non-overlapping elements and expect estimate to grow
    hll3 = samkhya.HllSketch(14)
    for i in range(5_000, 10_000):
        hll3.add(str(i).encode())
    before = hll2.estimate()
    hll2.merge(hll3)
    assert hll2.estimate() > before


def test_bloom_no_false_negatives_and_round_trip() -> None:
    """Inserted items must always be found; round-trip must preserve membership."""
    bf = samkhya.BloomFilter(1_000, 0.01)
    items = [f"key-{i}".encode() for i in range(500)]
    for item in items:
        bf.add(item)
    for item in items:
        assert bf.contains(item), f"{item!r} not found after insert"

    # round-trip
    data = bf.to_bytes()
    bf2 = samkhya.BloomFilter.from_bytes(data)
    for item in items:
        assert bf2.contains(item), f"{item!r} missing after round-trip"


def test_count_min_never_undercounts_and_round_trip() -> None:
    """CMS estimate must be >= exact count; round-trip must preserve estimates."""
    cms = samkhya.CountMinSketch(4096, 5)
    exact: dict[bytes, int] = {}
    for i in range(1_000):
        key = f"item-{i % 100}".encode()
        cnt = (i % 7) + 1
        cms.add(key, cnt)
        exact[key] = exact.get(key, 0) + cnt

    for key, true_cnt in exact.items():
        est = cms.estimate(key)
        assert est >= true_cnt, f"{key!r}: est {est} < true {true_cnt}"

    # round-trip
    data = cms.to_bytes()
    cms2 = samkhya.CountMinSketch.from_bytes(data)
    for key, true_cnt in exact.items():
        assert cms2.estimate(key) >= true_cnt


def test_histogram_range_estimate_and_round_trip() -> None:
    """EquiDepthHistogram must return non-negative range estimates and survive round-trip."""
    boundaries = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0]
    counts = [100, 150, 120, 80, 90]
    hist = samkhya.EquiDepthHistogram(boundaries, counts)
    assert hist.total == sum(counts)
    assert hist.buckets == len(counts)
    est = hist.range_estimate(10.0, 30.0)
    assert est > 0

    # round-trip
    data = hist.to_bytes()
    hist2 = samkhya.EquiDepthHistogram.from_bytes(data)
    assert hist.range_estimate(10.0, 30.0) == hist2.range_estimate(10.0, 30.0)


def test_product_bound_matches_simple_product() -> None:
    """product_bound([a, b, c]) should equal a * b * c as float."""
    result = samkhya.product_bound([10.0, 100.0, 1_000.0])
    assert result == pytest.approx(1_000_000.0)


def test_agm_bound_is_tighter_than_product_with_selectivity() -> None:
    """AGM bound with selectivity < 1 should be <= product bound."""
    cards = [1_000.0, 2_000.0]
    joins = [(0, 1, 0.01)]
    agm = samkhya.agm_bound(joins, cards)
    product = samkhya.product_bound(cards)
    assert agm <= product, f"AGM {agm} > product {product}"
    assert agm >= 0.0
