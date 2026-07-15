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

/// Puffin file property declaring the samkhya portability contract version.
pub const SAMKHYA_SCHEMA_VERSION_PROPERTY: &str = "samkhya.schema-version";

/// Current samkhya portability contract version.
pub const SAMKHYA_SCHEMA_VERSION: &str = "1";

/// Validate an optional samkhya file-schema marker.
///
/// Absence is accepted for v1 files written before the marker was introduced.
/// An explicit future or malformed value fails closed so readers do not
/// silently reinterpret a changed cross-engine contract.
pub fn validate_samkhya_schema_version(version: Option<&str>) -> Result<()> {
    match version {
        None | Some(SAMKHYA_SCHEMA_VERSION) => Ok(()),
        Some(other) => Err(Error::InvalidPuffin(format!(
            "unsupported {} {other:?}; expected {SAMKHYA_SCHEMA_VERSION}",
            SAMKHYA_SCHEMA_VERSION_PROPERTY
        ))),
    }
}

/// Maximum permitted size for a single blob payload, in bytes.
///
/// Caps untrusted `length` and decompressed-size values read from the
/// JSON footer of a Puffin sidecar. A Bloom filter sized for 1B keys at
/// 1% FPR fits comfortably under this limit (~1.2 GiB); legitimate
/// HLL/CMS/histogram payloads are sub-MiB. Attacker-controlled sidecars
/// declaring larger sizes are rejected before allocation, eliminating
/// the trivial DoS surface where a small file claims a multi-EiB blob.
const MAX_BLOB_LEN: u64 = 2 * 1024 * 1024 * 1024;

/// Maximum permitted size for the JSON footer payload, in bytes.
///
/// The footer is a `serde_json` blob enumerating every per-blob metadata
/// record. 16 MiB accommodates pathologically large numbers of blobs
/// while still letting the reader reject attacker-controlled `payload_len`
/// values that would force a multi-GiB allocation for the footer alone.
const MAX_FOOTER_LEN: u64 = 16 * 1024 * 1024;

/// Maximum number of blob entries enumerated in a single footer.
///
/// Caps the `blobs` array length after JSON parse so an attacker cannot
/// stage a footer with millions of trivially-sized blob records that
/// each individually pass the per-blob length cap. Legitimate samkhya
/// sidecars carry one blob per stat per column — even a wide table with
/// one HLL + one Bloom + one CMS + two histograms per column tops out
/// well under this limit.
const MAX_BLOB_COUNT: usize = 65_536;

/// Compression codec applied to a blob payload before it is written to the file.
///
/// The codec name is recorded in the blob's `compression-codec` metadata so
/// readers can decompress lazily without scanning unrelated blobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionCodec {
    /// Payload is stored verbatim.
    None,
    /// Payload is compressed with zstd (default level).
    Zstd,
}

impl CompressionCodec {
    /// Codec name as serialized in blob metadata (Puffin spec convention).
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::puffin::CompressionCodec;
    ///
    /// assert_eq!(CompressionCodec::None.as_str(), "none");
    /// assert_eq!(CompressionCodec::Zstd.as_str(), "zstd");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            CompressionCodec::None => "none",
            CompressionCodec::Zstd => "zstd",
        }
    }

    fn from_meta(meta: Option<&str>) -> Result<Self> {
        match meta {
            None | Some("none") => Ok(CompressionCodec::None),
            Some("zstd") => Ok(CompressionCodec::Zstd),
            Some(other) => Err(Error::InvalidPuffin(format!(
                "unsupported blob compression codec {other:?}"
            ))),
        }
    }
}

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
///
/// # Examples
///
/// ```
/// use samkhya_core::puffin::Blob;
///
/// let payload = b"sketch bytes";
/// let blob = Blob::new("samkhya.hll-v1", vec![7], payload);
/// assert_eq!(blob.kind, "samkhya.hll-v1");
/// assert_eq!(blob.fields, vec![7]);
/// assert_eq!(blob.payload, payload);
/// ```
pub struct Blob<'a> {
    pub kind: String,
    pub fields: Vec<i32>,
    pub payload: &'a [u8],
    pub properties: BTreeMap<String, String>,
}

