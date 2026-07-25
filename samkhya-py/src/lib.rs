//! samkhya — Python bindings via PyO3.
//!
//! Exposes the core sketch and LpBound APIs to Python so dbt-style users
//! can consume samkhya's portable, feedback-driven cardinality correction
//! primitives without a Rust toolchain.
//!
//! The compiled module is `samkhya._native`; the `samkhya` package
//! re-exports its public API so `samkhya.HllSketch(14)` works directly.

#![deny(rustdoc::broken_intra_doc_links)]

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyType};

use serde::{Deserialize, Serialize};

use samkhya_core::Error as CoreError;
use samkhya_core::degree::{AttributeDegree, JoinGraph, JoinRelation};
use samkhya_core::lpbound::{ProductBound, UpperBound};
use samkhya_core::sketches::{
    BloomFilter as CoreBloom, CountMinSketch as CoreCms, EquiDepthHistogram as CoreHistogram,
    HllSketch as CoreHll, Sketch,
};

create_exception!(samkhya, SamkhyaError, PyException);

fn map_err(e: CoreError) -> PyErr {
    SamkhyaError::new_err(e.to_string())
}

// =============================================================================
// HllSketch
// =============================================================================

/// HyperLogLog cardinality sketch.
///
/// Precision `p` controls register count (`2^p`) and relative error
/// (~ 1.04 / sqrt(2^p)). Valid range: `p` in `[4, 18]`.
#[pyclass(module = "samkhya", name = "HllSketch", from_py_object)]
#[derive(Clone)]
pub struct PyHllSketch {
    inner: CoreHll,
}

#[pymethods]
impl PyHllSketch {
    #[new]
    fn new(p: u8) -> PyResult<Self> {
        CoreHll::new(p).map(|inner| Self { inner }).map_err(map_err)
    }

    /// Add a single item (raw bytes) to the sketch.
    fn add(&mut self, item: &[u8]) {
        self.inner.add(item);
    }

    /// Return the current cardinality estimate as a float.
    fn estimate(&self) -> f64 {
        self.inner.estimate() as f64
    }

    /// Merge another sketch (must share precision) into `self`.
    fn merge(&mut self, other: &PyHllSketch) -> PyResult<()> {
        self.inner.merge(&other.inner).map_err(map_err)
    }

    /// Precision parameter (number of register-index bits).
    #[getter]
    fn precision(&self) -> u8 {
        self.inner.precision()
    }

    /// Serialize the sketch to a portable byte payload.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.to_bytes().map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserialize a sketch produced by `to_bytes`.
    #[classmethod]
    fn from_bytes(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
        CoreHll::from_bytes(data)
            .map(|inner| Self { inner })
            .map_err(map_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "HllSketch(precision={}, estimate={})",
            self.inner.precision(),
            self.inner.estimate()
        )
    }
}

// =============================================================================
// BloomFilter
// =============================================================================

/// Bloom filter sized for `n_items` at the given false-positive rate.
#[pyclass(module = "samkhya", name = "BloomFilter", from_py_object)]
#[derive(Clone)]
pub struct PyBloomFilter {
    inner: CoreBloom,
}

#[pymethods]
impl PyBloomFilter {
    #[new]
    fn new(n_items: usize, fp_rate: f64) -> Self {
        Self {
            inner: CoreBloom::new(n_items, fp_rate),
        }
    }

    /// Insert an item (raw bytes) into the filter.
    fn add(&mut self, item: &[u8]) {
        self.inner.insert(item);
    }

    /// Return `True` if the filter may contain the item (subject to fp_rate),
    /// `False` if it definitely does not.
    fn contains(&self, item: &[u8]) -> bool {
        self.inner.contains(item)
    }

    /// Total number of bits in the underlying bit vector.
    #[getter]
    fn num_bits(&self) -> u64 {
        self.inner.num_bits()
    }

    /// Number of hash functions per insert / lookup.
    #[getter]
    fn num_hashes(&self) -> u32 {
        self.inner.num_hashes()
    }

