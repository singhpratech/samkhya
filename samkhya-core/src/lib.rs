//! samkhya-core — portable cardinality correction primitives.
#![deny(rustdoc::broken_intra_doc_links)]

pub mod degree;
pub mod error;
pub mod feedback;
pub mod lpbound;
pub mod portable;
pub mod puffin;
pub mod residual;
pub mod sketches;
pub mod stats;

pub use error::{Error, Result};
pub use portable::{DecodedColumnStats, PortableSketchBlob, PortableStatsSnapshot};
pub use stats::ColumnStats;

/// Top-level re-export for the LLM-pluggable corrector backend
/// (`samkhya-core::residual::llm::LlmHttpCorrector`). Available only when
/// the `llm_http` cargo feature is enabled. Mirrors the wire contract of
/// the TabPFN HTTP backend — see `residual` module docs for the safety
/// contract and `bench-results/19_llm_corrector.md` for routing
/// guidance.
#[cfg(feature = "llm_http")]
pub use residual::llm::{LlmHttpCorrector, LlmHttpOptions};
