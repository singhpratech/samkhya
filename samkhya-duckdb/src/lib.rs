//! samkhya-duckdb — client-side DuckDB integration for samkhya.
//!
//! This crate exposes an always-on, client-side [`sidecar`] module for
//! consuming validated portable Puffin statistics. With the `bundled` feature,
//! it also uses the embedded `duckdb` Rust client (no `cxx` required) to build
//! sketches from query results and capture feedback observations.
//!
//! A true `.duckdb_extension` (registered server-side, with cxx-bridged
//! sketch functions) remains a future deliverable. The integration here
//! is deliberately client-side: it runs SQL via the embedded connection
//! and digests rows in Rust.
//!
//! ## Features
//!
//! - `bundled` (off by default) — enables SQL-backed sketch construction and
//!   feedback capture through the embedded `duckdb` crate. Portable sidecar
//!   decoding remains available without this feature or any C++ compilation.
//!
//! [`HllSketch`]: samkhya_core::sketches::HllSketch
//! [`BloomFilter`]: samkhya_core::sketches::BloomFilter

#![deny(rust_2018_idioms)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod sidecar;

#[cfg(feature = "bundled")]
pub mod sketcher;

#[cfg(feature = "bundled")]
pub mod feedback;

#[cfg(feature = "bundled")]
pub use duckdb;