    /// Serialize the filter to a portable byte payload.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.to_bytes().map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserialize a filter produced by `to_bytes`.
    #[classmethod]
    fn from_bytes(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
        CoreBloom::from_bytes(data)
            .map(|inner| Self { inner })
            .map_err(map_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "BloomFilter(num_bits={}, num_hashes={})",
            self.inner.num_bits(),
            self.inner.num_hashes()
        )
    }
}

// =============================================================================
// CountMinSketch
// =============================================================================

/// Count-Min Sketch — frequency estimation for skewed value detection.
///
/// `width` is the number of counters per row; `depth` is the number of
/// rows (independent hash functions). Memory: `4 * width * depth` bytes.
#[pyclass(module = "samkhya", name = "CountMinSketch", from_py_object)]
#[derive(Clone)]
pub struct PyCountMinSketch {
    inner: CoreCms,
}

#[pymethods]
impl PyCountMinSketch {
    #[new]
    fn new(width: usize, depth: usize) -> PyResult<Self> {
        let depth_u32: u32 = depth
            .try_into()
            .map_err(|_| SamkhyaError::new_err(format!("depth {depth} exceeds u32::MAX")))?;
        let width_u32: u32 = width
            .try_into()
            .map_err(|_| SamkhyaError::new_err(format!("width {width} exceeds u32::MAX")))?;
        CoreCms::new(depth_u32, width_u32)
            .map(|inner| Self { inner })
            .map_err(map_err)
    }

    /// Add `count` occurrences of `item` to the sketch.
    fn add(&mut self, item: &[u8], count: u64) {
        // Core takes u32; saturate to keep the binding total a safe upper
        // bound — the sketch only over-counts, never undercounts.
        let count_u32 = u32::try_from(count).unwrap_or(u32::MAX);
        self.inner.add(item, count_u32);
    }

    /// Estimate the frequency of `item` (always an upper bound).
    fn estimate(&self, item: &[u8]) -> u64 {
        u64::from(self.inner.estimate(item))
    }

    /// Number of counters per row.
    #[getter]
    fn width(&self) -> u32 {
        self.inner.width()
    }

    /// Number of independent hash rows.
    #[getter]
    fn depth(&self) -> u32 {
        self.inner.depth()
    }

    /// Total weight added (sum of all `count`s passed to `add`).
    #[getter]
    fn total(&self) -> u64 {
        self.inner.total()
    }

    /// Serialize the sketch to a portable byte payload.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.to_bytes().map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserialize a sketch produced by `to_bytes`.
    #[classmethod]
    fn from_bytes(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
        CoreCms::from_bytes(data)
            .map(|inner| Self { inner })
            .map_err(map_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "CountMinSketch(width={}, depth={}, total={})",
            self.inner.width(),
            self.inner.depth(),
            self.inner.total()
        )
    }
}

// =============================================================================
// EquiDepthHistogram
// =============================================================================

/// Mirror of `samkhya_core::sketches::histogram::EquiDepthHistogram`'s
/// serde layout. Lets us construct a histogram from explicit
/// `(boundaries, counts)` even though the core struct's fields are
/// private — we serialize this twin and round-trip through `from_bytes`,
/// which exercises the same bincode codec the rest of samkhya uses.
#[derive(Serialize, Deserialize)]
struct HistogramSerde {
    boundaries: Vec<f64>,
    counts: Vec<u64>,
    total: u64,
}

/// Equi-depth histogram for range / inequality predicate selectivity.
///
/// Constructed directly from `(boundaries, counts)`: `boundaries` has
/// `len() == counts.len() + 1` and is non-decreasing; `counts[i]` is the
/// number of items in the half-open bin `[boundaries[i], boundaries[i+1]]`.
#[pyclass(module = "samkhya", name = "EquiDepthHistogram", from_py_object)]
#[derive(Clone)]
pub struct PyEquiDepthHistogram {
    inner: CoreHistogram,
}

