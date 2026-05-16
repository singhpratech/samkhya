// SPDX-License-Identifier: Apache-2.0
//
// samkhya-core: randomized property tests for the Iceberg Puffin
// sidecar reader/writer.
//
// Sole author: Prateek Singh.
//
// The Puffin codec is a load-bearing dependency for every sketch we
// ship: a bug here corrupts every downstream estimate. These property
// tests pound the reader with random blob payloads, kind tags, and
// metadata, then with random truncations and footer-byte mutations
// to confirm that malformed input always produces a clean `Result`
// error rather than a panic. Each `proptest!` block runs at least
// 1024 cases.

use std::io::Cursor;

use proptest::collection::vec as pvec;
use proptest::prelude::*;

use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};

fn cases() -> ProptestConfig {
    ProptestConfig::with_cases(1024)
}

// One synthetic blob: random kind tag, random payload.
fn arb_blob() -> impl Strategy<Value = (String, Vec<u8>)> {
    (
        "[a-z]{1,8}\\.[a-z0-9-]{1,16}",
        pvec(any::<u8>(), 0..512usize),
    )
}

// Build a Puffin file from a vector of (kind, payload) pairs and
// return the serialized bytes.
fn build_puffin(blobs: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = PuffinWriter::new(Cursor::new(Vec::new()));
    for (i, (kind, payload)) in blobs.iter().enumerate() {
        w.add_blob(Blob::new(kind.clone(), vec![i as i32], payload))
            .unwrap();
    }
    w.finish().unwrap().into_inner()
}

proptest! {
    #![proptest_config(cases())]

    // Round-trip preserves every blob's bytes, kind, and ordering.
    #[test]
    fn puffin_round_trips_blobs(
        blobs in pvec(arb_blob(), 0..16usize),
    ) {
        let bytes = build_puffin(&blobs);
        let mut reader = PuffinReader::open(Cursor::new(bytes)).unwrap();
        prop_assert_eq!(reader.blobs().len(), blobs.len());
        for (i, (kind, payload)) in blobs.iter().enumerate() {
            prop_assert_eq!(&reader.blobs()[i].kind, kind);
            prop_assert_eq!(reader.blobs()[i].fields.clone(), vec![i as i32]);
            let got = reader.read_blob(i).unwrap();
            prop_assert_eq!(&got, payload);
        }
    }

    // Ordering is preserved: blob i is always at index i in the footer.
    #[test]
    fn puffin_preserves_blob_ordering(
        blobs in pvec(arb_blob(), 1..16usize),
    ) {
        let bytes = build_puffin(&blobs);
        let reader = PuffinReader::open(Cursor::new(bytes)).unwrap();
        let meta = reader.blobs();
        prop_assert_eq!(meta.len(), blobs.len());
        for (i, (kind, _)) in blobs.iter().enumerate() {
            prop_assert_eq!(&meta[i].kind, kind);
            prop_assert_eq!(meta[i].fields.clone(), vec![i as i32]);
        }
    }

    // Truncating the file at any prefix length must produce a clean
    // error, never a panic.
    #[test]
    fn truncation_is_clean_error(
        blobs in pvec(arb_blob(), 1..8usize),
        trunc_pick in 0u64..1024u64,
    ) {
        let bytes = build_puffin(&blobs);
        let new_len = (trunc_pick as usize) % bytes.len().max(1);
        let mut truncated = bytes.clone();
        truncated.truncate(new_len);
        // The reader must either succeed (vanishingly unlikely on a
        // random truncation, but legal if the prefix happens to be
        // a valid file) or return a Result::Err — never panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PuffinReader::open(Cursor::new(truncated))
        }));
        prop_assert!(
            result.is_ok(),
            "panic on truncated input at len {new_len}"
        );
        // If it returned a value, the value must itself be Ok or Err
        // (both are fine — what matters is the no-panic contract).
        let _ = result.unwrap();
    }

    // Mutating random bytes inside the footer area must produce a
    // clean error, never a panic. We target the trailing 64 bytes
    // (which always contains the magic, payload length, flags, and
    // most of the JSON footer payload for small files).
    #[test]
    fn footer_mutation_is_clean_error(
        blobs in pvec(arb_blob(), 1..8usize),
        mutations in pvec((0u64..64u64, any::<u8>()), 1..8usize),
    ) {
        let bytes = build_puffin(&blobs);
        let mut mutated = bytes.clone();
        let footer_start = mutated.len().saturating_sub(64);
        for (offset, byte) in &mutations {
            let pos = footer_start + ((*offset as usize) % 64).min(mutated.len() - footer_start - 1);
            mutated[pos] = *byte;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PuffinReader::open(Cursor::new(mutated))
        }));
        prop_assert!(
            result.is_ok(),
            "panic on mutated footer"
        );
        // Whether the resulting reader is Ok or Err is a function of
        // which byte we hit; we only require the no-panic contract.
        if let Ok(Ok(mut reader)) = result {
            // If the reader opened, every recorded blob must still be
            // readable without panicking (the JSON footer might have
            // accepted the mutation, but the payloads still live at
            // their original offsets).
            let n = reader.blobs().len();
            for i in 0..n {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    reader.read_blob(i)
                }));
                prop_assert!(r.is_ok(), "panic reading blob {i} of mutated file");
            }
        }
    }
}
