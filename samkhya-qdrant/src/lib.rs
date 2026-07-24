//! Provable match-count ceilings for filtered vector search.
//!
//! # The decision this exists to inform
//!
//! A filtered vector search — "find the 10 nearest vectors *where*
//! `category = 'shoes'`" — can be executed two ways, and picking wrong is
//! expensive:
//!
//! * **Pre-filter.** Materialise the matching points first, then compare
//!   every one of them exactly. Cost grows with the number of matches, and it
//!   is the right choice when the filter is selective.
//! * **Post-filter.** Run approximate nearest-neighbour search over the whole
//!   index, then discard non-matching results. Cost is roughly constant, but
//!   when the filter is selective the search wastes almost all of its work and
//!   may return too few surviving results to fill the requested `k`.
//!
//! Choosing between them requires knowing how many points match — before
//! matching them. Every engine that supports filtered search estimates this;
//! Qdrant, for one, keeps payload-index cardinality statistics for exactly
//! this decision.
//!
//! # Why an estimate is the wrong tool here
//!
//! The failure that hurts is *under*-estimating the match count: the planner
//! concludes the filter is selective, chooses pre-filtering, and then walks a
//! set far larger than it budgeted for. A two-sided estimate under-shoots
//! roughly half the time.
//!
//! A Count-Min sketch never *under*-counts a frequency. So for an equality
//! condition, its estimate is not merely a guess — it is a **provable upper
//! bound** on the number of matching points, and the planner can decide
//! against a number it knows the truth cannot exceed. That is the same
//! one-sided-error argument that makes samkhya's join ceiling provable, applied
//! to filter selectivity instead of join output.
//!
//! The bound composes soundly through boolean structure:
//!
//! | Filter | Sound ceiling on matches |
//! | ------ | ------------------------ |
//! | `A AND B` | `min(bound(A), bound(B))` — an intersection is no larger than either side |
//! | `A OR B`  | `min(total, bound(A) + bound(B))` — a union is no larger than the sum |
//! | `NOT A`   | `total` — excluding points cannot be bounded below `total` without a *lower* bound on `A`, which no sketch here provides |
//!
//! `NOT` is deliberately weak rather than quietly wrong. A ceiling that is
//! loose costs a suboptimal plan; one that is unsound costs a wrong one.
//!
//! # Scope
//!
//! This crate computes bounds and recommends a strategy. It does not link
//! Qdrant, run a server, or execute a search — the decision surface is
//! deliberately a pure function so it can be tested against brute force and
//! embedded wherever the caller likes.
//!
//! # Example
//!
//! ```
//! use samkhya_qdrant::{Condition, Filter, PayloadStats, SearchStrategy, StrategyParams};
//! use samkhya_core::sketches::CountMinSketch;
//!
//! let mut category = CountMinSketch::with_defaults();
//! for _ in 0..40 { category.add(b"shoes", 1); }
//! for _ in 0..9_960 { category.add(b"other", 1); }
//!
//! let stats = PayloadStats::new(10_000).with_field("category", category);
//!
//! let filter = Filter::must(vec![Condition::match_value("category", "shoes")]);
//! let ceiling = stats.bound_matches(&filter);
//! assert!(ceiling >= 40, "the ceiling must not fall below the truth");
//!
//! // Few enough matches that scanning them exactly beats an index search.
//! assert_eq!(
//!     stats.choose_strategy(&filter, &StrategyParams::default()),
//!     SearchStrategy::PreFilter
//! );
//! ```

use std::collections::BTreeMap;

use samkhya_core::sketches::CountMinSketch;

/// A single condition over one payload field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `field == value`.
    Match { field: String, value: String },
    /// `field IN (values...)`. Bounded as the sum of the per-value bounds,
    /// capped at the collection size.
    Any { field: String, values: Vec<String> },
    /// A condition this crate cannot bound below the collection size — an
    /// unindexed field, a range over a field with no histogram, a geo query.
    /// Represented explicitly so an unbounded condition is a visible choice
    /// rather than a silent default.
    Unbounded,
}

impl Condition {
    /// `field == value`.
    pub fn match_value(field: impl Into<String>, value: impl Into<String>) -> Self {
        Condition::Match {
            field: field.into(),
            value: value.into(),
        }
    }

    /// `field IN (values...)`.
    pub fn any_of<I, S>(field: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Condition::Any {
            field: field.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }
}

/// A boolean combination of conditions, shaped like the filter clauses a
/// vector database exposes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// All of these must hold.
    pub must: Vec<Condition>,
    /// At least one of these must hold. Empty means "no disjunctive clause",
    /// not "nothing matches".
    pub should: Vec<Condition>,
    /// None of these may hold.
    pub must_not: Vec<Condition>,
}

impl Filter {
    /// A conjunction.
    pub fn must(conditions: Vec<Condition>) -> Self {
        Filter {
            must: conditions,
            ..Default::default()
        }
    }

    /// A disjunction.
    pub fn should(conditions: Vec<Condition>) -> Self {
        Filter {
            should: conditions,
            ..Default::default()
        }
    }