#[pymethods]
impl PyEquiDepthHistogram {
    #[new]
    fn new(boundaries: Vec<f64>, counts: Vec<u64>) -> PyResult<Self> {
        if counts.is_empty() {
            return Err(SamkhyaError::new_err("counts must be non-empty"));
        }
        if boundaries.len() != counts.len() + 1 {
            return Err(SamkhyaError::new_err(format!(
                "boundaries.len() must equal counts.len() + 1 (got {} vs {})",
                boundaries.len(),
                counts.len()
            )));
        }
        // Validate monotonic non-decreasing boundaries up front so we
        // don't rely on the core to surface confusing downstream errors.
        for win in boundaries.windows(2) {
            if win[0] > win[1] {
                return Err(SamkhyaError::new_err("boundaries must be non-decreasing"));
            }
        }
        let total: u64 = counts.iter().copied().fold(0u64, u64::saturating_add);
        let serde_twin = HistogramSerde {
            boundaries,
            counts,
            total,
        };
        let bytes = bincode::serialize(&serde_twin)
            .map_err(|e| SamkhyaError::new_err(format!("histogram encode failed: {e}")))?;
        let inner = CoreHistogram::from_bytes(&bytes).map_err(map_err)?;
        Ok(Self { inner })
    }

    /// Estimate the number of items in the inclusive range `[low, high]`.
    fn range_estimate(&self, low: f64, high: f64) -> u64 {
        self.inner.estimate_range(low, high)
    }

    /// Total number of items represented across all buckets.
    #[getter]
    fn total(&self) -> u64 {
        self.inner.total()
    }

    /// Number of buckets.
    #[getter]
    fn buckets(&self) -> usize {
        self.inner.buckets()
    }

    /// Serialize the histogram to a portable byte payload.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.to_bytes().map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserialize a histogram produced by `to_bytes`.
    #[classmethod]
    fn from_bytes(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
        CoreHistogram::from_bytes(data)
            .map(|inner| Self { inner })
            .map_err(map_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "EquiDepthHistogram(buckets={}, total={})",
            self.inner.buckets(),
            self.inner.total()
        )
    }
}

// =============================================================================
// LpBound top-level functions
// =============================================================================

/// Cartesian-product upper bound — the trivial pessimistic ceiling.
///
/// Returns `prod(card_estimates)` as an `f64`. Used as the safety floor
/// when no join structure is known.
#[pyfunction]
fn product_bound(card_estimates: Vec<f64>) -> f64 {
    if card_estimates.is_empty() {
        return 0.0;
    }
    // Convert non-negative inputs to u64 (saturating), call the core
    // ProductBound, and surface the result back as f64 so Python callers
    // get a uniform numeric type.
    let rows: Vec<u64> = card_estimates
        .iter()
        .map(|&c| {
            if !c.is_finite() || c < 0.0 {
                0
            } else if c >= u64::MAX as f64 {
                u64::MAX
            } else {
                c as u64
            }
        })
        .collect();
    ProductBound.ceiling(&rows, &[]) as f64
}

/// Convert Python floats to saturating, non-negative row counts.
fn to_row_counts(card_estimates: &[f64]) -> Vec<u64> {
    card_estimates
        .iter()
        .map(|&c| {
            if !c.is_finite() || c < 0.0 {
                0
            } else if c >= u64::MAX as f64 {
                u64::MAX
            } else {
                c as u64
            }
        })
        .collect()
}

/// Provable upper bound for an equi-join graph.
///
/// `joins` is a list of `(left_idx, right_idx, predicate_selectivity)`
/// tuples; `card_estimates` is the per-relation row count.
///
/// Changed in 1.2.0 — soundness fix
/// --------------------------------
/// Through 1.1 this multiplied the ceiling by the product of the supplied
/// selectivities. Selectivities are in `[0, 1]`, so that could only shrink
/// the result: passing `0.01` returned a "bound" a hundred times below the
/// real ceiling, and a corrector clamped to it would underestimate. A
/// provable bound cannot be tightened by an estimate. The selectivity
/// field is now ignored here; use `selectivity_estimate` if you want the
/// old System-R-style value, which is an estimate and is labelled as one.
///
/// For a bound that is both provable *and* tighter than the Cartesian
/// product, pass distinct counts to `join_ceiling`.
#[pyfunction]
fn agm_bound(_joins: Vec<(usize, usize, f64)>, card_estimates: Vec<f64>) -> f64 {
    if card_estimates.is_empty() {
        return 0.0;
    }
    let rows = to_row_counts(&card_estimates);
    // Given only row counts and which pairs are joined, the Cartesian
    // product is the only sound ceiling: every row of every relation may
    // carry the same key value.
    ProductBound.ceiling(&rows, &[]) as f64
}

