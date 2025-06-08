// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::Arc;

use arrow::record_batch::RecordBatch;
use object_store::{ObjectStore, path::Path as ObjectPath};
use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    file::{
        properties::WriterProperties,
        reader::{FileReader, SerializedFileReader},
        statistics::Statistics,
    },
};

/// Writes a `RecordBatch` to a Parquet file using object store, with optional compression.
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batch_to_parquet(
    batch: RecordBatch,
    path: &str,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    write_batches_to_parquet(&[batch], path, compression, max_row_group_size).await
}

/// Writes multiple `RecordBatch` items to a Parquet file using object store, with optional compression and row group sizing.
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batches_to_parquet(
    batches: &[RecordBatch],
    path: &str,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    let (object_store, base_path, _) = create_object_store_from_path(path)?;
    let object_path = if base_path.is_empty() {
        ObjectPath::from(path)
    } else {
        ObjectPath::from(format!("{}/{}", base_path, path))
    };

    write_batches_to_object_store(
        batches,
        object_store,
        &object_path,
        compression,
        max_row_group_size,
    )
    .await
}

/// Writes multiple `RecordBatch` items to an object store URI, with optional compression and row group sizing.
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batches_to_object_store(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    // Create a temporary buffer to write the parquet data
    let mut buffer = Vec::new();

    let writer_props = WriterProperties::builder()
        .set_compression(compression.unwrap_or(parquet::basic::Compression::SNAPPY))
        .set_max_row_group_size(max_row_group_size.unwrap_or(5000))
        .build();

    let mut writer = ArrowWriter::try_new(&mut buffer, batches[0].schema(), Some(writer_props))?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;

    // Upload the buffer to object store
    object_store.put(path, buffer.into()).await?;

    Ok(())
}

