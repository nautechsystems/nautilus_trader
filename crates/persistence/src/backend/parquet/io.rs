// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

#![expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "Parquet I/O functions forward Arrow/object-store errors and use validated schema paths"
)]

use std::{collections::HashMap, sync::Arc};

use ahash::AHashMap;
use anyhow::Context;
use arrow::{datatypes::Schema, record_batch::RecordBatch};
use nautilus_serialization::arrow::{KEY_IDENTIFIER, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION};
use object_store::{
    ObjectStore, ObjectStoreExt, PutMode, PutOptions, buffered::BufReader, path::Path as ObjectPath,
};
use parquet::{
    arrow::{
        ArrowSchemaConverter, ArrowWriter, ParquetRecordBatchStreamBuilder,
        arrow_reader::ParquetRecordBatchReaderBuilder,
    },
    basic::{Compression, ZstdLevel},
    file::{
        metadata::{KeyValue, SortingColumn},
        properties::WriterProperties,
        reader::{FileReader, SerializedFileReader},
        statistics::Statistics,
    },
    schema::types::ColumnPath,
};
use url::Url;

pub(crate) use crate::common::paths::file_uri_to_native_path;
pub use crate::common::paths::normalize_path_to_uri;

pub(crate) struct ObjectStoreLocation {
    pub object_store: Arc<dyn ObjectStore>,
    pub base_path: String,
    pub original_uri: String,
    store_root_url: Option<Url>,
}

impl ObjectStoreLocation {
    pub(crate) fn store_root_url(&self) -> Option<&Url> {
        self.store_root_url.as_ref()
    }
}

/// Writes a `RecordBatch` to a Parquet file using object store, with optional compression.
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batch_to_parquet(
    batch: RecordBatch,
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    write_batches_to_parquet(
        &[batch],
        path,
        storage_options,
        compression,
        max_row_group_size,
    )
    .await
}

/// Writes multiple `RecordBatch` items to a Parquet file using object store, with optional compression, row group sizing, and storage options.
///
/// # Errors
///
/// Returns an error if `batches` is empty, writing to Parquet fails, or any I/O operation fails.
pub async fn write_batches_to_parquet(
    batches: &[RecordBatch],
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    let (object_store, base_path, _) = create_object_store_from_path(path, storage_options)?;

    write_batches_to_object_store(
        batches,
        object_store,
        &object_path_under_base(&base_path, path),
        compression,
        max_row_group_size,
        None,
    )
    .await
}

