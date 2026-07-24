//! Provable join ceilings derived from degree statistics.
//!
//! # Why this module exists
//!
//! A ceiling is only useful if it is *sound*: for every database instance
//! consistent with the statistics it was handed, the ceiling must be at
//! least the true output cardinality. Otherwise a correction layer clamped
//! to it can publish an estimate below the truth, which is exactly the
//! regression the envelope exists to prevent.
//!
//! The bounds in [`crate::lpbound`] that predate this module accept only
//! *row counts* plus a list of joined relation pairs. That input is not
//! enough to beat the Cartesian product:
//!
//! > **Fact.** Given only per-relation row counts and which pairs of
//! > relations are joined by equality, the worst-case output cardinality is
//! > the full product of the row counts — put every row of every relation
//! > on one single join-key value and the equi-join degenerates to a cross
//! > product. Any ceiling below that product is therefore unsound.
//!
//! To do better you need one more statistic per relation and join
//! attribute: an upper bound on the **degree** — how many rows can share a
//! single value of that attribute.
//!
//! # The bound
//!
//! **Theorem (spanning-tree degree ceiling).** Let `Q` be an equi-join over
//! relations `R_1 … R_n` whose join graph is `G`, and let `T` be any
//! spanning tree of a connected component of `G`, rooted at `r`. Then
//!
//! ```text
//! |Q| ≤ |R_r| · Π  maxdeg(R_v, a_uv)
//!               (u→v) ∈ T, v ≠ r
//! ```
//!
//! where `maxdeg(R, a)` is the largest number of rows of `R` sharing one
//! value of attribute `a`.
//!
//! *Proof.* Materialise the relations in BFS order from `r`. The partial
//! result starts at `|R_r|` tuples. Joining child `v` to its parent `u` on
//! attribute `a`: every partial tuple already fixes a value of `a`
//! (inherited from `u`), and at most `maxdeg(R_v, a)` rows of `R_v` carry
//! any single value, so the count multiplies by at most that factor. Join
//! edges outside `T` only filter and can never add tuples. ∎
//!
//! The bound is sound for **bag** semantics — duplicate rows included —
//! which is what SQL engines actually execute.
//!
//! # Where the degrees come from
//!
//! Any *over*-estimate of the true maximum degree keeps the ceiling sound.
//! Three sources, cheapest first:
//!
//! 1. **Row count.** `maxdeg ≤ rows`, always. With nothing else the ceiling
//!    degrades to the Cartesian product — sound, useless, never wrong.
//! 2. **Distinct count:** `maxdeg ≤ rows − distinct + 1`. Spend one value on
//!    each distinct key and pile every remaining row onto one of them. Exact
//!    for a key column (`distinct == rows` ⇒ `maxdeg ≤ 1`), which is why the
//!    foreign-key joins that dominate analytical workloads bound tightly from
//!    statistics samkhya already carries.
//!
//!    The count must be a *lower* bound on the truth, because the arithmetic
//!    subtracts it. An HLL point estimate is two-sided and will not do; use
//!    [`AttributeDegree::from_hll_floor`], which takes a distinct-count floor.
//! 3. **Frequency sketch.** A Count-Min sketch never *under*-estimates a
//!    frequency, so its largest counter bounds every key's degree at once —
//!    a far tighter bound than (2) under skew, and derivable without
//!    knowing which key is the hot one. The guarantee holds as long as no
//!    counter has saturated, which the constructor checks. See
//!    [`AttributeDegree::from_count_min`].
//!
//! (3) is what makes the ceiling *portable*: the sketch already rides in
//! the Puffin sidecar, so a bound proved from statistics written by one
//! engine holds in another, with no shared catalog and no re-scan.
//!
//! # Example
//!
//! A textbook foreign-key join: 10 orders, 100 line items, 10 distinct
//! order keys on both sides. The true output is 100 rows.
//!
//! ```
//! use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
//!
//! const ORDER_KEY: u32 = 0;
//!
//! let orders = JoinRelation::new(10)
//!     .with_degree(ORDER_KEY, AttributeDegree::from_distinct(10, 10));
//! let lineitem = JoinRelation::new(100)
//!     .with_degree(ORDER_KEY, AttributeDegree::from_distinct(100, 10));
//!
//! let graph = JoinGraph::new(vec![orders, lineitem])
//!     .with_edge(0, 1, ORDER_KEY);
//!
//! // Exactly the true cardinality — and provable, not estimated.
//! assert_eq!(graph.ceiling(), 100);
//! ```

