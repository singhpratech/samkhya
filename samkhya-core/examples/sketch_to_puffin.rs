//! End-to-end demo: build sketches over synthetic ids, persist them to a
//! Puffin sidecar, reopen the file and recover the sketches, then verify
//! cardinality / membership round-trips.
//!
//! Now also exposes a `--sweep` mode (WAVE5-H pipeline closure) that drives
//! a parameter grid of (sketch_kind, n_rows, configuration) and records
//! per-trial wallclock + estimated RSS-delta for each cell. The sweep
//! emits a single JSON sidecar to `bench-results/09_memory_profile_raw.json`
//! (path overrideable via `SAMKHYA_RAW_OUT`) that downstream BCa CI scripts
//! consume.
//!
//! Smoke run (single-blob round-trip, original behaviour preserved):
//! ```text
//! cargo run -p samkhya-core --example sketch_to_puffin
//! ```
//!
//! Sweep mode (per-cell JSON sidecar):
//! ```text
//! SAMKHYA_RAW_OUT=bench-results/09_memory_profile_raw.json \
//!     cargo run --release -p samkhya-core --example sketch_to_puffin -- --sweep
//! ```

use std::collections::HashSet;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::Instant;

use samkhya_core::Result;
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{BloomFilter, HllSketch, Sketch};
use tempfile::NamedTempFile;

fn smoke_main() -> Result<()> {
    // 1. Generate 10_000 ids with ~5_000 distinct values.
    let total = 10_000u64;
    let distinct_target = 5_000u64;
    let ids: Vec<u64> = (0..total).map(|i| i % distinct_target).collect();
    let true_distinct = ids.iter().copied().collect::<HashSet<u64>>().len() as u64;

    let mut hll = HllSketch::new(14)?;
    let mut bloom = BloomFilter::new(10_000, 0.01);
    for id in &ids {
        let bytes = id.to_le_bytes();
        hll.add(&bytes);
        bloom.insert(&bytes);
    }

    let hll_bytes = hll.to_bytes()?;
    let bloom_bytes = bloom.to_bytes()?;

    let tmp = NamedTempFile::new()?;
    let path = tmp.path().to_path_buf();
    {
        let file = OpenOptions::new().write(true).truncate(true).open(&path)?;
        let mut writer = PuffinWriter::new(file);
        writer.add_blob(Blob::new(HllSketch::KIND, vec![1], &hll_bytes))?;
        writer.add_blob(Blob::new(BloomFilter::KIND, vec![1], &bloom_bytes))?;
        writer.finish()?;
    }

    let mut reader = PuffinReader::open(File::open(&path)?)?;

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
        let truly_present = probe < distinct_target;
        println!("  id={probe:>8}  bloom_says={present:<5}  actually_inserted={truly_present}");
    }
    Ok(())
}

/// Parse `/proc/self/statm` and return resident-set size in bytes.
/// Returns 0 on non-Linux or if the file is unreadable; the sweep
/// gracefully degrades on those hosts (the wallclock cell is still useful).
fn rss_bytes() -> u64 {
    let s = match std::fs::read_to_string("/proc/self/statm") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    // statm: size resident shared text lib data dt — page-units.
    let resident_pages: u64 = s
        .split_whitespace()
        .nth(1)
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    let page_size = 4096u64; // standard on x86_64 Linux; off-by-architecture is harmless for our deltas.
    resident_pages.saturating_mul(page_size)
}

#[derive(Clone)]
struct CellCfg {
    /// Sketch family identifier — "hll" or "bloom".
    kind: &'static str,
    /// Sketch configuration parameter — HLL precision or Bloom expected
    /// capacity. The interpretation is `kind`-dependent.
    config: u64,
    /// Number of inserts per trial.
    n_rows: u64,
}

