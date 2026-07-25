# samkhya — Python bindings

Python bindings for samkhya: four portable statistics sketches
(HyperLogLog, Bloom, Count-Min, equi-depth histogram) and a provable
join-cardinality ceiling — an upper bound the join provably cannot
exceed. Compiled Rust behind a stable-ABI (`abi3-py39`) wheel, so one wheel
serves every CPython 3.9+ interpreter on the platforms published —
currently `manylinux_2_34 x86_64`. Elsewhere pip falls back to the sdist,
which does need a Rust toolchain and maturin.

## Install

```bash
pip install samkhya
```

From a source checkout of this directory: `pip install maturin`, then
`maturin develop --release` (editable) or `maturin build --release`.

## Sketches

```python
import samkhya

# Precision 14 gives 2^14 = 16384 registers; relative error ~0.8%.
hll = samkhya.HllSketch(14)
for i in range(1000):
    hll.add(str(i).encode("utf-8"))
print(round(hll.estimate()))          # ~1000

# HLL sketches merge, and every sketch serialises for transport
# (e.g. an Iceberg Puffin blob).
second = samkhya.HllSketch(14)      # same precision, or merge raises
second.add(b"1001")
hll.merge(second)
restored = samkhya.HllSketch.from_bytes(hll.to_bytes())
assert restored.estimate() == hll.estimate()
```

`BloomFilter(n_items, fp_rate)`, `CountMinSketch(width, depth)`, and
`EquiDepthHistogram(boundaries, counts)` share the `to_bytes` /
`from_bytes` shape. `merge` is bound only on `HllSketch`. Full signatures are in the type stubs:
https://github.com/singhpratech/samkhya/blob/main/samkhya-py/python/samkhya/__init__.pyi

## The join ceiling

`join_ceiling` computes a spanning-tree degree ceiling: sound for bag
semantics, and exactly tight on foreign-key joins.

```python
import samkhya

rows = [10.0, 100.0]        # 10 orders, 100 line items
joins = [(0, 1)]            # relation 0 joins relation 1
distinct = [10.0, 10.0]     # 10 distinct order keys on both sides

print(samkhya.join_ceiling(joins, rows, distinct))  # 100.0 — the true size
print(samkhya.product_bound(rows))                  # 1000.0
```

Without `distinct_counts` the ceiling degrades to the Cartesian product:
given only row counts and which pairs are joined, every row can carry the
same key value, so nothing below the product is provable.

**`distinct_counts` must be a lower bound on the true distinct count.**
The degree is derived as `rows - distinct + 1`, so an overstated distinct
count understates the degree and makes the ceiling unsound. Do not feed
it `HllSketch.estimate()`, which is two-sided and exceeds the truth about
half the time. From Python the sound source is an exact distinct count. Do **not**
derive it from a Count-Min sketch: Count-Min bounds *frequencies*, not
distinct values, so feeding it here yields `rows - maxfreq + 1`, which
understates the degree and produces exactly the unsound ceiling this
paragraph warns about. If you need a sketch-derived degree, use the Rust
API — `samkhya_core::degree::AttributeDegree::from_hll_floor` and
`from_count_min` produce degrees directly rather than values to pass
here. Entries that are zero,
larger than the row count, or absent degrade safely to "no degree
information" rather than to a wrong answer.

`distinct_counts` is indexed per relation, not per (relation, join
column): if a relation joins on several columns, pass the smallest count
among them, which overstates the degree and stays sound.

## Function reference

- `join_ceiling(joins, card_estimates, distinct_counts=None) -> float`
  The bound to use; `joins` is a list of `(left_idx, right_idx)`.
- `product_bound(card_estimates) -> float` — Cartesian product fallback.
- `agm_bound(joins, card_estimates) -> float` — compatibility shim. Its
  selectivity field is ignored since 1.2; it returns the product.
- `selectivity_estimate(joins, card_estimates) -> float` —
  `prod(card_estimates) * prod(clamped selectivities)`. An estimate, not
  a ceiling: it lands below the true cardinality routinely. Never clamp
  to it. (It is close in spirit to the pre-1.2 `agm_bound`, but not equal
  — that one applied a `min * max` shortcut this does not.)
- `samkhya_version() -> str` and `samkhya.__version__` — the underlying
  crate version.

## Changed in 1.2 — soundness fix

A 2026-07-24 audit found the bound family shipped through 1.1 was not
sound: it returned ceilings below the true cardinality in 2,179 of 3,704
measured bound-evaluations (58.8%), from multiplying a ceiling by
selectivities in `[0, 1]`, which can only shrink it. 1.2 replaces that
path with the degree ceiling above: 0 violations, same trials. Two
published headline numbers are withdrawn: a 40.95x bound-tightness
figure and a 1.038x JOB-Slow speedup.

## Errors

Malformed serialised payloads, a merge across mismatched precisions, and
out-of-range `HllSketch` / `CountMinSketch` parameters raise
`samkhya.SamkhyaError`, a subclass of `Exception`.

`BloomFilter` is the exception: out-of-range parameters are **clamped,
not rejected**. `BloomFilter(1000, 0.0)` returns a filter rather than
raising, because the Python binding wraps the infallible constructor.
Validate `fp_rate` yourself if it comes from user input.

## Scope

This wheel exposes the sketches and the ceiling functions, nothing else:
no query-engine integration, feedback store, or correction loop — those
live in the Rust crates at https://github.com/singhpratech/samkhya. The
theorem, its proof, and the full degree-source API are documented at
https://docs.rs/samkhya-core under `samkhya_core::degree`.

Licensed under Apache-2.0.