/// Reads only the Arrow schema (including key/value metadata) of a Parquet object.
///
/// Avoids decoding any record batches; use when only schema metadata is needed.
///
/// # Errors
///
/// Returns an error if the object cannot be fetched or its footer cannot be parsed.
pub async fn read_parquet_schema_from_object_store(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<Arc<Schema>> {
    let object = object_store.head(path).await?;
    if object.size == 0 {
        return Ok(Arc::new(Schema::empty()));
    }

    let reader = BufReader::new(object_store, &object);
    let builder = ParquetRecordBatchStreamBuilder::new(reader).await?;
    Ok(builder.schema().clone())
}

/// Reads a Parquet file from an object store and returns all record batches plus
/// the Arrow schema from the builder. The builder's schema includes metadata restored
/// from the file's `ARROW:schema` `key_value_metadata`; use it for decoding instead of
/// each batch's schema (which has metadata stripped).
///
/// # Errors
///
/// Returns an error if the path cannot be read or Parquet parsing fails.
pub async fn read_parquet_from_object_store(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<(Vec<RecordBatch>, Arc<Schema>)> {
    let data = object_store.get(path).await?.bytes().await?;
    if data.is_empty() {
        return Ok((Vec::new(), Arc::new(Schema::empty())));
    }

    let builder = ParquetRecordBatchReaderBuilder::try_new(data)?;
    let schema = builder.schema().clone();
    let batches = builder.build()?.collect::<Result<Vec<_>, _>>()?;

    Ok((batches, schema))
}

/// Writes multiple `RecordBatch` items to an object store URI, with optional compression,
/// row group sizing, and `key_value_metadata` (e.g. for instrument `type_name` so it survives roundtrip).
///
/// # Errors
///
/// Returns an error if `batches` is empty, writing to Parquet fails, or any I/O operation fails.
pub async fn write_batches_to_object_store(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
    key_value_metadata: Option<Vec<KeyValue>>,
) -> anyhow::Result<()> {
    write_batches_to_object_store_with_mode(
        batches,
        object_store,
        path,
        compression,
        max_row_group_size,
        key_value_metadata,
        PutMode::Overwrite,
    )
    .await
}

pub(crate) async fn write_batches_to_object_store_create(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
    key_value_metadata: Option<Vec<KeyValue>>,
) -> anyhow::Result<()> {
    write_batches_to_object_store_with_mode(
        batches,
        object_store,
        path,
        compression,
        max_row_group_size,
        key_value_metadata,
        PutMode::Create,
    )
    .await
}

async fn write_batches_to_object_store_with_mode(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
    key_value_metadata: Option<Vec<KeyValue>>,
    put_mode: PutMode,
) -> anyhow::Result<()> {
    let Some(first) = batches.first() else {
        anyhow::bail!("Cannot write Parquet file {path} with no record batches");
    };

    // Create a temporary buffer to write the parquet data
    let mut buffer = Vec::new();

    let schema = first.schema();
    let sorting_columns = parquet_sorting_columns(schema.as_ref())?;
    let mut props_builder = WriterProperties::builder()
        .set_compression(compression.unwrap_or(Compression::ZSTD(ZstdLevel::default())))
        .set_max_row_group_row_count(Some(
            max_row_group_size.unwrap_or(super::DEFAULT_ROW_GROUP_SIZE),
        ))
        .set_sorting_columns(sorting_columns)
        .set_key_value_metadata(key_value_metadata);

    if schema.index_of(KEY_IDENTIFIER).is_ok() {
        props_builder =
            props_builder.set_column_bloom_filter_enabled(ColumnPath::from(KEY_IDENTIFIER), true);
    }

    let writer_props = props_builder.build();

    let mut writer = ArrowWriter::try_new(&mut buffer, schema, Some(writer_props))?;
    for batch in batches {
        writer.write(batch)?;
    }

    writer.close()?;

    // Upload the buffer to object store
    object_store
        .put_opts(
            path,
            buffer.into(),
            PutOptions {
                mode: put_mode,
                ..Default::default()
            },
        )
        .await?;

    Ok(())
}

fn parquet_sorting_columns(schema: &Schema) -> anyhow::Result<Option<Vec<SortingColumn>>> {
    if schema.index_of("ts_init").is_err() {
        return Ok(None);
    }

    let parquet_schema = ArrowSchemaConverter::new().convert(schema)?;
    let mut names = Vec::with_capacity(2);
    names.push("ts_init");
    if schema.index_of(KEY_IDENTIFIER).is_ok() {
        names.push(KEY_IDENTIFIER);
    }

    let columns = names
        .into_iter()
        .map(|name| {
            let index = parquet_schema
                .columns()
                .iter()
                .position(|column| {
                    column
                        .path()
                        .parts()
                        .first()
                        .is_some_and(|part| part == name)
                })
                .ok_or_else(|| anyhow::anyhow!("Parquet schema is missing sort column {name}"))?;

            Ok(SortingColumn {
                column_idx: i32::try_from(index)?,
                descending: false,
                nulls_first: false,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(Some(columns))
}

/// Deduplicates a slice of `RecordBatch` items, removing rows that are identical across all columns.
///
/// Rows are compared by encoding each row to a canonical byte sequence using Arrow's row format.
/// Only the first occurrence of each unique row is retained; the relative order of unique rows
/// is preserved.
///
/// # Errors
///
/// Returns an error if the row converter cannot be constructed or if the `take` kernel fails.
fn deduplicate_record_batches(batches: &[RecordBatch]) -> anyhow::Result<Vec<RecordBatch>> {
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    let schema = batches[0].schema();

    let fields: Vec<arrow::row::SortField> = schema
        .fields()
        .iter()
        .map(|f| arrow::row::SortField::new(f.data_type().clone()))
        .collect();

    let converter = arrow::row::RowConverter::new(fields)?;
    let mut seen: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    let mut result: Vec<RecordBatch> = Vec::new();

    for batch in batches {
        let rows = converter.convert_columns(batch.columns())?;
        let mut indices: Vec<u32> = Vec::new();

        for (i, row) in rows.iter().enumerate() {
            if seen.insert(row.as_ref().to_vec()) {
                indices.push(u32::try_from(i)?);
            }
        }

        if !indices.is_empty() {
            let index_array = arrow::array::UInt32Array::from(indices);
            let deduped_columns: Vec<arrow::array::ArrayRef> = batch
                .columns()
                .iter()
                .map(|col| arrow::compute::take(col.as_ref(), &index_array, None))
                .collect::<Result<_, _>>()?;
            result.push(RecordBatch::try_new(schema.clone(), deduped_columns)?);
        }
    }

    Ok(result)
}

/// Combines multiple Parquet files using object store with storage options
///
/// # Errors
///
/// Returns an error if file reading or writing fails.
pub async fn combine_parquet_files(
    file_paths: Vec<&str>,
    new_file_path: &str,
    storage_options: Option<AHashMap<String, String>>,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
    deduplicate: Option<bool>,
) -> anyhow::Result<()> {
    if file_paths.len() <= 1 {
        return Ok(());
    }

    // Create object store from the first file path (assuming all files are in the same store)
    let (object_store, base_path, _) =
        create_object_store_from_path(file_paths[0], storage_options)?;

    // Convert string paths to ObjectPath
    let object_paths: Vec<ObjectPath> = file_paths
        .iter()
        .map(|path| object_path_under_base(&base_path, path))
        .collect();

    combine_parquet_files_from_object_store(
        object_store,
        object_paths,
        &object_path_under_base(&base_path, new_file_path),
        compression,
        max_row_group_size,
        deduplicate,
    )
    .await
}

/// Combines multiple Parquet files from object store
///
/// # Errors
///
/// Returns an error if file reading or writing fails.
pub async fn combine_parquet_files_from_object_store(
    object_store: Arc<dyn ObjectStore>,
    file_paths: Vec<ObjectPath>,
    new_file_path: &ObjectPath,
    compression: Option<Compression>,
    max_row_group_size: Option<usize>,
    deduplicate: Option<bool>,
) -> anyhow::Result<()> {
    if file_paths.len() <= 1 {
        return Ok(());
    }

    let mut all_batches: Vec<RecordBatch> = Vec::new();
    let mut schema_with_metadata: Option<Arc<arrow::datatypes::Schema>> = None;
    let mut schema_source: Option<&ObjectPath> = None;
    let mut field_metadata_sources = HashMap::new();

    // Read all files from object store
    for path in &file_paths {
        let data = object_store.get(path).await?.bytes().await?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(data)?;

        let candidate_schema = builder.schema().clone();
        schema_with_metadata = Some(
            if let (Some(schema), Some(source)) = (&schema_with_metadata, schema_source) {
                let reconciled = reconcile_consolidation_schema_with_sources(
                    schema,
                    source,
                    &field_metadata_sources,
                    &candidate_schema,
                    path,
                )?;

                if reconciled.schema_source == ConsolidationSchemaSource::Candidate {
                    schema_source = Some(path);
                }

                for key in reconciled.candidate_field_metadata {
                    field_metadata_sources.insert(key, path.clone());
                }

                reconciled.schema
            } else {
                schema_source = Some(path);
                field_metadata_sources
                    .extend(field_metadata_keys(&candidate_schema).map(|key| (key, path.clone())));
                candidate_schema
            },
        );

        for batch in builder.build()? {
            all_batches.push(batch?);
        }
    }

    // Re-apply the preserved schema metadata to all collected batches so that
    // write_batches_to_object_store (which uses batches[0].schema()) can encode
    // the correct Arrow schema metadata into the combined output file.
    if let Some(schema) = &schema_with_metadata {
        all_batches = all_batches
            .into_iter()
            .map(|b| RecordBatch::try_new(schema.clone(), b.columns().to_vec()))
            .collect::<Result<Vec<_>, _>>()?;
    }

    // Deduplicate rows if requested
    let batches_to_write = if deduplicate.unwrap_or(false) {
        deduplicate_record_batches(&all_batches)?
    } else {
        all_batches
    };

    // Write combined batches to new location
    write_batches_to_object_store(
        &batches_to_write,
        object_store.clone(),
        new_file_path,
        compression,
        max_row_group_size,
        None,
    )
    .await?;

    // Remove the merged files
    for path in &file_paths {
        if path != new_file_path {
            object_store.delete(path).await?;
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsolidationSchemaSource {
    Current,
    Candidate,
}

#[derive(Debug)]
struct ReconciledConsolidationSchema {
    schema: Arc<Schema>,
    schema_source: ConsolidationSchemaSource,
    candidate_field_metadata: Vec<(String, String)>,
}

fn reconcile_consolidation_schema_with_sources(
    current: &Arc<Schema>,
    current_path: &ObjectPath,
    current_field_metadata_sources: &HashMap<(String, String), ObjectPath>,
    candidate: &Arc<Schema>,
    candidate_path: &ObjectPath,
) -> anyhow::Result<ReconciledConsolidationSchema> {
    anyhow::ensure!(
        current.fields().len() == candidate.fields().len(),
        "Cannot consolidate Parquet files {current_path} and {candidate_path}: field schemas differ"
    );
    let mut candidate_field_metadata = Vec::new();

    let fields = current
        .fields()
        .iter()
        .zip(candidate.fields())
        .map(|(current, candidate)| {
            anyhow::ensure!(
                current.name() == candidate.name()
                    && current.data_type() == candidate.data_type()
                    && current.is_nullable() == candidate.is_nullable(),
                "Cannot consolidate Parquet files {current_path} and {candidate_path}: field schemas differ"
            );
            let mut metadata = current.metadata().clone();
            for (key, value) in candidate.metadata() {
                if let Some(current_value) = metadata.get(key) {
                    let source = current_field_metadata_sources
                        .get(&(current.name().clone(), key.clone()))
                        .unwrap_or(current_path);
                    anyhow::ensure!(
                        current_value == value,
                        "Cannot consolidate Parquet files {source} and {candidate_path}: field '{}' metadata differs",
                        current.name(),
                    );
                } else {
                    metadata.insert(key.clone(), value.clone());
                    candidate_field_metadata.push((current.name().clone(), key.clone()));
                }
            }

            Ok(Arc::new(current.as_ref().clone().with_metadata(metadata)))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let schema_with_fields =
        |metadata| Arc::new(Schema::new_with_metadata(fields.clone(), metadata));

    if current.metadata() == candidate.metadata() {
        return Ok(ReconciledConsolidationSchema {
            schema: schema_with_fields(current.metadata().clone()),
            schema_source: ConsolidationSchemaSource::Current,
            candidate_field_metadata,
        });
    }

    let without_precision = |schema: &Schema| {
        let mut metadata = schema.metadata().clone();
        metadata.remove(KEY_PRICE_PRECISION);
        metadata.remove(KEY_SIZE_PRECISION);
        metadata
    };

    let is_precision_fallback = |metadata: &HashMap<String, String>| {
        metadata.get(KEY_PRICE_PRECISION).map(String::as_str) == Some("0")
            && metadata.get(KEY_SIZE_PRECISION).map(String::as_str) == Some("0")
    };

    let has_precision = |metadata: &HashMap<String, String>| {
        metadata.contains_key(KEY_PRICE_PRECISION) && metadata.contains_key(KEY_SIZE_PRECISION)
    };

    let current_metadata = current.metadata();
    let candidate_metadata = candidate.metadata();

    if without_precision(current) == without_precision(candidate) {
        match (
            is_precision_fallback(current_metadata),
            is_precision_fallback(candidate_metadata),
        ) {
            (true, false) if has_precision(candidate_metadata) => {
                return Ok(ReconciledConsolidationSchema {
                    schema: schema_with_fields(candidate.metadata().clone()),
                    schema_source: ConsolidationSchemaSource::Candidate,
                    candidate_field_metadata,
                });
            }
            (false, true) if has_precision(current_metadata) => {
                return Ok(ReconciledConsolidationSchema {
                    schema: schema_with_fields(current.metadata().clone()),
                    schema_source: ConsolidationSchemaSource::Current,
                    candidate_field_metadata,
                });
            }
            _ => {}
        }
    }

    anyhow::bail!(
        "Cannot consolidate Parquet files {current_path} and {candidate_path}: schema metadata differs: {current_metadata:?} versus {candidate_metadata:?}"
    )
}

fn field_metadata_keys(schema: &Schema) -> impl Iterator<Item = (String, String)> + '_ {
    schema.fields().iter().flat_map(|field| {
        field
            .metadata()
            .keys()
            .map(|key| (field.name().clone(), key.clone()))
    })
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata using object store with storage options.
///
/// # Errors
///
/// Returns an error if the file cannot be read, metadata parsing fails, or the column is missing or has no statistics.
pub async fn min_max_from_parquet_metadata(
    file_path: &str,
    storage_options: Option<AHashMap<String, String>>,
    column_name: &str,
) -> anyhow::Result<(u64, u64)> {
    let (object_store, base_path, _) = create_object_store_from_path(file_path, storage_options)?;
    let object_path = object_path_under_base(&base_path, file_path);

    min_max_from_parquet_metadata_object_store(object_store, &object_path, column_name).await
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata in object store.
///
/// # Errors
///
/// Returns an error if the file cannot be read, metadata parsing fails, or the column is missing or has no statistics.
pub async fn min_max_from_parquet_metadata_object_store(
    object_store: Arc<dyn ObjectStore>,
    file_path: &ObjectPath,
    column_name: &str,
) -> anyhow::Result<(u64, u64)> {
    // Download the parquet file from object store
    let data = object_store.get(file_path).await?.bytes().await?;
    let reader = SerializedFileReader::new(data)?;

    let metadata = reader.metadata();
    let mut overall_min_value: Option<i64> = None;
    let mut overall_max_value: Option<i64> = None;

    // Iterate through all row groups
    for i in 0..metadata.num_row_groups() {
        let row_group = metadata.row_group(i);

        // Iterate through all columns in this row group
        for j in 0..row_group.num_columns() {
            let col_metadata = row_group.column(j);

            if col_metadata.column_path().string() == column_name {
                if let Some(stats) = col_metadata.statistics() {
                    // Check if we have Int64 statistics
                    if let Statistics::Int64(int64_stats) = stats {
                        // Extract min value if available
                        if let Some(&min_value) = int64_stats.min_opt()
                            && (overall_min_value.is_none()
                                || min_value < overall_min_value.unwrap())
                        {
                            overall_min_value = Some(min_value);
                        }

                        // Extract max value if available
                        if let Some(&max_value) = int64_stats.max_opt()
                            && (overall_max_value.is_none()
                                || max_value > overall_max_value.unwrap())
                        {
                            overall_max_value = Some(max_value);
                        }
                    } else {
                        anyhow::bail!("Warning: Column name '{column_name}' is not of type i64.");
                    }
                } else {
                    anyhow::bail!(
                        "Warning: Statistics not available for column '{column_name}' in row group {i}."
                    );
                }
            }
        }
    }

    // Return the min/max pair if both are available
    if let (Some(min), Some(max)) = (overall_min_value, overall_max_value) {
        Ok((u64::try_from(min)?, u64::try_from(max)?))
    } else {
        anyhow::bail!(
            "Column '{column_name}' not found or has no Int64 statistics in any row group."
        )
    }
}

/// Creates an object store from a URI string with optional storage options.
///
/// Supports multiple cloud storage providers:
/// - AWS S3: `s3://bucket/path`
/// - Google Cloud Storage: `gs://bucket/path` or `gcs://bucket/path`
/// - Azure Blob Storage: `az://account/container/path` or `abfs://container@account.dfs.core.windows.net/path`
/// - HTTP/WebDAV: `http://` or `https://`
/// - Local files: `file://path` or plain paths
///
/// # Parameters
///
/// - `path`: The URI string for the storage location.
/// - `storage_options`: Optional `HashMap` containing storage-specific configuration options:
///   - For S3: `endpoint_url`, region, `access_key_id`, `secret_access_key`, `session_token`, etc.
///   - For GCS: `service_account_path`, `service_account_key`, `project_id`, etc.
///   - For Azure: `account_name`, `account_key`, `sas_token`, etc.
///
/// Returns a tuple of (`ObjectStore`, `base_path`, `normalized_uri`)
pub fn create_object_store_from_path(
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let location = create_object_store_location_from_path(path, storage_options)?;
    Ok((
        location.object_store,
        location.base_path,
        location.original_uri,
    ))
}

// `storage_options` is only consumed by the cloud-feature arms,
// so keep the allow scoped to the no-cloud build.
#[cfg_attr(
    not(feature = "cloud"),
    allow(unused_variables, clippy::needless_pass_by_value)
)]
pub(crate) fn create_object_store_location_from_path(
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<ObjectStoreLocation> {
    let uri = normalize_path_to_uri(path)?;

    let (object_store, base_path, original_uri) = match uri.as_str() {
        #[cfg(feature = "cloud")]
        s if s.starts_with("s3://") => create_s3_store(&uri, storage_options),
        #[cfg(feature = "cloud")]
        s if s.starts_with("gs://") || s.starts_with("gcs://") => {
            create_gcs_store(&uri, storage_options)
        }
        #[cfg(feature = "cloud")]
        s if s.starts_with("az://") => create_azure_store(&uri, storage_options),
        #[cfg(feature = "cloud")]
        s if s.starts_with("abfs://") => create_abfs_store(&uri, storage_options),
        #[cfg(feature = "cloud")]
        s if s.starts_with("http://") || s.starts_with("https://") => {
            create_http_store(&uri, storage_options)
        }
        #[cfg(not(feature = "cloud"))]
        s if s.starts_with("s3://")
            || s.starts_with("gs://")
            || s.starts_with("gcs://")
            || s.starts_with("az://")
            || s.starts_with("abfs://")
            || s.starts_with("http://")
            || s.starts_with("https://") =>
        {
            anyhow::bail!("Cloud storage support requires the 'cloud' feature: {uri}")
        }
        s if s.starts_with("file://") => create_local_store(&uri, true),
        _ => create_local_store(&uri, false), // Fallback: assume local path
    }?;

    let store_root_url = Url::parse(&original_uri)
        .ok()
        .filter(|url| is_remote_uri_scheme(url.scheme()))
        .map(|_| remote_store_root_url(&original_uri))
        .transpose()?;
    Ok(ObjectStoreLocation {
        object_store,
        base_path,
        original_uri,
        store_root_url,
    })
}

fn object_path_under_base(base_path: &str, path: &str) -> ObjectPath {
    if base_path.is_empty() {
        ObjectPath::from(path)
    } else {
        ObjectPath::from(format!("{base_path}/{path}"))
    }
}

pub(crate) fn is_remote_uri_scheme(scheme: &str) -> bool {
    matches!(
        scheme,
        "s3" | "gs" | "gcs" | "az" | "abfs" | "http" | "https"
    )
}

pub(crate) fn remote_store_root_url(uri: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(uri)?;
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn remote_full_uri(uri: &str, object_path: &str) -> anyhow::Result<String> {
    let root = remote_store_root_url(uri)?;
    let root = root.as_str().trim_end_matches('/');
    let object_path = object_path.trim_start_matches('/');

    if object_path.is_empty() {
        Ok(root.to_string())
    } else {
        Ok(format!("{root}/{object_path}"))
    }
}

/// Appends an encoded object-store path to the local storage URI.
/// Preserve the encoded names used by the native object-store backend.
pub(crate) fn append_path_to_file_uri(base_uri: &str, path: &str) -> String {
    if let Ok(mut url) = Url::parse(base_uri) {
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty();
            segments.extend(
                path.trim_end_matches('/')
                    .split('/')
                    .filter(|segment| !segment.is_empty()),
            );
        }

        return url.to_string();
    }

    format!(
        "{}/{}",
        base_uri.trim_end_matches('/'),
        path.trim_end_matches('/')
    )
}

/// Decodes a percent-encoded `object_store` path segment back to its logical form.
///
/// `object_store` lists path segments in URL-encoded form, so a non-ASCII instrument
/// directory reads back with each non-ASCII byte as a `%XX` sequence. Decoding recovers the
/// original id for matching against `urisafe_instrument_id`. Returns the input unchanged when
/// it is not valid percent-encoded UTF-8.
pub(crate) fn decode_object_store_segment(segment: &str) -> String {
    ObjectPath::from_url_path(segment).map_or_else(|_| segment.to_string(), String::from)
}

fn create_local_store(
    uri: &str,
    is_file_uri: bool,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let path = if is_file_uri {
        file_uri_to_native_path(uri)
    } else {
        uri.to_string()
    };

    let local_store =
        object_store::local::LocalFileSystem::new_with_prefix(&path).with_context(|| {
            format!(
                "failed to open local storage directory '{path}'; \
             create it if it does not exist and check access permissions"
            )
        })?;

    Ok((Arc::new(local_store), String::new(), uri.to_string()))
}

/// Helper function to create S3 object store with options.
#[cfg(feature = "cloud")]
fn create_s3_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid S3 URI: missing bucket")?;

    let mut builder = object_store::aws::AmazonS3Builder::new().with_bucket_name(&bucket);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, value) in options {
            match key.as_str() {
                // Accept legacy storage-option aliases alongside native names.
                "endpoint_url" | "endpoint" => {
                    builder = builder.with_endpoint(&value);
                }
                "region" => {
                    builder = builder.with_region(&value);
                }
                "access_key_id" | "key" => {
                    builder = builder.with_access_key_id(&value);
                }
                "secret_access_key" | "secret" => {
                    builder = builder.with_secret_access_key(&value);
                }
                "session_token" | "token" => {
                    builder = builder.with_token(&value);
                }
                "allow_http" => {
                    let allow_http = value.to_lowercase() == "true";
                    builder = builder.with_allow_http(allow_http);
                }
                _ => {
                    // Ignore unknown options for forward compatibility
                    log::warn!("Unknown S3 storage option: {key}");
                }
            }
        }
    }

    let s3_store = builder.build()?;
    Ok((Arc::new(s3_store), path, uri.to_string()))
}

/// Helper function to create GCS object store with options.
#[cfg(feature = "cloud")]
fn create_gcs_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid GCS URI: missing bucket")?;

    let mut builder = object_store::gcp::GoogleCloudStorageBuilder::new().with_bucket_name(&bucket);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, value) in options {
            match key.as_str() {
                "service_account_path" | "credential_path" => {
                    builder = builder.with_service_account_path(&value);
                }
                "service_account_key" => {
                    builder = builder.with_service_account_key(&value);
                }
                "project_id" => {
                    // Note: GoogleCloudStorageBuilder doesn't have with_project_id method
                    // This would need to be handled via environment variables or service account
                    log::warn!(
                        "project_id should be set via service account or environment variables"
                    );
                }
                "application_credentials" => {
                    // Set GOOGLE_APPLICATION_CREDENTIALS env var required by Google auth libraries.
                    // SAFETY: std::env::set_var is marked unsafe because it mutates global state and
                    // can break signal-safe code. We only call it during configuration before any
                    // multi-threaded work starts, so it is considered safe in this context.
                    unsafe {
                        std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", &value);
                    }
                }
                _ => {
                    // Ignore unknown options for forward compatibility
                    log::warn!("Unknown GCS storage option: {key}");
                }
            }
        }
    }

    let gcs_store = builder.build()?;
    Ok((Arc::new(gcs_store), path, uri.to_string()))
}

/// Helper function to create Azure object store with options.
#[cfg(feature = "cloud")]
fn create_azure_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let container = extract_host(&url, "Invalid Azure URI: missing container")?;

    let mut builder =
        object_store::azure::MicrosoftAzureBuilder::new().with_container_name(container);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        builder = apply_azure_storage_options(builder, options, "Azure");
    }

    let azure_store = builder.build()?;
    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Helper function to create Azure object store from abfs:// URI with options.
#[cfg(feature = "cloud")]
fn create_abfs_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let host = extract_host(&url, "Invalid ABFS URI: missing host")?;

    // Extract account from host (account.dfs.core.windows.net)
    let account = host
        .split('.')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid ABFS URI: cannot extract account from host"))?;

    // Extract container from username part
    let container = url
        .username()
        .split('@')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid ABFS URI: missing container"))?;

    let mut builder = object_store::azure::MicrosoftAzureBuilder::new()
        .with_account(account)
        .with_container_name(container);

    // Apply storage options if provided (same as Azure store)
    if let Some(options) = storage_options {
        builder = apply_azure_storage_options(builder, options, "ABFS");
    }

    let azure_store = builder.build()?;
    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Applies shared Azure storage options to the builder; `store_label` names the URI
/// scheme ("Azure" or "ABFS") in unknown-option warnings.
#[cfg(feature = "cloud")]
fn apply_azure_storage_options(
    mut builder: object_store::azure::MicrosoftAzureBuilder,
    options: AHashMap<String, String>,
    store_label: &str,
) -> object_store::azure::MicrosoftAzureBuilder {
    for (key, value) in options {
        match key.as_str() {
            "account_name" => {
                builder = builder.with_account(&value);
            }
            "account_key" => {
                builder = builder.with_access_key(&value);
            }
            "sas_token" => {
                // Parse SAS token as query string parameters
                let query_pairs: Vec<(String, String)> = value
                    .split('&')
                    .filter_map(|pair| {
                        let mut parts = pair.split('=');
                        match (parts.next(), parts.next()) {
                            (Some(key), Some(val)) => Some((key.to_string(), val.to_string())),
                            _ => None,
                        }
                    })
                    .collect();

                builder = builder.with_sas_authorization(query_pairs);
            }
            "client_id" => {
                builder = builder.with_client_id(&value);
            }
            "client_secret" => {
                builder = builder.with_client_secret(&value);
            }
            "tenant_id" => {
                builder = builder.with_tenant_id(&value);
            }
            _ => {
                // Ignore unknown options for forward compatibility
                log::warn!("Unknown {store_label} storage option: {key}");
            }
        }
    }

    builder
}

/// Helper function to create HTTP object store with options.
#[cfg(feature = "cloud")]
fn create_http_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (_, path) = parse_url_and_path(uri)?;
    let base_url = remote_store_root_url(uri)?
        .as_str()
        .trim_end_matches('/')
        .to_string();

    let builder = object_store::http::HttpBuilder::new().with_url(base_url);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, _value) in options {
            // HTTP builder has limited configuration options
            // Most HTTP-specific options would be handled via client options
            // Ignore unknown options for forward compatibility
            log::warn!("Unknown HTTP storage option: {key}");
        }
    }

    let http_store = builder.build()?;
    Ok((Arc::new(http_store), path, uri.to_string()))
}

/// Helper function to parse URL and extract path component.
#[cfg(feature = "cloud")]
fn parse_url_and_path(uri: &str) -> anyhow::Result<(Url, String)> {
    let url = Url::parse(uri)?;
    let path = url.path().trim_start_matches('/').to_string();
    Ok((url, path))
}

/// Helper function to extract host from URL with error handling.
#[cfg(feature = "cloud")]
fn extract_host(url: &Url, error_msg: &str) -> anyhow::Result<String> {
    url.host_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("{error_msg}"))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    #[cfg(feature = "cloud")]
    use ahash::AHashMap;
    use arrow::{
        array::{ArrayRef, StringArray, UInt64Array},
        datatypes::{DataType, Field, Schema},
    };
    use nautilus_serialization::arrow::json_string_field;
    use parquet::file::{properties::ReaderProperties, serialized_reader::ReadOptionsBuilder};
    use rstest::rstest;

    use super::*;

    fn consolidation_depth_schema(
        price_precision: &str,
        size_precision: &str,
        instrument_id: &str,
    ) -> Arc<Schema> {
        Arc::new(Schema::new_with_metadata(
            vec![Field::new("bids", DataType::Utf8, false)],
            HashMap::from([
                (KEY_PRICE_PRECISION.to_string(), price_precision.to_string()),
                (KEY_SIZE_PRECISION.to_string(), size_precision.to_string()),
                (KEY_IDENTIFIER.to_string(), instrument_id.to_string()),
            ]),
        ))
    }

    fn reconcile_consolidation_schema(
        current: &Arc<Schema>,
        current_path: &ObjectPath,
        candidate: &Arc<Schema>,
        candidate_path: &ObjectPath,
    ) -> anyhow::Result<ReconciledConsolidationSchema> {
        let field_metadata_sources = field_metadata_keys(current)
            .map(|key| (key, current_path.clone()))
            .collect();
        reconcile_consolidation_schema_with_sources(
            current,
            current_path,
            &field_metadata_sources,
            candidate,
            candidate_path,
        )
    }

    #[rstest]
    fn consolidation_schema_prefers_populated_depth_precision_in_either_order() {
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let populated = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let fallback_path = ObjectPath::from("empty.parquet");
        let populated_path = ObjectPath::from("populated.parquet");

        let fallback_first =
            reconcile_consolidation_schema(&fallback, &fallback_path, &populated, &populated_path)
                .unwrap();
        let populated_first =
            reconcile_consolidation_schema(&populated, &populated_path, &fallback, &fallback_path)
                .unwrap();

        assert_eq!(
            fallback_first.schema_source,
            ConsolidationSchemaSource::Candidate,
        );
        assert_eq!(
            populated_first.schema_source,
            ConsolidationSchemaSource::Current,
        );

        for schema in [fallback_first.schema, populated_first.schema] {
            assert_eq!(schema.metadata()[KEY_PRICE_PRECISION], "2");
            assert_eq!(schema.metadata()[KEY_SIZE_PRECISION], "3");
        }
    }

    #[rstest]
    fn consolidation_schema_rejects_other_metadata_mismatches() {
        let current = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let candidate = consolidation_depth_schema("2", "3", "BTCUSDT.BINANCE");
        let current_path = ObjectPath::from("eth.parquet");
        let candidate_path = ObjectPath::from("btc.parquet");

        let error =
            reconcile_consolidation_schema(&current, &current_path, &candidate, &candidate_path)
                .unwrap_err();

        assert!(error.to_string().contains("eth.parquet and btc.parquet"));
        assert!(error.to_string().contains("ETHUSDT.BINANCE"));
        assert!(error.to_string().contains("BTCUSDT.BINANCE"));
    }

    #[rstest]
    fn consolidation_schema_rejects_two_populated_precisions() {
        let current = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let candidate = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let current_path = ObjectPath::from("precision-2.parquet");
        let candidate_path = ObjectPath::from("precision-4.parquet");

        let error =
            reconcile_consolidation_schema(&current, &current_path, &candidate, &candidate_path)
                .unwrap_err();

        assert!(error.to_string().contains("precision-2.parquet"));
        assert!(error.to_string().contains("precision-4.parquet"));
    }

    #[rstest]
    fn consolidation_schema_keeps_fallback_for_all_empty_files() {
        let first = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let second = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");

        let reconciled = reconcile_consolidation_schema(
            &first,
            &ObjectPath::from("first-empty.parquet"),
            &second,
            &ObjectPath::from("second-empty.parquet"),
        )
        .unwrap();

        assert_eq!(reconciled.schema.metadata()[KEY_PRICE_PRECISION], "0");
        assert_eq!(reconciled.schema.metadata()[KEY_SIZE_PRECISION], "0");
    }

    #[rstest]
    #[case::missing_first(true)]
    #[case::fallback_first(false)]
    fn consolidation_schema_rejects_missing_precision_against_fallback(
        #[case] missing_first: bool,
    ) {
        let missing = Arc::new(Schema::new_with_metadata(
            vec![Field::new("bids", DataType::Utf8, false)],
            HashMap::from([(KEY_IDENTIFIER.to_string(), "ETHUSDT.BINANCE".to_string())]),
        ));
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let missing_path = ObjectPath::from("missing.parquet");
        let fallback_path = ObjectPath::from("fallback.parquet");

        let error = if missing_first {
            reconcile_consolidation_schema(&missing, &missing_path, &fallback, &fallback_path)
        } else {
            reconcile_consolidation_schema(&fallback, &fallback_path, &missing, &missing_path)
        }
        .unwrap_err();

        assert!(error.to_string().contains("schema metadata differs"));
    }

    #[rstest]
    fn consolidation_schema_merges_json_field_annotation_in_either_order() {
        let bare = Arc::new(Schema::new(vec![Field::new("info", DataType::Utf8, true)]));
        let annotated = Arc::new(Schema::new(vec![json_string_field("info", true)]));
        let bare_path = ObjectPath::from("bare.parquet");
        let annotated_path = ObjectPath::from("annotated.parquet");

        let bare_first =
            reconcile_consolidation_schema(&bare, &bare_path, &annotated, &annotated_path).unwrap();
        let annotated_first =
            reconcile_consolidation_schema(&annotated, &annotated_path, &bare, &bare_path).unwrap();

        assert_eq!(bare_first.schema, annotated_first.schema);
        assert_eq!(
            bare_first.schema.field_with_name("info").unwrap(),
            &json_string_field("info", true),
        );
    }

    #[rstest]
    fn consolidation_field_metadata_conflict_names_the_winning_file() {
        let bare = Arc::new(Schema::new(vec![Field::new("info", DataType::Utf8, true)]));
        let annotated = Arc::new(Schema::new(vec![json_string_field("info", true)]));

        let conflicting = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true).with_metadata(HashMap::from([(
                "ARROW:extension:name".to_string(),
                "other.extension".to_string(),
            )])),
        ]));
        let bare_path = ObjectPath::from("bare.parquet");
        let annotated_path = ObjectPath::from("annotated.parquet");
        let conflicting_path = ObjectPath::from("conflicting.parquet");
        let reconciled =
            reconcile_consolidation_schema(&bare, &bare_path, &annotated, &annotated_path).unwrap();

        assert_eq!(reconciled.candidate_field_metadata.len(), 2);
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("info".to_string(), "ARROW:extension:name".to_string())),
        );
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("info".to_string(), "ARROW:extension:metadata".to_string())),
        );
        let field_metadata_sources = reconciled
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, annotated_path.clone()))
            .collect();
        let error = reconcile_consolidation_schema_with_sources(
            &reconciled.schema,
            &bare_path,
            &field_metadata_sources,
            &conflicting,
            &conflicting_path,
        )
        .unwrap_err();
        assert!(error.to_string().contains("annotated.parquet"));
        assert!(error.to_string().contains("conflicting.parquet"));
        assert!(error.to_string().contains("field 'info' metadata differs"));
    }

    #[rstest]
    fn consolidation_conflict_names_the_last_winning_schema() {
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let precision_2 = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let precision_4 = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let fallback_path = ObjectPath::from("fallback.parquet");
        let precision_2_path = ObjectPath::from("precision-2.parquet");
        let precision_4_path = ObjectPath::from("precision-4.parquet");
        let reconciled = reconcile_consolidation_schema(
            &fallback,
            &fallback_path,
            &precision_2,
            &precision_2_path,
        )
        .unwrap();

        let error = reconcile_consolidation_schema(
            &reconciled.schema,
            &precision_2_path,
            &precision_4,
            &precision_4_path,
        )
        .unwrap_err();

        assert!(error.to_string().contains("precision-2.parquet"));
        assert!(error.to_string().contains("precision-4.parquet"));
    }

    #[rstest]
    fn consolidation_field_metadata_does_not_replace_precision_source() {
        let precision_2 = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");

        let fallback = Arc::new(Schema::new_with_metadata(
            vec![json_string_field("bids", false)],
            consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE")
                .metadata()
                .clone(),
        ));
        let precision_4 = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let precision_2_path = ObjectPath::from("precision-2.parquet");
        let fallback_path = ObjectPath::from("fallback-annotated.parquet");
        let precision_4_path = ObjectPath::from("precision-4.parquet");
        let reconciled = reconcile_consolidation_schema(
            &precision_2,
            &precision_2_path,
            &fallback,
            &fallback_path,
        )
        .unwrap();

        assert_eq!(reconciled.schema_source, ConsolidationSchemaSource::Current,);
        assert_eq!(reconciled.candidate_field_metadata.len(), 2);
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("bids".to_string(), "ARROW:extension:name".to_string())),
        );
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("bids".to_string(), "ARROW:extension:metadata".to_string())),
        );
        let field_metadata_sources = reconciled
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, fallback_path.clone()))
            .collect();
        let error = reconcile_consolidation_schema_with_sources(
            &reconciled.schema,
            &precision_2_path,
            &field_metadata_sources,
            &precision_4,
            &precision_4_path,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("precision-2.parquet"));
        assert!(message.contains("precision-4.parquet"));
        assert!(!message.contains("fallback-annotated.parquet"));
    }

    #[rstest]
    fn consolidation_field_metadata_tracks_each_origin() {
        let bare = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true),
            Field::new("balances", DataType::Utf8, true),
        ]));

        let info = Arc::new(Schema::new(vec![
            json_string_field("info", true),
            Field::new("balances", DataType::Utf8, true),
        ]));

        let balances = Arc::new(Schema::new(vec![
            json_string_field("info", true),
            json_string_field("balances", true),
        ]));

        let conflicting = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true).with_metadata(HashMap::from([(
                "ARROW:extension:name".to_string(),
                "other.extension".to_string(),
            )])),
            json_string_field("balances", true),
        ]));
        let bare_path = ObjectPath::from("bare.parquet");
        let info_path = ObjectPath::from("info.parquet");
        let balances_path = ObjectPath::from("balances.parquet");
        let conflicting_path = ObjectPath::from("conflicting.parquet");
        let with_info =
            reconcile_consolidation_schema(&bare, &bare_path, &info, &info_path).unwrap();
        let mut field_metadata_sources = with_info
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, info_path.clone()))
            .collect::<HashMap<_, _>>();
        let with_balances = reconcile_consolidation_schema_with_sources(
            &with_info.schema,
            &bare_path,
            &field_metadata_sources,
            &balances,
            &balances_path,
        )
        .unwrap();

        for key in with_balances.candidate_field_metadata {
            field_metadata_sources.insert(key, balances_path.clone());
        }

        let error = reconcile_consolidation_schema_with_sources(
            &with_balances.schema,
            &bare_path,
            &field_metadata_sources,
            &conflicting,
            &conflicting_path,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("info.parquet"));
        assert!(message.contains("conflicting.parquet"));
        assert!(!message.contains("balances.parquet"));
    }

    #[tokio::test]
    async fn write_batches_to_object_store_rejects_empty_input() {
        let object_store = Arc::new(object_store::memory::InMemory::new());

        let error = write_batches_to_object_store(
            &[],
            object_store,
            &ObjectPath::from("empty.parquet"),
            None,
            None,
            None,
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot write Parquet file empty.parquet with no record batches"
        );
    }

    #[tokio::test]
    async fn default_writer_sets_zstd_sorting_and_identifier_bloom_filter() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.parquet");

        let object_store = Arc::new(
            object_store::local::LocalFileSystem::new_with_prefix(directory.path()).unwrap(),
        );
        let object_path = ObjectPath::from("layout.parquet");

        let schema = Arc::new(Schema::new(vec![
            Field::new("identifier", DataType::Utf8, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["AUD/USD.SIM", "AUD/USD.SIM"])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1_u64, 2])) as ArrayRef,
            ],
        )
        .unwrap();

        write_batches_to_object_store(&[batch], object_store, &object_path, None, None, None)
            .await
            .unwrap();

        let read_options = ReadOptionsBuilder::new()
            .with_reader_properties(
                ReaderProperties::builder()
                    .set_read_bloom_filter(true)
                    .build(),
            )
            .build();
        let reader = SerializedFileReader::new_with_options(
            std::fs::File::open(path).unwrap(),
            read_options,
        )
        .unwrap();
        let row_group = reader.metadata().row_group(0);
        let sorting = row_group.sorting_columns().unwrap();

        assert_eq!(crate::backend::parquet::DEFAULT_ROW_GROUP_SIZE, 131_072);
        assert_eq!(
            sorting,
            &vec![
                SortingColumn {
                    column_idx: 1,
                    descending: false,
                    nulls_first: false,
                },
                SortingColumn {
                    column_idx: 0,
                    descending: false,
                    nulls_first: false,
                },
            ],
        );
        assert!(
            row_group
                .columns()
                .iter()
                .all(|column| column.compression() == Compression::ZSTD(ZstdLevel::default())),
        );
        assert!(
            reader
                .get_row_group(0)
                .unwrap()
                .get_column_bloom_filter(0)
                .is_some(),
        );
    }

    #[rstest]
    fn test_create_object_store_from_path_local() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("nautilus_test");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = create_object_store_from_path(temp_dir.to_str().unwrap(), None);
        if let Err(e) = &result {
            println!("Error: {e:?}");
        }

        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "");
        // The URI should be normalized to file:// format
        assert_eq!(uri, format!("file://{}", temp_dir.to_str().unwrap()));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[rstest]
    fn append_path_to_file_uri_extends_windows_drive_catalog() {
        assert_eq!(
            append_path_to_file_uri(
                "file:///C:/data/catalog",
                "data/quotes/EURUSD.SIM/file.parquet",
            ),
            "file:///C:/data/catalog/data/quotes/EURUSD.SIM/file.parquet",
        );
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_s3() {
        let mut options = AHashMap::new();
        options.insert(
            "endpoint_url".to_string(),
            "https://test.endpoint.com".to_string(),
        );
        options.insert("region".to_string(), "us-west-2".to_string());
        options.insert("access_key_id".to_string(), "test_key".to_string());
        options.insert("secret_access_key".to_string(), "test_secret".to_string());

        let result = create_object_store_from_path("s3://test-bucket/path", Some(options));
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "s3://test-bucket/path");
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_azure() {
        let mut options = AHashMap::new();
        options.insert("account_name".to_string(), "testaccount".to_string());
        // Use a valid base64 encoded key for testing
        options.insert("account_key".to_string(), "dGVzdGtleQ==".to_string()); // "testkey" in base64

        let result = create_object_store_from_path("az://container/path", Some(options));
        if let Err(e) = &result {
            println!("Azure Error: {e:?}");
        }

        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "az://container/path");
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_gcs() {
        // Test GCS without service account (will use default credentials or fail gracefully)
        let mut options = AHashMap::new();
        options.insert("project_id".to_string(), "test-project".to_string());

        let result = create_object_store_from_path("gs://test-bucket/path", Some(options));
        // GCS might fail due to missing credentials, but we're testing the path parsing
        // The function should at least parse the URI correctly before failing on auth
        match result {
            Ok((_, base_path, uri)) => {
                assert_eq!(base_path, "path");
                assert_eq!(uri, "gs://test-bucket/path");
            }
            Err(e) => {
                // Expected to fail due to missing credentials, but should contain bucket info
                let error_msg = format!("{e:?}");
                assert!(error_msg.contains("test-bucket") || error_msg.contains("credential"));
            }
        }
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_empty_options() {
        let result = create_object_store_from_path("s3://test-bucket/path", None);
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "s3://test-bucket/path");
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_remote_store_root_url_preserves_authority() {
        let https_root = remote_store_root_url("https://example.com:9000/base/path").unwrap();
        assert_eq!(
            https_root.as_str().trim_end_matches('/'),
            "https://example.com:9000"
        );

        let abfs_root =
            remote_store_root_url("abfs://container@account.dfs.core.windows.net/base/path")
                .unwrap();
        assert_eq!(
            abfs_root.as_str().trim_end_matches('/'),
            "abfs://container@account.dfs.core.windows.net"
        );

        let full_uri = remote_full_uri(
            "https://example.com:9000/base/path",
            "base/path/data/%5E/file.parquet",
        )
        .unwrap();
        assert_eq!(
            full_uri,
            "https://example.com:9000/base/path/data/%5E/file.parquet"
        );

        let location = create_object_store_location_from_path("s3://test-bucket/path", None)
            .expect("S3 location should be created");
        assert_eq!(location.base_path, "path");
        assert_eq!(
            remote_store_root_url(&location.original_uri)
                .expect("S3 should be remote")
                .as_str()
                .trim_end_matches('/'),
            "s3://test-bucket"
        );
    }
}