const SWEEP_CELLS: &[CellCfg] = &[
    // HLL: 4 precisions × 3 scales = 12 cells
    CellCfg {
        kind: "hll",
        config: 10,
        n_rows: 10_000,
    },
    CellCfg {
        kind: "hll",
        config: 10,
        n_rows: 100_000,
    },
    CellCfg {
        kind: "hll",
        config: 10,
        n_rows: 1_000_000,
    },
    CellCfg {
        kind: "hll",
        config: 12,
        n_rows: 10_000,
    },
    CellCfg {
        kind: "hll",
        config: 12,
        n_rows: 100_000,
    },
    CellCfg {
        kind: "hll",
        config: 12,
        n_rows: 1_000_000,
    },
    CellCfg {
        kind: "hll",
        config: 14,
        n_rows: 10_000,
    },
    CellCfg {
        kind: "hll",
        config: 14,
        n_rows: 100_000,
    },
    CellCfg {
        kind: "hll",
        config: 14,
        n_rows: 1_000_000,
    },
    CellCfg {
        kind: "hll",
        config: 16,
        n_rows: 10_000,
    },
    CellCfg {
        kind: "hll",
        config: 16,
        n_rows: 100_000,
    },
    CellCfg {
        kind: "hll",
        config: 16,
        n_rows: 1_000_000,
    },
    // Bloom: 3 capacities × 3 scales = 9 cells
    CellCfg {
        kind: "bloom",
        config: 1_000,
        n_rows: 10_000,
    },
    CellCfg {
        kind: "bloom",
        config: 10_000,
        n_rows: 10_000,
    },
    CellCfg {
        kind: "bloom",
        config: 100_000,
        n_rows: 100_000,
    },
    CellCfg {
        kind: "bloom",
        config: 10_000,
        n_rows: 100_000,
    },
    CellCfg {
        kind: "bloom",
        config: 100_000,
        n_rows: 1_000_000,
    },
    CellCfg {
        kind: "bloom",
        config: 1_000_000,
        n_rows: 1_000_000,
    },
];

const TRIALS_PER_CELL: usize = 10;

/// Run one trial: build the sketch in memory, serialize it, write a
/// single-blob Puffin file to a tempfile, reopen and read the blob back,
/// deserialize. Returns (build_ns, write_ns, read_ns, deser_ns, rss_delta_bytes,
/// payload_bytes).
fn run_sweep_trial(cell: &CellCfg, trial: usize) -> Result<(u64, u64, u64, u64, i64, u64)> {
    let seed = 0xA5A5_5A5A_DEAD_BEEFu64
        .wrapping_add(cell.config)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(cell.n_rows)
        .wrapping_add(trial as u64);

    let rss_before = rss_bytes();

    // Build phase.
    let build_start = Instant::now();
    let payload: Vec<u8> = match cell.kind {
        "hll" => {
            let p = cell.config as u8;
            let mut h = HllSketch::new(p)?;
            let mut state = seed;
            for _ in 0..cell.n_rows {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                h.add(&z.to_le_bytes());
            }
            h.to_bytes()?
        }
        "bloom" => {
            let cap = cell.config as usize;
            let mut b = BloomFilter::new(cap, 0.01);
            let mut state = seed;
            for _ in 0..cell.n_rows {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                b.insert(&z.to_le_bytes());
            }
            b.to_bytes()?
        }
        _ => unreachable!(),
    };
    let build_ns = build_start.elapsed().as_nanos() as u64;
    let payload_bytes = payload.len() as u64;

    let tmp = NamedTempFile::new()?;
    let path = tmp.path().to_path_buf();

    // Write phase.
    let write_start = Instant::now();
    {
        let file = OpenOptions::new().write(true).truncate(true).open(&path)?;
        let mut writer = PuffinWriter::new(file);
        let kind_tag = match cell.kind {
            "hll" => HllSketch::KIND,
            "bloom" => BloomFilter::KIND,
            _ => unreachable!(),
        };
        writer.add_blob(Blob::new(kind_tag, vec![1], &payload))?;
        writer.finish()?;
    }
    let write_ns = write_start.elapsed().as_nanos() as u64;

    // Read phase.
    let read_start = Instant::now();
    let mut reader = PuffinReader::open(File::open(&path)?)?;
    let kind_tag = match cell.kind {
        "hll" => HllSketch::KIND,
        "bloom" => BloomFilter::KIND,
        _ => unreachable!(),
    };
    let (idx, _) = reader
        .find_blob(kind_tag)
        .expect("just-written blob present in footer");
    let read_payload = reader.read_blob(idx)?;
    let read_ns = read_start.elapsed().as_nanos() as u64;

    // Deserialize phase.
    let deser_start = Instant::now();
    match cell.kind {
        "hll" => {
            let _h = HllSketch::from_bytes(&read_payload)?;
        }
        "bloom" => {
            let _b = BloomFilter::from_bytes(&read_payload)?;
        }
        _ => unreachable!(),
    }
    let deser_ns = deser_start.elapsed().as_nanos() as u64;

    let rss_after = rss_bytes();
    let rss_delta = rss_after as i64 - rss_before as i64;

    Ok((
        build_ns,
        write_ns,
        read_ns,
        deser_ns,
        rss_delta,
        payload_bytes,
    ))
}

