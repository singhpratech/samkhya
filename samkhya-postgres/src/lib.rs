//! samkhya-postgres — PostgreSQL adapter for samkhya.
//!
//! # Build modes
//!
//! - **Default** (no features): empty `rlib`. Compiles without
//!   PostgreSQL development headers; suitable for `cargo check
//!   --workspace` in CI environments that do not have `libpq-dev` /
//!   `postgresql-server-dev-*` installed.
//! - **`pg_extension`** feature **plus** `samkhya_pgrx_enabled` rustc
//!   cfg flag: pulls in [pgrx] and exposes the functions defined
//!   below as a loadable PostgreSQL extension targeting **PostgreSQL
//!   17** (the only major supported in v1.0). Build with `cargo pgrx`
//!   — see this crate's README. The recommended invocation is:
//!
//!   ```bash
//!   RUSTFLAGS="--cfg=samkhya_pgrx_enabled" \
//!     cargo pgrx run pg17 --features pg_extension --package samkhya-postgres
//!   ```
//!
//! # v1.0 double-gating + single-version pin (pg17)
//!
//! pgrx 0.12.9's `pgrx-pg-sys` build script panics at bindgen-time
//! when more than one `pg$VERSION` feature is simultaneously active:
//!
//! ```text
//! Error: Multiple `pg$VERSION` features found.
//! `--no-default-features` may be required.
//! Found: pg13, pg14, pg15, pg16, pg17
//! ```
//!
//! Cargo's `--all-features` (used by `cargo check --workspace
//! --all-features` and similar workspace-wide gates) activates every
//! feature in a crate's `[features]` table simultaneously. With the
//! canonical pgrx feature-flag pattern (pg13..pg17 as parallel
//! features that each forward `pgrx/pgNN`), workspace gates therefore
//! cannot pass — pgrx-pg-sys's build script panics before any
//! manifest-level `compile_error!` we add ever fires.
//!
//! For v1.0 the design is:
//!
//! 1. **Single-version pin (pg17)**. The `pg_extension` Cargo feature
//!    forwards `pgrx/pg17`. No pg13..pg16 features are declared.
//! 2. **Target-cfg dep isolation**. The pgrx dependency lives under
//!    `[target.'cfg(samkhya_pgrx_enabled)'.dependencies]` in
//!    `Cargo.toml`. Under `cargo check --workspace --all-features`
//!    (where `samkhya_pgrx_enabled` is unset), pgrx is excluded from
//!    the dep graph and `pg_extension` is a harmless no-op. Under
//!    `RUSTFLAGS="--cfg=samkhya_pgrx_enabled" cargo pgrx run pg17
//!    --features pg_extension`, pgrx enters the dep graph and the
//!    extension module below compiles.
//!
//! v1.1 will restore pg13..pg16 when one of:
//!
//! - pgrx 0.13+ removes the feature-multiplexing constraint, or
//! - the pgrx-using code is moved to a non-workspace sub-crate that
//!   does not participate in `--workspace --all-features` gates.
//!
//! See `feedback-pgrx-feature-isolation` memory for the full
//! design-decision record and retire conditions.
//!
//! # Provided SQL functions (when built as an extension)
//!
//! - `samkhya_hll_count(input anyarray) -> bigint` — build a samkhya
//!   `HllSketch` from the input array and return its estimated
//!   distinct-element count. Useful as a quick sanity check that the
//!   in-engine sketch agrees with the portable sketch produced by
//!   samkhya-core.
//! - `samkhya_puffin_inspect(path text) -> jsonb` — open a Puffin
//!   sidecar file on the server filesystem and return per-blob
//!   metadata (`kind`, `offset`, `length`, `fields`,
//!   `compression-codec`).
//!
//! # Scope
//!
//! This is the current scaffold. A future compatibility release can add the
//! operator-side cardinality hook (replacing `get_relation_info_hook` and friends)
//! so the planner picks up samkhya's portable, feedback-driven,
//! self-correcting row estimates without per-query SQL changes. The
//! `get_relation_info_hook` integration is intentionally deferred
//! because it requires deeper pgrx hook plumbing than belongs in a
//! scaffold.
//!
//! [pgrx]: https://github.com/pgcentralfoundation/pgrx

#![cfg_attr(
    not(all(feature = "pg_extension", samkhya_pgrx_enabled)),
    deny(rust_2018_idioms)
)]

// ---------------------------------------------------------------------
// Non-extension build: empty rlib.
//
// The stub compiles whenever the `pg_extension` feature is OFF, OR
// when the `samkhya_pgrx_enabled` cfg flag is unset. The latter
// catches `cargo check --workspace --all-features` (which enables
// `pg_extension` but does not set the cfg flag), keeping the
// workspace-wide gate green on hosts without PG dev headers.
// ---------------------------------------------------------------------

#[cfg(not(all(feature = "pg_extension", samkhya_pgrx_enabled)))]
mod stub {
    //! Stub surface that compiles without pgrx.
    //!
    //! The real extension entry points only exist when the
    //! `pg_extension` feature is enabled. We keep one trivially
    //! callable stub here so `cargo check` exercises something other
    //! than an empty crate root, and so downstream tooling that lists
    //! crate items has at least one symbol to point at.

