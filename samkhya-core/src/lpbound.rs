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

/// Frequency-moment chain-join upper bound.
///
/// Assumes each equality predicate `(i, j)` joins on a single key whose
/// distinct-value count is given by `distinct_counts[i]` and
/// `distinct_counts[j]`. The bound is:
///
/// ```text
/// |R_i ⋈ R_j| ≤ |R_i| * |R_j| / max(D_i, D_j)
/// ```
///
/// (Uniform-distribution worst case; tight in expectation when join
/// keys are evenly spread.) Applied sequentially across all equality
/// predicates: the result of each join feeds the next bound.
///
/// Tighter than [`AgmBound`] for tree / chain joins where each relation
/// has a non-trivial distinct-key count. Falls back to [`ProductBound`]
/// when no equality predicates are supplied.
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
    /// // Two 1000-row relations, joining on a key with 100 distinct values:
    /// // ceiling = 1000 * 1000 / max(100, 100) = 10_000.
    /// let cb = ChainBound::new(vec![100, 100]);
    /// assert_eq!(cb.ceiling(&[1_000, 1_000], &[(0, 1)]), 10_000);
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
        // Each predicate divides the running product by the larger of
        // the two endpoint distinct counts (or 1 if unknown).
        let mut bound: u128 = relations
            .iter()
            .fold(1u128, |acc, &n| acc.saturating_mul(n as u128));
        for &(i, j) in equality_predicates {
            let d_i = self.distinct_counts.get(i).copied().unwrap_or(1).max(1) as u128;
            let d_j = self.distinct_counts.get(j).copied().unwrap_or(1).max(1) as u128;
            let d = d_i.max(d_j);
            bound /= d;
        }
        if bound > u64::MAX as u128 {
            u64::MAX
        } else {
            bound as u64
        }
    }
}

/// Coarse AGM-style upper bound for equi-joins.
///
/// Returns `min(product, |R_min| * |R_max|)` when at least one equality
/// predicate exists, otherwise falls back to [`ProductBound`]. This is a
/// placeholder approximation; the true AGM / LpBound bound requires
/// fractional edge cover / LP relaxation — see `LpJoinBound` (under the
/// `lp_solver` feature) for the principled construction.
///
/// # Examples
///
/// ```
/// use samkhya_core::lpbound::{AgmBound, ProductBound, UpperBound};
///
/// let r = [1_000u64, 1_000_000];
/// let bound = AgmBound.ceiling(&r, &[(0, 1)]);
/// // AGM is always at least as tight as the cartesian product.
/// assert!(bound <= ProductBound.ceiling(&r, &[]));
/// ```
pub struct AgmBound;

