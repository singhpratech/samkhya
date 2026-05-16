//! samkhya-duckdb — client-side DuckDB integration for samkhya.
//!
//! This crate uses the embedded `duckdb` Rust client (no `cxx` required)
//! to populate samkhya's portable sketches from DuckDB query results.
//! The resulting [`HllSketch`]/[`BloomFilter`] payloads are serialized
//! through the same Puffin-blob path used by every other engine adapter,
//! so cardinality stats round-trip across engines without re-scanning.
//!
//! A true `.duckdb_extension` (registered server-side, with cxx-bridged
//! sketch functions) remains a future deliverable. The integration here
//! is deliberately client-side: it runs SQL via the embedded connection
//! and digests rows in Rust.
//!
//! ## Features
//!
//! - `bundled` (off by default) — enables the `duckdb` crate with its
//!   `bundled` feature so neither `libduckdb` nor a C++ toolchain need
//!   to be present in the consumer's environment beyond what
//!   `duckdb-sys` already vendors. With the feature disabled this crate
//!   is essentially empty — no symbols, no link-time deps — which keeps
//!   default workspace builds and CI exclusion-friendly.
//!
//! [`HllSketch`]: samkhya_core::sketches::HllSketch
//! [`BloomFilter`]: samkhya_core::sketches::BloomFilter

#![deny(rust_2018_idioms)]

#[cfg(feature = "bundled")]
pub mod sketcher;

#[cfg(feature = "bundled")]
pub mod feedback;

#[cfg(feature = "bundled")]
pub use duckdb;
