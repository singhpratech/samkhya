#![no_main]
use libfuzzer_sys::fuzz_target;
use samkhya_core::sketches::{CorrelatedHistogram2D, Sketch};

fuzz_target!(|data: &[u8]| {
    let _ = CorrelatedHistogram2D::from_bytes(data);
});
