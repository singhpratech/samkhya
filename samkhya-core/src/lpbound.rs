//! Pessimistic upper-bound envelope for cardinality estimates.
//!
//! Inspired by **LpBound** \[Zhang et al., SIGMOD 2025 Best Paper\]. The
//! envelope provides a *provable ceiling* on the cardinality of a join:
//! no correction may exceed it, so cold-start plans are bounded by the
//! native estimate or this ceiling — whichever is tighter — and never
//! degrade below baseline.
//!
//! # Preferred bound
//!
//! When the `lp_solver` Cargo feature is enabled, `LpJoinBound` (a real
//! fractional-edge-cover LP solved with `good_lp`'s pure-Rust `microlp`
//! backend) is the preferred ceiling. It is provably tighter than the
//! coarse [`ProductBound`] / [`AgmBound`] / [`ChainBound`] approximations
//! for any non-trivial cyclic join (triangles, squares, cliques) and
//! exactly matches the AGM ρ\*-derived bound for all join shapes the
//! attribute-hypergraph can represent.
//!
//! # Scaffolding bounds (always available)
//!
//! [`ProductBound`], [`AgmBound`], and [`ChainBound`] remain shipped
//! without the LP dependency for builds that want a constant-time
//! ceiling, for unit tests, and as the safety floor when the LP solver
//! fails (numerical edge cases, malformed join graphs). They are
//! scaffolding for the full LpBound, not a replacement: prefer
//! `LpJoinBound` (under the `lp_solver` feature) in any release build that
//! can afford the `good_lp` dependency.
//!
//! # Empirical bound ordering
//!
//! The empirical campaign (`bench-results/07_lpbound_tightness.md`,
//! 1,080 trials across path/star/cycle/clique topologies × n ∈ {3, 5, 7}
//! × ℓ_p ∈ {1, 2, ∞}) measured the actual partial order:
//!
//! ```text
//!   ProductBound  ≥  { ChainBound,  AgmBound }  ≥  LpJoinBound
//! ```
//!
//! `ChainBound` and `AgmBound` are **not strictly ordered** — `ChainBound`
//! is routinely the tighter of the two (it divides by every per-edge
//! distinct count, while AGM uses a fractional-edge-cover shortcut). The
//! `LpJoinBound ≤ AgmBound` leg holds in 86.4% of trials; size-7
//! cyclic/clique under uniform ℓ_p=1 exposes an LP-conditioning corner
//! (~13.6% violation) where the LP-derived ceiling overshoots AGM's
//! `min × max` shortcut. The query optimizer should evaluate all three
//! scaffolding bounds and take the minimum rather than assuming a strict
//! chain.

use crate::{Error, Result};

/// Trait every upper-bound provider implements.
///
/// Implementations return an *inclusive* row-count ceiling that the join
/// can never exceed. A correction layer must never produce an estimate
/// above this number.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::{ProductBound, UpperBound};
///
/// // Cartesian product (sound but very loose).
/// let bound = ProductBound.ceiling(&[100, 200], &[]);
/// assert_eq!(bound, 20_000);
/// ```
pub trait UpperBound {
    /// Compute the inclusive ceiling for a join.
    ///
    /// * `relations`           — input row counts for each base relation
    /// * `equality_predicates` — pairs of relation indices joined by `=`
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64;
}

/// Cartesian-product upper bound. Sound but very loose.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::{ProductBound, UpperBound};
///
/// // Empty predicate list: the bound is the unconstrained product.
/// assert_eq!(ProductBound.ceiling(&[10, 20, 30], &[]), 6000);
/// // Overflow saturates to u64::MAX rather than wrapping.
/// assert_eq!(ProductBound.ceiling(&[u64::MAX, 2], &[]), u64::MAX);
/// ```
pub struct ProductBound;

impl UpperBound for ProductBound {
    fn ceiling(&self, relations: &[u64], _eq: &[(usize, usize)]) -> u64 {
        relations.iter().fold(1u64, |acc, &n| acc.saturating_mul(n))
    }
}