impl UpperBound for AgmBound {
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }
        let product: u64 = relations.iter().fold(1u64, |acc, &n| acc.saturating_mul(n));
        let min_r = *relations.iter().min().unwrap_or(&0);
        let max_r = *relations.iter().max().unwrap_or(&0);
        product.min(min_r.saturating_mul(max_r))
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
    pub fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        self.solve(relations, equality_predicates, /*use_distinct=*/ false)
    }

    /// Like [`Self::ceiling`] but folds the supplied distinct counts
    /// into the per-relation objective coefficient. If the supplied
    /// vector is shorter than `relations` or contains zero entries the
    /// missing entries fall back to the row count.
    pub fn ceiling_with_distinct(
        &self,
        relations: &[u64],
        equality_predicates: &[(usize, usize)],
    ) -> u64 {
        self.solve(relations, equality_predicates, /*use_distinct=*/ true)
    }

    /// Core solver: build the per-connected-component LP, solve each,
    /// and multiply the per-component ceilings. Falls back to
    /// [`ProductBound`] / [`AgmBound`] if the solver fails for any
    /// reason — the envelope must never crash the engine.
    fn solve(
        &self,
        relations: &[u64],
        equality_predicates: &[(usize, usize)],
        use_distinct: bool,
    ) -> u64 {
        if relations.is_empty() {
            return 0;
        }
        if equality_predicates.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }

        // Validate predicate indices; defensively drop any out-of-range
        // pair. A misbuilt join graph must never crash the envelope.
        let n = relations.len();
        let preds: Vec<(usize, usize)> = equality_predicates
            .iter()
            .copied()
            .filter(|&(i, j)| i < n && j < n && i != j)
            .collect();
        if preds.is_empty() {
            return ProductBound.ceiling(relations, &[]);
        }

        // Build connected components over the relation graph induced by
        // the equality predicates. Each component's LP is independent;
        // we solve them separately and multiply the ceilings.
        let components = connected_components(n, &preds);
        let mut total: u128 = 1;
        for comp in &components {
            let ceil = self.solve_component(relations, &preds, comp, use_distinct);
            total = total.saturating_mul(ceil as u128);
            if total >= u64::MAX as u128 {
                return u64::MAX;
            }
        }
        total as u64
    }

    /// Solve the AGM LP restricted to a single connected component.
    fn solve_component(
        &self,
        relations: &[u64],
        all_predicates: &[(usize, usize)],
        component: &[usize],
        use_distinct: bool,
    ) -> u64 {
        // Singleton component (no predicate incident): output cardinality
        // is just the relation size.
        if component.len() == 1 {
            return relations[component[0]];
        }

        // Subset of predicates whose endpoints both lie in this
        // component. (Every predicate connecting two members of a
        // component is in the component by definition; we filter only
        // for safety.)
        let in_comp: std::collections::HashSet<usize> = component.iter().copied().collect();
        let comp_preds: Vec<(usize, usize)> = all_predicates
            .iter()
            .copied()
            .filter(|&(i, j)| in_comp.contains(&i) && in_comp.contains(&j))
            .collect();

        // Build the LP via `good_lp`. One x_r per relation in the
        // component; one >= 1 constraint per equality predicate
        // (each predicate introduces a distinct shared attribute).
        use good_lp::{
            Expression, ProblemVariables, Solution, SolverModel, default_solver, variable,
        };

        let mut vars = ProblemVariables::new();
        // Map: relation index in the global `relations` vector ->
        // good_lp variable handle.
        let mut var_for: std::collections::HashMap<usize, good_lp::Variable> =
            std::collections::HashMap::with_capacity(component.len());
        // Build the objective expression in tandem with adding
        // variables so coefficients line up with relation order.
        let mut objective = Expression::with_capacity(component.len());
        for &r in component {
            let v = vars.add(variable().min(0.0));
            var_for.insert(r, v);
            let row_count = relations[r];
            // Coefficient = log(effective size). Effective size = row
            // count, optionally clamped to the supplied distinct count
            // (which can only shrink it).
            let mut size_f = row_count as f64;
            if use_distinct {
                if let Some(&d) = self.distinct_counts.get(r) {
                    if d > 0 {
                        size_f = size_f.min(d as f64);
                    }
                }
            }
            // log(0) is undefined; we treat an empty relation as
            // contributing 0 to the log-sum (the join is empty anyway).
            // log(1) is 0 and would let the LP put unbounded weight on
            // that variable without paying — we clamp the coefficient
            // away from zero with a tiny epsilon so the objective is
            // strictly minimised.
            let coef = if size_f <= 1.0 { 0.0 } else { size_f.ln() };
            objective.add_mul(coef, v);
        }

        // Add one >= 1 fractional-cover constraint per predicate.
        // good_lp's `Expression >> rhs` operator builds a >= constraint.
        let mut model = vars.minimise(&objective).using(default_solver);
        for &(i, j) in &comp_preds {
            let xi = var_for[&i];
            let xj = var_for[&j];
            let lhs: Expression = xi + xj;
            model = model.with(lhs.geq(1.0));
        }

        match model.solve() {
            Ok(sol) => {
                let lp_min = sol.eval(&objective);
                // exp(LP_min) is the AGM ceiling. Guard against
                // negative noise from the simplex and against overflow.
                let raw = lp_min.exp();
                if !raw.is_finite() || raw < 0.0 {
                    return self.fallback(relations, &comp_preds, component);
                }
                // Always at least 1 (a join with at least one row in
                // both endpoints can return one row); cap at u64::MAX.
                let raw = raw.max(1.0);
                if raw >= u64::MAX as f64 {
                    u64::MAX
                } else {
                    // Snap to nearest integer when within a tight
                    // relative epsilon: `exp(ln(n))` for integer `n`
                    // can drift to `n + 1e-12` and a blind ceil would
                    // push the per-component bound a full integer
                    // above the true AGM optimum, breaking the
                    // contract that LpJoinBound <= AgmBound. Only
                    // ceil when the LP value is materially above the
                    // nearest integer.
                    let rounded = raw.round();
                    let snap_eps = 1e-9_f64.max(raw.abs() * 1e-12);
                    if (raw - rounded).abs() <= snap_eps {
                        rounded as u64
                    } else {
                        raw.ceil() as u64
                    }
                }
            }
            Err(_) => self.fallback(relations, &comp_preds, component),
        }
    }

    /// Conservative fallback when the LP solver fails. Returns the
    /// minimum row count among the component's relations (a valid AGM
    /// upper bound when at least one predicate covers every relation in
    /// the component), or [`ProductBound`] over the component otherwise.
    fn fallback(
        &self,
        relations: &[u64],
        comp_preds: &[(usize, usize)],
        component: &[usize],
    ) -> u64 {
        if comp_preds.is_empty() {
            return component
                .iter()
                .map(|&r| relations[r])
                .fold(1u64, |a, n| a.saturating_mul(n));
        }
        let comp_rows: Vec<u64> = component.iter().map(|&r| relations[r]).collect();
        let agm = AgmBound;
        agm.ceiling(&comp_rows, &[(0, 1)])
    }
}

