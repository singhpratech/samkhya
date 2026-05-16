//! Small CSV helpers shared by the sketch builders.

use std::fs::File;
use std::path::Path;

use samkhya_core::{Error, Result};

fn map_csv(e: csv::Error) -> Error {
    Error::Serde(format!("csv: {e}"))
}

/// Iterate over the requested column and pass every raw cell to `sink`.
///
/// `header == true` skips the first record (treated as a header row).
pub fn for_each_cell<F>(input: &Path, column: usize, header: bool, mut sink: F) -> Result<()>
where
    F: FnMut(&str) -> Result<()>,
{
    let file = File::open(input)?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(header)
        .flexible(true)
        .from_reader(file);
    for record in rdr.records() {
        let rec = record.map_err(map_csv)?;
        let cell = rec.get(column).ok_or_else(|| {
            Error::Serde(format!(
                "csv row has no column {column} (got {} fields)",
                rec.len()
            ))
        })?;
        sink(cell)?;
    }
    Ok(())
}

/// Collect a numeric column as `f64`s. Empty cells are skipped; non-numeric
/// cells produce a descriptive error.
pub fn collect_f64(input: &Path, column: usize, header: bool) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    let mut row: usize = 0;
    for_each_cell(input, column, header, |cell| {
        row += 1;
        let trimmed = cell.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let v: f64 = trimmed.parse().map_err(|e| {
            Error::Serde(format!(
                "csv row {row} column {column}: cannot parse '{cell}' as f64: {e}"
            ))
        })?;
        out.push(v);
        Ok(())
    })?;
    Ok(out)
}
