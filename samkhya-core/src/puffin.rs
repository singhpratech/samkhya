//! Iceberg Puffin sidecar format — reader and writer.
//!
//! Spec: <https://iceberg.apache.org/puffin-spec/>
//!
//! Layout:
//! ```text
//!  +-----------+
//!  | Magic     |  4 bytes "PFA1"
//!  +-----------+
//!  | Blob 1    |  variable
//!  +-----------+
//!  | Blob 2    |  variable
//!  +-----------+
//!  | ...       |
//!  +-----------+
//!  | Footer    |  Magic + JSON payload + payload-len(LE u32)
//!  |           |  + flags(LE u32) + Magic
//!  +-----------+
//! ```

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const MAGIC: &[u8; 4] = b"PFA1";

/// Footer payload (JSON-encoded inside the file).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FooterPayload {
    pub blobs: Vec<BlobMetadata>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// Per-blob metadata stored in the footer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobMetadata {
    #[serde(rename = "type")]
    pub kind: String,
    pub fields: Vec<i32>,
    #[serde(
        rename = "snapshot-id",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub snapshot_id: Option<i64>,
    #[serde(
        rename = "sequence-number",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sequence_number: Option<i64>,
    pub offset: u64,
    pub length: u64,
    #[serde(
        rename = "compression-codec",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub compression_codec: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// A blob to be written to a Puffin file.
pub struct Blob<'a> {
    pub kind: String,
    pub fields: Vec<i32>,
    pub payload: &'a [u8],
    pub properties: BTreeMap<String, String>,
}

impl<'a> Blob<'a> {
    pub fn new(kind: impl Into<String>, fields: Vec<i32>, payload: &'a [u8]) -> Self {
        Self {
            kind: kind.into(),
            fields,
            payload,
            properties: BTreeMap::new(),
        }
    }
}

/// Streaming writer for Puffin files.
pub struct PuffinWriter<W: Write + Seek> {
    inner: W,
    blobs: Vec<BlobMetadata>,
    pos: u64,
    wrote_head: bool,
}

impl<W: Write + Seek> PuffinWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            blobs: Vec::new(),
            pos: 0,
            wrote_head: false,
        }
    }

    fn ensure_head(&mut self) -> Result<()> {
        if !self.wrote_head {
            self.inner.write_all(MAGIC)?;
            self.pos += MAGIC.len() as u64;
            self.wrote_head = true;
        }
        Ok(())
    }

    /// Append a blob to the file.
    pub fn add_blob(&mut self, blob: Blob<'_>) -> Result<()> {
        self.ensure_head()?;
        let offset = self.pos;
        self.inner.write_all(blob.payload)?;
        let length = blob.payload.len() as u64;
        self.pos += length;
        self.blobs.push(BlobMetadata {
            kind: blob.kind,
            fields: blob.fields,
            snapshot_id: None,
            sequence_number: None,
            offset,
            length,
            compression_codec: None,
            properties: blob.properties,
        });
        Ok(())
    }

    /// Finalize the file: write the footer and return the inner writer.
    pub fn finish(mut self) -> Result<W> {
        self.ensure_head()?;
        let footer = FooterPayload {
            blobs: self.blobs,
            properties: BTreeMap::new(),
        };
        let payload = serde_json::to_vec(&footer)
            .map_err(|e| Error::InvalidPuffin(format!("footer JSON encode: {e}")))?;
        let payload_len = u32::try_from(payload.len())
            .map_err(|_| Error::InvalidPuffin("footer payload exceeds u32".into()))?;

        self.inner.write_all(MAGIC)?;
        self.inner.write_all(&payload)?;
        self.inner.write_u32::<LittleEndian>(payload_len)?;
        self.inner.write_u32::<LittleEndian>(0)?; // flags
        self.inner.write_all(MAGIC)?;
        Ok(self.inner)
    }
}

/// Reader for Puffin files — parses the footer once, lazily loads blob payloads.
pub struct PuffinReader<R: Read + Seek> {
    inner: R,
    footer: FooterPayload,
}