/// Degree-derived chain-join upper bound.
///
/// Takes a per-relation distinct-key count and converts it into a sound
/// bound on the relation's maximum join degree,
/// `maxdeg_i ≤ |R_i| − D_i + 1`, then applies the spanning-tree degree
/// ceiling from [`crate::degree`]. Falls back to [`ProductBound`] when no
/// equality predicates are supplied.
///
/// # Caller obligation
///
/// `distinct_counts[i]` must be the distinct-value count of the *join key*
/// relation `i` carries, and it must not over-state the truth — an HLL
/// reading that comes back high would relax the derived degree bound in the
/// unsafe direction. [`crate::sketches::HllSketch`] readings should be used
/// at or below their estimate, not above it.
///
/// # Soundness note (changed in 1.2.0)
///
/// Through v1.1 this bound divided the Cartesian product by
/// `max(D_i, D_j)` per predicate. That formula is a uniform-distribution
/// *estimate*, not an upper bound: under skew it lands below the true
/// cardinality. Concretely, two 20-row relations with 5 distinct keys each
/// and 16 rows piled on one key join to 260 rows, while the old formula
/// returned 80. The bound now returns 320 for that instance — larger, and
/// actually provable. See `crate::degree` for the theorem.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::{ChainBound, UpperBound};
///
/// // A foreign-key join: 10 orders, 100 line items, 10 distinct keys on
/// // both sides. Bounds exactly at the true output of 100 rows.
/// let cb = ChainBound::new(vec![10, 10]);
/// assert_eq!(cb.ceiling(&[10, 100], &[(0, 1)]), 100);
/// ```
pub struct ChainBound {
    pub distinct_counts: Vec<u64>,
}

impl ChainBound {
    /// Construct a chain-join bound from per-relation distinct-key counts.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::lpbound::{ChainBound, UpperBound};
    ///
    /// // Two 1000-row relations over a key with 100 distinct values: at
    /// // worst 901 rows share one value, so the ceiling is 1000 * 901.
    /// let cb = ChainBound::new(vec![100, 100]);
    /// assert_eq!(cb.ceiling(&[1_000, 1_000], &[(0, 1)]), 901_000);
    /// ```
    pub fn new(distinct_counts: Vec<u64>) -> Self {
        Self { distinct_counts }
    }
}

impl UpperBound for ChainBound {
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }
        degree_graph(relations, equality_predicates, Some(&self.distinct_counts)).ceiling()
    }
}

/// Build the [`crate::degree::JoinGraph`] implied by the legacy
/// `(row counts, predicate pairs)` surface.
///
/// Each predicate is treated as introducing its own join attribute, and a
/// relation's degree on every attribute it touches is derived from its
/// single supplied distinct count. With no distinct counts the degrees are
/// unknown and the ceiling collapses to the Cartesian product — sound, and
/// the honest answer for that input.
fn degree_graph(
    relations: &[u64],
    equality_predicates: &[(usize, usize)],
    distinct_counts: Option<&[u64]>,
) -> crate::degree::JoinGraph {
    use crate::degree::{AttributeDegree, JoinGraph, JoinRelation};

    let n = relations.len();
    let mut built: Vec<JoinRelation> = relations
        .iter()
        .map(|&rows| JoinRelation::new(rows))
        .collect();

    for (attribute, &(i, j)) in equality_predicates.iter().enumerate() {
        if i >= n || j >= n || i == j {
            continue;
        }
        let attribute = attribute as u32;
        for endpoint in [i, j] {
            let rows = relations[endpoint];
            let degree = match distinct_counts.and_then(|d| d.get(endpoint).copied()) {
                Some(distinct) => AttributeDegree::from_distinct(rows, distinct),
                None => AttributeDegree::unknown(rows),
            };
            built[endpoint] = std::mem::replace(&mut built[endpoint], JoinRelation::new(rows))
                .with_degree(attribute, degree);
        }
    }

    let mut graph = JoinGraph::new(built);
    for (attribute, &(i, j)) in equality_predicates.iter().enumerate() {
        graph = graph.with_edge(i, j, attribute as u32);
    }
    graph
}

/// Cartesian-product bound retained under its historical name.
///
/// # Soundness note (changed in 1.2.0)
///
/// Through v1.1 this returned `min(product, |R_min| · |R_max|)`. That
/// shortcut is not an AGM bound and is unsound for three or more relations:
/// three 3-row relations chained on one shared key value join to 27 rows,
/// while the shortcut returned 9. Given only row counts and which pairs are
/// joined, the Cartesian product is the *only* sound ceiling — every row of
/// every relation may share a single key value. This type therefore now
/// returns exactly [`ProductBound`].
///
/// To do better, supply degree statistics via [`crate::degree::JoinGraph`],
/// which bounds the same foreign-key join at 100 rows instead of 1000.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::{AgmBound, ProductBound, UpperBound};
///
/// let r = [1_000u64, 1_000_000];
/// assert_eq!(
///     AgmBound.ceiling(&r, &[(0, 1)]),
///     ProductBound.ceiling(&r, &[])
/// );
/// ```
#[deprecated(
    since = "1.2.0",
    note = "the min*max shortcut was unsound for 3+ relations and now simply returns \
            ProductBound; use samkhya_core::degree::JoinGraph for a bound that is both \
            provable and tighter"
)]
pub struct AgmBound;

