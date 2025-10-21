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
    storage_options: Option<std::collections::HashMap<String, String>>,
    compression: Option<parquet::basic::Compression>,
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
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batches_to_parquet(
    batches: &[RecordBatch],
    path: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    let (object_store, base_path, _) = create_object_store_from_path(path, storage_options)?;
    let object_path = if base_path.is_empty() {
        ObjectPath::from(path)
    } else {
        ObjectPath::from(format!("{base_path}/{path}"))
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

/// Combines multiple Parquet files using object store with storage options
///
/// # Errors
///
/// Returns an error if file reading or writing fails.
pub async fn combine_parquet_files(
    file_paths: Vec<&str>,
    new_file_path: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
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
        .map(|path| {
            if base_path.is_empty() {
                ObjectPath::from(*path)
            } else {
                ObjectPath::from(format!("{base_path}/{path}"))
            }
        })
        .collect();

    let new_object_path = if base_path.is_empty() {
        ObjectPath::from(new_file_path)
    } else {
        ObjectPath::from(format!("{base_path}/{new_file_path}"))
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
    for path in &file_paths {
        if path != new_file_path {
            object_store.delete(path).await?;
        }
    }

    Ok(())
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata using object store with storage options.
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
    storage_options: Option<std::collections::HashMap<String, String>>,
    column_name: &str,
) -> anyhow::Result<(u64, u64)> {
    let (object_store, base_path, _) = create_object_store_from_path(file_path, storage_options)?;
    let object_path = if base_path.is_empty() {
        ObjectPath::from(file_path)
    } else {
        ObjectPath::from(format!("{base_path}/{file_path}"))
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
        Ok((min as u64, max as u64))
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
    storage_options: Option<std::collections::HashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let uri = normalize_path_to_uri(path);

    match uri.as_str() {
        s if s.starts_with("s3://") => create_s3_store(&uri, storage_options),
        s if s.starts_with("gs://") || s.starts_with("gcs://") => {
            create_gcs_store(&uri, storage_options)
        }
        s if s.starts_with("az://") => create_azure_store(&uri, storage_options),
        s if s.starts_with("abfs://") => create_abfs_store(&uri, storage_options),
        s if s.starts_with("http://") || s.starts_with("https://") => {
            create_http_store(&uri, storage_options)
        }
        s if s.starts_with("file://") => create_local_store(&uri, true),
        _ => create_local_store(&uri, false), // Fallback: assume local path
    }
}

/// Normalizes a path to URI format for consistent object store usage.
///
/// If the path is already a URI (contains "://"), returns it as-is.
/// Otherwise, converts local paths to file:// URIs with proper cross-platform handling.
///
/// Supported URI schemes:
/// - `s3://` for AWS S3
/// - `gs://` or `gcs://` for Google Cloud Storage
/// - `az://` or `abfs://` for Azure Blob Storage
/// - `http://` or `https://` for HTTP/WebDAV
/// - `file://` for local files
///
/// # Cross-platform Path Handling
///
/// - Unix absolute paths: `/path/to/file` → `file:///path/to/file`
/// - Windows drive paths: `C:\path\to\file` → `file:///C:/path/to/file`
/// - Windows UNC paths: `\\server\share\file` → `file://server/share/file`
/// - Relative paths: converted to absolute using current directory
#[must_use]
pub fn normalize_path_to_uri(path: &str) -> String {
    if path.contains("://") {
        // Already a URI - return as-is
        path.to_string()
    } else {
        // Convert local path to file:// URI with cross-platform support
        if is_absolute_path(path) {
            path_to_file_uri(path)
        } else {
            // Relative path - make it absolute first
            let absolute_path = std::env::current_dir().unwrap().join(path);
            path_to_file_uri(&absolute_path.to_string_lossy())
        }
    }
}

/// Checks if a path is absolute on the current platform.
#[must_use]
fn is_absolute_path(path: &str) -> bool {
    if path.starts_with('/') {
        // Unix absolute path
        true
    } else if path.len() >= 3
        && path.chars().nth(1) == Some(':')
        && path.chars().nth(2) == Some('\\')
    {
        // Windows drive path like C:\
        true
    } else if path.len() >= 3
        && path.chars().nth(1) == Some(':')
        && path.chars().nth(2) == Some('/')
    {
        // Windows drive path with forward slashes like C:/
        true
    } else if path.starts_with("\\\\") {
        // Windows UNC path
        true
    } else {
        false
    }
}

/// Converts an absolute path to a file:// URI with proper platform handling.
#[must_use]
fn path_to_file_uri(path: &str) -> String {
    if path.starts_with('/') {
        // Unix absolute path
        format!("file://{path}")
    } else if path.len() >= 3 && path.chars().nth(1) == Some(':') {
        // Windows drive path - normalize separators and add proper prefix
        let normalized = path.replace('\\', "/");
        format!("file:///{normalized}")
    } else if let Some(without_prefix) = path.strip_prefix("\\\\") {
        // Windows UNC path \\server\share -> file://server/share
        let normalized = without_prefix.replace('\\', "/");
        format!("file://{normalized}")
    } else {
        // Fallback - treat as relative to root
        format!("file://{path}")
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

/// Helper function to create S3 object store with options
fn create_s3_store(
    uri: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid S3 URI: missing bucket")?;

    let mut builder = object_store::aws::AmazonS3Builder::new().with_bucket_name(&bucket);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, value) in options {
            match key.as_str() {
                "endpoint_url" => {
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

/// Helper function to create GCS object store with options
fn create_gcs_store(
    uri: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid GCS URI: missing bucket")?;

    let mut builder = object_store::gcp::GoogleCloudStorageBuilder::new().with_bucket_name(&bucket);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, value) in options {
            match key.as_str() {
                "service_account_path" => {
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

/// Helper function to create Azure object store with options
fn create_azure_store(
    uri: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, _) = parse_url_and_path(uri)?;
    let container = extract_host(&url, "Invalid Azure URI: missing container")?;

    let path = url.path().trim_start_matches('/').to_string();

    let mut builder =
        object_store::azure::MicrosoftAzureBuilder::new().with_container_name(container);

    // Apply storage options if provided
    if let Some(options) = storage_options {
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
                    log::warn!("Unknown Azure storage option: {key}");
                }
            }
        }
    }

    let azure_store = builder.build()?;
    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Helper function to create Azure object store from abfs:// URI with options.
fn create_abfs_store(
    uri: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
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
                    log::warn!("Unknown ABFS storage option: {key}");
                }
            }
        }
    }

    let azure_store = builder.build()?;
    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Helper function to create HTTP object store with options.
fn create_http_store(
    uri: &str,
    storage_options: Option<std::collections::HashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let base_url = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));

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
fn parse_url_and_path(uri: &str) -> anyhow::Result<(url::Url, String)> {
    let url = url::Url::parse(uri)?;
    let path = url.path().trim_start_matches('/').to_string();
    Ok((url, path))
}

/// Helper function to extract host from URL with error handling.
fn extract_host(url: &url::Url, error_msg: &str) -> anyhow::Result<String> {
    url.host_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("{}", error_msg))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;

    use super::*;

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
    fn test_create_object_store_from_path_s3() {
        let mut options = HashMap::new();
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
    fn test_create_object_store_from_path_azure() {
        let mut options = HashMap::new();
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
    fn test_create_object_store_from_path_gcs() {
        // Test GCS without service account (will use default credentials or fail gracefully)
        let mut options = HashMap::new();
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
    fn test_create_object_store_from_path_empty_options() {
        let result = create_object_store_from_path("s3://test-bucket/path", None);
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "s3://test-bucket/path");
    }

    #[rstest]
    fn test_parse_url_and_path() {
        let result = parse_url_and_path("s3://bucket/path/to/file");
        assert!(result.is_ok());
        let (url, path) = result.unwrap();
        assert_eq!(url.scheme(), "s3");
        assert_eq!(url.host_str().unwrap(), "bucket");
        assert_eq!(path, "path/to/file");
    }

    #[rstest]
    fn test_extract_host() {
        let url = url::Url::parse("s3://test-bucket/path").unwrap();
        let result = extract_host(&url, "Test error");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-bucket");
    }

    #[rstest]
    fn test_normalize_path_to_uri() {
        // Unix absolute paths
        assert_eq!(normalize_path_to_uri("/tmp/test"), "file:///tmp/test");

        // Windows drive paths
        assert_eq!(
            normalize_path_to_uri("C:\\tmp\\test"),
            "file:///C:/tmp/test"
        );
        assert_eq!(normalize_path_to_uri("C:/tmp/test"), "file:///C:/tmp/test");
        assert_eq!(
            normalize_path_to_uri("D:\\data\\file.txt"),
            "file:///D:/data/file.txt"
        );

        // Windows UNC paths
        assert_eq!(
            normalize_path_to_uri("\\\\server\\share\\file"),
            "file://server/share/file"
        );

        // Already URIs - should remain unchanged
        assert_eq!(
            normalize_path_to_uri("s3://bucket/path"),
            "s3://bucket/path"
        );
        assert_eq!(
            normalize_path_to_uri("file:///tmp/test"),
            "file:///tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("https://example.com/path"),
            "https://example.com/path"
        );
    }

    #[rstest]
    fn test_is_absolute_path() {
        // Unix absolute paths
        assert!(is_absolute_path("/tmp/test"));
        assert!(is_absolute_path("/"));

        // Windows drive paths
        assert!(is_absolute_path("C:\\tmp\\test"));
        assert!(is_absolute_path("C:/tmp/test"));
        assert!(is_absolute_path("D:\\"));
        assert!(is_absolute_path("Z:/"));

        // Windows UNC paths
        assert!(is_absolute_path("\\\\server\\share"));
        assert!(is_absolute_path("\\\\localhost\\c$"));

        // Relative paths
        assert!(!is_absolute_path("tmp/test"));
        assert!(!is_absolute_path("./test"));
        assert!(!is_absolute_path("../test"));
        assert!(!is_absolute_path("test.txt"));

        // Edge cases
        assert!(!is_absolute_path(""));
        assert!(!is_absolute_path("C"));
        assert!(!is_absolute_path("C:"));
        assert!(!is_absolute_path("\\"));
    }

    #[rstest]
    fn test_path_to_file_uri() {
        // Unix absolute paths
        assert_eq!(path_to_file_uri("/tmp/test"), "file:///tmp/test");
        assert_eq!(path_to_file_uri("/"), "file:///");

        // Windows drive paths
        assert_eq!(path_to_file_uri("C:\\tmp\\test"), "file:///C:/tmp/test");
        assert_eq!(path_to_file_uri("C:/tmp/test"), "file:///C:/tmp/test");
        assert_eq!(path_to_file_uri("D:\\"), "file:///D:/");

        // Windows UNC paths
        assert_eq!(
            path_to_file_uri("\\\\server\\share\\file"),
            "file://server/share/file"
        );
        assert_eq!(
            path_to_file_uri("\\\\localhost\\c$\\test"),
            "file://localhost/c$/test"
        );
    }
}
