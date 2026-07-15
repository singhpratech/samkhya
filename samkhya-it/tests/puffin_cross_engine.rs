#![cfg(feature = "puffin-cross-engine")]

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::sync::Arc;

use datafusion::arrow::array::StringArray;
use datafusion::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::stats::Precision;
use datafusion::datasource::{MemTable, TableProvider};
use datafusion::execution::context::SessionContext;
use iceberg::compression::CompressionCodec as IcebergCompressionCodec;
use iceberg::io::FileIO;
use iceberg::puffin::PuffinWriter as IcebergPuffinWriter;
use iceberg::puffin::{Blob as IcebergBlob, PuffinReader as IcebergPuffinReader};
use iceberg::spec::{
    BlobMetadata as IcebergBlobMetadata, FormatVersion, MAIN_BRANCH, NestedField, Operation,
    PartitionSpec, PrimitiveType, Schema as IcebergSchema, Snapshot, SortOrder, StatisticsFile,
    Summary, TableMetadataBuilder, Type,
};
use iceberg::{TableIdent, table::Table};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{EquiDepthHistogram, HllSketch, Sketch};
use samkhya_datafusion::SamkhyaTableProvider;
use samkhya_duckdb::duckdb::Connection;
use samkhya_duckdb::sidecar::decode_portable_column;
use samkhya_duckdb::sketcher::build_hll_from_query;
use samkhya_iceberg::snapshot::{discover_puffin_sidecars, load_portable_stats_from_table};
use tempfile::TempDir;