#[allow(deprecated)]
impl UpperBound for AgmBound {
    fn ceiling(&self, relations: &[u64], _equality_predicates: &[(usize, usize)]) -> u64 {
        ProductBound.ceiling(relations, &[])
    }
}

/// Clamp an estimate to a ceiling. Returns [`Error::LpBoundExceeded`]
/// if the estimate exceeds the ceiling — this signals a correction-layer
/// bug, since corrections must respect the envelope.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::clamp_estimate;
///
/// // Within the ceiling → Ok(value).
/// assert_eq!(clamp_estimate(500.0, 1000).unwrap(), 500);
/// // Exceeding the ceiling → Err signalling a corrector violation.
/// assert!(clamp_estimate(1500.0, 1000).is_err());
/// ```
pub fn clamp_estimate(estimate: f64, ceiling: u64) -> Result<u64> {
    let clamped = estimate.max(0.0).min(u64::MAX as f64) as u64;
    if clamped <= ceiling {
        Ok(clamped)
    } else {
        Err(Error::LpBoundExceeded {
            estimate,
            ceiling: ceiling as f64,
        })
    }
}

/// Clamp without erroring; saturates to `ceiling`. Use this in production
/// paths where a misbehaving corrector must never crash the engine.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::saturating_clamp;
///
/// assert_eq!(saturating_clamp(500.0, 1000), 500);
/// assert_eq!(saturating_clamp(2000.0, 1000), 1000);   // clamps to ceiling
/// assert_eq!(saturating_clamp(-5.0, 1000), 0);        // negative → 0
/// assert_eq!(saturating_clamp(f64::NAN, 1000), 0);    // NaN is treated as 0
/// ```
pub fn saturating_clamp(estimate: f64, ceiling: u64) -> u64 {
    let clamped = estimate.max(0.0).min(u64::MAX as f64) as u64;
    clamped.min(ceiling)
}

// =============================================================================
// LpJoinBound — real fractional-edge-cover LP (the v0.5.0 deliverable).
// =============================================================================

/// Real fractional-edge-cover LP join bound — the principled AGM / LpBound
/// construction the coarse [`AgmBound`] / [`ChainBound`] approximate.
///
/// # Formulation
///
/// Build the join's *attribute hypergraph*:
///
/// * one variable `x_r ≥ 0` per relation `r`;
/// * each equality predicate `(i, j)` contributes one shared attribute
///   `a` covered by both `R_i` and `R_j`;
/// * for every shared attribute `a` we add a fractional-cover constraint
///
///   ```text
///   sum_{r : a ∈ schema(r)} x_r ≥ 1
///   ```
///
/// * the objective is to minimise the log-cardinality of the join,
///
///   ```text
///   minimise   sum_r x_r * log|R_r|
///   ```
///
/// The provable join-cardinality ceiling is `exp(minimum)`. This is the
/// classical **Atserias–Grohe–Marx fractional-edge-cover bound** that
/// LpBound (Zhang et al., SIGMOD 2025) extends to ℓp-norm degree
/// sequences; the AGM bound is the p=∞ specialisation and is exactly
/// what we ship here.
///
/// # Per-component decomposition
///
/// Equality predicates partition the relations into connected
/// components. Variables in distinct components share no constraint, so
/// the LP decomposes: the bound on the whole join graph is the
/// **product** of the bounds on each connected component. We exploit
/// this by solving one (small) LP per component instead of one big LP.
///
/// # Tightness vs the coarse bounds
///
/// * 2-relation single-predicate join: LP returns `min(|R_i|, |R_j|)`
///   (the real AGM bound for a single shared attribute), which is
///   strictly tighter than [`AgmBound`]'s `|R_min| * |R_max|`
///   approximation whenever both relations are non-empty.
/// * Triangle (3 relations, 3 predicates each on a distinct attribute):
///   LP returns `(|R_0| * |R_1| * |R_2|)^{1/2}`, the famous AGM triangle
///   bound. Strictly tighter than [`ChainBound`] and [`ProductBound`]
///   for any non-trivial relation sizes.
/// * Disconnected components: LP returns the product of the
///   per-component bounds, matching the trivial decomposition.
///
/// # Solver
///
/// Backed by [`good_lp`] with the pure-Rust `microlp` backend
/// (no system libraries, no C/C++ toolchain — compiles cleanly on any
/// Rust 1.94+ host). The LP is small (one variable per relation, one
/// constraint per shared attribute) so solve time is negligible.
#[cfg(feature = "lp_solver")]
pub struct LpJoinBound {
    /// Optional per-relation distinct-count hint. When provided, the
    /// objective coefficient for relation `r` is `log(min(|R_r|, D_r))`
    /// rather than `log|R_r|`, which can only tighten the bound (the
    /// join output on a key column cannot exceed the column's distinct
    /// support). Empty by default.
    distinct_counts: Vec<u64>,
}