use std::collections::BTreeMap;

use crate::lpbound::ProductBound;
use crate::lpbound::UpperBound;

/// Identifier for an equi-join attribute.
///
/// Values are opaque and caller-assigned: two relations share an attribute
/// exactly when the caller gives them the same `AttributeId`. Adapters
/// typically derive these from column ordinals, Iceberg field IDs, or a
/// resolved join-key interner.
pub type AttributeId = u32;

/// Upper bound on how many rows of one relation can share a single value
/// of one join attribute.
///
/// # Soundness obligation
///
/// [`max_degree`](Self::max_degree) must be **at least** the true maximum
/// degree. Every constructor in this type either derives that guarantee or
/// documents it as the caller's obligation. Supplying an under-estimate
/// silently makes the resulting ceiling unsound, which defeats the entire
/// point of the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeDegree {
    max_degree: u64,
}

impl AttributeDegree {
    /// The weakest sound degree: every row could share one value.
    ///
    /// Always correct, never informative — a graph built entirely from
    /// these yields the Cartesian product.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::AttributeDegree;
    ///
    /// assert_eq!(AttributeDegree::unknown(500).max_degree(), 500);
    /// ```
    pub const fn unknown(rows: u64) -> Self {
        Self { max_degree: rows }
    }

    /// Derive a degree bound from a row count and a distinct-value count:
    /// `maxdeg ≤ rows − distinct + 1`.
    ///
    /// Assign one row to each of the `distinct` values, then pile every
    /// remaining row onto a single value. Nothing can beat that
    /// concentration.
    ///
    /// # Soundness obligation
    ///
    /// `distinct` must be a **lower** bound on the true number of distinct
    /// values. The arithmetic subtracts it, so an over-stated distinct
    /// count under-states the degree and yields a ceiling *below* the
    /// truth — the exact failure this module exists to prevent.
    ///
    /// This matters because the obvious source is the wrong one:
    /// [`HllSketch::estimate`](crate::sketches::HllSketch::estimate) is
    /// approximately unbiased and two-sided, so it exceeds the truth about
    /// half the time. Use [`from_hll_floor`](Self::from_hll_floor), which
    /// takes a value that is never above the truth.
    ///
    /// A `distinct` of zero (unknown) or greater than `rows` (an
    /// inconsistent reading) falls back to [`unknown`](Self::unknown)
    /// rather than producing an unsound value.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::AttributeDegree;
    ///
    /// // A key column: every value occurs once.
    /// assert_eq!(AttributeDegree::from_distinct(100, 100).max_degree(), 1);
    /// // 100 rows over 10 values: at worst 91 share one value.
    /// assert_eq!(AttributeDegree::from_distinct(100, 10).max_degree(), 91);
    /// // Unknown distinct count degrades safely.
    /// assert_eq!(AttributeDegree::from_distinct(100, 0).max_degree(), 100);
    /// ```
    pub const fn from_distinct(rows: u64, distinct: u64) -> Self {
        if distinct == 0 || distinct > rows {
            return Self::unknown(rows);
        }
        // rows >= distinct >= 1, so this cannot underflow.
        Self {
            max_degree: rows - distinct + 1,
        }
    }

