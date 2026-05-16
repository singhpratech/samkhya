//! samkhya-py — Python bindings via PyO3.
//!
//! Exposes the core stats / sketch API to Python so dbt-style users can
//! consume samkhya's portable, feedback-driven cardinality correction
//! primitives without a Rust toolchain.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use samkhya_core::sketches::{BloomFilter as CoreBloom, HllSketch as CoreHll, Sketch};
use samkhya_core::stats::ColumnStats as CoreColumnStats;
use samkhya_core::Error as CoreError;

create_exception!(samkhya, SamkhyaError, PyException);

fn map_err(e: CoreError) -> PyErr {
    SamkhyaError::new_err(e.to_string())
}

/// HyperLogLog cardinality sketch.
///
/// Precision `p` ∈ [4, 18] controls register count (`2^p`) and relative
/// error (≈ 1.04 / √(2^p)). Use `to_bytes` / `from_bytes` to round-trip
/// the sketch through Iceberg Puffin sidecars or any other transport.
#[pyclass(module = "samkhya", name = "HllSketch")]
#[derive(Clone)]
pub struct PyHllSketch {
    inner: CoreHll,
}

#[pymethods]
impl PyHllSketch {
    #[new]
    fn new(precision: u8) -> PyResult<Self> {
        CoreHll::new(precision)
            .map(|inner| Self { inner })
            .map_err(map_err)
    }

    /// Add a single item (raw bytes) to the sketch.
    fn add(&mut self, item: &[u8]) {
        self.inner.add(item);
    }

    /// Return the current cardinality estimate.
    fn estimate(&self) -> u64 {
        self.inner.estimate()
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
        Ok(PyBytes::new_bound(py, &bytes))
    }

    /// Deserialize a sketch produced by `to_bytes`.
    #[classmethod]
    fn from_bytes(_cls: &Bound<'_, pyo3::types::PyType>, data: &[u8]) -> PyResult<Self> {
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

/// Bloom filter sized for a target capacity and false-positive rate.
#[pyclass(module = "samkhya", name = "BloomFilter")]
#[derive(Clone)]
pub struct PyBloomFilter {
    inner: CoreBloom,
}

#[pymethods]
impl PyBloomFilter {
    #[new]
    fn new(capacity: usize, fp_rate: f64) -> Self {
        Self {
            inner: CoreBloom::new(capacity, fp_rate),
        }
    }

    /// Insert an item (raw bytes) into the filter.
    fn insert(&mut self, item: &[u8]) {
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

    /// Number of hash functions per insert/lookup.
    #[getter]
    fn num_hashes(&self) -> u32 {
        self.inner.num_hashes()
    }

    /// Serialize the filter to a portable byte payload.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner.to_bytes().map_err(map_err)?;
        Ok(PyBytes::new_bound(py, &bytes))
    }

    /// Deserialize a filter produced by `to_bytes`.
    #[classmethod]
    fn from_bytes(_cls: &Bound<'_, pyo3::types::PyType>, data: &[u8]) -> PyResult<Self> {
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

/// Column-level statistics that samkhya adapters inject into native optimizers.
///
/// Supports a builder-style API (`stats.with_row_count(n)`) for ergonomic
/// chaining as well as direct attribute access via getters/setters.
#[pyclass(module = "samkhya", name = "ColumnStats")]
#[derive(Clone)]
pub struct PyColumnStats {
    inner: CoreColumnStats,
}

#[pymethods]
impl PyColumnStats {
    #[new]
    fn new() -> Self {
        Self {
            inner: CoreColumnStats::new(),
        }
    }

    fn with_row_count(&self, n: u64) -> Self {
        Self {
            inner: self.inner.clone().with_row_count(n),
        }
    }

    fn with_distinct_count(&self, n: u64) -> Self {
        Self {
            inner: self.inner.clone().with_distinct_count(n),
        }
    }

    fn with_null_count(&self, n: u64) -> Self {
        Self {
            inner: self.inner.clone().with_null_count(n),
        }
    }

    fn with_upper_bound(&self, n: u64) -> Self {
        Self {
            inner: self.inner.clone().with_upper_bound(n),
        }
    }

    #[getter]
    fn row_count(&self) -> Option<u64> {
        self.inner.row_count
    }

    #[setter]
    fn set_row_count(&mut self, v: Option<u64>) {
        self.inner.row_count = v;
    }

    #[getter]
    fn distinct_count(&self) -> Option<u64> {
        self.inner.distinct_count
    }

    #[setter]
    fn set_distinct_count(&mut self, v: Option<u64>) {
        self.inner.distinct_count = v;
    }

    #[getter]
    fn null_count(&self) -> Option<u64> {
        self.inner.null_count
    }

    #[setter]
    fn set_null_count(&mut self, v: Option<u64>) {
        self.inner.null_count = v;
    }

    #[getter]
    fn upper_bound_rows(&self) -> Option<u64> {
        self.inner.upper_bound_rows
    }

    #[setter]
    fn set_upper_bound_rows(&mut self, v: Option<u64>) {
        self.inner.upper_bound_rows = v;
    }

    fn __repr__(&self) -> String {
        format!(
            "ColumnStats(row_count={:?}, distinct_count={:?}, null_count={:?}, upper_bound_rows={:?})",
            self.inner.row_count,
            self.inner.distinct_count,
            self.inner.null_count,
            self.inner.upper_bound_rows,
        )
    }
}

/// Top-level PyO3 module: `samkhya`.
#[pymodule]
fn samkhya(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("SamkhyaError", py.get_type_bound::<SamkhyaError>())?;
    m.add_class::<PyHllSketch>()?;
    m.add_class::<PyBloomFilter>()?;
    m.add_class::<PyColumnStats>()?;
    Ok(())
}
