# 20 — Bound-family soundness audit and repair

**Date:** 2026-07-24
**Author:** Prateek Singh (sole)
**Crate under test:** `samkhya-core` (`lp_solver` feature enabled)
**Source example:** `samkhya-core/examples/lpbound_tightness.rs` (instrumented in 1.2.0)
**Property suite:** `samkhya-core/tests/soundness_degree.rs`
**Hardware:** see `bench-results/00_hardware_profile.md`

---

## Verdict

**A ceiling is sound when, for every database instance consistent with the
statistics it was given, `ceiling >= |join output|`.** Measured against
materialised instances, three of the four bounds shipped through v1.1
were not sound. `ProductBound` was. The repaired family in 1.2.0 is.

| Bound | v1.1 violations | v1.2 violations |
| ----- | --------------: | --------------: |
| `ProductBound`  |   0 / 926 (0.0%) | 0 / 926 (0.0%) |
| `ChainBound`    | 889 / 926 (96.0%) | 0 / 926 (0.0%) |
| `AgmBound`      | 580 / 926 (62.6%) | 0 / 926 (0.0%) |
| `LpJoinBound`   | 710 / 926 (76.7%) | 0 / 926 (0.0%) |
| **Total**       | **2,179 / 3,704 (58.8%)** | **0 / 3,704 (0.0%)** |

1,080 trials (4 topologies × 3 sizes × 3 ℓ_p regimes × 30 trials); 154 trials
excluded because the materialised truth exceeds `u64::MAX`, where every bound
saturates and the comparison measures arithmetic rather than soundness.

**The 40.95× star-5 bound-tightness headline from file 07 is withdrawn.** Under
the corrected harness the same measurement is **1.070×**. The explanation is in
§3.

---

## 1. What was wrong

### 1.1 The information-theoretic root cause

`UpperBound::ceiling` takes per-relation **row counts** and a list of joined
relation **pairs**. That input does not determine anything below the Cartesian
product:

> Put every row of every relation on one single join-key value. The equi-join
> degenerates to a cross product, so the true output *is* the product of the row
> counts. Any ceiling below the product is therefore unsound on that instance.

Three of the four shipped bounds returned values below the product from that
input, so all three were unsound by construction.

### 1.2 `LpJoinBound` — the LP omitted private-attribute constraints

The fractional-edge-cover LP added one cover constraint per **predicate**
(`x_i + x_j >= 1`). The AGM bound requires one per **attribute**, and every
relation contributing a column no other relation covers must take a full unit of
cover weight. Those constraints were never added, so the LP happily assigned
weight 0 to relations whose columns nothing else could cover.

The defect is invisible on a triangle — where the per-predicate and per-attribute
constraint sets coincide — which is the shape the unit tests and doc examples
used. It is severe on precisely the shapes that dominate analytical workloads:

```
LpJoinBound::ceiling(&[10, 100], &[(0, 1)])  ->  10
```

A join of a 10-row table to a 100-row table on a foreign key returns 100 rows.
The "provable ceiling" was 10× below the truth.

### 1.3 `AgmBound` — `min × max` is not an AGM bound

`min(product, |R_min| · |R_max|)` drops every relation but two. Three 3-row
relations chained on one shared key value join to 27 rows; the shortcut returned
9.

### 1.4 `ChainBound` — an average-case estimate branded as a ceiling

Dividing the product by `max(D_i, D_j)` per predicate is a uniform-distribution
*estimate*. Under skew it lands below the truth: two 20-row relations with 5
distinct keys and 16 rows piled on one key join to 260 rows; the formula returned
80.

### 1.5 Why the existing tests passed

`tests/property_lpbound.rs` ran 1,024 cases per property and checked only
*relative* invariants — `AgmBound <= ProductBound`, `LpJoinBound <= AgmBound`,
finiteness, monotonicity. Those hold perfectly well for a family of bounds that
are all wrong together. **No test compared any bound to a true cardinality.**

---

## 2. The harness masked it

`examples/lpbound_tightness.rs` did materialise instances and compute a true
cardinality. It then reported:

```rust
let r_lp = (lp_b / truth).max(1.0);
```

`.max(1.0)` clamps the ratio at 1. A bound *below* the truth — ratio < 1, the
signal that it is unsound — was recorded as exactly 1.0, indistinguishable from a
perfectly tight sound bound. The campaign was structurally incapable of reporting
a violation, and averaged 2,179 of them into its tightness means.

A second artefact: bounds are `u64` and saturate, while the materialised truth is
computed in `u128`. In the n=7 cells the truth exceeds `u64::MAX`, so every bound
including `ProductBound` reads as "below truth" for reasons that have nothing to
do with soundness. Those 154 trials are now counted and reported separately
rather than folded in.

### 2.1 What the harness reports now

* per-bound violation counts per cell, never clamped;
* unclamped `bound / truth` ratios, so a value below 1.0 stays visible;
* `saturated_trials`, excluded from violation counts;
* tightness improvements credited only when **both** bounds are sound on that
  instance.

---

## 3. Where 40.95× came from