const SNAPSHOT_ID: i64 = 4_242;
const STALE_SNAPSHOT_ID: i64 = 4_000;
const SEQUENCE_NUMBER: i64 = 7;
const FIELD_ID: i32 = 17;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[tokio::test(flavor = "multi_thread")]
async fn one_sidecar_round_trips_across_iceberg_datafusion_and_duckdb() -> TestResult {
    let temp = TempDir::new()?;
    let sidecar_path = temp.path().join("current.puffin");
    let stale_path = temp.path().join("missing-stale.puffin");
    let values: Vec<String> = (0..128).map(|value| format!("user-{value:04}")).collect();

    // DuckDB and the engine-neutral producer hash the same UTF-8 bytes.
    let connection = Connection::open_in_memory()?;
    connection.execute("CREATE TABLE users(value VARCHAR NOT NULL)", [])?;
    {
        let mut insert = connection.prepare("INSERT INTO users VALUES (?1)")?;
        for value in &values {
            insert.execute([value.as_str()])?;
        }
    }
    let duckdb_hll =
        build_hll_from_query(&connection, "SELECT value FROM users ORDER BY value", 10)?;
    let mut core_hll = HllSketch::new(10)?;
    for value in &values {
        core_hll.add(value.as_bytes());
    }
    let hll_bytes = core_hll.to_bytes()?;
    assert_eq!(duckdb_hll.to_bytes()?, hll_bytes);

    let histogram_values: Vec<f64> = (0..128).map(|value| value as f64).collect();
    let histogram = EquiDepthHistogram::from_values(&histogram_values, 8)?;
    let histogram_bytes = histogram.to_bytes()?;
    write_core_sidecar(&sidecar_path, &hll_bytes, &histogram_bytes)?;

    // Apache Iceberg's independent reader accepts the core-written file.
    let file_io = FileIO::new_with_fs();
    let official_reader = IcebergPuffinReader::new(
        file_io.new_input(sidecar_path.to_str().expect("UTF-8 temp path"))?,
    );
    let official_metadata = official_reader.file_metadata().await?.clone();
    assert_eq!(official_metadata.blobs().len(), 3);
    assert!(official_metadata.properties()["created-by"].contains("samkhya-core"));
    let official_hll_meta = official_metadata
        .blobs()
        .iter()
        .find(|blob| blob.blob_type() == HllSketch::KIND)
        .expect("official reader found HLL");
    assert_eq!(official_hll_meta.fields(), [FIELD_ID]);
    assert_eq!(official_hll_meta.snapshot_id(), SNAPSHOT_ID);
    assert_eq!(official_hll_meta.sequence_number(), SEQUENCE_NUMBER);
    assert_eq!(
        official_reader.blob(official_hll_meta).await?.data(),
        hll_bytes
    );

    // A real TableMetadata instance drives current-snapshot discovery. The
    // stale path intentionally does not exist, proving it is not loaded.
    let sidecar_bytes = std::fs::read(&sidecar_path)?;
    let table = build_table(
        temp.path().to_str().expect("UTF-8 temp path"),
        sidecar_path.to_str().expect("UTF-8 sidecar path"),
        stale_path.to_str().expect("UTF-8 stale path"),
        sidecar_bytes.len() as i64,
        puffin_footer_size(&sidecar_bytes),
    )?;
    let discovered = discover_puffin_sidecars(&table).await?;
    assert_eq!(discovered.snapshot_id, Some(SNAPSHOT_ID));
    assert_eq!(discovered.paths, vec![sidecar_path.clone()]);

    let portable = load_portable_stats_from_table(&table).await?;
    assert_eq!(portable.snapshot_id(), Some(SNAPSHOT_ID));
    assert_eq!(portable.blobs().len(), 2, "unknown kinds must be skipped");
    assert_eq!(
        portable
            .blobs()
            .iter()
            .find(|blob| blob.kind() == HllSketch::KIND)
            .expect("portable HLL")
            .payload(),
        hll_bytes
    );
    assert_eq!(
        portable
            .blobs()
            .iter()
            .find(|blob| blob.kind() == EquiDepthHistogram::KIND)
            .expect("portable histogram")
            .payload(),
        histogram_bytes
    );

    // Iceberg field ID 17 is explicitly bound to DataFusion ordinal 0.
    let arrow_schema = Arc::new(ArrowSchema::new(vec![Field::new(
        "value",
        DataType::Utf8,
        false,
    )]));
    let batch = RecordBatch::try_new(
        Arc::clone(&arrow_schema),
        vec![Arc::new(StringArray::from(values.clone()))],
    )?;
    let inner = Arc::new(MemTable::try_new(arrow_schema, vec![vec![batch]])?);
    let wrapped =
        Arc::new(SamkhyaTableProvider::new(inner).try_with_portable_stats(&portable, FIELD_ID, 0)?);
    let expected_ndv = core_hll.estimate() as usize;
    let provider_stats = wrapped.statistics().expect("provider stats");
    assert_eq!(
        provider_stats.column_statistics[0].distinct_count,
        Precision::Inexact(expected_ndv)
    );
    let context = SessionContext::new();
    context.register_table("users", wrapped.clone() as Arc<dyn TableProvider>)?;
    let output = context
        .sql("SELECT value FROM users")
        .await?
        .collect()
        .await?;
    assert_eq!(
        output.iter().map(RecordBatch::num_rows).sum::<usize>(),
        values.len()
    );
    let physical = wrapped.scan(&context.state(), None, &[], None).await?;
    assert_eq!(
        physical.statistics()?.column_statistics[0].distinct_count,
        Precision::Inexact(expected_ndv)
    );

    // DuckDB consumes the same decoded payload client-side; query semantics
    // remain unchanged because this API does not claim optimizer injection.
    let rows_before: i64 =
        connection.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
    let duckdb_stats = decode_portable_column(&portable, FIELD_ID)?.expect("DuckDB stats");
    assert_eq!(duckdb_stats.iceberg_field_id(), FIELD_ID);
    assert_eq!(
        duckdb_stats.hll().expect("DuckDB HLL").to_bytes()?,
        hll_bytes
    );
    assert_eq!(
        duckdb_stats.histogram().expect("DuckDB histogram").total(),
        128
    );
    assert_eq!(
        duckdb_stats.column_stats().distinct_count,
        Some(core_hll.estimate())
    );
    let rows_after: i64 =
        connection.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
    assert_eq!(rows_before, rows_after);

    // Reverse compatibility: the Apache Iceberg writer produces a file the
    // synchronous core reader accepts with identical metadata and bytes.
    assert_official_writer_is_core_readable(&file_io, temp.path(), &hll_bytes).await?;
    Ok(())
}

