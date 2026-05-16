//! `samkhya inspect <path>` — dump a Puffin sidecar.
//!
//! Ports the `inspect_puffin` example into the CLI: prints the footer
//! JSON, lists every blob, and decodes any blob whose `kind` matches a
//! known samkhya sketch (`samkhya.hll-v1`, `samkhya.bloom-v1`,
//! `samkhya.cms-v1`, `samkhya.histogram-equidepth-v1`).

use std::fs::File;
use std::path::Path;

use samkhya_core::Result;
use samkhya_core::puffin::PuffinReader;
use samkhya_core::sketches::{BloomFilter, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch};

pub fn run(path: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut reader = PuffinReader::open(file)?;

    println!("== puffin file: {} ==", path.display());
    println!("blob count: {}", reader.blobs().len());
    if let Ok(json) = serde_json::to_string_pretty(reader.footer()) {
        println!("\nfooter (json):");
        println!("{json}");
    }

    let metas = reader.blobs().to_vec();
    for (i, meta) in metas.iter().enumerate() {
        println!();
        println!("blob #{i}:");
        println!("  kind:            {}", meta.kind);
        println!("  fields:          {:?}", meta.fields);
        println!("  offset:          {}", meta.offset);
        println!("  length:          {}", meta.length);
        if let Some(codec) = &meta.compression_codec {
            println!("  compression:     {codec}");
        }

        let payload = match reader.read_blob(i) {
            Ok(p) => p,
            Err(e) => {
                println!("  payload:         (could not read: {e})");
                continue;
            }
        };

        match meta.kind.as_str() {
            k if k == HllSketch::KIND => {
                if let Ok(s) = HllSketch::from_bytes(&payload) {
                    println!(
                        "  decoded:         HLL (precision={}, estimate={})",
                        s.precision(),
                        s.estimate()
                    );
                }
            }
            k if k == BloomFilter::KIND => {
                if let Ok(s) = BloomFilter::from_bytes(&payload) {
                    println!(
                        "  decoded:         Bloom (bits={}, hashes={})",
                        s.num_bits(),
                        s.num_hashes()
                    );
                }
            }
            k if k == CountMinSketch::KIND => {
                if let Ok(s) = CountMinSketch::from_bytes(&payload) {
                    println!(
                        "  decoded:         CMS (depth={}, width={}, total={})",
                        s.depth(),
                        s.width(),
                        s.total()
                    );
                }
            }
            k if k == EquiDepthHistogram::KIND => {
                if let Ok(s) = EquiDepthHistogram::from_bytes(&payload) {
                    println!(
                        "  decoded:         Histogram (buckets={}, total={})",
                        s.buckets(),
                        s.total()
                    );
                }
            }
            other => {
                println!(
                    "  decoded:         (unknown kind '{other}'; raw {} bytes)",
                    payload.len()
                );
            }
        }
    }
    Ok(())
}