impl<R: Read + Seek> PuffinReader<R> {
    pub fn open(mut inner: R) -> Result<Self> {
        let file_len = inner.seek(SeekFrom::End(0))?;
        // 4 (head magic) + 4 (footer head magic) + 4 (payload len) + 4 (flags) + 4 (trailing magic)
        if file_len < 20 {
            return Err(Error::InvalidPuffin(format!(
                "file too short: {file_len} bytes"
            )));
        }

        // Trailing magic
        inner.seek(SeekFrom::End(-4))?;
        let mut trailing = [0u8; 4];
        inner.read_exact(&mut trailing)?;
        if &trailing != MAGIC {
            return Err(Error::InvalidPuffin("trailing magic missing".into()));
        }

        // Flags (4 bytes before trailing magic) — read but ignored for now.
        inner.seek(SeekFrom::End(-8))?;
        let _flags = inner.read_u32::<LittleEndian>()?;

        // Payload length (4 bytes before flags)
        inner.seek(SeekFrom::End(-12))?;
        let payload_len = inner.read_u32::<LittleEndian>()? as u64;

        let footer_total = 16u64 + payload_len; // head magic + payload + len + flags + trailing magic
        if file_len < footer_total {
            return Err(Error::InvalidPuffin(
                "payload length exceeds file size".into(),
            ));
        }

        // Payload bytes
        let payload_start = file_len - 12 - payload_len;
        inner.seek(SeekFrom::Start(payload_start))?;
        let mut payload = vec![0u8; payload_len as usize];
        inner.read_exact(&mut payload)?;

        // Footer head magic (4 bytes before payload)
        inner.seek(SeekFrom::Start(payload_start - 4))?;
        let mut footer_head = [0u8; 4];
        inner.read_exact(&mut footer_head)?;
        if &footer_head != MAGIC {
            return Err(Error::InvalidPuffin("footer head magic missing".into()));
        }

        // File head magic
        inner.seek(SeekFrom::Start(0))?;
        let mut head = [0u8; 4];
        inner.read_exact(&mut head)?;
        if &head != MAGIC {
            return Err(Error::InvalidPuffin("file head magic missing".into()));
        }

        let footer: FooterPayload = serde_json::from_slice(&payload)
            .map_err(|e| Error::InvalidPuffin(format!("footer JSON decode: {e}")))?;

        Ok(Self { inner, footer })
    }

    pub fn footer(&self) -> &FooterPayload {
        &self.footer
    }

    pub fn blobs(&self) -> &[BlobMetadata] {
        &self.footer.blobs
    }

    /// Read a blob's payload by index.
    pub fn read_blob(&mut self, idx: usize) -> Result<Vec<u8>> {
        let meta = self
            .footer
            .blobs
            .get(idx)
            .ok_or_else(|| Error::InvalidPuffin(format!("blob index {idx} out of range")))?;
        self.inner.seek(SeekFrom::Start(meta.offset))?;
        let mut buf = vec![0u8; meta.length as usize];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Find the first blob whose `type` (kind) matches `kind`.
    pub fn find_blob(&self, kind: &str) -> Option<(usize, &BlobMetadata)> {
        self.footer
            .blobs
            .iter()
            .enumerate()
            .find(|(_, b)| b.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::sketches::{HllSketch, Sketch};

    #[test]
    fn round_trip_empty() {
        let writer = PuffinWriter::new(Cursor::new(Vec::new()));
        let cursor = writer.finish().unwrap();
        let reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        assert!(reader.blobs().is_empty());
    }

    #[test]
    fn round_trip_single_blob() {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob(Blob::new("samkhya.test-v1", vec![0], b"hello puffin"))
            .unwrap();
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        assert_eq!(reader.blobs().len(), 1);
        assert_eq!(reader.blobs()[0].kind, "samkhya.test-v1");
        assert_eq!(reader.read_blob(0).unwrap(), b"hello puffin");
    }

    #[test]
    fn round_trip_multiple_blobs() {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob(Blob::new("samkhya.hll-v1", vec![1], &[1, 2, 3, 4, 5]))
            .unwrap();
        writer
            .add_blob(Blob::new("samkhya.bloom-v1", vec![2], &[10, 20, 30]))
            .unwrap();
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        assert_eq!(reader.blobs().len(), 2);
        assert_eq!(reader.read_blob(0).unwrap(), vec![1, 2, 3, 4, 5]);
        assert_eq!(reader.read_blob(1).unwrap(), vec![10, 20, 30]);
        assert_eq!(
            reader.find_blob("samkhya.bloom-v1").map(|(i, _)| i),
            Some(1)
        );
        assert_eq!(reader.find_blob("absent.kind").map(|(i, _)| i), None);
    }

    #[test]
    fn round_trip_hll_sketch_through_puffin() {
        let mut hll = HllSketch::new(12).unwrap();
        for i in 0..1000u32 {
            hll.add(&i.to_le_bytes());
        }
        let payload = hll.to_bytes().unwrap();

        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob(Blob::new(HllSketch::KIND, vec![7], &payload))
            .unwrap();
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        let (idx, meta) = reader.find_blob(HllSketch::KIND).unwrap();
        assert_eq!(meta.fields, vec![7]);
        let blob_bytes = reader.read_blob(idx).unwrap();
        let hll2 = HllSketch::from_bytes(&blob_bytes).unwrap();
        let err = (hll2.estimate() as f64 - 1000.0).abs() / 1000.0;
        assert!(err < 0.1, "HLL estimate via Puffin off by {err}");
    }

    #[test]
    fn rejects_too_short_file() {
        let result = PuffinReader::open(Cursor::new(vec![0u8; 5]));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_bad_trailing_magic() {
        let mut buf = vec![0u8; 20];
        buf[0..4].copy_from_slice(MAGIC);
        // trailing 4 bytes are not magic
        let result = PuffinReader::open(Cursor::new(buf));
        assert!(result.is_err());
    }
}
