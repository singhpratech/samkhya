#![no_main]
use libfuzzer_sys::fuzz_target;
use samkhya_core::sketches::{EquiDepthHistogram, Sketch};

fuzz_target!(|data: &[u8]| {
    let _ = EquiDepthHistogram::from_bytes(data);
});
