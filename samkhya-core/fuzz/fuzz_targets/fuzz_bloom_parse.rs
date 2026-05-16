#![no_main]
use libfuzzer_sys::fuzz_target;
use samkhya_core::sketches::{BloomFilter, Sketch};

fuzz_target!(|data: &[u8]| {
    let _ = BloomFilter::from_bytes(data);
});
