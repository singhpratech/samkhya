//! Inspect a Puffin file and dump its blob metadata.
//!
//! Usage:
//! ```sh
//! cargo run -p samkhya-core --example inspect_puffin -- <path/to/file.puffin>
//! ```
//!
//! Prints the footer JSON metadata, lists every blob with its kind /
//! fields / offset / length / compression codec, and decodes any blobs
//! whose `kind` matches a known samkhya sketch (`samkhya.hll-v1`,
//! `samkhya.bloom-v1`, `samkhya.cms-v1`, `samkhya.histogram-equidepth-v1`).

use std::env;
use std::fs::File;
use std::process::ExitCode;

use samkhya_core::Result;
use samkhya_core::puffin::PuffinReader;
use samkhya_core::sketches::{BloomFilter, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch};

fn run(path: &str) -> Result<()> {
    let file = File::open(path)?;
    let mut reader = PuffinReader::open(file)?;

    println!("== puffin file: {path} ==");
    println!("blob count: {}", reader.blobs().len());
    if let Ok(json) = serde_json::to_string_pretty(reader.footer()) {
        println!("\nfooter (json):");
        println!("{json}");
    }

    for (i, meta) in reader.blobs().to_vec().iter().enumerate() {
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
                println!("  decoded:         (unknown kind '{other}'; raw {} bytes)", payload.len());
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: {} <path/to/file.puffin>", args[0]);
        return ExitCode::from(2);
    }
    match run(&args[1]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
