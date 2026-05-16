# samkhya — Python bindings

Python bindings for [samkhya](https://github.com/singhpratech/samkhya): portable,
feedback-driven cardinality correction primitives for embedded analytical engines
(DuckDB, Polars, DataFusion, gpudb).

Built on top of `samkhya-core` (Rust) via [PyO3](https://pyo3.rs/) with a
stable-ABI (`abi3-py39`) wheel — one wheel per platform, all CPython 3.9+ versions.

## Install

```bash
# Development build (from source)
pip install maturin
maturin develop --release --manifest-path samkhya-py/Cargo.toml

# Or build a wheel
maturin build --release --manifest-path samkhya-py/Cargo.toml
```

## Quick usage

### HyperLogLog — distinct counting

```python
import samkhya

hll = samkhya.HllSketch(precision=14)  # 2^14 = 16,384 registers
for i in range(10_000):
    hll.add(str(i).encode("utf-8"))

print(hll.estimate())          # ≈ 10000 (relative error < 1%)
print(hll.precision)           # 14

# Round-trip through Puffin / any byte transport
payload = hll.to_bytes()
restored = samkhya.HllSketch.from_bytes(payload)
assert restored.estimate() == hll.estimate()

# Merge sketches across partitions
other = samkhya.HllSketch(precision=14)
for i in range(10_000, 20_000):
    other.add(str(i).encode("utf-8"))
hll.merge(other)
print(hll.estimate())          # ≈ 20000
```

### Bloom filter — set membership

```python
import samkhya

bf = samkhya.BloomFilter(capacity=10_000, fp_rate=0.01)
for i in range(10_000):
    bf.insert(i.to_bytes(4, "little"))

assert bf.contains((42).to_bytes(4, "little"))         # True
assert not bf.contains((99_999).to_bytes(4, "little")) # almost certainly False

print(bf.num_bits, bf.num_hashes)

payload = bf.to_bytes()
restored = samkhya.BloomFilter.from_bytes(payload)
```

### Column statistics

```python
import samkhya

stats = (
    samkhya.ColumnStats()
    .with_row_count(1_000_000)
    .with_distinct_count(42_000)
    .with_null_count(120)
    .with_upper_bound(50_000)   # LpBound-style provable ceiling
)
print(stats)
```

## Error handling

All recoverable errors from the Rust core surface as `samkhya.SamkhyaError`
(a subclass of `Exception`):

```python
try:
    samkhya.HllSketch(precision=3)   # out of [4, 18]
except samkhya.SamkhyaError as e:
    print("rejected:", e)
```

## License

Apache-2.0.
