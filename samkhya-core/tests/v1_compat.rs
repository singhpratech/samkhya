//! Frozen v1 payloads: new readers must continue to decode deployed sketches.

use samkhya_core::sketches::{EquiDepthHistogram, HllSketch, Sketch};

const HLL_V1: &str = "04100000000000000000000400000000020000020002000002";
const HISTOGRAM_V1: &str = "0300000000000000000000000000f03f000000000000004000000000000010400200000000000000020000000000000002000000000000000400000000000000";

fn decode_hex(input: &str) -> Vec<u8> {
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(digits, 16).unwrap()
        })
        .collect()
}

#[test]
fn reads_frozen_hll_v1_payload() {
    let bytes = decode_hex(HLL_V1);
    let sketch = HllSketch::from_bytes(&bytes).expect("v1 HLL remains readable");
    assert_eq!(sketch.to_bytes().unwrap(), bytes);
    assert!((3..=7).contains(&sketch.estimate()));
}

#[test]
fn reads_frozen_equi_depth_v1_payload() {
    let bytes = decode_hex(HISTOGRAM_V1);
    let histogram = EquiDepthHistogram::from_bytes(&bytes).expect("v1 histogram remains readable");
    assert_eq!(histogram.to_bytes().unwrap(), bytes);
    assert_eq!(histogram.total(), 4);
    assert_eq!(histogram.min(), Some(1.0));
    assert_eq!(histogram.max(), Some(4.0));
}