#[cfg(feature = "lp_solver")]
impl UpperBound for LpJoinBound {
    fn ceiling(&self, relations: &[u64], equality_predicates: &[(usize, usize)]) -> u64 {
        self.ceiling(relations, equality_predicates)
    }
}

/// Return the connected components of the graph on `0..n` with edges
/// given by `edges`. Each component is a sorted list of vertex indices.
/// Singleton vertices (no incident edge) appear as one-element
/// components — every relation index in `0..n` is in exactly one
/// component.
#[cfg(feature = "lp_solver")]
fn connected_components(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for &(a, b) in edges {
        if a >= n || b >= n {
            continue;
        }
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for v in 0..n {
        let r = find(&mut parent, v);
        groups.entry(r).or_default().push(v);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    for c in &mut out {
        c.sort_unstable();
    }
    // Sort components by their smallest member for deterministic order.
    out.sort_by_key(|c| c[0]);
    out
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
    fn agm_no_predicates_falls_back_to_product() {
        assert_eq!(AgmBound.ceiling(&[10, 20, 30], &[]), 10 * 20 * 30);
    }

    #[test]
    fn agm_with_predicates_tighter_than_product() {
        let r = [1_000u64, 1_000_000];
        let bound = AgmBound.ceiling(&r, &[(0, 1)]);
        let product = ProductBound.ceiling(&r, &[]);
        assert!(bound <= product);
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
        // Two relations of 1000 rows each, joining on a key with 100 distinct values.
        // Product = 1_000_000; ChainBound = 1000 * 1000 / 100 = 10_000.
        let r = [1_000u64, 1_000];
        let cb = ChainBound::new(vec![100, 100]);
        let bound = cb.ceiling(&r, &[(0, 1)]);
        assert_eq!(bound, 10_000);
        let product = ProductBound.ceiling(&r, &[]);
        assert!(bound < product);
    }

    #[test]
    fn chain_bound_three_table_chain() {
        // R1(1000) ⋈ R2(2000) ⋈ R3(500), join keys 100 distinct each side.
        // Product = 1e9. Chain = 1e9 / 100 / 100 = 100_000.
        let r = [1_000u64, 2_000, 500];
        let cb = ChainBound::new(vec![100, 100, 100]);
        let bound = cb.ceiling(&r, &[(0, 1), (1, 2)]);
        assert_eq!(bound, 100_000);
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

    /// 2-table single-edge join: the LP returns the principled AGM
    /// bound `min(|R_0|, |R_1|)` and is therefore a valid (tighter than
    /// or equal) refinement of the coarse [`AgmBound`] approximation,
    /// which for two relations reduces to `|R_0| * |R_1|`.
    #[test]
    fn two_table_join_matches_principled_agm() {
        let r = [1_000u64, 1_000_000u64];
        let lp = LpJoinBound::new();
        let bound = lp.ceiling(&r, &[(0, 1)]);
        // True AGM single-edge bound is min(|R_0|, |R_1|).
        // Allow ceil()'s +/-1 floating-point noise.
        assert!(
            (999..=1_001).contains(&bound),
            "expected ≈1000, got {bound}"
        );
        // And the LP bound must never exceed the coarse AGM bound it
        // replaces (validity / refinement contract).
        let coarse = AgmBound.ceiling(&r, &[(0, 1)]);
        assert!(
            bound <= coarse,
            "LP bound {bound} must not exceed coarse AGM {coarse}"
        );
    }

    /// Triangle: 3 relations, 3 equality predicates each on a distinct
    /// shared attribute. The fractional AGM cover number is ρ\* = 3/2,
    /// so the LP bound is `(|R_0| * |R_1| * |R_2|)^{1/2}`.
    #[test]
    fn triangle_strictly_tighter_than_chain_and_product() {
        // Use round numbers so the closed-form expectation is exact.
        let r = [1_000u64, 1_000u64, 1_000u64];
        let preds = [(0usize, 1usize), (1, 2), (0, 2)];
        let lp = LpJoinBound::new();
        let bound = lp.ceiling(&r, &preds);

        // sqrt(1e9) ≈ 31_622.78  → expect 31_623 (after ceil).
        assert!(
            (31_000u64..=32_000u64).contains(&bound),
            "expected ≈31_623, got {bound}"
        );

        // Strictly tighter than product = 1e9.
        let product = ProductBound.ceiling(&r, &preds);
        assert!(bound < product, "LP {bound} should be < product {product}");

        // Strictly tighter than the chain bound under realistic
        // distinct-count hints (≈10 distinct join keys per relation —
        // matches the regime where ChainBound is meaningful but not
        // pathologically optimistic).
        let cb = ChainBound::new(vec![10, 10, 10]);
        let chain = cb.ceiling(&r, &preds);
        assert!(
            bound < chain,
            "LP {bound} should be < chain {chain} on the triangle"
        );
    }

    /// Square (4-cycle): R_0 — R_1 — R_2 — R_3 — R_0. AGM ρ\* = 2, so
    /// the LP bound is `sqrt(|R_0|*|R_2|*|R_1|*|R_3|)` ≈ `(N)^2` for
    /// equally sized N, vs the product = N^4.
    #[test]
    fn square_strictly_tighter_than_chain_and_product() {
        let r = [100u64, 100u64, 100u64, 100u64];
        let preds = [(0usize, 1usize), (1, 2), (2, 3), (3, 0)];
        let lp = LpJoinBound::new();
        let bound = lp.ceiling(&r, &preds);

        // 4-cycle AGM optimum is ρ* = 2 (alternating x = 1, 0, 1, 0 or
        // x = 1/2 each); for equal sizes this gives N^2 = 10_000.
        // Allow a generous numerical tolerance.
        assert!(
            (5_000..=15_000).contains(&bound),
            "expected ≈10_000, got {bound}"
        );

        // Strictly tighter than product = 1e8.
        let product = ProductBound.ceiling(&r, &preds);
        assert!(bound < product, "LP {bound} should be < product {product}");

        // Strictly tighter than the chain bound under modest
        // distinct-count hints. d=4 per relation, 4 predicates →
        // chain = 100^4 / 4^4 = 1e8 / 256 ≈ 390_625, which is
        // looser than the LP's ≈10_000.
        let cb = ChainBound::new(vec![4, 4, 4, 4]);
        let chain = cb.ceiling(&r, &preds);
        assert!(
            bound < chain,
            "LP {bound} should be < chain {chain} on the 4-cycle"
        );
    }

    /// Disconnected join graph: two independent 2-table joins.
    /// The LP decomposes into one LP per connected component, and the
    /// total bound is the product of the per-component bounds.
    #[test]
    fn disconnected_components_multiply() {
        // Component A: R_0 ⋈ R_1 (relations of sizes 100, 200, single
        // predicate). Per-component AGM bound = min(100, 200) = 100.
        // Component B: R_2 ⋈ R_3 (sizes 50, 70). Per-component bound
        // = min(50, 70) = 50. Total expected ≈ 100 * 50 = 5_000.
        let r = [100u64, 200, 50, 70];
        let preds = [(0usize, 1usize), (2, 3)];
        let lp = LpJoinBound::new();
        let bound = lp.ceiling(&r, &preds);
        assert!(
            (4_900..=5_100).contains(&bound),
            "expected ≈5000, got {bound}"
        );
    }

    /// Singleton relation (no incident predicate) keeps its row count in
    /// the product of component bounds.
    #[test]
    fn singleton_component_contributes_row_count() {
        let r = [100u64, 200, 99];
        // Only R_0 and R_1 are joined; R_2 is isolated.
        let preds = [(0usize, 1usize)];
        let lp = LpJoinBound::new();
        let bound = lp.ceiling(&r, &preds);
        // Component {0,1}: min(100, 200) = 100. Component {2}: 99.
        // Total ≈ 9_900.
        assert!(
            (9_800..=10_000).contains(&bound),
            "expected ≈9_900, got {bound}"
        );
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

    /// `ceiling_with_distinct` clamps the per-relation objective
    /// coefficient by `min(|R|, D)`. With a tight distinct-count hint
    /// the bound only gets smaller (tighter).
    #[test]
    fn ceiling_with_distinct_is_at_most_unconstrained() {
        let r = [1_000u64, 1_000];
        let preds = [(0usize, 1usize)];
        let with_d = LpJoinBound::with_distinct_counts(vec![10, 10]);
        let unconstrained = LpJoinBound::new();
        let a = with_d.ceiling_with_distinct(&r, &preds);
        let b = unconstrained.ceiling(&r, &preds);
        assert!(a <= b, "distinct-aware bound {a} must be tighter than {b}");
        // With 10 distinct values on each side the bound collapses to 10.
        assert!(a <= 11, "expected ≈10 with D=10, got {a}");
    }
}
