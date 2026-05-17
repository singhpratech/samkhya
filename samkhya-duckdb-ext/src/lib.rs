//! samkhya-duckdb-ext — v1.0 Rust <-> C++ FFI scaffold for samkhya
//! statistics injection into DuckDB.
//!
//! # What ships in v1.0
//!
//! A working cxx bridge with a small, deliberate surface:
//!
//! * `HllHandle` — opaque Rust wrapper around `samkhya_core::sketches::hll::HllSketch`.
//! * `hll_new`, `hll_add`, `hll_estimate` — build and query a sketch
//!   from C++.
//! * `puffin_inspect` — read a Puffin sidecar from disk and return the
//!   per-blob metadata (kind/offset/length) as a `Vec<PuffinBlobInfo>`.
//!
//! From the C++ side (see `src/wrapper.cc`) these are reachable today.
//! The crate compiles to a `staticlib` so DuckDB's C++ build can link
//! the archive directly.
//!
//! # What waits for v1.1
//!
//! The actual DuckDB optimizer extension — the hook that walks
//! `LogicalGet` nodes, looks up sketches in a `_samkhya_stats` table,
//! and overrides the planner's cardinality estimate — depends on
//! DuckDB Issue #11638 ("OptimizerExtension API for cardinality
//! overrides"). Until that lands upstream there is no stable C++
//! surface to plug into. The wrapper's `samkhya_register` function is
//! forward-declared so v1.1 can fill in the body without touching the
//! cxx layer below.
//!
//! Reference: <https://github.com/duckdb/duckdb/issues/11638>
//!
//! # Default build vs `no_cxx`
//!
//! The default build invokes `cxx_build` (requires a C++17 compiler
//! but no DuckDB headers). The `no_cxx` Cargo feature disables the
//! bridge and the C++ compilation step for minimal CI images. Under
//! `no_cxx` the Rust API is still available; only the C++ surface is
//! gone.

#![deny(rust_2018_idioms)]
#![deny(rustdoc::broken_intra_doc_links)]

// ---------------------------------------------------------------------
// Rust-side wrappers around the samkhya-core primitives the bridge
// exposes. Defined unconditionally so the crate's public Rust API
// stays stable across the `no_cxx` feature toggle — only the cxx
// bridge module below is feature-gated.
// ---------------------------------------------------------------------

use samkhya_core::puffin::PuffinReader;
use samkhya_core::sketches::hll::HllSketch;

/// Opaque handle the C++ side holds via `rust::Box<HllHandle>`.
///
/// We wrap rather than re-export so the bridge surface stays decoupled
/// from samkhya-core's internal type layout; future changes to
/// `HllSketch` don't propagate into the C++ ABI.
pub struct HllHandle(HllSketch);

/// Construct a fresh HLL sketch at the requested precision.
///
/// Falls back to precision = 12 (~1.6% relative error, 4 KiB state)
/// when the caller passes something outside `[4, 18]`. Choosing a
/// fallback rather than surfacing an error keeps the cxx bridge
/// signature simple; precision-validation is a Rust-side concern.
pub fn hll_new(p: u8) -> Box<HllHandle> {
    let inner =
        HllSketch::new(p).unwrap_or_else(|_| HllSketch::new(12).expect("p=12 is always valid"));
    Box::new(HllHandle(inner))
}

/// Insert one byte-string item into the sketch.
pub fn hll_add(h: &mut HllHandle, bytes: &[u8]) {
    h.0.add(bytes);
}

/// Return the current cardinality estimate as `f64`.
///
/// The core API returns `u64`; we widen to `f64` because the DuckDB
/// optimizer extension (v1.1) consumes cardinality estimates as
/// floating-point selectivity multipliers.
pub fn hll_estimate(h: &HllHandle) -> f64 {
    h.0.estimate() as f64
}

/// Inspect a Puffin sidecar at `path`, returning one `PuffinBlobInfo`
/// per blob. Returns an empty vector on any I/O or parse error — the
/// C++ side treats "no blobs" and "couldn't read" identically (both
/// mean "no override available"), so flattening the failure here keeps
/// the bridge surface ergonomic.
pub fn puffin_inspect(path: &str) -> Vec<ffi::PuffinBlobInfo> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(reader) = PuffinReader::open(file) else {
        return Vec::new();
    };
    reader
        .blobs()
        .iter()
        .map(|b| ffi::PuffinBlobInfo {
            kind: b.kind.clone(),
            offset: b.offset,
            length: b.length,
        })
        .collect()
}

// ---------------------------------------------------------------------
// The cxx bridge. Excluded under `no_cxx` so the crate still compiles
// on images without a C++ toolchain. Everything reachable from C++
// goes through this module.
// ---------------------------------------------------------------------

#[cfg(not(feature = "no_cxx"))]
#[cxx::bridge(namespace = "samkhya")]
mod ffi {
    /// Per-blob view returned by [`puffin_inspect`]. Kept deliberately
    /// thin: kind tag, byte offset, byte length. Anything richer
    /// (snapshot ID, properties) ships in v1.1 once the optimizer hook
    /// actually needs it.
    struct PuffinBlobInfo {
        kind: String,
        offset: u64,
        length: u64,
    }

    extern "Rust" {
        type HllHandle;

        fn hll_new(p: u8) -> Box<HllHandle>;
        fn hll_add(h: &mut HllHandle, bytes: &[u8]);
        fn hll_estimate(h: &HllHandle) -> f64;

        fn puffin_inspect(path: &str) -> Vec<PuffinBlobInfo>;
    }
}

// Stand-in for the `ffi::PuffinBlobInfo` type when the cxx bridge is
// disabled. Lets `puffin_inspect` keep the same signature regardless
// of which build configuration is active.
#[cfg(feature = "no_cxx")]
mod ffi {
    pub struct PuffinBlobInfo {
        pub kind: String,
        pub offset: u64,
        pub length: u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hll_round_trip_through_handle() {
        let mut h = hll_new(12);
        for i in 0..1_000u32 {
            hll_add(&mut h, &i.to_le_bytes());
        }
        let est = hll_estimate(&h);
        let err = (est - 1_000.0).abs() / 1_000.0;
        assert!(err < 0.10, "estimate {est} off by {err}");
    }

    #[test]
    fn hll_new_clamps_invalid_precision() {
        // p=3 is out of range; constructor must still return a usable
        // sketch (the fallback at p=12).
        let mut h = hll_new(3);
        hll_add(&mut h, b"x");
        assert!(hll_estimate(&h) >= 1.0);
    }

    #[test]
    fn puffin_inspect_missing_file_returns_empty() {
        let info = puffin_inspect("/nonexistent/path/should/not/exist.puffin");
        assert!(info.is_empty());
    }
}
