//! Fuzz the five sketch decoders.
//!
//! Each sketch ships its own `from_bytes(&[u8]) -> Result<Self>` codec. They
//! are all reachable through Puffin sidecars (the on-disk format) and
//! through the `samkhya-py` bindings (the in-process format), which means
//! every decoder runs on attacker-controlled bytes. The invariant under
//! test is that **no decoder panics on arbitrary input** — malformed bytes
//! must surface as `Err(_)`, never as an unwrap / panic / overflow.
//!
//! Coverage shape: split the input into five disjoint slices and feed each
//! decoder one. Splitting (rather than feeding all five the same bytes) is
//! the cheap way to maximize per-decoder coverage at a given fuzz budget;
//! it costs one byte of input-prefix and keeps each decoder seeing
//! independent attacker patterns.

#![no_main]

use libfuzzer_sys::fuzz_target;
use samkhya_core::sketches::{
    BloomFilter, CorrelatedHistogram2D, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch,
};

/// Split `data` into `n` roughly-equal slices. Returns at most `n` slices;
/// shorter inputs simply yield fewer.
fn split(data: &[u8], n: usize) -> Vec<&[u8]> {
    if data.is_empty() || n == 0 {
        return Vec::new();
    }
    let chunk = data.len() / n;
    if chunk == 0 {
        // Input too short to split: feed the whole thing to the first decoder.
        return vec![data];
    }
    (0..n)
        .map(|i| {
            let start = i * chunk;
            let end = if i + 1 == n { data.len() } else { start + chunk };
            &data[start..end]
        })
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let parts = split(data, 5);

    // Each decoder runs in its own scoped block so panics produce a
    // distinctive backtrace pointing at the offending decoder rather than a
    // generic location somewhere in this fuzz harness.
    if let Some(p) = parts.first() {
        let _ = HllSketch::from_bytes(p);
    }
    if let Some(p) = parts.get(1) {
        let _ = BloomFilter::from_bytes(p);
    }
    if let Some(p) = parts.get(2) {
        let _ = CountMinSketch::from_bytes(p);
    }
    if let Some(p) = parts.get(3) {
        let _ = EquiDepthHistogram::from_bytes(p);
    }
    if let Some(p) = parts.get(4) {
        let _ = CorrelatedHistogram2D::from_bytes(p);
    }
});