#[cfg(feature = "lp_solver")]
impl Default for LpJoinBound {
    fn default() -> Self {
        Self::new()
    }
}

/// One relation described as a hyperedge: its row count, the join
/// attributes it exposes, and whether it also carries columns nothing else
/// covers.
///
/// The `has_private_attributes` flag is what makes a fractional edge cover
/// well defined. A relation contributing any column that no other relation
/// supplies must take a full unit of cover weight, because the output
/// projected onto that relation's columns is a subset of the relation
/// itself. Defaulting the flag to `true` keeps the bound sound for callers
/// that have not thought about it — the honest default for a safety
/// envelope.
#[cfg(feature = "lp_solver")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperRelation {
    /// Row count of the relation.
    pub rows: u64,
    /// Join attributes this relation exposes. Two relations share an
    /// attribute exactly when the same identifier appears in both lists.
    pub attributes: Vec<u32>,
    /// Whether the relation contributes output columns no other relation
    /// covers. `true` is the safe default.
    pub has_private_attributes: bool,
}

#[cfg(feature = "lp_solver")]
impl HyperRelation {
    /// A relation that carries private columns in addition to its join
    /// attributes — the ordinary `SELECT *` case.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::lpbound::HyperRelation;
    ///
    /// let r = HyperRelation::new(1_000, vec![0, 1]);
    /// assert!(r.has_private_attributes);
    /// ```
    pub fn new(rows: u64, attributes: Vec<u32>) -> Self {
        Self {
            rows,
            attributes,
            has_private_attributes: true,
        }
    }

    /// A relation already projected down to its join attributes, so nothing
    /// outside the cover needs charging.
    ///
    /// Declare this only when it is true — a semi-join-reduced input, a
    /// pure bridge table, or a query that projects to join keys. Declaring
    /// it falsely makes the ceiling unsound.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::lpbound::HyperRelation;
    ///
    /// let r = HyperRelation::projected(1_000, vec![0, 1]);
    /// assert!(!r.has_private_attributes);
    /// ```
    pub fn projected(rows: u64, attributes: Vec<u32>) -> Self {
        Self {
            rows,
            attributes,
            has_private_attributes: false,
        }
    }
}

#[cfg(feature = "lp_solver")]
impl LpJoinBound {
    /// Construct a bound with no distinct-count overrides. The objective
    /// uses `log|R_r|` for every relation.
    pub fn new() -> Self {
        Self {
            distinct_counts: Vec::new(),
        }
    }

    /// Construct a bound that uses the supplied per-relation distinct
    /// counts to tighten the objective coefficients.
    pub fn with_distinct_counts(distinct_counts: Vec<u64>) -> Self {
        Self { distinct_counts }
    }

