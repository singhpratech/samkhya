//! End-to-end demo: build sketches over synthetic ids, persist them to a
//! Puffin sidecar, reopen the file and recover the sketches, then verify
//! cardinality / membership round-trips.
//!
//! Run with:
//! ```text
//! cargo run -p samkhya-core --example sketch_to_puffin
//! ```

use std::collections::HashSet;
use std::fs::{File, OpenOptions};

use samkhya_core::Result;
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{BloomFilter, HllSketch, Sketch};
use tempfile::NamedTempFile;

fn main() -> Result<()> {
    // 1. Generate 10_000 ids with ~5_000 distinct values.
    //
    // We deterministically interleave two cycles so the distinct count is
    // exactly 5_000 — no RNG dependency, fully reproducible across runs.
    let total = 10_000u64;
    let distinct_target = 5_000u64;
    let ids: Vec<u64> = (0..total).map(|i| i % distinct_target).collect();
    let true_distinct = ids.iter().copied().collect::<HashSet<u64>>().len() as u64;

    // 2. Build sketches.
    let mut hll = HllSketch::new(14)?;
    let mut bloom = BloomFilter::new(10_000, 0.01);
    for id in &ids {
        let bytes = id.to_le_bytes();
        hll.add(&bytes);
        bloom.insert(&bytes);
    }

    // 3. Serialize via the Sketch trait.
    let hll_bytes = hll.to_bytes()?;
    let bloom_bytes = bloom.to_bytes()?;

    // 4. Write both to a Puffin file under a tempfile path.
    let tmp = NamedTempFile::new()?;
    let path = tmp.path().to_path_buf();
    {
        let file = OpenOptions::new().write(true).truncate(true).open(&path)?;
        let mut writer = PuffinWriter::new(file);
        writer.add_blob(Blob::new(HllSketch::KIND, vec![1], &hll_bytes))?;
        writer.add_blob(Blob::new(BloomFilter::KIND, vec![1], &bloom_bytes))?;
        writer.finish()?;
    }

    // 5. Reopen the file.
    let mut reader = PuffinReader::open(File::open(&path)?)?;

    // 6. Recover the sketches.
    let (hll_idx, _) = reader
        .find_blob(HllSketch::KIND)
        .expect("HLL blob missing from Puffin footer");
    let hll_payload = reader.read_blob(hll_idx)?;
    let hll_back = HllSketch::from_bytes(&hll_payload)?;

    let (bloom_idx, _) = reader
        .find_blob(BloomFilter::KIND)
        .expect("Bloom blob missing from Puffin footer");
    let bloom_payload = reader.read_blob(bloom_idx)?;
    let bloom_back = BloomFilter::from_bytes(&bloom_payload)?;

    // 7. Report.
    let estimate = hll_back.estimate();
    let err_pct = ((estimate as f64 - true_distinct as f64).abs() / true_distinct as f64) * 100.0;

    println!("samkhya-core: sketch -> Puffin -> sketch round-trip");
    println!("---------------------------------------------------");
    println!("Puffin path                : {}", path.display());
    println!("Rows ingested              : {total}");
    println!("True distinct count        : {true_distinct}");
    println!("HLL estimate (p=14)        : {estimate}");
    println!("HLL relative error         : {err_pct:.3}%");
    println!();
    println!("Bloom membership probes:");
    for &probe in &[0u64, 1, 42, 4_999, 5_000, 9_999, 1_000_000] {
        let bytes = probe.to_le_bytes();
        let present = bloom_back.contains(&bytes);
        let truly_present = (probe as u64) < distinct_target;
        println!("  id={probe:>8}  bloom_says={present:<5}  actually_inserted={truly_present}");
    }

    Ok(())
}