    /// Use a directly measured or sketch-derived upper bound on the degree.
    ///
    /// # Soundness obligation
    ///
    /// `upper_bound` must be greater than or equal to the true maximum
    /// degree. A Count-Min sketch satisfies this by construction: its
    /// frequency estimates never fall below the truth, so the maximum
    /// estimate over the inserted keys is a sound bound. An exact scan
    /// obviously satisfies it too. A sampled or averaged statistic does
    /// **not**.
    ///
    /// The value is capped at `rows`, since no relation can have a degree
    /// above its own row count.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::AttributeDegree;
    ///
    /// assert_eq!(AttributeDegree::from_upper_bound(1_000, 37).max_degree(), 37);
    /// // Capped at the row count.
    /// assert_eq!(AttributeDegree::from_upper_bound(20, 999).max_degree(), 20);
    /// ```
    pub const fn from_upper_bound(rows: u64, upper_bound: u64) -> Self {
        Self {
            max_degree: if upper_bound < rows {
                upper_bound
            } else {
                rows
            },
        }
    }

    /// Derive a sound degree bound from an HLL sketch of the join column.
    ///
    /// Uses [`HllSketch::nonzero_registers`](crate::sketches::HllSketch::nonzero_registers),
    /// a distinct-count floor, rather than the two-sided point estimate —
    /// see [`from_distinct`](Self::from_distinct) for why that distinction
    /// decides whether the resulting ceiling is sound.
    ///
    /// The floor saturates at the register count, so on a high-cardinality
    /// column this degrades toward [`unknown`](Self::unknown) rather than
    /// toward a wrong answer. A Count-Min sketch
    /// ([`from_count_min`](Self::from_count_min)) bounds far more tightly
    /// when one is available.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::AttributeDegree;
    /// use samkhya_core::sketches::HllSketch;
    ///
    /// let mut hll = HllSketch::new(12).unwrap();
    /// for i in 0..1_000u32 { hll.add(&i.to_le_bytes()); }
    ///
    /// let degree = AttributeDegree::from_hll_floor(1_000, &hll);
    /// // Sound: never below the true maximum degree of 1.
    /// assert!(degree.max_degree() >= 1);
    /// assert!(degree.max_degree() <= 1_000);
    /// ```
    pub fn from_hll_floor(rows: u64, sketch: &crate::sketches::HllSketch) -> Self {
        Self::from_distinct(rows, sketch.nonzero_registers())
    }

    /// Derive a sound degree bound from a Count-Min sketch of the join
    /// column — the tightest source available without an exact scan.
    ///
    /// For any key `k`, `true_freq(k) <= estimate(k) <= max counter`, so
    /// the sketch's largest counter bounds every key's degree at once.
    /// Returns [`unknown`](Self::unknown) when the sketch has saturated,
    /// because that chain of inequalities depends on Count-Min's
    /// never-undercount property, which `u32` saturation breaks.
    ///
    /// This is the link that makes the ceiling *portable*: a Count-Min
    /// sketch written into a Puffin sidecar by one engine yields a
    /// provable join ceiling in another, with no shared catalog and no
    /// re-scan.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::AttributeDegree;
    /// use samkhya_core::sketches::CountMinSketch;
    ///
    /// let mut cms = CountMinSketch::with_defaults();
    /// for _ in 0..9 { cms.add(b"hot-key", 1); }
    /// for _ in 0..2 { cms.add(b"cold-key", 1); }
    ///
    /// let degree = AttributeDegree::from_count_min(11, &cms);
    /// // Bounds the true maximum degree of 9 from above, and beats the
    /// // row count the caller would otherwise have to assume.
    /// assert!(degree.max_degree() >= 9);
    /// assert!(degree.max_degree() <= 11);
    /// ```
    pub fn from_count_min(rows: u64, sketch: &crate::sketches::CountMinSketch) -> Self {
        match sketch.max_frequency_bound() {
            Some(bound) => Self::from_upper_bound(rows, u64::from(bound)),
            None => Self::unknown(rows),
        }
    }

    /// The bounded maximum degree.
    pub const fn max_degree(&self) -> u64 {
        self.max_degree
    }
}

/// One relation participating in a join, with its per-attribute degrees.
#[derive(Debug, Clone)]
pub struct JoinRelation {
    rows: u64,
    degrees: BTreeMap<AttributeId, AttributeDegree>,
}