/// Combines multiple Parquet files using object store
///
/// # Errors
///
/// Returns an error if file reading or writing fails.
pub async fn combine_parquet_files(
    file_paths: Vec<&str>,
    new_file_path: &str,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    if file_paths.len() <= 1 {
        return Ok(());
    }

    // Create object store from the first file path (assuming all files are in the same store)
    let (object_store, base_path, _) = create_object_store_from_path(file_paths[0])?;

    // Convert string paths to ObjectPath
    let object_paths: Vec<ObjectPath> = file_paths
        .iter()
        .map(|path| {
            if base_path.is_empty() {
                ObjectPath::from(*path)
            } else {
                ObjectPath::from(format!("{}/{}", base_path, path))
            }
        })
        .collect();

    let new_object_path = if base_path.is_empty() {
        ObjectPath::from(new_file_path)
    } else {
        ObjectPath::from(format!("{}/{}", base_path, new_file_path))
    };

    combine_parquet_files_from_object_store(
        object_store,
        object_paths,
        &new_object_path,
        compression,
        max_row_group_size,
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
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    if file_paths.len() <= 1 {
        return Ok(());
    }

    let mut all_batches: Vec<RecordBatch> = Vec::new();

    // Read all files from object store
    for path in &file_paths {
        let data = object_store.get(path).await?.bytes().await?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(data)?;
        let mut reader = builder.build()?;

        for batch in reader.by_ref() {
            all_batches.push(batch?);
        }
    }

    // Write combined batches to new location
    write_batches_to_object_store(
        &all_batches,
        object_store.clone(),
        new_file_path,
        compression,
        max_row_group_size,
    )
    .await?;

    // Remove the merged files
    for path in file_paths.iter() {
        object_store.delete(path).await?;
    }

    Ok(())
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata using object store.
///
/// # Errors
///
/// Returns an error if the file cannot be read, metadata parsing fails, or the column is missing or has no statistics.
///
/// # Panics
///
/// Panics if the Parquet metadata's min/max unwrap operations fail unexpectedly.
pub async fn min_max_from_parquet_metadata(
    file_path: &str,
    column_name: &str,
) -> anyhow::Result<(u64, u64)> {
    let (object_store, base_path, _) = create_object_store_from_path(file_path)?;
    let object_path = if base_path.is_empty() {
        ObjectPath::from(file_path)
    } else {
        ObjectPath::from(format!("{}/{}", base_path, file_path))
    };

    min_max_from_parquet_metadata_object_store(object_store, &object_path, column_name).await
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata in object store.
///
/// # Errors
///
/// Returns an error if the file cannot be read, metadata parsing fails, or the column is missing or has no statistics.
///
/// # Panics
///
/// Panics if the Parquet metadata's min/max unwrap operations fail unexpectedly.
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
                        if let Some(&min_value) = int64_stats.min_opt() {
                            if overall_min_value.is_none() || min_value < overall_min_value.unwrap()
                            {
                                overall_min_value = Some(min_value);
                            }
                        }

                        // Extract max value if available
                        if let Some(&max_value) = int64_stats.max_opt() {
                            if overall_max_value.is_none() || max_value > overall_max_value.unwrap()
                            {
                                overall_max_value = Some(max_value);
                            }
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
        Ok((min as u64, max as u64))
    } else {
        anyhow::bail!(
            "Column '{column_name}' not found or has no Int64 statistics in any row group."
        )
    }
}

/// Creates an object store from a URI string, normalizing the path if needed.
///
/// Supports multiple cloud storage providers:
/// - AWS S3: `s3://bucket/path`
/// - Google Cloud Storage: `gs://bucket/path` or `gcs://bucket/path`
/// - Azure Blob Storage: `azure://account/container/path` or `abfs://container@account.dfs.core.windows.net/path`
/// - HTTP/WebDAV: `http://` or `https://`
/// - Local files: `file://path` or plain paths
///
/// Returns a tuple of (ObjectStore, base_path, normalized_uri)
pub fn create_object_store_from_path(
    path: &str,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let uri = normalize_path_to_uri(path);

    match uri.as_str() {
        s if s.starts_with("s3://") => create_s3_store(&uri),
        s if s.starts_with("gs://") || s.starts_with("gcs://") => create_gcs_store(&uri),
        s if s.starts_with("azure://") => create_azure_store(&uri),
        s if s.starts_with("abfs://") => create_abfs_store(&uri),
        s if s.starts_with("http://") || s.starts_with("https://") => create_http_store(&uri),
        s if s.starts_with("file://") => create_local_store(&uri, true),
        _ => create_local_store(&uri, false), // Fallback: assume local path
    }
}

/// Normalizes a path to URI format for consistent object store usage.
///
/// If the path is already a URI (contains "://"), returns it as-is.
/// Otherwise, converts local paths to file:// URIs.
///
/// Supported URI schemes:
/// - `s3://` for AWS S3
/// - `gs://` or `gcs://` for Google Cloud Storage
/// - `azure://` or `abfs://` for Azure Blob Storage
/// - `http://` or `https://` for HTTP/WebDAV
/// - `file://` for local files
pub fn normalize_path_to_uri(path: &str) -> String {
    if path.contains("://") {
        // Already a URI - return as-is
        path.to_string()
    } else {
        // Convert local path to file:// URI
        if path.starts_with('/') {
            format!("file://{}", path)
        } else {
            format!(
                "file://{}",
                std::env::current_dir().unwrap().join(path).display()
            )
        }
    }
}

/// Helper function to create local file system object store
fn create_local_store(
    uri: &str,
    is_file_uri: bool,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let path = if is_file_uri {
        uri.strip_prefix("file://").unwrap_or(uri)
    } else {
        uri
    };

    let local_store = object_store::local::LocalFileSystem::new_with_prefix(path)?;
    Ok((Arc::new(local_store), String::new(), uri.to_string()))
}

/// Helper function to create S3 object store
fn create_s3_store(uri: &str) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid S3 URI: missing bucket")?;

    let s3_store = object_store::aws::AmazonS3Builder::new()
        .with_bucket_name(&bucket)
        .build()?;

    Ok((Arc::new(s3_store), path, uri.to_string()))
}

/// Helper function to create GCS object store
fn create_gcs_store(uri: &str) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid GCS URI: missing bucket")?;

    let gcs_store = object_store::gcp::GoogleCloudStorageBuilder::new()
        .with_bucket_name(&bucket)
        .build()?;

    Ok((Arc::new(gcs_store), path, uri.to_string()))
}

/// Helper function to create Azure object store from azure:// URI
fn create_azure_store(uri: &str) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, _) = parse_url_and_path(uri)?;
    let account = extract_host(&url, "Invalid Azure URI: missing account")?;

    let path_segments: Vec<&str> = url.path().trim_start_matches('/').split('/').collect();
    if path_segments.is_empty() || path_segments[0].is_empty() {
        return Err(anyhow::anyhow!("Invalid Azure URI: missing container"));
    }

    let container = path_segments[0];
    let path = if path_segments.len() > 1 {
        path_segments[1..].join("/")
    } else {
        String::new()
    };

    let azure_store = object_store::azure::MicrosoftAzureBuilder::new()
        .with_account(&account)
        .with_container_name(container)
        .build()?;

    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Helper function to create Azure object store from abfs:// URI
fn create_abfs_store(uri: &str) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
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

    let azure_store = object_store::azure::MicrosoftAzureBuilder::new()
        .with_account(account)
        .with_container_name(container)
        .build()?;

    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Helper function to create HTTP object store
fn create_http_store(uri: &str) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let base_url = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));

    let http_store = object_store::http::HttpBuilder::new()
        .with_url(base_url)
        .build()?;

    Ok((Arc::new(http_store), path, uri.to_string()))
}

/// Helper function to parse URL and extract path component
fn parse_url_and_path(uri: &str) -> anyhow::Result<(url::Url, String)> {
    let url = url::Url::parse(uri)?;
    let path = url.path().trim_start_matches('/').to_string();
    Ok((url, path))
}

/// Helper function to extract host from URL with error handling
fn extract_host(url: &url::Url, error_msg: &str) -> anyhow::Result<String> {
    url.host_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("{}", error_msg))
}
