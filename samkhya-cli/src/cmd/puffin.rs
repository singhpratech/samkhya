//! `samkhya puffin ...` — pack and verify Puffin sidecars.

use std::fs::{self, File};
use std::path::Path;

use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{BloomFilter, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch};
use samkhya_core::{Error, Result};

/// Read each payload file and append it as a blob with the matching KIND.
pub fn pack(
    out: &Path,
    hll: &[std::path::PathBuf],
    bloom: &[std::path::PathBuf],
    cms: &[std::path::PathBuf],
    histogram: &[std::path::PathBuf],
) -> Result<()> {
    if hll.is_empty() && bloom.is_empty() && cms.is_empty() && histogram.is_empty() {
        return Err(Error::InvalidPuffin(
            "pack: no payload files provided (need at least one of --hll / --bloom / --cms / --histogram)"
                .into(),
        ));
    }

    // Collect payloads up front; PuffinWriter borrows the bytes.
    struct Entry {
        kind: &'static str,
        bytes: Vec<u8>,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut validate_named = |kind: &'static str,
                              paths: &[std::path::PathBuf],
                              validator: fn(&[u8]) -> Result<()>|
     -> Result<()> {
        for p in paths {
            let bytes = fs::read(p)?;
            validator(&bytes).map_err(|e| {
                Error::InvalidPuffin(format!(
                    "{}: payload at {} failed to decode: {}",
                    kind,
                    p.display(),
                    e
                ))
            })?;
            entries.push(Entry { kind, bytes });
        }
        Ok(())
    };
    validate_named(HllSketch::KIND, hll, |b| {
        HllSketch::from_bytes(b).map(|_| ())
    })?;
    validate_named(BloomFilter::KIND, bloom, |b| {
        BloomFilter::from_bytes(b).map(|_| ())
    })?;
    validate_named(CountMinSketch::KIND, cms, |b| {
        CountMinSketch::from_bytes(b).map(|_| ())
    })?;
    validate_named(EquiDepthHistogram::KIND, histogram, |b| {
        EquiDepthHistogram::from_bytes(b).map(|_| ())
    })?;

    let file = File::create(out)?;
    let mut writer = PuffinWriter::new(file);
    for e in &entries {
        writer.add_blob(Blob::new(e.kind, Vec::new(), &e.bytes))?;
    }
    writer.finish()?;
    println!("wrote {} blob(s) to {}", entries.len(), out.display());
    for e in &entries {
        println!("  - {} ({} bytes)", e.kind, e.bytes.len());
    }
    Ok(())
}

/// Full structural validation:
///   - footer parses
///   - every blob's payload is readable at the recorded offset/length
///   - every known-kind payload decodes through `Sketch::from_bytes`
pub fn verify(path: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut reader = PuffinReader::open(file)?;
    let total = reader.blobs().len();
    println!("== verify: {} ==", path.display());
    println!("blob count: {total}");

    let metas = reader.blobs().to_vec();
    let mut errors: Vec<String> = Vec::new();
    for (i, meta) in metas.iter().enumerate() {
        let payload = match reader.read_blob_decompressed(i) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("blob #{i} ({}): read failed: {e}", meta.kind));
                continue;
            }
        };
        let decode_result: Result<&'static str> = match meta.kind.as_str() {
            k if k == HllSketch::KIND => HllSketch::from_bytes(&payload).map(|_| "HLL"),
            k if k == BloomFilter::KIND => BloomFilter::from_bytes(&payload).map(|_| "Bloom"),
            k if k == CountMinSketch::KIND => CountMinSketch::from_bytes(&payload).map(|_| "CMS"),
            k if k == EquiDepthHistogram::KIND => {
                EquiDepthHistogram::from_bytes(&payload).map(|_| "Histogram")
            }
            _ => Ok("unknown"),
        };
        match decode_result {
            Ok(label) => println!("  blob #{i}: {} ({label}) ok", meta.kind),
            Err(e) => errors.push(format!("blob #{i} ({}): decode failed: {e}", meta.kind)),
        }
    }

    if errors.is_empty() {
        println!("ok ({total} blob(s))");
        Ok(())
    } else {
        for e in &errors {
            eprintln!("error: {e}");
        }
        Err(Error::InvalidPuffin(format!(
            "{} blob(s) failed validation",
            errors.len()
        )))
    }
}