#[tokio::test]
async fn table_loader_rejects_current_snapshot_with_wrong_sequence_number() -> TestResult {
    let temp = TempDir::new()?;
    let sidecar_path = temp.path().join("wrong-sequence.puffin");
    let stale_path = temp.path().join("missing-stale.puffin");
    let hll_bytes = HllSketch::new(10)?.to_bytes()?;
    let histogram_bytes = EquiDepthHistogram::from_values(&[1.0, 2.0], 2)?.to_bytes()?;
    write_core_sidecar_with_sequence(
        &sidecar_path,
        &hll_bytes,
        &histogram_bytes,
        SEQUENCE_NUMBER + 1,
    )?;
    let sidecar_bytes = std::fs::read(&sidecar_path)?;
    let table = build_table(
        temp.path().to_str().expect("UTF-8 temp path"),
        sidecar_path.to_str().expect("UTF-8 sidecar path"),
        stale_path.to_str().expect("UTF-8 stale path"),
        sidecar_bytes.len() as i64,
        puffin_footer_size(&sidecar_bytes),
    )?;

    let error = load_portable_stats_from_table(&table)
        .await
        .expect_err("wrong sequence number must fail closed");
    assert!(error.to_string().contains("sequence-number"));
    Ok(())
}

fn write_core_sidecar(
    path: &std::path::Path,
    hll_bytes: &[u8],
    histogram_bytes: &[u8],
) -> TestResult {
    write_core_sidecar_with_sequence(path, hll_bytes, histogram_bytes, SEQUENCE_NUMBER)
}

fn write_core_sidecar_with_sequence(
    path: &std::path::Path,
    hll_bytes: &[u8],
    histogram_bytes: &[u8],
    sequence_number: i64,
) -> TestResult {
    let mut writer =
        PuffinWriter::new(File::create(path)?).with_file_property("samkhya.schema-version", "1");
    writer.add_blob_for_snapshot(
        Blob::new(HllSketch::KIND, vec![FIELD_ID], hll_bytes),
        SNAPSHOT_ID,
        sequence_number,
    )?;
    writer.add_blob_for_snapshot(
        Blob::new(EquiDepthHistogram::KIND, vec![FIELD_ID], histogram_bytes),
        SNAPSHOT_ID,
        sequence_number,
    )?;
    writer.add_blob_for_snapshot(
        Blob::new("vendor.future-statistics-v1", vec![FIELD_ID], b"opaque"),
        SNAPSHOT_ID,
        sequence_number,
    )?;
    writer.finish()?.sync_all()?;
    Ok(())
}