impl JoinRelation {
    /// A relation of `rows` rows with no degree information. Every
    /// attribute defaults to [`AttributeDegree::unknown`].
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::JoinRelation;
    ///
    /// assert_eq!(JoinRelation::new(42).rows(), 42);
    /// ```
    pub fn new(rows: u64) -> Self {
        Self {
            rows,
            degrees: BTreeMap::new(),
        }
    }

    /// Attach a degree bound for one join attribute.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::{AttributeDegree, JoinRelation};
    ///
    /// let r = JoinRelation::new(100)
    ///     .with_degree(7, AttributeDegree::from_distinct(100, 100));
    /// assert_eq!(r.degree(7).max_degree(), 1);
    /// ```
    pub fn with_degree(mut self, attribute: AttributeId, degree: AttributeDegree) -> Self {
        self.degrees.insert(attribute, degree);
        self
    }

    /// Row count of this relation.
    pub const fn rows(&self) -> u64 {
        self.rows
    }

    /// Degree bound for `attribute`, defaulting to the always-sound
    /// [`AttributeDegree::unknown`] when none was supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::JoinRelation;
    ///
    /// // No degree registered → falls back to the row count.
    /// assert_eq!(JoinRelation::new(64).degree(3).max_degree(), 64);
    /// ```
    pub fn degree(&self, attribute: AttributeId) -> AttributeDegree {
        self.degrees
            .get(&attribute)
            .copied()
            .unwrap_or_else(|| AttributeDegree::unknown(self.rows))
    }
}

/// An equality predicate binding two relations on one shared attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinEdge {
    /// Index of the left relation in [`JoinGraph`]'s relation vector.
    pub left: usize,
    /// Index of the right relation.
    pub right: usize,
    /// The attribute both sides are compared on.
    pub attribute: AttributeId,
}

/// A join query described precisely enough to bound provably.
///
/// See the [module documentation](self) for the theorem this implements and
/// for where the degree statistics come from.
#[derive(Debug, Clone, Default)]
pub struct JoinGraph {
    relations: Vec<JoinRelation>,
    edges: Vec<JoinEdge>,
}

impl JoinGraph {
    /// Build a graph over `relations`, with no predicates yet.
    pub fn new(relations: Vec<JoinRelation>) -> Self {
        Self {
            relations,
            edges: Vec::new(),
        }
    }

    /// Add an equality predicate between two relations on one attribute.
    ///
    /// Out-of-range indices and self-edges are dropped: a misbuilt join
    /// graph must degrade the ceiling, never corrupt or panic it.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::{JoinGraph, JoinRelation};
    ///
    /// let g = JoinGraph::new(vec![JoinRelation::new(5), JoinRelation::new(7)])
    ///     .with_edge(0, 1, 0)
    ///     .with_edge(0, 9, 0);   // dropped: index 9 does not exist
    /// assert_eq!(g.edges().len(), 1);
    /// ```
    pub fn with_edge(mut self, left: usize, right: usize, attribute: AttributeId) -> Self {
        let n = self.relations.len();
        if left < n && right < n && left != right {
            self.edges.push(JoinEdge {
                left,
                right,
                attribute,
            });
        }
        self
    }

    /// The relations in this graph.
    pub fn relations(&self) -> &[JoinRelation] {
        &self.relations
    }

    /// The equality predicates in this graph.
    pub fn edges(&self) -> &[JoinEdge] {
        &self.edges
    }

    /// A provable inclusive ceiling on the join's output cardinality.
    ///
    /// Never returns a value below the true cardinality of any database
    /// instance consistent with the supplied statistics, provided every
    /// [`AttributeDegree`] honours its soundness obligation.
    ///
    /// The ceiling is the minimum of the Cartesian product and the
    /// spanning-tree degree bound evaluated from every possible root.
    /// Because *every* spanning tree yields a sound ceiling, the search
    /// over roots affects only tightness, never correctness.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
    ///
    /// // Three 3-row relations chained on two attributes, every row on the
    /// // same key: the join really does degenerate to 27 rows, and the
    /// // ceiling says so rather than pretending otherwise.
    /// let rel = |n| JoinRelation::new(n);
    /// let g = JoinGraph::new(vec![rel(3), rel(3), rel(3)])
    ///     .with_edge(0, 1, 0)
    ///     .with_edge(1, 2, 1);
    /// assert_eq!(g.ceiling(), 27);
    /// ```
    pub fn ceiling(&self) -> u64 {
        if self.relations.is_empty() {
            return 0;
        }

        let mut total: u128 = 1;
        for component in self.components() {
            let component_ceiling = self.component_ceiling(&component);
            total = total.saturating_mul(u128::from(component_ceiling));
            if total >= u128::from(u64::MAX) {
                return u64::MAX;
            }
        }
        total as u64
    }

