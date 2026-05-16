#![no_main]
use libfuzzer_sys::fuzz_target;
use samkhya_core::sketches::{HllSketch, Sketch};

fuzz_target!(|data: &[u8]| {
    let _ = HllSketch::from_bytes(data);
});
