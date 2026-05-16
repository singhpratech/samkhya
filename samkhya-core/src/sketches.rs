//! Cardinality and quantile sketches.
//!
//! All sketches expose a uniform `to_bytes` / `from_bytes` codec so they
//! round-trip through Iceberg Puffin sidecars without engine-specific glue.

pub mod bloom;
pub mod cms;
pub mod histogram;
pub mod hll;

pub use bloom::BloomFilter;
pub use cms::CountMinSketch;
pub use histogram::EquiDepthHistogram;
pub use hll::HllSketch;

use crate::Result;

/// Common trait every sketch implements: typed payload codec.
pub trait Sketch: Sized {
    /// Stable tag identifying the sketch kind. Used as the Puffin blob `type` field.
    const KIND: &'static str;

    fn to_bytes(&self) -> Result<Vec<u8>>;
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}