    /// Connected components of the relation graph induced by the edges.
    /// Every relation index appears in exactly one component; a relation
    /// with no incident edge forms a singleton.
    fn components(&self) -> Vec<Vec<usize>> {
        let n = self.relations.len();
        let mut parent: Vec<usize> = (0..n).collect();

        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }

        for edge in &self.edges {
            let a = find(&mut parent, edge.left);
            let b = find(&mut parent, edge.right);
            if a != b {
                parent[a] = b;
            }
        }

        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for v in 0..n {
            let root = find(&mut parent, v);
            groups.entry(root).or_default().push(v);
        }
        groups.into_values().collect()
    }

    /// Best sound ceiling for one connected component.
    fn component_ceiling(&self, component: &[usize]) -> u64 {
        // The Cartesian product over the component is always sound.
        let rows: Vec<u64> = component.iter().map(|&r| self.relations[r].rows).collect();
        let mut best = ProductBound.ceiling(&rows, &[]);

        if component.len() == 1 {
            return self.relations[component[0]].rows;
        }

        for &root in component {
            let candidate = self.spanning_tree_ceiling(component, root);
            if candidate < best {
                best = candidate;
            }
        }
        best
    }

    /// Grow a spanning tree greedily from `root`, always attaching the
    /// frontier relation whose degree factor is smallest. Any spanning tree
    /// gives a sound ceiling, so the greedy choice is a tightness
    /// heuristic, not a correctness requirement.
    fn spanning_tree_ceiling(&self, component: &[usize], root: usize) -> u64 {
        let mut visited: Vec<usize> = vec![root];
        let mut bound: u128 = u128::from(self.relations[root].rows);

        while visited.len() < component.len() {
            let mut best: Option<(usize, u64)> = None;

            for edge in &self.edges {
                // Consider the edge in whichever orientation crosses the
                // frontier: one endpoint visited, the other not.
                for (from, to) in [(edge.left, edge.right), (edge.right, edge.left)] {
                    if !visited.contains(&from) || visited.contains(&to) {
                        continue;
                    }
                    if !component.contains(&to) {
                        continue;
                    }
                    let factor = self.relations[to].degree(edge.attribute).max_degree();
                    if best.is_none_or(|(_, current)| factor < current) {
                        best = Some((to, factor));
                    }
                }
            }

            let Some((next, factor)) = best else {
                // Disconnected within the claimed component: fall back to
                // multiplying in the remaining row counts, which is sound.
                for &v in component {
                    if !visited.contains(&v) {
                        bound = bound.saturating_mul(u128::from(self.relations[v].rows));
                        visited.push(v);
                    }
                }
                break;
            };

            bound = bound.saturating_mul(u128::from(factor));
            visited.push(next);

            if bound >= u128::from(u64::MAX) {
                return u64::MAX;
            }
        }

        if bound >= u128::from(u64::MAX) {
            u64::MAX
        } else {
            bound as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every witness instance from the v1.1 soundness audit, with the true
    /// cardinality established by brute force in
    /// `tests/soundness_degree.rs`.
    #[test]
    fn fk_join_is_exactly_tight() {
        let orders = JoinRelation::new(10).with_degree(0, AttributeDegree::from_distinct(10, 10));
        let lineitem =
            JoinRelation::new(100).with_degree(0, AttributeDegree::from_distinct(100, 10));
        let g = JoinGraph::new(vec![orders, lineitem]).with_edge(0, 1, 0);
        assert_eq!(g.ceiling(), 100);
    }

    #[test]
    fn all_rows_on_one_key_yields_the_product() {
        let g = JoinGraph::new(vec![JoinRelation::new(4), JoinRelation::new(5)]).with_edge(0, 1, 0);
        assert_eq!(g.ceiling(), 20);
    }

    #[test]
    fn skewed_join_stays_above_truth() {
        // 20 rows, 5 distinct values, 16 rows piled on one value.
        // True cardinality is 260; maxdeg bounds at 16 → 20 * 16 = 320.
        let rel = || JoinRelation::new(20).with_degree(0, AttributeDegree::from_distinct(20, 5));
        let g = JoinGraph::new(vec![rel(), rel()]).with_edge(0, 1, 0);
        assert_eq!(g.ceiling(), 320);
        assert!(g.ceiling() >= 260);
    }

    #[test]
    fn star_with_key_hub_is_tight() {
        // Hub of 2 rows, three spokes of 4 rows each, all on one key value.
        let hub = JoinRelation::new(2);
        let spoke = || JoinRelation::new(4);
        let g = JoinGraph::new(vec![hub, spoke(), spoke(), spoke()])
            .with_edge(0, 1, 0)
            .with_edge(0, 2, 1)
            .with_edge(0, 3, 2);
        assert_eq!(g.ceiling(), 128);
    }

    #[test]
    fn key_star_collapses_to_the_hub() {
        // A hub joined to three dimension tables on their primary keys:
        // each spoke contributes a factor of exactly 1.
        let hub = JoinRelation::new(1_000);
        let dim = |n| JoinRelation::new(n).with_degree(0, AttributeDegree::from_distinct(n, n));
        let g = JoinGraph::new(vec![hub, dim(50), dim(60), dim(70)])
            .with_edge(0, 1, 0)
            .with_edge(0, 2, 0)
            .with_edge(0, 3, 0);
        assert_eq!(g.ceiling(), 1_000);
    }

    #[test]
    fn disconnected_components_multiply() {
        let g = JoinGraph::new(vec![
            JoinRelation::new(3),
            JoinRelation::new(4),
            JoinRelation::new(5),
        ])
        .with_edge(0, 1, 0);
        // Component {0,1} bounds at 3*4 = 12 with no degree info; {2} is 5.
        assert_eq!(g.ceiling(), 60);
    }

    #[test]
    fn empty_graph_is_zero() {
        assert_eq!(JoinGraph::new(Vec::new()).ceiling(), 0);
    }

    #[test]
    fn single_relation_is_its_row_count() {
        assert_eq!(JoinGraph::new(vec![JoinRelation::new(77)]).ceiling(), 77);
    }

    #[test]
    fn ceiling_never_exceeds_the_product() {
        let rel = |n| JoinRelation::new(n).with_degree(0, AttributeDegree::from_distinct(n, n));
        let g = JoinGraph::new(vec![rel(10), rel(20), rel(30)])
            .with_edge(0, 1, 0)
            .with_edge(1, 2, 0);
        assert!(g.ceiling() <= 10 * 20 * 30);
    }

    #[test]
    fn saturates_instead_of_overflowing() {
        let huge = || JoinRelation::new(u64::MAX);
        let g = JoinGraph::new(vec![huge(), huge(), huge()])
            .with_edge(0, 1, 0)
            .with_edge(1, 2, 0);
        assert_eq!(g.ceiling(), u64::MAX);
    }

    #[test]
    fn degree_from_distinct_rejects_inconsistent_input() {
        // distinct > rows cannot happen in a consistent reading; degrade
        // safely rather than underflowing.
        assert_eq!(AttributeDegree::from_distinct(10, 50).max_degree(), 10);
    }

    #[test]
    fn unknown_degrees_degrade_to_the_product() {
        let g = JoinGraph::new(vec![JoinRelation::new(6), JoinRelation::new(7)]).with_edge(0, 1, 0);
        assert_eq!(g.ceiling(), 42);
    }
}
