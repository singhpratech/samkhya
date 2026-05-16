//! samkhya-core — portable cardinality correction primitives.

/// Iceberg Puffin sidecar reader/writer.
pub mod puffin {}

/// Classical sketches: HLL/Theta/KLL/CMS/Bloom/t-digest.
pub mod sketches {}

/// Feedback recorder: observe (plan, estimate, actual) triples.
pub mod feedback {}

/// LpBound pessimistic upper-bound envelope (SIGMOD 2025).
pub mod lpbound {}

/// Residual correction model (sub-MB, sub-ms).
pub mod residual {}