impl<'a> Blob<'a> {
    /// Build a blob with empty properties. Add metadata afterwards by
    /// inserting into `blob.properties`.
    ///
    /// # Examples
    ///
    /// ```
    /// use samkhya_core::puffin::Blob;
    ///
    /// let blob = Blob::new("samkhya.bloom-v1", vec![1, 2], &[0u8, 1, 2, 3]);
    /// assert!(blob.properties.is_empty());
    /// ```
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
///
/// Wraps any `Write + Seek` sink. The magic header is written lazily on the
/// first blob (or finish); call [`PuffinWriter::finish`] to flush the footer
/// and recover the inner writer.
///
/// # Examples
///
/// ```
/// use std::io::Cursor;
/// use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
///
/// let mut writer = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
/// writer.add_blob(Blob::new("samkhya.test-v1", vec![0], b"hello")).unwrap();
/// let cursor = writer.finish().unwrap();
///
/// // Round-trip: open the bytes back as a reader.
/// let reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
/// assert_eq!(reader.blobs().len(), 1);
/// ```
pub struct PuffinWriter<W: Write + Seek> {
    inner: W,
    blobs: Vec<BlobMetadata>,
    properties: BTreeMap<String, String>,
    pos: u64,
    wrote_head: bool,
}

impl<W: Write + Seek> PuffinWriter<W> {
    pub fn new(inner: W) -> Self {
        let mut properties = BTreeMap::new();
        properties.insert(
            "created-by".to_owned(),
            format!("samkhya-core version {}", env!("CARGO_PKG_VERSION")),
        );
        properties.insert(
            SAMKHYA_SCHEMA_VERSION_PROPERTY.to_owned(),
            SAMKHYA_SCHEMA_VERSION.to_owned(),
        );
        Self {
            inner,
            blobs: Vec::new(),
            properties,
            pos: 0,
            wrote_head: false,
        }
    }

    /// Set a file-level Puffin footer property.
    pub fn with_file_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
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
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    /// use samkhya_core::puffin::{Blob, PuffinWriter};
    ///
    /// let mut writer = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
    /// writer.add_blob(Blob::new("samkhya.test-v1", vec![0], b"payload")).unwrap();
    /// let _bytes = writer.finish().unwrap().into_inner();
    /// ```
    pub fn add_blob(&mut self, blob: Blob<'_>) -> Result<()> {
        // Puffin v1 requires both fields even when the Iceberg snapshot is not
        // known yet; -1 is the specification's unassigned sentinel.
        self.add_blob_for_snapshot(blob, -1, -1)
    }

    /// Append an uncompressed blob with required Iceberg snapshot metadata.
    pub fn add_blob_for_snapshot(
        &mut self,
        blob: Blob<'_>,
        snapshot_id: i64,
        sequence_number: i64,
    ) -> Result<()> {
        self.ensure_head()?;
        let offset = self.pos;
        self.inner.write_all(blob.payload)?;
        let length = blob.payload.len() as u64;
        self.pos += length;
        self.blobs.push(BlobMetadata {
            kind: blob.kind,
            fields: blob.fields,
            snapshot_id: Some(snapshot_id),
            sequence_number: Some(sequence_number),
            offset,
            length,
            compression_codec: None,
            properties: blob.properties,
        });
        Ok(())
    }

    /// Append a blob with payload compressed under `codec`.
    ///
    /// `CompressionCodec::None` is equivalent to [`Self::add_blob`].
    /// `CompressionCodec::Zstd` requires the `zstd` feature; otherwise this
    /// returns an [`Error::InvalidPuffin`] explaining the missing feature.
    pub fn add_blob_compressed(&mut self, blob: Blob<'_>, codec: CompressionCodec) -> Result<()> {
        self.add_blob_compressed_for_snapshot(blob, codec, -1, -1)
    }

    /// Append a blob with compression and required snapshot metadata.
    pub fn add_blob_compressed_for_snapshot(
        &mut self,
        blob: Blob<'_>,
        codec: CompressionCodec,
        snapshot_id: i64,
        sequence_number: i64,
    ) -> Result<()> {
        match codec {
            CompressionCodec::None => {
                self.add_blob_for_snapshot(blob, snapshot_id, sequence_number)
            }
            CompressionCodec::Zstd => self.add_blob_zstd(blob, snapshot_id, sequence_number),
        }
    }

