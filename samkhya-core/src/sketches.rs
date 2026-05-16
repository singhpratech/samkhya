//! Cardinality and quantile sketches.
//!
//! All sketches expose a uniform `to_bytes` / `from_bytes` codec so they
//! round-trip through Iceberg Puffin sidecars without engine-specific glue.

pub mod bloom;
pub mod cms;
pub mod correlated;
pub mod histogram;
pub mod hll;

pub use bloom::BloomFilter;
pub use cms::CountMinSketch;
pub use correlated::CorrelatedHistogram2D;
pub use histogram::EquiDepthHistogram;
pub use hll::HllSketch;

use crate::Result;

/// Common trait every sketch implements: typed payload codec.
///
/// Pair `to_bytes` / `from_bytes` with [`crate::puffin`] to round-trip
/// sketches through Iceberg Puffin sidecars without engine-specific glue.
///
/// # Examples
///
/// ```
/// use samkhya_core::sketches::{HllSketch, Sketch};
///
/// let mut hll = HllSketch::try_new(10).unwrap();
/// for i in 0..256u32 {
///     hll.add(&i.to_le_bytes());
/// }
/// // The KIND tag is the Puffin blob type field for this sketch.
/// assert_eq!(HllSketch::KIND, "samkhya.hll-v1");
///
/// // Round-trip through bytes.
/// let bytes = hll.to_bytes().unwrap();
/// let restored = HllSketch::from_bytes(&bytes).unwrap();
/// assert_eq!(hll.precision(), restored.precision());
/// ```
pub trait Sketch: Sized {
    /// Stable tag identifying the sketch kind. Used as the Puffin blob `type` field.
    const KIND: &'static str;

    fn to_bytes(&self) -> Result<Vec<u8>>;
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}
