# samkhya-wasm

**A provable join-cardinality ceiling, and the sketches behind it, in JavaScript.**

A Rust engine inside, a plain JS API outside. No server, no native module, no
build step for the consumer — an 84 KB WebAssembly binary with generated
TypeScript definitions.

```bash
npm install samkhya
```

```js
import init, { HllSketch, joinCeiling } from 'samkhya';
await init();

const hll = new HllSketch(12);
for (const row of rows) hll.add(row.orderKey);

// 10 orders joined to 100 line items over 10 distinct keys.
joinCeiling([10, 100], [0, 1], [10, 10]);   // 100 — exactly the true output
joinCeiling([10, 100], [0, 1], []);         // 1000 — the Cartesian product
```

## What this is

`joinCeiling` is not an estimate. It returns a number the join provably cannot
exceed, derived from row counts and distinct counts by a spanning-tree degree
bound. On foreign-key joins — the shape that dominates analytical workloads —
it is exactly tight. The theorem and its brute-force verification are in
[`bench-results/20_bound_soundness.md`](../bench-results/20_bound_soundness.md).

The sketches use the same byte format as the Rust and Python packages, so a
sketch built in a browser deserialises unchanged in a Rust query engine.

## The one thing to get right

`joinCeiling` derives a degree bound as `rows - distinct + 1`, so it
**subtracts** the distinct count. A count above the truth produces a ceiling
*below* it, which defeats the point.

`HllSketch.estimate()` is two-sided — it exceeds the truth about half the time.
Pass `HllSketch.distinctFloor()` instead, which counts non-zero registers and
therefore can never be above the truth.

```js
joinCeiling(rows, edges, [hll.distinctFloor()]);   // sound
joinCeiling(rows, edges, [hll.estimate()]);        // not necessarily
```

## API

| Export | What it does |
| ------ | ------------ |
| `HllSketch` | Distinct counts. `estimate()` (two-sided), `distinctFloor()` (never above the truth), `merge`, `toBytes`/`fromBytes`. |
| `CountMinSketch` | Frequencies. `estimate()` never undercounts unless `isSaturated()`; `maxFrequencyBound()` bounds the hottest key without knowing which it is. |
| `BloomFilter` | Membership. False positives possible, false negatives not. |
| `joinCeiling(rows, edges, distinctCounts)` | The provable ceiling. `edges` is flattened pairs: `[0,1, 1,2]` is a three-way chain. |
| `productBound(rows)` | The Cartesian product — the ceiling that holds when nothing is known. |

## What is not here

Corrector training and the feedback store. Those need SQLite, which is not
available on `wasm32`. Train with the Rust or Python tooling; this package is
for computing statistics and bounds where the data already is.

## Building

```bash
wasm-pack build --target bundler --out-dir pkg --release   # browsers/bundlers
wasm-pack build --target nodejs  --out-dir pkg-node --release
```

Apache-2.0. Sole author: Prateek Singh.