fn build_table(
    table_location: &str,
    current_path: &str,
    stale_path: &str,
    file_size: i64,
    footer_size: i64,
) -> TestResult<Table> {
    let fields = (1..=FIELD_ID).map(|field_id| {
        let name = if field_id == FIELD_ID {
            "value".to_owned()
        } else {
            format!("padding_{field_id}")
        };
        NestedField::required(field_id, name, Type::Primitive(PrimitiveType::String)).into()
    });
    let schema = IcebergSchema::builder().with_fields(fields).build()?;
    let summary = || Summary {
        operation: Operation::Append,
        additional_properties: HashMap::new(),
    };
    let stale_snapshot = Snapshot::builder()
        .with_snapshot_id(STALE_SNAPSHOT_ID)
        .with_sequence_number(SEQUENCE_NUMBER - 1)
        .with_timestamp_ms(1_700_000_000_000)
        .with_manifest_list("/unused-stale-manifest.avro")
        .with_schema_id(0)
        .with_summary(summary())
        .build();
    let current_snapshot = Snapshot::builder()
        .with_snapshot_id(SNAPSHOT_ID)
        .with_parent_snapshot_id(Some(STALE_SNAPSHOT_ID))
        .with_sequence_number(SEQUENCE_NUMBER)
        .with_timestamp_ms(1_700_000_000_001)
        .with_manifest_list("/unused-current-manifest.avro")
        .with_schema_id(0)
        .with_summary(summary())
        .build();
    let blob_metadata = vec![
        iceberg_blob_metadata(HllSketch::KIND),
        iceberg_blob_metadata(EquiDepthHistogram::KIND),
        iceberg_blob_metadata("vendor.future-statistics-v1"),
    ];
    let current_statistics = StatisticsFile {
        snapshot_id: SNAPSHOT_ID,
        statistics_path: current_path.to_owned(),
        file_size_in_bytes: file_size,
        file_footer_size_in_bytes: footer_size,
        key_metadata: None,
        blob_metadata,
    };
    let stale_statistics = StatisticsFile {
        snapshot_id: STALE_SNAPSHOT_ID,
        statistics_path: stale_path.to_owned(),
        file_size_in_bytes: 0,
        file_footer_size_in_bytes: 0,
        key_metadata: None,
        blob_metadata: Vec::new(),
    };
    let metadata = TableMetadataBuilder::new(
        schema,
        PartitionSpec::unpartition_spec().into_unbound(),
        SortOrder::unsorted_order(),
        table_location.to_owned(),
        FormatVersion::V2,
        HashMap::new(),
    )?
    .set_branch_snapshot(stale_snapshot, MAIN_BRANCH)?
    .set_branch_snapshot(current_snapshot, MAIN_BRANCH)?
    .set_statistics(stale_statistics)
    .set_statistics(current_statistics)
    .build()?
    .metadata;

    Ok(Table::builder()
        .metadata(metadata)
        .identifier(TableIdent::from_strs(["test", "puffin_cross_engine"])?)
        .file_io(FileIO::new_with_fs())
        .build()?)
}

fn iceberg_blob_metadata(kind: &str) -> IcebergBlobMetadata {
    IcebergBlobMetadata {
        r#type: kind.to_owned(),
        snapshot_id: SNAPSHOT_ID,
        sequence_number: SEQUENCE_NUMBER,
        fields: vec![FIELD_ID],
        properties: HashMap::new(),
    }
}

fn puffin_footer_size(bytes: &[u8]) -> i64 {
    let payload_length = u32::from_le_bytes(
        bytes[bytes.len() - 12..bytes.len() - 8]
            .try_into()
            .expect("four-byte footer length"),
    );
    i64::from(payload_length) + 16
}

async fn assert_official_writer_is_core_readable(
    file_io: &FileIO,
    directory: &std::path::Path,
    hll_bytes: &[u8],
) -> TestResult {
    let path = directory.join("official.puffin");
    let output = file_io.new_output(path.to_str().expect("UTF-8 official path"))?;
    let mut writer = IcebergPuffinWriter::new(
        &output,
        HashMap::from([("created-by".to_owned(), "iceberg-rust 0.9.1".to_owned())]),
        false,
    )
    .await?;
    writer
        .add(
            IcebergBlob::builder()
                .r#type(HllSketch::KIND.to_owned())
                .fields(vec![FIELD_ID])
                .snapshot_id(SNAPSHOT_ID)
                .sequence_number(SEQUENCE_NUMBER)
                .data(hll_bytes.to_vec())
                .properties(HashMap::new())
                .build(),
            IcebergCompressionCodec::None,
        )
        .await?;
    writer.close().await?;

    let mut core_reader = PuffinReader::open(File::open(path)?)?;
    assert_eq!(core_reader.blobs().len(), 1);
    assert_eq!(core_reader.blobs()[0].snapshot_id, Some(SNAPSHOT_ID));
    assert_eq!(
        core_reader.blobs()[0].sequence_number,
        Some(SEQUENCE_NUMBER)
    );
    assert_eq!(core_reader.read_blob_decompressed(0)?, hll_bytes);
    Ok(())
}