    /// A negation.
    pub fn must_not(conditions: Vec<Condition>) -> Self {
        Filter {
            must_not: conditions,
            ..Default::default()
        }
    }

    /// Whether this filter constrains anything at all.
    pub fn is_empty(&self) -> bool {
        self.must.is_empty() && self.should.is_empty() && self.must_not.is_empty()
    }
}

/// Per-field frequency sketches for one collection.
#[derive(Debug, Default)]
pub struct PayloadStats {
    total_points: u64,
    fields: BTreeMap<String, CountMinSketch>,
}

impl PayloadStats {
    /// Statistics for a collection of `total_points` points, with no fields
    /// indexed yet.
    pub fn new(total_points: u64) -> Self {
        Self {
            total_points,
            fields: BTreeMap::new(),
        }
    }

    /// Attach a Count-Min sketch over one payload field's values.
    ///
    /// The sketch must have been built by adding one entry per point, so its
    /// estimates bound point counts rather than something else.
    pub fn with_field(mut self, field: impl Into<String>, sketch: CountMinSketch) -> Self {
        self.fields.insert(field.into(), sketch);
        self
    }

    /// Number of points in the collection.
    pub fn total_points(&self) -> u64 {
        self.total_points
    }

    /// A provable upper bound on how many points a single condition matches.
    ///
    /// Falls back to `total_points` whenever the sketch is missing or has
    /// saturated — saturation is the one condition that breaks Count-Min's
    /// never-undercount guarantee, and a bound that rests on a broken
    /// guarantee is not a bound.
    pub fn bound_condition(&self, condition: &Condition) -> u64 {
        match condition {
            Condition::Unbounded => self.total_points,
            Condition::Match { field, value } => self.bound_value(field, value),
            Condition::Any { field, values } => {
                // A union over values: no larger than the sum, and never
                // larger than the collection.
                let mut sum: u64 = 0;
                for value in values {
                    sum = sum.saturating_add(self.bound_value(field, value));
                    if sum >= self.total_points {
                        return self.total_points;
                    }
                }
                sum.min(self.total_points)
            }
        }
    }

    fn bound_value(&self, field: &str, value: &str) -> u64 {
        let Some(sketch) = self.fields.get(field) else {
            return self.total_points;
        };
        if sketch.is_saturated() {
            return self.total_points;
        }
        u64::from(sketch.estimate(value.as_bytes())).min(self.total_points)
    }

    /// A provable upper bound on how many points the whole filter matches.
    ///
    /// See the [module documentation](self) for the composition rules and why
    /// `must_not` cannot tighten the bound.
    pub fn bound_matches(&self, filter: &Filter) -> u64 {
        if filter.is_empty() {
            return self.total_points;
        }

        let mut bound = self.total_points;

        // Conjunction: the intersection is no larger than its smallest term.
        for condition in &filter.must {
            bound = bound.min(self.bound_condition(condition));
        }

        // Disjunction: the union is no larger than the sum of its terms. Only
        // tightens the result if the sum is genuinely smaller.
        if !filter.should.is_empty() {
            let mut sum: u64 = 0;
            for condition in &filter.should {
                sum = sum.saturating_add(self.bound_condition(condition));
                if sum >= self.total_points {
                    sum = self.total_points;
                    break;
                }
            }
            bound = bound.min(sum);
        }

        // Negation cannot tighten an upper bound without a lower bound on the
        // excluded set, which no sketch here provides. Left deliberately loose.
        bound
    }

    /// Recommend an execution strategy for a filtered search.
    pub fn choose_strategy(&self, filter: &Filter, params: &StrategyParams) -> SearchStrategy {
        let bound = self.bound_matches(filter);
        if bound <= params.prefilter_max_points {
            return SearchStrategy::PreFilter;
        }
        let fraction = if self.total_points == 0 {
            1.0
        } else {
            bound as f64 / self.total_points as f64
        };
        if fraction <= params.prefilter_max_fraction {
            SearchStrategy::PreFilter
        } else {
            SearchStrategy::PostFilter
        }
    }
}

/// How a filtered search should be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStrategy {
    /// Materialise the matching points and compare them exactly.
    PreFilter,
    /// Search the index, then discard non-matching results.
    PostFilter,
}

/// Thresholds for [`PayloadStats::choose_strategy`].
///
/// Both are compared against the *ceiling*, never an estimate, so a
/// `PreFilter` recommendation means "the matching set provably fits in this
/// budget" rather than "it probably does".
#[derive(Debug, Clone, Copy)]
pub struct StrategyParams {
    /// Pre-filter whenever the matching set provably fits in this many points.
    pub prefilter_max_points: u64,
    /// Pre-filter whenever it provably covers no more than this fraction of
    /// the collection.
    pub prefilter_max_fraction: f64,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            prefilter_max_points: 1_000,
            prefilter_max_fraction: 0.01,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a sketch over a value distribution given as (value, count).
    fn sketch(entries: &[(&str, u32)]) -> CountMinSketch {
        let mut cms = CountMinSketch::with_defaults();
        for (value, count) in entries {
            cms.add(value.as_bytes(), *count);
        }
        cms
    }