The star-5 headline was `median(AgmBound / LpJoinBound)` over instances where
`LpJoinBound` collapsed a star to its hub row count — which it did because it
never charged the spokes any cover weight (§1.2). The ratio was large exactly in
proportion to how far below the truth the denominator had fallen.

With the soundness filter applied to the v1.1 bounds, **no star-5 trial has both
bounds sound**, so the corrected v1.1 figure is undefined (`NaN`). With the
repaired 1.2.0 bounds the same measurement is **1.070×**.

The number should never have been published as a wallclock speedup either; that
mislabel is corrected in `EVIDENCE.md` and the technical paper.

---

## 4. The repair

### 4.1 A sound bound needs degree statistics

`samkhya-core::degree` implements:

> **Theorem (spanning-tree degree ceiling).** For an equi-join over `R_1 … R_n`
> with join graph `G`, and any spanning tree `T` of a connected component rooted
> at `r`:
>
> ```text
> |Q| <= |R_r| · Π  maxdeg(R_v, a_uv)
>            (u→v) ∈ T, v ≠ r
> ```
>
> *Proof.* Materialise in BFS order from `r`. The partial result starts at
> `|R_r|`. Joining child `v` on attribute `a`: every partial tuple already fixes
> a value of `a`, and at most `maxdeg(R_v, a)` rows of `R_v` carry any single
> value, so the count multiplies by at most that factor. Non-tree edges only
> filter. ∎

Sound for bag semantics, which is what engines execute.

### 4.2 Degrees from statistics samkhya already carries

| Source | Bound on `maxdeg` | Notes |
| ------ | ----------------- | ----- |
| row count | `rows` | always true; ceiling degrades to the product |
| distinct count (HLL) | `rows − distinct + 1` | exact for key columns |
| Count-Min sketch | largest counter | tightest; requires no saturation |

The distinct-count derivation is the important one: it needs nothing samkhya was
not already writing into its Puffin sidecars, and it is **exactly tight** on
foreign-key joins, since a key column gives `maxdeg <= 1`.

The Count-Min derivation is what makes the ceiling portable. For any key `k`,
`true_freq(k) <= estimate(k) <= max counter`, so the largest counter bounds every
key's degree at once without knowing which key is hot. The chain depends on
Count-Min's never-undercount property, which `u32` saturation breaks;
`CountMinSketch::max_frequency_bound` returns `None` in that case rather than an
unsound number.

### 4.3 Tightness of the repaired bound

Measured on the audit witnesses (true cardinality by brute force):

| Instance | truth | Product | v1.1 LpJoin | v1.2 degree ceiling |
| -------- | ----: | ------: | ----------: | ------------------: |
| FK join, orders(10) ⋈ lineitem(100), 10 distinct keys | 100 | 1,000 | **10** ✗ | **100** ✓ exact |
| 2-rel, all rows on one key (4 × 5) | 20 | 20 | **4** ✗ | 20 ✓ exact |
| 3-rel chain, all rows on one key (3 × 3 × 3) | 27 | 27 | **3** ✗ | 27 ✓ exact |
| skewed 20 × 20, 5 distinct, 16 on one key | 260 | 400 | **20** ✗ | 320 ✓ |
| 4-rel star, hub 2, spokes 4/4/4, one key | 128 | 128 | **2** ✗ | 128 ✓ exact |

Sound on all five, exactly tight on four, and 10× tighter than the product on the
foreign-key shape.

---

## 5. Reproduction

```bash
# Property suite: generates instances, brute-forces the true join, asserts
# ceiling >= truth. 6 properties x 2,048 cases.
cargo test -p samkhya-core --test soundness_degree

# Instrumented tightness campaign (1,080 trials).
cargo run --release -p samkhya-core --example lpbound_tightness --features lp_solver

# The v1.1 comparison: same harness, previous bound implementations.
git worktree add --detach /tmp/samkhya-v11 v1.1.0
cp samkhya-core/examples/lpbound_tightness.rs /tmp/samkhya-v11/samkhya-core/examples/
cd /tmp/samkhya-v11 && cargo run --release -p samkhya-core \
    --example lpbound_tightness --features lp_solver
```

Expect `"soundness_violations_total": 0` on 1.2.0 and `2179` on v1.1.0.

---

## 6. What this does not resolve

* The bound is only as sound as the degree statistics it is handed. Every
  `AttributeDegree` constructor either derives the guarantee or documents the
  obligation, but a caller that supplies an under-estimate gets an unsound
  ceiling. There is no way around that: it is the one number the ceiling rests on.
* The repaired `ChainBound` and the degree-free `LpJoinBound::ceiling` both
  collapse to the Cartesian product when no distinct counts are supplied. That is
  the honest answer for that input, not a regression to hide.
* Cyclic queries still bound loosely. `LpJoinBound::ceiling_hypergraph` returns
  the true AGM bound (`n^1.5` for a triangle) but requires the attribute schema
  *and* a declaration that the relations carry no private columns. Deriving that
  declaration automatically from an engine's plan is not implemented.
* No end-to-end measurement in this repo yet exercises the repaired ceiling
  inside a query plan. The DataFusion adapter gained a derived per-input ceiling
  in 1.2.0, but its effect on plan quality is unmeasured.