    /// Returns the samkhya-postgres crate version string.
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn version_is_non_empty() {
            assert!(!version().is_empty());
        }
    }
}

#[cfg(not(all(feature = "pg_extension", samkhya_pgrx_enabled)))]
pub use stub::version;

// ---------------------------------------------------------------------
// pgrx-backed extension build.
//
// Activated only when BOTH:
//   - the `pg_extension` Cargo feature is enabled, AND
//   - the `samkhya_pgrx_enabled` rustc cfg flag is set
//     (typically via `RUSTFLAGS="--cfg=samkhya_pgrx_enabled"`).
// The double-gate ensures workspace-wide `--all-features` builds do
// not pull pgrx into the dep graph on hosts without PG dev headers.
// ---------------------------------------------------------------------

#[cfg(all(feature = "pg_extension", samkhya_pgrx_enabled))]
mod extension {
    use pgrx::prelude::*;
    use pgrx::{AnyElement, Array, JsonB};
    use samkhya_core::puffin::PuffinReader;
    use samkhya_core::sketches::HllSketch;
    use serde_json::{Map, Value, json};
    use std::fs::File;
    use std::io::BufReader;

    pgrx::pg_module_magic!();

    /// Build a samkhya HLL sketch from the input array and return its
    /// estimated distinct-element count.
    ///
    /// `NULL` elements are skipped. The sketch precision is fixed at
    /// 14 (≈16 KiB of registers, ≈0.81% relative standard error),
    /// matching the default used elsewhere in samkhya.
    #[pg_extern(immutable, parallel_safe)]
    fn samkhya_hll_count(input: Array<'_, AnyElement>) -> i64 {
        const PRECISION: u8 = 14;

        let mut hll = match HllSketch::new(PRECISION) {
            Ok(h) => h,
            Err(e) => error!("samkhya_hll_count: failed to build HLL sketch: {e}"),
        };

        for elem in input.iter().flatten() {
            // Hash the raw Datum bytes. This treats two values as
            // equal iff their on-disk representation is bitwise equal,
            // which is correct for fixed-width Postgres types and for
            // canonicalized varlena types. For non-canonical varlena
            // inputs the caller should pre-canonicalize.
            let datum = elem.into_datum();
            let bytes = datum.to_ne_bytes();
            hll.add(&bytes);
        }

        hll.estimate() as i64
    }

    /// Open a Puffin sidecar file at `path` on the server filesystem
    /// and return per-blob metadata as JSONB.
    ///
    /// The returned object has shape:
    /// ```json
    /// {
    ///   "blobs": [
    ///     {
    ///       "kind": "samkhya.hll-v1",
    ///       "fields": [7],
    ///       "offset": 4,
    ///       "length": 16384,
    ///       "compression_codec": null
    ///     }
    ///   ]
    /// }
    /// ```
    #[pg_extern(stable, parallel_safe)]
    fn samkhya_puffin_inspect(path: &str) -> JsonB {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => error!("samkhya_puffin_inspect: open {path}: {e}"),
        };
        let reader = match PuffinReader::open(BufReader::new(file)) {
            Ok(r) => r,
            Err(e) => error!("samkhya_puffin_inspect: parse {path}: {e}"),
        };

        let blobs: Vec<Value> = reader
            .blobs()
            .iter()
            .map(|b| {
                let mut entry = Map::new();
                entry.insert("kind".into(), Value::String(b.kind.clone()));
                entry.insert(
                    "fields".into(),
                    Value::Array(b.fields.iter().map(|f| json!(*f)).collect()),
                );
                entry.insert("offset".into(), json!(b.offset));
                entry.insert("length".into(), json!(b.length));
                entry.insert(
                    "compression_codec".into(),
                    match &b.compression_codec {
                        Some(c) => Value::String(c.clone()),
                        None => Value::Null,
                    },
                );
                Value::Object(entry)
            })
            .collect();

        JsonB(json!({ "blobs": blobs }))
    }

    // -----------------------------------------------------------------
    // pg_test plumbing — exercised by `cargo pgrx test`.
    // -----------------------------------------------------------------

    #[cfg(any(test, feature = "pg_test"))]
    #[pg_schema]
    mod tests {
        use pgrx::prelude::*;

        #[pg_test]
        fn hll_count_on_small_array_is_plausible() {
            let n: Option<i64> = Spi::get_one(
                "SELECT samkhya_hll_count(ARRAY[1, 2, 3, 4, 5, 5, 5]::int[]::anyarray)",
            )
            .expect("Spi::get_one");
            let n = n.expect("non-null result");
            // Five distinct ints; HLL at p=14 should land close.
            assert!((1..=10).contains(&n), "estimate {n} not near 5");
        }
    }

    /// pgrx test framework entry point.
    #[cfg(test)]
    pub mod pg_test {
        pub fn setup(_options: Vec<&str>) {}

        pub fn postgresql_conf_options() -> Vec<&'static str> {
            vec![]
        }
    }
}