fn sweep_main() -> Result<()> {
    let raw_path = env::var("SAMKHYA_RAW_OUT")
        .unwrap_or_else(|_| "bench-results/09_memory_profile_raw.json".to_string());

    println!("# sketch_to_puffin --sweep: per-cell build/write/read/deser/RSS sidecar");
    println!(
        "kind,config,n_rows,trial,build_ns,write_ns,read_ns,deser_ns,rss_delta_bytes,payload_bytes"
    );

    let mut cell_records: Vec<String> = Vec::with_capacity(SWEEP_CELLS.len());
    let overall_start = Instant::now();

    for cell in SWEEP_CELLS {
        let mut build_ns_v: Vec<u64> = Vec::with_capacity(TRIALS_PER_CELL);
        let mut write_ns_v: Vec<u64> = Vec::with_capacity(TRIALS_PER_CELL);
        let mut read_ns_v: Vec<u64> = Vec::with_capacity(TRIALS_PER_CELL);
        let mut deser_ns_v: Vec<u64> = Vec::with_capacity(TRIALS_PER_CELL);
        let mut rss_delta_v: Vec<i64> = Vec::with_capacity(TRIALS_PER_CELL);
        let mut payload_bytes_v: Vec<u64> = Vec::with_capacity(TRIALS_PER_CELL);

        for t in 0..TRIALS_PER_CELL {
            let (b, w, r, d, rss, p) = run_sweep_trial(cell, t)?;
            build_ns_v.push(b);
            write_ns_v.push(w);
            read_ns_v.push(r);
            deser_ns_v.push(d);
            rss_delta_v.push(rss);
            payload_bytes_v.push(p);
            println!(
                "{},{},{},{},{},{},{},{},{},{}",
                cell.kind, cell.config, cell.n_rows, t, b, w, r, d, rss, p
            );
        }

        let join_u64 = |v: &[u64]| {
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let join_i64 = |v: &[i64]| {
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        cell_records.push(format!(
            "{{\"kind\":\"{}\",\"config\":{},\"n_rows\":{},\"trials\":{TRIALS_PER_CELL},\"build_ns\":[{}],\"write_ns\":[{}],\"read_ns\":[{}],\"deser_ns\":[{}],\"rss_delta_bytes\":[{}],\"payload_bytes\":[{}]}}",
            cell.kind,
            cell.config,
            cell.n_rows,
            join_u64(&build_ns_v),
            join_u64(&write_ns_v),
            join_u64(&read_ns_v),
            join_u64(&deser_ns_v),
            join_i64(&rss_delta_v),
            join_u64(&payload_bytes_v),
        ));
    }

    let elapsed = overall_start.elapsed();
    println!();
    println!(
        "# wall: {:.2}s, cells: {}, trials/cell: {}",
        elapsed.as_secs_f64(),
        SWEEP_CELLS.len(),
        TRIALS_PER_CELL
    );

    let body = format!(
        "{{\"benchmark\":\"sketch_to_puffin_sweep\",\"seed_scheme\":\"0xA5A5_5A5A_DEAD_BEEF + config*0x9E37 + n_rows + trial\",\"trials_per_cell\":{TRIALS_PER_CELL},\"cells\":[{}]}}",
        cell_records.join(",")
    );
    let mut f = File::create(&raw_path).expect("create raw output file");
    f.write_all(body.as_bytes()).expect("write raw output");
    eprintln!("# raw per-cell vectors written to {raw_path}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--sweep") {
        sweep_main()
    } else {
        smoke_main()
    }
}
