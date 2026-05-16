//! Fuzz the Puffin sidecar reader.
//!
//! Feeds arbitrary byte sequences into [`PuffinReader::open`] and any of the
//! lazy-read entry points reachable through it. The invariant under test is
//! **`PuffinReader::open` never panics**: malformed input must surface as
//! `Err(samkhya_core::Error::InvalidPuffin(_))` (or an I/O error), never as
//! an unwrap / index-out-of-range / arithmetic overflow.
//!
//! Why this matters: in v0.7.0 the Puffin reader is embedded inside the
//! DuckDB cxx extension, so any panic crosses a C++ boundary as an abort
//! and takes the whole DuckDB process with it. The same payload also
//! enters the DataFusion adapter via `SamkhyaTableProvider::scan`. Both
//! call sites must be panic-safe regardless of what arrives on disk.
//!
//! Coverage shape:
//!   * Open the reader on the raw bytes (exercises footer trailer parsing,
//!     payload-length validation, JSON decode).
//!   * If open succeeds, iterate every recorded blob and call
//!     `read_blob(idx)` — this exercises the per-blob `offset` / `length`
//!     fields, which are attacker-controlled inside the JSON footer.
//!   * Then exercise `read_blob_decompressed(idx)`, which dispatches on the
//!     codec metadata; this is the third panic surface (zstd decoder).
//!
//! All `unwrap()`s are on the `Result` returned by the reader API; the
//! whole point of the fuzz target is that those are *expected* to surface
//! errors and never to panic.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use samkhya_core::puffin::PuffinReader;

fuzz_target!(|data: &[u8]| {
    // Cursor<&[u8]> implements both Read and Seek, which is exactly what
    // PuffinReader::open requires. No allocation on the input path.
    let cursor = Cursor::new(data);
    let reader = match PuffinReader::open(cursor) {
        Ok(r) => r,
        Err(_) => return,
    };

    // Snapshot blob count before consuming the reader so we don't re-borrow
    // it inside the per-blob loop.
    let n = reader.blobs().len();

    // Re-open for the mutable read path. Cursor is cheap to construct.
    let mut reader = match PuffinReader::open(Cursor::new(data)) {
        Ok(r) => r,
        Err(_) => return,
    };

    for idx in 0..n {
        // read_blob trusts the JSON-encoded offset / length pair; if those
        // are out-of-range it must surface an Err, never panic.
        let _ = reader.read_blob(idx);
        // read_blob_decompressed dispatches on the codec metadata, then
        // re-reads if the codec is None / runs the zstd decoder otherwise.
        let _ = reader.read_blob_decompressed(idx);
    }
});