    fn shoes_stats() -> PayloadStats {
        PayloadStats::new(10_000).with_field(
            "category",
            sketch(&[("shoes", 40), ("shirts", 60), ("other", 9_900)]),
        )
    }

    #[test]
    fn an_equality_bound_never_falls_below_the_truth() {
        let stats = shoes_stats();
        let filter = Filter::must(vec![Condition::match_value("category", "shoes")]);
        assert!(stats.bound_matches(&filter) >= 40);
    }

    #[test]
    fn a_selective_filter_recommends_prefiltering() {
        let stats = shoes_stats();
        let filter = Filter::must(vec![Condition::match_value("category", "shoes")]);
        assert_eq!(
            stats.choose_strategy(&filter, &StrategyParams::default()),
            SearchStrategy::PreFilter
        );
    }

    #[test]
    fn an_unselective_filter_recommends_postfiltering() {
        let stats = shoes_stats();
        let filter = Filter::must(vec![Condition::match_value("category", "other")]);
        assert_eq!(
            stats.choose_strategy(&filter, &StrategyParams::default()),
            SearchStrategy::PostFilter
        );
    }

    #[test]
    fn a_conjunction_takes_the_tightest_term() {
        let stats = PayloadStats::new(10_000)
            .with_field("category", sketch(&[("shoes", 40), ("other", 9_960)]))
            .with_field("colour", sketch(&[("red", 500), ("other", 9_500)]));
        let filter = Filter::must(vec![
            Condition::match_value("category", "shoes"),
            Condition::match_value("colour", "red"),
        ]);
        // The intersection cannot exceed the smaller side.
        assert!(stats.bound_matches(&filter) <= 500);
        assert!(stats.bound_matches(&filter) >= 40);
    }

    #[test]
    fn a_disjunction_is_bounded_by_the_sum() {
        let stats = PayloadStats::new(10_000).with_field(
            "category",
            sketch(&[("shoes", 40), ("shirts", 60), ("other", 9_900)]),
        );
        let filter = Filter::should(vec![
            Condition::match_value("category", "shoes"),
            Condition::match_value("category", "shirts"),
        ]);
        let bound = stats.bound_matches(&filter);
        assert!(bound >= 100, "union of 40 and 60 is 100");
        assert!(bound <= 10_000);
    }

    #[test]
    fn any_of_is_a_union_over_values() {
        let stats = shoes_stats();
        let filter = Filter::must(vec![Condition::any_of("category", ["shoes", "shirts"])]);
        assert!(stats.bound_matches(&filter) >= 100);
    }

    #[test]
    fn negation_does_not_tighten_the_bound() {
        let stats = shoes_stats();
        let filter = Filter::must_not(vec![Condition::match_value("category", "shoes")]);
        // Excluding 40 of 10,000 leaves 9,960 — and without a lower bound on
        // the excluded set the honest ceiling is the whole collection.
        assert_eq!(stats.bound_matches(&filter), 10_000);
    }

    #[test]
    fn an_unindexed_field_falls_back_to_the_collection_size() {
        let stats = shoes_stats();
        let filter = Filter::must(vec![Condition::match_value("not_indexed", "x")]);
        assert_eq!(stats.bound_matches(&filter), 10_000);
        assert_eq!(
            stats.choose_strategy(&filter, &StrategyParams::default()),
            SearchStrategy::PostFilter
        );
    }

    #[test]
    fn an_empty_filter_bounds_at_the_collection_size() {
        let stats = shoes_stats();
        assert_eq!(stats.bound_matches(&Filter::default()), 10_000);
    }

    #[test]
    fn a_saturated_sketch_gives_up_rather_than_lying() {
        let mut cms = CountMinSketch::with_defaults();
        cms.add(b"hot", u32::MAX);
        let stats = PayloadStats::new(10_000).with_field("category", cms);
        let filter = Filter::must(vec![Condition::match_value("category", "anything")]);
        // Saturation breaks the never-undercount guarantee, so the bound must
        // fall back rather than rest on it.
        assert_eq!(stats.bound_matches(&filter), 10_000);
    }

    /// The headline property, checked against brute force: for a collection
    /// whose contents we control, the ceiling is never below the true match
    /// count, across a range of selectivities.
    #[test]
    fn the_ceiling_dominates_brute_force_across_selectivities() {
        for hot in [1u32, 5, 50, 500, 5_000] {
            let total = 10_000u64;
            let cold = total as u32 - hot;
            let stats =
                PayloadStats::new(total).with_field("f", sketch(&[("hot", hot), ("cold", cold)]));

            let filter = Filter::must(vec![Condition::match_value("f", "hot")]);
            let bound = stats.bound_matches(&filter);
            assert!(
                bound >= u64::from(hot),
                "ceiling {bound} fell below the true match count {hot}"
            );
            assert!(bound <= total, "ceiling {bound} exceeded the collection");
        }
    }
}
