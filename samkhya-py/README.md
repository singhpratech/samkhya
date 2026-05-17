# samkhya — Python bindings

> Portable, feedback-driven cardinality correction for embedded analytical
> engines (DuckDB, Polars, DataFusion, gpudb).

This wheel exposes [samkhya-core](../samkhya-core)'s classical sketches
(HyperLogLog, Bloom, Count-Min, equi-depth histogram) and its LpBound
ceiling helpers to Python, with no Rust toolchain required at install
time.

Built on top of [PyO3](https://pyo3.rs/) with a stable-ABI (`abi3-py39`)
wheel — one wheel per platform serves every CPython 3.9+ interpreter.

## Install

```bash
pip install samkhya
```

For a from-source build of this directory:

```bash
pip install maturin
maturin develop --release            # editable install into the current venv
maturin build --release              # produce a redistributable wheel
```

## Quickstart — count 1000 distinct items, get back ~42 for a small set

```python
import samkhya

# Precision 14 gives 2^14 = 16384 registers; relative error ~ 0.8%.
hll = samkhya.HllSketch(14)
for i in range(1000):
    hll.add(str(i).encode("utf-8"))

print(f"~1000 → {hll.estimate():.0f}")

# A second sketch over the first 42 distinct items returns ~42.
small = samkhya.HllSketch(14)
for i in range(42):
    small.add(str(i).encode("utf-8"))
print(f"~42 → {small.estimate():.0f}")

# Sketches are mergeable and serialisable for transport (e.g. Iceberg Puffin).
hll.merge(small)
payload: bytes = hll.to_bytes()
restored = samkhya.HllSketch.from_bytes(payload)
assert restored.estimate() == hll.estimate()
```

The same API style applies to `BloomFilter`, `CountMinSketch`, and
`EquiDepthHistogram` — see the type stubs in
[`python/samkhya/__init__.pyi`](python/samkhya/__init__.pyi) for full
signatures.

## LpBound — keep corrected estimates honest

Every corrected cardinality estimate samkhya emits is clamped from above
by a provable pessimistic ceiling derived from the AGM /
fractional-edge-cover bound (Atserias–Grohe–Marx; extended to ℓp-norms
by Zhang et al., SIGMOD 2025 Best Paper). The Python wheel exposes two
ceiling helpers that operate on plain row-count and selectivity inputs:

```python
import samkhya

# Cartesian-product safety floor for three relations.
print(samkhya.product_bound([1_000, 2_000, 3_000]))   # 6_000_000_000.0

# Selectivity-weighted AGM ceiling for an equi-join graph.
# joins: list of (left_idx, right_idx, predicate_selectivity)
rows = [1_000_000, 1_000_000]
joins = [(0, 1, 1e-5)]
print(samkhya.agm_bound(joins, rows))                 # ~ 10_000.0
```

`product_bound` is the trivial worst case; `agm_bound` collapses the
ceiling using the supplied predicate selectivities. Cold-start plans are
always either the native estimate or the ceiling — whichever is tighter
— and never degrade below baseline.

## Errors

Recoverable errors from the core (out-of-range sketch parameters, malformed
serialised payloads, etc.) surface as `samkhya.SamkhyaError`, a subclass
of the built-in `Exception`:

```python
try:
    samkhya.HllSketch(3)               # precision must be in [4, 18]
except samkhya.SamkhyaError as exc:
    print("rejected:", exc)
```

## License

Apache-2.0. See the [workspace README](../README.md) for the broader
samkhya project layout and the Rust crate documentation.
