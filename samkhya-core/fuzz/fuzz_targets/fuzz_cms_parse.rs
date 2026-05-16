#![no_main]
use libfuzzer_sys::fuzz_target;
use samkhya_core::sketches::{CountMinSketch, Sketch};

fuzz_target!(|data: &[u8]| {
    let _ = CountMinSketch::from_bytes(data);
});
