//! WAVE-5F audit helper: read row_count blobs from CSV-built `*.puffin`
//! and Parquet-built `*.parquet.puffin` sidecars and print a table.
//!
//! Throwaway — not part of the v1.0 build matrix. Run via:
//!   cargo run -p samkhya-bench --release --example wave5f_audit -- samkhya-bench/data/job

use std::fs::File;
use std::path::Path;

use samkhya_bench::imdb::{ROW_COUNT_KIND, TABLES};
use samkhya_core::puffin::PuffinReader;

fn read_row_count(path: &Path) -> Option<u64> {
    let file = File::open(path).ok()?;
    let mut reader = PuffinReader::open(file).ok()?;
    let metas = reader.blobs().to_vec();
    for (i, meta) in metas.iter().enumerate() {
        if meta.kind == ROW_COUNT_KIND {
            let payload = reader.read_blob(i).ok()?;
            if payload.len() >= 8 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&payload[..8]);
                return Some(u64::from_le_bytes(buf));
            }
        }
    }
    None
}

fn main() {
    let imdb_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "samkhya-bench/data/job".to_string());
    let imdb_dir = Path::new(&imdb_dir);
    println!(
        "{:<20} {:>14} {:>14} {:>10}",
        "table", "csv_rows", "parquet_rows", "match"
    );
    println!("{}", "-".repeat(62));
    let mut total_csv: u64 = 0;
    let mut total_parq: u64 = 0;
    let mut all_match = true;
    for &t in TABLES {
        let csv_side = imdb_dir.join(format!("{t}.puffin"));
        let parq_side = imdb_dir.join(format!("{t}.parquet.puffin"));
        let c = read_row_count(&csv_side);
        let p = read_row_count(&parq_side);
        let m = matches!((c, p), (Some(x), Some(y)) if x == y);
        if !m {
            all_match = false;
        }
        total_csv += c.unwrap_or(0);
        total_parq += p.unwrap_or(0);
        println!(
            "{:<20} {:>14} {:>14} {:>10}",
            t,
            c.map(|n| n.to_string()).unwrap_or_else(|| "MISSING".into()),
            p.map(|n| n.to_string()).unwrap_or_else(|| "MISSING".into()),
            if m { "yes" } else { "NO" }
        );
    }
    println!("{}", "-".repeat(62));
    println!(
        "{:<20} {:>14} {:>14} {:>10}",
        "TOTAL",
        total_csv,
        total_parq,
        if all_match { "yes" } else { "NO" }
    );
}