    #[cfg(feature = "zstd")]
    fn add_blob_zstd(
        &mut self,
        blob: Blob<'_>,
        snapshot_id: i64,
        sequence_number: i64,
    ) -> Result<()> {
        self.ensure_head()?;
        let compressed = zstd::encode_all(blob.payload, 0)
            .map_err(|e| Error::InvalidPuffin(format!("zstd encode: {e}")))?;
        let offset = self.pos;
        self.inner.write_all(&compressed)?;
        let length = compressed.len() as u64;
        self.pos += length;
        self.blobs.push(BlobMetadata {
            kind: blob.kind,
            fields: blob.fields,
            snapshot_id: Some(snapshot_id),
            sequence_number: Some(sequence_number),
            offset,
            length,
            compression_codec: Some(CompressionCodec::Zstd.as_str().to_string()),
            properties: blob.properties,
        });
        Ok(())
    }

    #[cfg(not(feature = "zstd"))]
    fn add_blob_zstd(
        &mut self,
        _blob: Blob<'_>,
        _snapshot_id: i64,
        _sequence_number: i64,
    ) -> Result<()> {
        Err(Error::InvalidPuffin(
            "zstd codec requested but the `zstd` cargo feature is disabled".into(),
        ))
    }

    /// Finalize the file: write the footer and return the inner writer.
    pub fn finish(mut self) -> Result<W> {
        self.ensure_head()?;
        let footer = FooterPayload {
            blobs: self.blobs,
            properties: self.properties,
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
///
/// # Examples
///
/// ```
/// use std::io::Cursor;
/// use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
///
/// // Write a file in memory, then read it back.
/// let mut w = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
/// w.add_blob(Blob::new("samkhya.hll-v1", vec![1], b"sketch bytes")).unwrap();
/// let bytes = w.finish().unwrap().into_inner();
///
/// let mut reader = PuffinReader::open(Cursor::new(bytes)).unwrap();
/// let (idx, meta) = reader.find_blob("samkhya.hll-v1").unwrap();
/// assert_eq!(meta.kind, "samkhya.hll-v1");
/// assert_eq!(reader.read_blob(idx).unwrap(), b"sketch bytes");
/// ```
pub struct PuffinReader<R: Read + Seek> {
    inner: R,
    footer: FooterPayload,
    blob_data_end: u64,
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

        // A compressed footer (bit 0) and all reserved flags are unsupported
        // by this synchronous reader. Reject explicitly instead of trying to
        // parse compressed bytes as JSON or silently accepting future flags.
        inner.seek(SeekFrom::End(-8))?;
        let flags = inner.read_u32::<LittleEndian>()?;
        if flags != 0 {
            let reason = if flags & 1 == 1 {
                "compressed Puffin footers are not supported"
            } else {
                "Puffin footer contains unsupported reserved flags"
            };
            return Err(Error::InvalidPuffin(format!("{reason}: 0x{flags:08x}")));
        }

        // Payload length (4 bytes before flags)
        inner.seek(SeekFrom::End(-12))?;
        let payload_len = inner.read_u32::<LittleEndian>()? as u64;

        if payload_len > MAX_FOOTER_LEN {
            return Err(Error::InvalidPuffin(format!(
                "footer payload length {payload_len} exceeds MAX_FOOTER_LEN {MAX_FOOTER_LEN}"
            )));
        }

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

        if footer.blobs.len() > MAX_BLOB_COUNT {
            return Err(Error::InvalidPuffin(format!(
                "footer enumerates {} blobs; exceeds MAX_BLOB_COUNT {MAX_BLOB_COUNT}",
                footer.blobs.len()
            )));
        }

        Ok(Self {
            inner,
            footer,
            blob_data_end: payload_start - 4,
        })
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
        if meta.length > MAX_BLOB_LEN {
            return Err(Error::InvalidPuffin(format!(
                "blob {idx} length {} exceeds MAX_BLOB_LEN {MAX_BLOB_LEN}",
                meta.length
            )));
        }
        let blob_end = meta
            .offset
            .checked_add(meta.length)
            .ok_or_else(|| Error::InvalidPuffin(format!("blob {idx} offset overflow")))?;
        if meta.offset < MAGIC.len() as u64 || blob_end > self.blob_data_end {
            return Err(Error::InvalidPuffin(format!(
                "blob {idx} range {}..{blob_end} falls outside payload region {}..{}",
                meta.offset,
                MAGIC.len(),
                self.blob_data_end
            )));
        }
        self.inner.seek(SeekFrom::Start(meta.offset))?;
        let mut buf = vec![0u8; meta.length as usize];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Read a blob's payload by index and decompress it according to the
    /// blob's `compression_codec` metadata.
    ///
    /// Falls back to the raw bytes when the codec is `None` / unset. Returns
    /// an error if the recorded codec is unsupported in the current build
    /// (e.g. `zstd` without the `zstd` feature).
    pub fn read_blob_decompressed(&mut self, idx: usize) -> Result<Vec<u8>> {
        let codec = {
            let meta =
                self.footer.blobs.get(idx).ok_or_else(|| {
                    Error::InvalidPuffin(format!("blob index {idx} out of range"))
                })?;
            CompressionCodec::from_meta(meta.compression_codec.as_deref())?
        };
        let raw = self.read_blob(idx)?;
        match codec {
            CompressionCodec::None => Ok(raw),
            CompressionCodec::Zstd => decode_zstd(&raw),
        }
    }

    /// Find the first blob whose `type` (kind) matches `kind`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    /// use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
    ///
    /// let mut w = PuffinWriter::new(Cursor::new(Vec::<u8>::new()));
    /// w.add_blob(Blob::new("samkhya.hll-v1", vec![0], b"x")).unwrap();
    /// let bytes = w.finish().unwrap().into_inner();
    /// let reader = PuffinReader::open(Cursor::new(bytes)).unwrap();
    /// assert!(reader.find_blob("samkhya.hll-v1").is_some());
    /// assert!(reader.find_blob("absent.kind").is_none());
    /// ```
    pub fn find_blob(&self, kind: &str) -> Option<(usize, &BlobMetadata)> {
        self.footer
            .blobs
            .iter()
            .enumerate()
            .find(|(_, b)| b.kind == kind)
    }
}

/// Bounded streaming decompression helper shared across compression
/// codecs.
///
/// Reads from `reader` into a fresh `Vec<u8>` whose final length is at
/// most `max_bytes`. The `+1` sentinel byte distinguishes "decompressed
/// exactly max_bytes" (legitimate boundary) from "would have produced
/// more than max_bytes" (attacker-controlled). `codec_label` appears in
/// the error message so the caller can identify which codec tripped.
///
/// Centralising this here means a future LZ4 / Brotli / Snappy codec
/// only needs to construct a `Read` and call this function; the cap is
/// not re-derived per codec. See `documents/SECURITY-REVIEW-2026-05-17.md`
/// item M6. Currently used only by the zstd path; broaden the cfg gate
/// when a new codec lands.
#[cfg(feature = "zstd")]
fn decompress_bounded<R: std::io::Read>(
    mut reader: R,
    max_bytes: usize,
    codec_label: &'static str,
) -> Result<Vec<u8>> {
    use std::io::Read as _;

    let mut out = Vec::with_capacity(max_bytes.min(256 * 1024));
    let n = (&mut reader)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| Error::InvalidPuffin(format!("{codec_label} decode: {e}")))?;
    let _ = n; // silence unused-binding when read_to_end's return is not used
    if out.len() > max_bytes {
        return Err(Error::InvalidPuffin(format!(
            "{codec_label} decompressed size exceeds {max_bytes}"
        )));
    }
    Ok(out)
}

#[cfg(feature = "zstd")]
fn decode_zstd(raw: &[u8]) -> Result<Vec<u8>> {
    // Stream into a bounded buffer so a high-ratio compression bomb
    // cannot force a multi-GiB allocation. The cap and the sentinel
    // logic live in `decompress_bounded`.
    let decoder = zstd::stream::Decoder::new(raw)
        .map_err(|e| Error::InvalidPuffin(format!("zstd decoder init: {e}")))?;
    decompress_bounded(decoder, MAX_BLOB_LEN as usize, "zstd")
}

#[cfg(not(feature = "zstd"))]
fn decode_zstd(_raw: &[u8]) -> Result<Vec<u8>> {
    Err(Error::InvalidPuffin(
        "blob is zstd-compressed but the `zstd` cargo feature is disabled".into(),
    ))
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
        assert_eq!(reader.blobs()[0].snapshot_id, Some(-1));
        assert_eq!(reader.blobs()[0].sequence_number, Some(-1));
        assert!(reader.footer().properties["created-by"].starts_with("samkhya-core version "));
        assert_eq!(reader.read_blob(0).unwrap(), b"hello puffin");
    }

    #[test]
    fn snapshot_aware_blob_metadata_round_trips() {
        let mut writer =
            PuffinWriter::new(Cursor::new(Vec::new())).with_file_property("deployment", "golden");
        writer
            .add_blob_for_snapshot(Blob::new("samkhya.test-v1", vec![17], b"payload"), 4_242, 7)
            .unwrap();
        let cursor = writer.finish().unwrap();

        let reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        assert_eq!(reader.blobs()[0].snapshot_id, Some(4_242));
        assert_eq!(reader.blobs()[0].sequence_number, Some(7));
        assert_eq!(reader.footer().properties["deployment"], "golden");
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

    #[test]
    fn read_blob_decompressed_no_codec_is_passthrough() {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob(Blob::new("samkhya.test-v1", vec![0], b"plain payload"))
            .unwrap();
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        assert_eq!(reader.read_blob_decompressed(0).unwrap(), b"plain payload");
    }

    #[test]
    fn read_blob_decompressed_rejects_unknown_codec() {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob(Blob::new("samkhya.test-v1", vec![0], b"plain payload"))
            .unwrap();
        let cursor = writer.finish().unwrap();
        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        reader.footer.blobs[0].compression_codec = Some("gzip".to_owned());

        let error = reader.read_blob_decompressed(0).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported blob compression codec")
        );
    }

    #[test]
    fn open_rejects_unsupported_footer_flags() {
        let writer = PuffinWriter::new(Cursor::new(Vec::new()));
        let mut bytes = writer.finish().unwrap().into_inner();
        let flags_offset = bytes.len() - 8;
        bytes[flags_offset..flags_offset + 4].copy_from_slice(&2_u32.to_le_bytes());

        let error = PuffinReader::open(Cursor::new(bytes)).err().unwrap();
        assert!(error.to_string().contains("unsupported reserved flags"));
    }

    #[test]
    fn read_blob_rejects_oversized_length() {
        // Build a minimal valid Puffin file then tamper with the JSON
        // footer to claim a blob length of MAX_BLOB_LEN + 1. The reader
        // must reject the read_blob call before allocating.
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob(Blob::new("samkhya.test-v1", vec![0], b"x"))
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        // Locate the JSON footer payload (between footer-head magic and
        // the payload-len trailer). The header is the first 4 bytes; the
        // last 12 bytes are payload-len(4) + flags(4) + trailing-magic(4).
        let file_len = bytes.len();
        let payload_len =
            u32::from_le_bytes(bytes[file_len - 12..file_len - 8].try_into().unwrap()) as usize;
        let payload_start = file_len - 12 - payload_len;
        let payload_end = file_len - 12;
        let json = std::str::from_utf8(&bytes[payload_start..payload_end])
            .unwrap()
            .to_string();
        // Replace the legitimate length:1 with an oversized value. The
        // payload was `b"x"` so the field reads `"length":1`.
        let bogus_len = MAX_BLOB_LEN + 1;
        let tampered_json = json.replacen("\"length\":1", &format!("\"length\":{bogus_len}"), 1);
        assert_ne!(tampered_json, json, "tamper failed — payload schema drift?");

        // Rebuild the file with the tampered footer. Adjust payload_len
        // to match the new JSON byte length.
        let mut tampered = Vec::with_capacity(file_len + 32);
        tampered.extend_from_slice(&bytes[..payload_start]);
        tampered.extend_from_slice(tampered_json.as_bytes());
        let new_payload_len = tampered_json.len() as u32;
        tampered.write_u32::<LittleEndian>(new_payload_len).unwrap();
        tampered.write_u32::<LittleEndian>(0).unwrap();
        tampered.extend_from_slice(MAGIC);

        let mut reader = PuffinReader::open(Cursor::new(tampered)).unwrap();
        let err = reader.read_blob(0).unwrap_err();
        match err {
            Error::InvalidPuffin(msg) => assert!(
                msg.contains("MAX_BLOB_LEN"),
                "expected MAX_BLOB_LEN rejection, got: {msg}"
            ),
            other => panic!("expected InvalidPuffin, got {other:?}"),
        }
    }

    #[test]
    fn open_rejects_oversized_footer_payload_len() {
        // Build a minimal file then rewrite its payload-len field to a
        // value that exceeds MAX_FOOTER_LEN. Reader must reject before
        // allocating a footer-sized buffer.
        let writer = PuffinWriter::new(Cursor::new(Vec::new()));
        let bytes = writer.finish().unwrap().into_inner();
        let mut tampered = bytes.clone();
        let oversized = (MAX_FOOTER_LEN as u32).saturating_add(1);
        let len_offset = tampered.len() - 12;
        tampered[len_offset..len_offset + 4].copy_from_slice(&oversized.to_le_bytes());
        match PuffinReader::open(Cursor::new(tampered)) {
            Ok(_) => panic!("expected MAX_FOOTER_LEN rejection, got Ok"),
            Err(Error::InvalidPuffin(msg)) => assert!(
                msg.contains("MAX_FOOTER_LEN"),
                "expected MAX_FOOTER_LEN rejection, got: {msg}"
            ),
            Err(other) => panic!("expected InvalidPuffin, got {other:?}"),
        }
    }

    #[cfg(not(feature = "zstd"))]
    #[test]
    fn requesting_zstd_without_feature_errors() {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        let err = writer
            .add_blob_compressed(
                Blob::new("samkhya.test-v1", vec![0], b"x"),
                CompressionCodec::Zstd,
            )
            .unwrap_err();
        assert!(matches!(err, Error::InvalidPuffin(_)));
    }
}

#[cfg(all(test, feature = "zstd"))]
mod zstd_tests {
    use std::io::Cursor;

