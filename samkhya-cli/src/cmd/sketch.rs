//! `samkhya sketch ...` — build sketches from CSV columns.

use std::fs;
use std::path::Path;

use samkhya_core::Result;
use samkhya_core::sketches::{BloomFilter, CountMinSketch, EquiDepthHistogram, HllSketch, Sketch};

use super::csv_io;

fn maybe_write(output: Option<&Path>, payload: &[u8]) -> Result<()> {
    if let Some(path) = output {
        fs::write(path, payload)?;
        println!("wrote {} bytes to {}", payload.len(), path.display());
    }
    Ok(())
}

pub fn hll(
    input: &Path,
    column: usize,
    precision: u8,
    header: bool,
    output: Option<&Path>,
) -> Result<()> {
    let mut sketch = HllSketch::new(precision)?;
    let mut rows: u64 = 0;
    csv_io::for_each_cell(input, column, header, |cell| {
        sketch.add(cell.as_bytes());
        rows += 1;
        Ok(())
    })?;
    let estimate = sketch.estimate();
    println!("== HLL ==");
    println!("input:      {}", input.display());
    println!("column:     {column}");
    println!("precision:  {precision}");
    println!("rows fed:   {rows}");
    println!("estimate:   {estimate}");
    let bytes = sketch.to_bytes()?;
    println!("payload:    {} bytes", bytes.len());
    maybe_write(output, &bytes)?;
    Ok(())
}

pub fn bloom(
    input: &Path,
    column: usize,
    capacity: usize,
    fp_rate: f64,
    header: bool,
    output: Option<&Path>,
) -> Result<()> {
    let mut sketch = BloomFilter::new(capacity, fp_rate);
    let mut rows: u64 = 0;
    csv_io::for_each_cell(input, column, header, |cell| {
        sketch.insert(cell.as_bytes());
        rows += 1;
        Ok(())
    })?;
    println!("== Bloom ==");
    println!("input:      {}", input.display());
    println!("column:     {column}");
    println!("capacity:   {capacity}");
    println!("fp_rate:    {fp_rate}");
    println!("rows fed:   {rows}");
    println!("num_bits:   {}", sketch.num_bits());
    println!("num_hashes: {}", sketch.num_hashes());
    let bytes = sketch.to_bytes()?;
    println!("payload:    {} bytes", bytes.len());
    maybe_write(output, &bytes)?;
    Ok(())
}

pub fn cms(
    input: &Path,
    column: usize,
    depth: u32,
    width: u32,
    header: bool,
    output: Option<&Path>,
) -> Result<()> {
    let mut sketch = CountMinSketch::new(depth, width)?;
    let mut rows: u64 = 0;
    csv_io::for_each_cell(input, column, header, |cell| {
        sketch.add(cell.as_bytes(), 1);
        rows += 1;
        Ok(())
    })?;
    println!("== Count-Min Sketch ==");
    println!("input:      {}", input.display());
    println!("column:     {column}");
    println!("depth:      {depth}");
    println!("width:      {width}");
    println!("rows fed:   {rows}");
    println!("total:      {}", sketch.total());
    let bytes = sketch.to_bytes()?;
    println!("payload:    {} bytes", bytes.len());
    maybe_write(output, &bytes)?;
    Ok(())
}

pub fn histogram(
    input: &Path,
    column: usize,
    buckets: usize,
    header: bool,
    output: Option<&Path>,
) -> Result<()> {
    let values = csv_io::collect_f64(input, column, header)?;
    let sketch = EquiDepthHistogram::from_values(&values, buckets)?;
    println!("== Equi-Depth Histogram ==");
    println!("input:      {}", input.display());
    println!("column:     {column}");
    println!("buckets:    {buckets}");
    println!("values:     {}", values.len());
    println!("total:      {}", sketch.total());
    println!("bucket cnt: {}", sketch.buckets());
    let bytes = sketch.to_bytes()?;
    println!("payload:    {} bytes", bytes.len());
    maybe_write(output, &bytes)?;
    Ok(())
}