    /// Same semantics as [`UpperBound::ceiling`]; surfaced here so
    /// callers can avoid importing the trait when they already hold an
    /// `&LpJoinBound`.
    ///
    /// # Soundness note (changed in 1.2.0)
    ///
    /// Row counts plus a list of joined relation *pairs* do not determine a
    /// fractional edge cover: the pair list says nothing about the columns
    /// each relation contributes to the output, and every relation that
    /// carries a column no other relation covers must take a full unit of
    /// cover weight. Through v1.1 this method solved an LP with one
    /// constraint per predicate and no private-attribute constraints, which
    /// bounded a 10-row ⋈ 100-row foreign-key join at 10 rows — the join
    /// really returns 100.
    ///
    /// This entry point now delegates to the degree-derived ceiling in
    /// [`crate::degree`], which is provable on the same input. Use
    /// [`Self::ceiling_hypergraph`] when the attribute schema is known and
    /// the fractional-edge-cover LP is genuinely applicable — that path
    /// still returns the AGM `n^1.5` bound for a triangle.
    pub fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }
        degree_graph(relations, equality_predicates, None).ceiling()
    }

    /// Like [`Self::ceiling`] but folds the distinct counts supplied to
    /// [`Self::with_distinct_counts`] into a sound per-relation degree
    /// bound (`maxdeg ≤ rows − distinct + 1`). Missing or inconsistent
    /// entries fall back to the row count.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::lpbound::LpJoinBound;
    ///
    /// // 10 orders, 100 line items, 10 distinct order keys: bounds exactly.
    /// let bound = LpJoinBound::with_distinct_counts(vec![10, 10]);
    /// assert_eq!(bound.ceiling_with_distinct(&[10, 100], &[(0, 1)]), 100);
    /// ```
    pub fn ceiling_with_distinct(
        &self,
        relations: &[u64],
        equality_predicates: &[(usize, usize)],
    ) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }
        degree_graph(relations, equality_predicates, Some(&self.distinct_counts)).ceiling()
    }

    /// Solve the genuine fractional-edge-cover LP over an explicit
    /// attribute hypergraph.
    ///
    /// This is the AGM bound as Atserias, Grohe and Marx define it: one
    /// cover constraint per *attribute*, not per predicate. For a triangle
    /// whose three relations expose only their join attributes it returns
    /// `(|R₀|·|R₁|·|R₂|)^(1/2)`; for relations carrying private columns it
    /// correctly charges each of them a full unit of cover weight and
    /// degrades toward the Cartesian product.
    ///
    /// The result is capped at [`ProductBound`] and falls back to it if the
    /// solver fails — the envelope must never crash the engine, and must
    /// never return below the product's guarantee.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::lpbound::{HyperRelation, LpJoinBound};
    ///
    /// // Triangle R(a,b), S(b,c), T(c,a): no private columns anywhere.
    /// let tri = vec![
    ///     HyperRelation::projected(100, vec![0, 1]),
    ///     HyperRelation::projected(100, vec![1, 2]),
    ///     HyperRelation::projected(100, vec![2, 0]),
    /// ];
    /// assert_eq!(LpJoinBound::new().ceiling_hypergraph(&tri), 1_000);
    ///
    /// // The same shape where each relation also carries its own columns:
    /// // every cover weight is forced to 1, so the ceiling is the product.
    /// let wide = vec![
    ///     HyperRelation::new(100, vec![0, 1]),
    ///     HyperRelation::new(100, vec![1, 2]),
    ///     HyperRelation::new(100, vec![2, 0]),
    /// ];
    /// assert_eq!(LpJoinBound::new().ceiling_hypergraph(&wide), 1_000_000);
    /// ```
    pub fn ceiling_hypergraph(&self, relations: &[HyperRelation]) -> u64 {
        let rows: Vec<u64> = relations.iter().map(|r| r.rows).collect();
        let product = ProductBound.ceiling(&rows, &[]);
        if relations.is_empty() {
            return 0;
        }
        // Any relation with private columns must be fully covered, so if
        // every relation has them the LP optimum is the product outright.
        if relations.iter().all(|r| r.has_private_attributes) {
            return product;
        }
        match self.solve_hypergraph(relations) {
            Some(value) => value.min(product),
            None => product,
        }
    }

    /// Build and solve the attribute-level cover LP. Returns `None` when
    /// the solver fails or produces a non-finite objective.
    fn solve_hypergraph(&self, relations: &[HyperRelation]) -> Option<u64> {
        use good_lp::{
            Expression, ProblemVariables, Solution, SolverModel, default_solver, variable,
        };

        let mut vars = ProblemVariables::new();
        let mut handles = Vec::with_capacity(relations.len());
        let mut objective = Expression::with_capacity(relations.len());

        for relation in relations {
            let v = vars.add(variable().min(0.0));
            handles.push(v);
            let size = relation.rows as f64;
            let coefficient = if size <= 1.0 { 0.0 } else { size.ln() };
            objective.add_mul(coefficient, v);
        }

        let mut model = vars.minimise(&objective).using(default_solver);

        // One cover constraint per distinct attribute.
        let attributes: std::collections::BTreeSet<u32> = relations
            .iter()
            .flat_map(|r| r.attributes.iter().copied())
            .collect();
        for attribute in attributes {
            let mut lhs = Expression::with_capacity(relations.len());
            let mut covered = false;
            for (idx, relation) in relations.iter().enumerate() {
                if relation.attributes.contains(&attribute) {
                    lhs.add_mul(1.0, handles[idx]);
                    covered = true;
                }
            }
            if covered {
                model = model.with(lhs.geq(1.0));
            }
        }

        // Private columns force a full unit of cover on their relation. A
        // relation exposing no join attribute at all is in the same
        // position: nothing can cover it, and under bag semantics it
        // multiplies the output by its own row count.
        for (idx, relation) in relations.iter().enumerate() {
            if relation.has_private_attributes || relation.attributes.is_empty() {
                let lhs: Expression = handles[idx].into();
                model = model.with(lhs.geq(1.0));
            }
        }

        let solution = model.solve().ok()?;
        let optimum = solution.eval(&objective).exp();
        if !optimum.is_finite() || optimum < 0.0 {
            return None;
        }
        let optimum = optimum.max(1.0);
        if optimum >= u64::MAX as f64 {
            return Some(u64::MAX);
        }
        // `exp(ln(n))` drifts; snap to the nearest integer when the value is
        // within a relative epsilon of it, otherwise round up.
        let rounded = optimum.round();
        let epsilon = 1e-9_f64.max(optimum.abs() * 1e-12);
        Some(if (optimum - rounded).abs() <= epsilon {
            rounded as u64
        } else {
            optimum.ceil() as u64
        })
    }
}