/// System-R-style selectivity-weighted cardinality *estimate*.
///
/// This is the pre-1.2 behaviour of `agm_bound`, under a name that says
/// what it is. It is an estimate, not a ceiling: it can and does land
/// below the true cardinality. Never clamp a corrector to it.
#[pyfunction]
fn selectivity_estimate(joins: Vec<(usize, usize, f64)>, card_estimates: Vec<f64>) -> f64 {
    if card_estimates.is_empty() {
        return 0.0;
    }
    let rows = to_row_counts(&card_estimates);
    let coarse = ProductBound.ceiling(&rows, &[]) as f64;
    let sel: f64 = joins.iter().map(|&(_, _, s)| s.clamp(0.0, 1.0)).product();
    (coarse * sel).max(0.0)
}

/// Provable join ceiling derived from row counts and distinct-value counts.
///
/// `joins` is a list of `(left_idx, right_idx)` pairs. `distinct_counts`
/// gives the number of distinct join-key values per relation; entries that
/// are zero, missing, or larger than the row count degrade safely to "no
/// degree information" rather than producing an unsound value.
///
/// This is the bound to use. On a foreign-key join of 10 orders to 100
/// line items over 10 distinct keys it returns exactly 100, where the
/// Cartesian product returns 1000 — tighter, and still provable.
#[pyfunction]
#[pyo3(signature = (joins, card_estimates, distinct_counts=None))]
fn join_ceiling(
    joins: Vec<(usize, usize)>,
    card_estimates: Vec<f64>,
    distinct_counts: Option<Vec<f64>>,
) -> f64 {
    if card_estimates.is_empty() {
        return 0.0;
    }
    let rows = to_row_counts(&card_estimates);
    let mut relations: Vec<JoinRelation> = rows.iter().map(|&n| JoinRelation::new(n)).collect();

    if let Some(distinct) = distinct_counts {
        let distinct = to_row_counts(&distinct);
        for (attribute, &(i, j)) in joins.iter().enumerate() {
            for endpoint in [i, j] {
                if endpoint >= rows.len() {
                    continue;
                }
                let degree = match distinct.get(endpoint) {
                    Some(&d) => AttributeDegree::from_distinct(rows[endpoint], d),
                    None => AttributeDegree::unknown(rows[endpoint]),
                };
                relations[endpoint] =
                    std::mem::replace(&mut relations[endpoint], JoinRelation::new(rows[endpoint]))
                        .with_degree(attribute as u32, degree);
            }
        }
    }

    let mut graph = JoinGraph::new(relations);
    for (attribute, &(i, j)) in joins.iter().enumerate() {
        graph = graph.with_edge(i, j, attribute as u32);
    }
    graph.ceiling() as f64
}

/// Return the samkhya-py crate version.
#[pyfunction]
fn samkhya_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// =============================================================================
// Module entry point
// =============================================================================

/// Native PyO3 module backing the `samkhya` Python package.
///
/// Maturin's mixed Rust/Python layout puts the compiled extension at
/// `samkhya._native`; the Python package's `__init__.py` re-exports the
/// public names so end-users only see `import samkhya`.
#[pymodule(gil_used = true)]
fn _native(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("SamkhyaError", py.get_type::<SamkhyaError>())?;

    m.add_class::<PyHllSketch>()?;
    m.add_class::<PyBloomFilter>()?;
    m.add_class::<PyCountMinSketch>()?;
    m.add_class::<PyEquiDepthHistogram>()?;

    m.add_function(wrap_pyfunction!(product_bound, m)?)?;
    m.add_function(wrap_pyfunction!(agm_bound, m)?)?;
    m.add_function(wrap_pyfunction!(selectivity_estimate, m)?)?;
    m.add_function(wrap_pyfunction!(join_ceiling, m)?)?;
    m.add_function(wrap_pyfunction!(samkhya_version, m)?)?;
    Ok(())
}