    use super::*;
    use crate::sketches::{HllSketch, Sketch};

    #[test]
    fn round_trip_compressed_blob() {
        // Payload with enough redundancy that zstd visibly shrinks it.
        let payload = vec![0xABu8; 8192];

        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob_compressed(
                Blob::new("samkhya.test-v1", vec![0], &payload),
                CompressionCodec::Zstd,
            )
            .unwrap();
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        let meta = &reader.blobs()[0];
        assert_eq!(meta.compression_codec.as_deref(), Some("zstd"));
        // Compressed length should be strictly smaller than the original.
        assert!((meta.length as usize) < payload.len());

        let decoded = reader.read_blob_decompressed(0).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn round_trip_compressed_hll_sketch() {
        let mut hll = HllSketch::new(14).unwrap();
        for i in 0..5_000u32 {
            hll.add(&i.to_le_bytes());
        }
        let bytes = hll.to_bytes().unwrap();

        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob_compressed(
                Blob::new(HllSketch::KIND, vec![1], &bytes),
                CompressionCodec::Zstd,
            )
            .unwrap();
        let cursor = writer.finish().unwrap();

        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        let (idx, meta) = reader.find_blob(HllSketch::KIND).unwrap();
        assert_eq!(meta.compression_codec.as_deref(), Some("zstd"));
        let decoded = reader.read_blob_decompressed(idx).unwrap();
        let hll2 = HllSketch::from_bytes(&decoded).unwrap();
        let err = (hll2.estimate() as f64 - 5_000.0).abs() / 5_000.0;
        assert!(err < 0.05, "HLL estimate off by {err}");
    }

    #[test]
    fn none_codec_via_compressed_api_matches_plain() {
        let mut writer = PuffinWriter::new(Cursor::new(Vec::new()));
        writer
            .add_blob_compressed(
                Blob::new("samkhya.test-v1", vec![0], b"identity"),
                CompressionCodec::None,
            )
            .unwrap();
        let cursor = writer.finish().unwrap();
        let mut reader = PuffinReader::open(Cursor::new(cursor.into_inner())).unwrap();
        assert!(reader.blobs()[0].compression_codec.is_none());
        assert_eq!(reader.read_blob_decompressed(0).unwrap(), b"identity");
    }
}