#[cfg(feature = "lp_solver")]
impl UpperBound for LpJoinBound {
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        self.ceiling(relations, equality_predicates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_bound_two_relations() {
        assert_eq!(ProductBound.ceiling(&[100, 200], &[]), 20_000);
    }

    #[test]
    fn product_bound_overflow_saturates() {
        assert_eq!(ProductBound.ceiling(&[u64::MAX, 2], &[]), u64::MAX);
    }

    #[test]
    fn product_bound_empty_relations() {
        assert_eq!(ProductBound.ceiling(&[], &[]), 1);
    }

    #[test]
    #[allow(deprecated)]
    fn agm_no_predicates_falls_back_to_product() {
        assert_eq!(AgmBound.ceiling(&[10, 20, 30], &[]), 10 * 20 * 30);
    }

    /// Since 1.2.0 the deprecated shortcut simply is the product: given
    /// only row counts and a predicate list there is nothing sound to
    /// gain, and the old `min * max` answer was below the truth for three
    /// or more relations.
    #[test]
    #[allow(deprecated)]
    fn agm_now_equals_the_product() {
        let r = [1_000u64, 1_000_000];
        assert_eq!(
            AgmBound.ceiling(&r, &[(0, 1)]),
            ProductBound.ceiling(&r, &[])
        );
        // The instance that exposed the defect: three 3-row relations on
        // one shared key value really do join to 27 rows.
        assert_eq!(AgmBound.ceiling(&[3, 3, 3], &[(0, 1), (1, 2)]), 27);
    }

    #[test]
    fn clamp_within_ceiling() {
        assert_eq!(clamp_estimate(500.0, 1000).unwrap(), 500);
    }

    #[test]
    fn clamp_exceeds_ceiling_errors() {
        let err = clamp_estimate(1500.0, 1000).unwrap_err();
        match err {
            Error::LpBoundExceeded { estimate, ceiling } => {
                assert_eq!(estimate, 1500.0);
                assert_eq!(ceiling, 1000.0);
            }
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn chain_bound_tighter_than_product() {
        // Two relations of 1000 rows each over a key with 100 distinct
        // values. At worst 1000 - 100 + 1 = 901 rows share one value, so
        // the ceiling is 1000 * 901 — below the product, and provable.
        let r = [1_000u64, 1_000];
        let cb = ChainBound::new(vec![100, 100]);
        let bound = cb.ceiling(&r, &[(0, 1)]);
        assert_eq!(bound, 901_000);
        let product = ProductBound.ceiling(&r, &[]);
        assert!(bound < product);
    }

    #[test]
    fn chain_bound_is_exact_on_a_foreign_key_join() {
        // The shape that dominates analytical workloads: a key side and a
        // fact side. maxdeg on the key side is 1, so the ceiling is the
        // fact table's row count exactly.
        let cb = ChainBound::new(vec![10, 10]);
        assert_eq!(cb.ceiling(&[10, 100], &[(0, 1)]), 100);
    }

    #[test]
    fn chain_bound_three_table_chain_stays_below_product() {
        // R0(1000) ⋈ R1(2000) ⋈ R2(500), 100 distinct join keys each.
        let r = [1_000u64, 2_000, 500];
        let cb = ChainBound::new(vec![100, 100, 100]);
        let bound = cb.ceiling(&r, &[(0, 1), (1, 2)]);
        let product = ProductBound.ceiling(&r, &[]);
        assert!(
            bound < product,
            "chain bound {bound} should be below product {product}"
        );
        // Sanity: still far above the old, unsound 100_000.
        assert!(bound > 100_000);
    }

    /// Regression guard for the v1.1 soundness defect. Two 20-row relations
    /// with 5 distinct keys and 16 rows piled on one of them really do join
    /// to 260 rows; the pre-1.2 formula returned 80.
    #[test]
    fn chain_bound_is_sound_under_skew() {
        let cb = ChainBound::new(vec![5, 5]);
        let bound = cb.ceiling(&[20, 20], &[(0, 1)]);
        assert!(
            bound >= 260,
            "skewed ceiling {bound} is below the true cardinality 260"
        );
    }

    #[test]
    fn chain_bound_no_predicates_falls_back() {
        let cb = ChainBound::new(vec![10, 20, 30]);
        assert_eq!(cb.ceiling(&[10, 20, 30], &[]), 10 * 20 * 30);
    }

    #[test]
    fn chain_bound_missing_distinct_count_defaults_to_one() {
        // No distinct count entry → defaults to 1, meaning no reduction.
        let cb = ChainBound::new(vec![]);
        let bound = cb.ceiling(&[100, 100], &[(0, 1)]);
        assert_eq!(bound, 10_000); // 100 * 100 / max(1, 1) = 10_000
    }

    #[test]
    fn saturating_clamp_saturates() {
        assert_eq!(saturating_clamp(500.0, 1000), 500);
        assert_eq!(saturating_clamp(2000.0, 1000), 1000);
        assert_eq!(saturating_clamp(-5.0, 1000), 0);
        assert_eq!(saturating_clamp(f64::NAN, 1000), 0);
    }
}

#[cfg(all(test, feature = "lp_solver"))]
mod lp_tests {
    use super::*;

    /// A 2-table join described only by row counts and "these two are
    /// joined" cannot be bounded below the product: every row of both
    /// relations may carry the same key. The pre-1.2 LP returned 1000 here,
    /// which a 1000 x 1_000_000 foreign-key join exceeds by three orders of
    /// magnitude.
    #[test]
    fn two_table_join_without_degrees_is_the_product() {
        let r = [1_000u64, 1_000_000u64];
        let lp = LpJoinBound::new();
        assert_eq!(lp.ceiling(&r, &[(0, 1)]), ProductBound.ceiling(&r, &[]));
    }

    /// With the attribute schema declared, the fractional-edge-cover LP is
    /// well posed and returns the textbook AGM triangle bound.
    #[test]
    fn triangle_hypergraph_matches_agm() {
        let tri = vec![
            HyperRelation::projected(1_000, vec![0, 1]),
            HyperRelation::projected(1_000, vec![1, 2]),
            HyperRelation::projected(1_000, vec![2, 0]),
        ];
        let bound = LpJoinBound::new().ceiling_hypergraph(&tri);
        // sqrt(1e9) = 31_622.77...
        assert!(
            (31_000u64..=32_000u64).contains(&bound),
            "expected ≈31_623, got {bound}"
        );
        assert!(bound < ProductBound.ceiling(&[1_000, 1_000, 1_000], &[]));
    }

    /// The same triangle where each relation also carries its own columns.
    /// Every cover weight is forced to 1, so the honest answer is the
    /// product — this is the case the pre-1.2 LP silently got wrong.
    #[test]
    fn triangle_with_private_columns_is_the_product() {
        let tri = vec![
            HyperRelation::new(1_000, vec![0, 1]),
            HyperRelation::new(1_000, vec![1, 2]),
            HyperRelation::new(1_000, vec![2, 0]),
        ];
        assert_eq!(
            LpJoinBound::new().ceiling_hypergraph(&tri),
            ProductBound.ceiling(&[1_000, 1_000, 1_000], &[])
        );
    }

    /// Square (4-cycle) over a projected hypergraph: AGM ρ* = 2, so equal
    /// relation sizes N give N².
    #[test]
    fn square_hypergraph_matches_agm() {
        let square = vec![
            HyperRelation::projected(100, vec![0, 1]),
            HyperRelation::projected(100, vec![1, 2]),
            HyperRelation::projected(100, vec![2, 3]),
            HyperRelation::projected(100, vec![3, 0]),
        ];
        let bound = LpJoinBound::new().ceiling_hypergraph(&square);
        assert!(
            (5_000..=15_000).contains(&bound),
            "expected ≈10_000, got {bound}"
        );
        assert!(bound < ProductBound.ceiling(&[100, 100, 100, 100], &[]));
    }

    /// A disconnected hypergraph decomposes: the LP optimum is the product
    /// of the per-component bounds.
    #[test]
    fn disconnected_components_multiply() {
        let graph = vec![
            HyperRelation::projected(100, vec![0]),
            HyperRelation::projected(200, vec![0]),
            HyperRelation::projected(50, vec![1]),
            HyperRelation::projected(70, vec![1]),
        ];
        let bound = LpJoinBound::new().ceiling_hypergraph(&graph);
        assert!(
            (4_900..=5_100).contains(&bound),
            "expected ≈5000, got {bound}"
        );
    }

    /// A relation exposing no join attribute cannot be covered by anything,
    /// so it must contribute its full row count.
    #[test]
    fn isolated_relation_contributes_row_count() {
        let graph = vec![
            HyperRelation::projected(100, vec![0]),
            HyperRelation::projected(200, vec![0]),
            HyperRelation::projected(99, Vec::new()),
        ];
        let bound = LpJoinBound::new().ceiling_hypergraph(&graph);
        assert!(
            (9_800..=10_000).contains(&bound),
            "expected ≈9_900, got {bound}"
        );
    }

    /// The hypergraph LP is capped at the Cartesian product in every case.
    #[test]
    fn hypergraph_never_exceeds_the_product() {
        let graph = vec![
            HyperRelation::projected(37, vec![0]),
            HyperRelation::new(41, vec![0, 1]),
            HyperRelation::projected(43, vec![1]),
        ];
        let bound = LpJoinBound::new().ceiling_hypergraph(&graph);
        assert!(bound <= ProductBound.ceiling(&[37, 41, 43], &[]));
    }

    /// The LP bound must never exceed the trivial product bound.
    #[test]
    fn lp_bound_dominates_product() {
        let r = [37u64, 41, 43, 47, 53];
        let preds = [(0usize, 1usize), (1, 2), (2, 3), (3, 4)];
        let lp = LpJoinBound::new();
        let bound = lp.ceiling(&r, &preds);
        let product = ProductBound.ceiling(&r, &preds);
        assert!(
            bound <= product,
            "LP bound {bound} must be ≤ product {product}"
        );
    }

    /// Empty relations → bound 0.
    #[test]
    fn empty_relations_zero() {
        let lp = LpJoinBound::new();
        assert_eq!(lp.ceiling(&[], &[]), 0);
    }

    /// No predicates → product bound (sanity passthrough).
    #[test]
    fn no_predicates_returns_product() {
        let lp = LpJoinBound::new();
        let r = [10u64, 20, 30];
        assert_eq!(lp.ceiling(&r, &[]), 6_000);
    }

    /// Distinct counts turn into a sound degree bound, so
    /// `ceiling_with_distinct` is tighter than the degree-free ceiling
    /// while staying above the truth.
    #[test]
    fn ceiling_with_distinct_is_at_most_unconstrained() {
        let r = [1_000u64, 1_000];
        let preds = [(0usize, 1usize)];
        let with_d = LpJoinBound::with_distinct_counts(vec![10, 10]);
        let a = with_d.ceiling_with_distinct(&r, &preds);
        let b = LpJoinBound::new().ceiling(&r, &preds);
        assert!(a <= b, "distinct-aware bound {a} must be tighter than {b}");
        // 1000 rows over 10 distinct values: at worst 991 share one value.
        assert_eq!(a, 991_000);
    }

    /// A key column collapses the ceiling to the other side's row count.
    #[test]
    fn ceiling_with_distinct_is_exact_on_a_key_join() {
        let bound = LpJoinBound::with_distinct_counts(vec![10, 10]);
        assert_eq!(bound.ceiling_with_distinct(&[10, 100], &[(0, 1)]), 100);
    }
}
