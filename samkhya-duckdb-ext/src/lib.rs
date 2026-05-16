//! samkhya-duckdb-ext — server-side DuckDB extension scaffold.
//!
//! This crate produces a `.duckdb_extension` (a renamed cdylib) that
//! DuckDB loads at runtime via `LOAD 'samkhya_duckdb_ext';`. It is
//! distinct from the sibling crate `samkhya-duckdb`, which is a Rust
//! *client* that drives a DuckDB connection from outside the engine.
//! This crate runs *inside* the engine and exposes samkhya's portable
//! sketches as DuckDB SQL functions plus a metadata table the planner
//! consults during cardinality estimation.
//!
//! ## Status
//!
//! This is **scaffolding** for the v0.7.0 roadmap deliverable. The cxx
//! bridge surface is declared, the C++ stub exists, and the build
//! plumbing is in place — but full DuckDB function registration is a
//! multi-week C++ effort tracked separately. See `README.md` for the
//! list of work items the next round needs to complete.
//!
//! ## Why cxx
//!
//! DuckDB's stable extension surface is C++. samkhya's sketches are
//! Rust. The standard bridge is the [`cxx`] crate, which generates a
//! mutually-validated FFI between the two: Rust owns the sketch data,
//! C++ owns DuckDB function registration, and neither side reaches into
//! the other's allocator.
//!
//! ## Default build is clean
//!
//! Without the `extension` feature, this crate compiles to an empty
//! cdylib (no symbols beyond what the Rust runtime requires) and does
//! not need a C++ toolchain or DuckDB headers. That keeps
//! `cargo check --workspace` runnable on minimal CI images.
//!
//! [`cxx`]: https://cxx.rs

#![deny(rust_2018_idioms)]

// ---------------------------------------------------------------------
// Default-feature path: an empty cdylib so the workspace builds clean.
// ---------------------------------------------------------------------

#[cfg(not(feature = "extension"))]
mod stub {
    //! With the `extension` feature off, the crate exports no symbols.
    //!
    //! This keeps `cargo check -p samkhya-duckdb-ext` runnable without
    //! a C++ toolchain or DuckDB headers, while still validating that
    //! the workspace member resolves and the crate metadata is
    //! syntactically correct.
}

// ---------------------------------------------------------------------
// extension-feature path: cxx bridge + Rust-side sketch wrappers.
// ---------------------------------------------------------------------

#[cfg(feature = "extension")]
mod bridge_impl {
    //! Rust side of the cxx bridge. Wraps `samkhya-core` sketches in
    //! opaque types that the C++ side holds via `Box<T>` and mutates
    //! through the small set of free functions declared in
    //! [`ffi`](self::ffi).
    //!
    //! Failure modes (allocation errors, deserialization errors) are
    //! flattened to either empty `Vec<u8>` returns or zeroed estimates
    //! so the C++ side never sees a Rust panic. A future iteration
    //! will thread a proper `Result` type through cxx's `Result<T>`
    //! support.

    use samkhya_core::sketches::{BloomFilter as CoreBloom, HllSketch as CoreHll};

    // --- Opaque Rust types exposed through the bridge -----------------

    /// Wrapper around the core HLL sketch.
    ///
    /// `cxx` requires bridged opaque types live at the crate root of
    /// the bridge module; we therefore re-export it from `ffi` via a
    /// `type` alias in the bridge declaration below.
    pub struct HllSketch {
        inner: CoreHll,
    }

    /// Wrapper around the core Bloom filter.
    pub struct BloomFilter {
        inner: CoreBloom,
    }

    // --- Rust-side implementations of the bridged free functions -----

    pub fn hll_new(precision: u8) -> Box<HllSketch> {
        // The default precision in `samkhya-core` clamps to the legal
        // range; we fall back to precision=12 (≈1.6% relative error,
        // 4 KiB state) if the caller hands us something out of range.
        let inner = CoreHll::new(precision).unwrap_or_else(|_| {
            CoreHll::new(12).expect("precision=12 is always valid")
        });
        Box::new(HllSketch { inner })
    }

    pub fn hll_add(hll: &mut HllSketch, item: &[u8]) {
        hll.inner.add(item);
    }

    pub fn hll_estimate(hll: &HllSketch) -> u64 {
        hll.inner.estimate()
    }

    pub fn hll_to_bytes(hll: &HllSketch) -> Vec<u8> {
        use samkhya_core::sketches::Sketch;
        hll.inner.to_bytes().unwrap_or_default()
    }

    pub fn hll_from_bytes(bytes: &[u8]) -> Box<HllSketch> {
        use samkhya_core::sketches::Sketch;
        let inner = CoreHll::from_bytes(bytes).unwrap_or_else(|_| {
            // Deserialization failure: return an empty p=12 sketch.
            // The C++ side will detect estimate==0 and surface a SQL
            // NULL, per the function-registration code in extension.cpp.
            CoreHll::new(12).expect("precision=12 is always valid")
        });
        Box::new(HllSketch { inner })
    }

    pub fn bloom_new(capacity: usize, fp_rate: f64) -> Box<BloomFilter> {
        Box::new(BloomFilter {
            inner: CoreBloom::new(capacity, fp_rate),
        })
    }

    pub fn bloom_insert(bf: &mut BloomFilter, item: &[u8]) {
        bf.inner.insert(item);
    }

    pub fn bloom_contains(bf: &BloomFilter, item: &[u8]) -> bool {
        bf.inner.contains(item)
    }

    pub fn bloom_to_bytes(bf: &BloomFilter) -> Vec<u8> {
        use samkhya_core::sketches::Sketch;
        bf.inner.to_bytes().unwrap_or_default()
    }

    pub fn bloom_from_bytes(bytes: &[u8]) -> Box<BloomFilter> {
        use samkhya_core::sketches::Sketch;
        let inner = CoreBloom::from_bytes(bytes)
            .unwrap_or_else(|_| CoreBloom::new(1024, 0.01));
        Box::new(BloomFilter { inner })
    }

    // --- The cxx bridge ----------------------------------------------
    //
    // The `ffi` module is the contract between Rust and C++. Every
    // type and function listed here gets a matching declaration in
    // the generated `samkhya_duckdb_ext/src/lib.rs.h` header, which
    // `src/extension.cpp` then includes.

    #[cxx::bridge(namespace = "samkhya")]
    pub mod ffi {
        extern "Rust" {
            // Opaque Rust types. cxx will emit forward-declarations
            // on the C++ side; instances are held via `rust::Box<T>`.
            type HllSketch;
            type BloomFilter;

            // HLL surface — what the DuckDB scalar / aggregate
            // functions in extension.cpp will call.
            fn hll_new(precision: u8) -> Box<HllSketch>;
            fn hll_add(hll: &mut HllSketch, item: &[u8]);
            fn hll_estimate(hll: &HllSketch) -> u64;
            fn hll_to_bytes(hll: &HllSketch) -> Vec<u8>;
            fn hll_from_bytes(bytes: &[u8]) -> Box<HllSketch>;

            // Bloom surface — same shape, different sketch.
            fn bloom_new(capacity: usize, fp_rate: f64) -> Box<BloomFilter>;
            fn bloom_insert(bf: &mut BloomFilter, item: &[u8]);
            fn bloom_contains(bf: &BloomFilter, item: &[u8]) -> bool;
            fn bloom_to_bytes(bf: &BloomFilter) -> Vec<u8>;
            fn bloom_from_bytes(bytes: &[u8]) -> Box<BloomFilter>;
        }
    }
}

#[cfg(feature = "extension")]
pub use bridge_impl::*;
