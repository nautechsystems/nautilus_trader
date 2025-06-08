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

//! Parquet data catalog for efficient storage and retrieval of financial market data.
//!
//! This module provides a comprehensive data catalog implementation that uses Apache Parquet
//! format for storing financial market data with object store backends. The catalog supports
//! various data types including quotes, trades, bars, order book data, and other market events.
//!
//! # Key Features
//!
//! - **Object Store Integration**: Works with local filesystems, S3, and other object stores
//! - **Data Type Support**: Handles all major financial data types (quotes, trades, bars, etc.)
//! - **Time-based Organization**: Organizes data by timestamp ranges for efficient querying
//! - **Consolidation**: Merges multiple files to optimize storage and query performance
//! - **Validation**: Ensures data integrity with timestamp ordering and interval validation
//!
//! # Architecture
//!
//! The catalog organizes data in a hierarchical structure:
//! ```text
//! data/
//! ├── quotes/
//! │   └── INSTRUMENT_ID/
//! │       └── start_ts-end_ts.parquet
//! ├── trades/
//! │   └── INSTRUMENT_ID/
//! │       └── start_ts-end_ts.parquet
//! └── bars/
//!     └── INSTRUMENT_ID/
//!         └── start_ts-end_ts.parquet
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::path::PathBuf;
//! use nautilus_persistence::backend::catalog::ParquetDataCatalog;
//!
//! // Create a new catalog
//! let catalog = ParquetDataCatalog::new(
//!     PathBuf::from("/path/to/data"),
//!     Some(5000),  // batch_size
//!     None,        // compression (defaults to SNAPPY)
//!     None,        // max_row_group_size (defaults to 5000)
//! );
//!
//! // Write data to the catalog
//! // catalog.write_to_parquet(data, None, None)?;
//! ```

use std::{
    fmt::Debug,
    ops::Bound,
    path::{Path, PathBuf},
    sync::Arc,
};

use datafusion::arrow::record_batch::RecordBatch;
use futures::StreamExt;
use heck::ToSnakeCase;
use itertools::Itertools;
use log::info;
use nautilus_core::{UnixNanos, datetime::unix_nanos_to_iso8601};
use nautilus_model::data::{
    Bar, Data, GetTsInit, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDepth10,
    QuoteTick, TradeTick, close::InstrumentClose,
};
use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};
use object_store::{ObjectStore, path::Path as ObjectPath};
use serde::Serialize;
use unbounded_interval_tree::interval_tree::IntervalTree;

use super::session::{self, DataBackendSession, QueryResult, build_query};
use crate::parquet::{
    combine_parquet_files_from_object_store, create_object_store_from_path,
    min_max_from_parquet_metadata_object_store, write_batches_to_object_store,
};

/// A high-performance data catalog for storing and retrieving financial market data using Apache Parquet format.
///
/// The `ParquetDataCatalog` provides a comprehensive solution for managing large volumes of financial
/// market data with efficient storage, querying, and consolidation capabilities. It supports various
/// object store backends including local filesystems, AWS S3, and other cloud storage providers.
///
/// # Features
///
/// - **Efficient Storage**: Uses Apache Parquet format with configurable compression
/// - **Object Store Backend**: Supports multiple storage backends through the `object_store` crate
/// - **Time-based Organization**: Organizes data by timestamp ranges for optimal query performance
/// - **Data Validation**: Ensures timestamp ordering and interval consistency
/// - **Consolidation**: Merges multiple files to reduce storage overhead and improve query speed
/// - **Type Safety**: Strongly typed data handling with compile-time guarantees
///
/// # Data Organization
///
/// Data is organized hierarchically by data type and instrument:
/// - `data/{data_type}/{instrument_id}/{start_ts}-{end_ts}.parquet`
/// - Files are named with their timestamp ranges for efficient range queries
/// - Intervals are validated to be disjoint to prevent data overlap
///
/// # Performance Considerations
///
/// - **Batch Size**: Controls memory usage during data processing
/// - **Compression**: SNAPPY compression provides good balance of speed and size
/// - **Row Group Size**: Affects query performance and memory usage
/// - **File Consolidation**: Reduces the number of files for better query performance
pub struct ParquetDataCatalog {
    /// The base path for data storage within the object store.
    base_path: String,
    /// The original URI provided when creating the catalog.
    original_uri: String,
    /// The object store backend for data persistence.
    object_store: Arc<dyn ObjectStore>,
    /// The DataFusion session for query execution.
    session: DataBackendSession,
    /// The number of records to process in each batch.
    batch_size: usize,
    /// The compression algorithm used for Parquet files.
    compression: parquet::basic::Compression,
    /// The maximum number of rows in each Parquet row group.
    max_row_group_size: usize,
}

impl Debug for ParquetDataCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ParquetDataCatalog))
            .field("base_path", &self.base_path)
            .finish()
    }
}

impl ParquetDataCatalog {
    /// Creates a new [`ParquetDataCatalog`] instance from a local file path.
    ///
    /// This is a convenience constructor that converts a local path to a URI format
    /// and delegates to [`Self::from_uri`].
    ///
    /// # Parameters
    ///
    /// - `base_path`: The base directory path for data storage
    /// - `batch_size`: Number of records to process in each batch (default: 5000)
    /// - `compression`: Parquet compression algorithm (default: SNAPPY)
    /// - `max_row_group_size`: Maximum rows per Parquet row group (default: 5000)
    ///
    /// # Panics
    ///
    /// Panics if the path cannot be converted to a valid URI or if the object store
    /// cannot be created from the path.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::path::PathBuf;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(
    ///     PathBuf::from("/tmp/nautilus_data"),
    ///     Some(1000),  // smaller batch size
    ///     None,        // default compression
    ///     None,        // default row group size
    /// );
    /// ```
    #[must_use]
    pub fn new(
        base_path: PathBuf,
        batch_size: Option<usize>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
    ) -> Self {
        let path_str = base_path.to_string_lossy().to_string();
        Self::from_uri(&path_str, batch_size, compression, max_row_group_size)
            .expect("Failed to create catalog from path")
    }

    /// Creates a new [`ParquetDataCatalog`] instance from a URI.
    ///
    /// Supports various URI schemes including local file paths, S3 URIs, and other
    /// object store backends supported by the `object_store` crate.
    ///
    /// # Parameters
    ///
    /// - `uri`: The URI for the data storage location (e.g., "s3://bucket/path", "/local/path")
    /// - `batch_size`: Number of records to process in each batch (default: 5000)
    /// - `compression`: Parquet compression algorithm (default: SNAPPY)
    /// - `max_row_group_size`: Maximum rows per Parquet row group (default: 5000)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The URI format is invalid or unsupported
    /// - The object store cannot be created or accessed
    /// - Authentication fails for cloud storage backends
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// // Local filesystem
    /// let local_catalog = ParquetDataCatalog::from_uri(
    ///     "/tmp/nautilus_data",
    ///     None, None, None
    /// )?;
    ///
    /// // S3 bucket
    /// let s3_catalog = ParquetDataCatalog::from_uri(
    ///     "s3://my-bucket/nautilus-data",
    ///     None, None, None
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn from_uri(
        uri: &str,
        batch_size: Option<usize>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
    ) -> anyhow::Result<Self> {
        let batch_size = batch_size.unwrap_or(5000);
        let compression = compression.unwrap_or(parquet::basic::Compression::SNAPPY);
        let max_row_group_size = max_row_group_size.unwrap_or(5000);

        let (object_store, base_path, original_uri) = create_object_store_from_path(uri)?;

        Ok(Self {
            base_path,
            original_uri,
            object_store,
            session: session::DataBackendSession::new(batch_size),
            batch_size,
            compression,
            max_row_group_size,
        })
    }

    /// Writes mixed data types to the catalog by separating them into type-specific collections.
    ///
    /// This method takes a heterogeneous collection of market data and separates it by type,
    /// then writes each type to its appropriate location in the catalog. This is useful when
    /// processing mixed data streams or bulk data imports.
    ///
    /// # Parameters
    ///
    /// - `data`: A vector of mixed [`Data`] enum variants
    /// - `start`: Optional start timestamp to override the data's natural range
    /// - `end`: Optional end timestamp to override the data's natural range
    ///
    /// # Notes
    ///
    /// - Data is automatically sorted by type before writing
    /// - Each data type is written to its own directory structure
    /// - Errors during individual type writes are currently ignored (TODO: improve error handling)
    /// - Instrument data handling is not yet implemented (TODO)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::data::Data;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let mixed_data: Vec<Data> = vec![/* mixed data types */];
    ///
    /// catalog.write_data_enum(mixed_data, None, None);
    /// ```
    pub fn write_data_enum(
        &self,
        data: Vec<Data>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) {
        let mut deltas: Vec<OrderBookDelta> = Vec::new();
        let mut depth10s: Vec<OrderBookDepth10> = Vec::new();
        let mut quotes: Vec<QuoteTick> = Vec::new();
        let mut trades: Vec<TradeTick> = Vec::new();
        let mut bars: Vec<Bar> = Vec::new();
        let mut mark_prices: Vec<MarkPriceUpdate> = Vec::new();
        let mut index_prices: Vec<IndexPriceUpdate> = Vec::new();
        let mut closes: Vec<InstrumentClose> = Vec::new();

        for d in data.iter().cloned() {
            match d {
                Data::Deltas(_) => continue,
                Data::Delta(d) => {
                    deltas.push(d);
                }
                Data::Depth10(d) => {
                    depth10s.push(*d);
                }
                Data::Quote(d) => {
                    quotes.push(d);
                }
                Data::Trade(d) => {
                    trades.push(d);
                }
                Data::Bar(d) => {
                    bars.push(d);
                }
                Data::MarkPriceUpdate(p) => {
                    mark_prices.push(p);
                }
                Data::IndexPriceUpdate(p) => {
                    index_prices.push(p);
                }
                Data::InstrumentClose(c) => {
                    closes.push(c);
                }
            }
        }

        // TODO: need to handle instruments here
        let _ = self.write_to_parquet(deltas, start, end);
        let _ = self.write_to_parquet(depth10s, start, end);
        let _ = self.write_to_parquet(quotes, start, end);
        let _ = self.write_to_parquet(trades, start, end);
        let _ = self.write_to_parquet(bars, start, end);
        let _ = self.write_to_parquet(mark_prices, start, end);
        let _ = self.write_to_parquet(index_prices, start, end);
        let _ = self.write_to_parquet(closes, start, end);
    }

    /// Writes typed data to a Parquet file in the catalog.
    ///
    /// This is the core method for persisting market data to the catalog. It handles data
    /// validation, batching, compression, and ensures proper file organization with
    /// timestamp-based naming.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to write, must implement required traits for serialization and cataloging
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to write (must be in ascending timestamp order)
    /// - `start`: Optional start timestamp to override the natural data range
    /// - `end`: Optional end timestamp to override the natural data range
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created file, or an empty path if no data was provided.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Data serialization to Arrow record batches fails
    /// - Object store write operations fail
    /// - File path construction fails
    /// - Timestamp interval validation fails after writing
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Data timestamps are not in ascending order
    /// - Record batches are empty after conversion
    /// - Required metadata is missing from the schema
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::data::QuoteTick;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let quotes: Vec<QuoteTick> = vec![/* quote data */];
    ///
    /// let path = catalog.write_to_parquet(quotes, None, None)?;
    /// println!("Data written to: {:?}", path);
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_to_parquet<T>(
        &self,
        data: Vec<T>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<PathBuf>
    where
        T: GetTsInit + EncodeToRecordBatch + CatalogPathPrefix,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let start_ts = start.unwrap_or(data.first().unwrap().ts_init());
        let end_ts = end.unwrap_or(data.last().unwrap().ts_init());

        let batches = self.data_to_record_batches(data)?;
        let schema = batches.first().expect("Batches are empty.").schema();
        let instrument_id = schema.metadata.get("instrument_id").cloned();

        let directory = self.make_path(T::path_prefix(), instrument_id)?;
        let filename = timestamps_to_filename(start_ts, end_ts);
        let path = PathBuf::from(format!("{}/{}", directory, filename));

        // Write all batches to parquet file
        info!(
            "Writing {} batches of {type_name} data to {path:?}",
            batches.len()
        );

        // Convert path to object store path
        let object_path = self.to_object_path(&path.to_string_lossy());

        // Use async runtime to write to object store
        self.execute_async(async {
            write_batches_to_object_store(
                &batches,
                self.object_store.clone(),
                &object_path,
                Some(self.compression),
                Some(self.max_row_group_size),
            )
            .await
        })?;
        let intervals = self.get_directory_intervals(&directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after writing a new file");
        }

        Ok(path)
    }

    /// Writes typed data to a JSON file in the catalog.
    ///
    /// This method provides an alternative to Parquet format for data export and debugging.
    /// JSON files are human-readable but less efficient for large datasets.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to write, must implement serialization and cataloging traits
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to write (must be in ascending timestamp order)
    /// - `path`: Optional custom directory path (defaults to catalog's standard structure)
    /// - `write_metadata`: Whether to write a separate metadata file alongside the data
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created JSON file.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - JSON serialization fails
    /// - Object store write operations fail
    /// - File path construction fails
    ///
    /// # Panics
    ///
    /// Panics if data timestamps are not in ascending order.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::path::PathBuf;
    /// use nautilus_model::data::TradeTick;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let trades: Vec<TradeTick> = vec![/* trade data */];
    ///
    /// let path = catalog.write_to_json(
    ///     trades,
    ///     Some(PathBuf::from("/custom/path")),
    ///     true  // write metadata
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_to_json<T>(
        &self,
        data: Vec<T>,
        path: Option<PathBuf>,
        write_metadata: bool,
    ) -> anyhow::Result<PathBuf>
    where
        T: GetTsInit + Serialize + CatalogPathPrefix + EncodeToRecordBatch,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let start_ts = data.first().unwrap().ts_init();
        let end_ts = data.last().unwrap().ts_init();

        let directory =
            path.unwrap_or_else(|| PathBuf::from(self.make_path(T::path_prefix(), None).unwrap()));
        let filename = timestamps_to_filename(start_ts, end_ts).replace(".parquet", ".json");
        let json_path = directory.join(&filename);

        info!(
            "Writing {} records of {type_name} data to {json_path:?}",
            data.len()
        );

        if write_metadata {
            let metadata = T::chunk_metadata(&data);
            let metadata_path = json_path.with_extension("metadata.json");
            info!("Writing metadata to {metadata_path:?}");

            // Use object store for metadata file
            let metadata_object_path = ObjectPath::from(metadata_path.to_string_lossy().as_ref());
            let metadata_json = serde_json::to_vec_pretty(&metadata)?;
            self.execute_async(async {
                self.object_store
                    .put(&metadata_object_path, metadata_json.into())
                    .await
                    .map_err(anyhow::Error::from)
            })?;
        }

        // Use object store for main JSON file
        let json_object_path = ObjectPath::from(json_path.to_string_lossy().as_ref());
        let json_data = serde_json::to_vec_pretty(&serde_json::to_value(data)?)?;
        self.execute_async(async {
            self.object_store
                .put(&json_object_path, json_data.into())
                .await
                .map_err(anyhow::Error::from)
        })?;

        Ok(json_path)
    }

    /// Validates that data timestamps are in ascending order.
    ///
    /// # Parameters
    ///
    /// - `data`: Slice of data records to validate
    /// - `type_name`: Name of the data type for error messages
    ///
    /// # Panics
    ///
    /// Panics if any timestamp is less than the previous timestamp.
    fn check_ascending_timestamps<T: GetTsInit>(data: &[T], type_name: &str) {
        assert!(
            data.windows(2).all(|w| w[0].ts_init() <= w[1].ts_init()),
            "{type_name} timestamps must be in ascending order"
        );
    }

    /// Converts data into Arrow record batches for Parquet serialization.
    ///
    /// This method chunks the data according to the configured batch size and converts
    /// each chunk into an Arrow record batch with appropriate metadata.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to convert, must implement required encoding traits
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to convert
    ///
    /// # Returns
    ///
    /// Returns a vector of Arrow [`RecordBatch`] instances ready for Parquet serialization.
    ///
    /// # Errors
    ///
    /// Returns an error if record batch encoding fails for any chunk.
    pub fn data_to_record_batches<T>(&self, data: Vec<T>) -> anyhow::Result<Vec<RecordBatch>>
    where
        T: GetTsInit + EncodeToRecordBatch,
    {
        let mut batches = Vec::new();

        for chunk in &data.into_iter().chunks(self.batch_size) {
            let data = chunk.collect_vec();
            let metadata = EncodeToRecordBatch::chunk_metadata(&data);
            let record_batch = T::encode_batch(&metadata, &data)?;
            batches.push(record_batch);
        }

        Ok(batches)
    }

    /// Extend the timestamp range of an existing parquet file by renaming it
    pub fn extend_file_name(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(data_cls, instrument_id)?;
        let intervals = self.get_directory_intervals(&directory)?;

        let start = start.as_u64();
        let end = end.as_u64();

        for interval in intervals {
            if interval.0 == end + 1 {
                // Extend backwards: new file covers [start, interval.1]
                self.rename_parquet_file(&directory, interval.0, interval.1, start, interval.1)?;
                break;
            } else if interval.1 == start - 1 {
                // Extend forwards: new file covers [interval.0, end]
                self.rename_parquet_file(&directory, interval.0, interval.1, interval.0, end)?;
                break;
            }
        }

        let intervals = self.get_directory_intervals(&directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after extending a file");
        }

        Ok(())
    }

    /// Helper method to list parquet files in a directory
    fn list_parquet_files(&self, directory: &str) -> anyhow::Result<Vec<String>> {
        self.execute_async(async {
            let prefix = ObjectPath::from(format!("{}/", directory));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut files = Vec::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                if object.location.as_ref().ends_with(".parquet") {
                    files.push(object.location.to_string());
                }
            }
            Ok::<Vec<String>, anyhow::Error>(files)
        })
    }

    /// Helper method to reconstruct full URI for S3 paths
    fn reconstruct_full_uri(&self, path_str: &str) -> String {
        if self.original_uri.starts_with("s3://") {
            // Extract bucket from the original URI
            let url = url::Url::parse(&self.original_uri).unwrap();
            let bucket = url.host_str().unwrap();
            format!("s3://{}/{}", bucket, path_str)
        } else {
            path_str.to_string()
        }
    }

    pub fn consolidate_catalog(
        &self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.consolidate_directory(&directory, start, end, ensure_contiguous_files)?;
        }

        Ok(())
    }

    pub fn consolidate_data(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(type_name, instrument_id)?;
        self.consolidate_directory(&directory, start, end, ensure_contiguous_files)
    }

    fn consolidate_directory(
        &self,
        directory: &str,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let parquet_files = self.list_parquet_files(directory)?;

        if parquet_files.len() <= 1 {
            return Ok(());
        }

        let mut files_to_consolidate = Vec::new();
        let mut intervals = Vec::new();
        let start = start.map(|t| t.as_u64());
        let end = end.map(|t| t.as_u64());

        for file in parquet_files {
            if let Some(interval) = parse_filename_timestamps(&file) {
                let (interval_start, interval_end) = interval;
                let include_file = match (start, end) {
                    (Some(s), Some(e)) => interval_start >= s && interval_end <= e,
                    (Some(s), None) => interval_start >= s,
                    (None, Some(e)) => interval_end <= e,
                    (None, None) => true,
                };

                if include_file {
                    files_to_consolidate.push(file);
                    intervals.push(interval);
                }
            }
        }

        intervals.sort_by_key(|&(start, _)| start);

        if !intervals.is_empty() {
            let file_name = timestamps_to_filename(
                UnixNanos::from(intervals[0].0),
                UnixNanos::from(intervals.last().unwrap().1),
            );
            let path = format!("{}/{}", directory, file_name);

            // Convert string paths to ObjectPath for the function call
            let object_paths: Vec<ObjectPath> = files_to_consolidate
                .iter()
                .map(|path| ObjectPath::from(path.as_str()))
                .collect();

            self.execute_async(async {
                combine_parquet_files_from_object_store(
                    self.object_store.clone(),
                    object_paths,
                    &ObjectPath::from(path),
                    Some(self.compression),
                    Some(self.max_row_group_size),
                )
                .await
            })?;
        }

        if ensure_contiguous_files.unwrap_or(true) && !are_intervals_contiguous(&intervals) {
            anyhow::bail!("Intervals are not disjoint after consolidating a directory");
        }

        Ok(())
    }

    /// Reset the filenames of parquet files to match their actual content timestamps
    pub fn reset_catalog_file_names(&self) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.reset_file_names(&directory)?;
        }

        Ok(())
    }

    /// Reset the filenames of parquet files for a specific data type and instrument ID
    pub fn reset_data_file_names(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(data_cls, instrument_id)?;
        self.reset_file_names(&directory)
    }

    /// Reset the filenames of parquet files in a directory
    fn reset_file_names(&self, directory: &str) -> anyhow::Result<()> {
        let parquet_files = self.list_parquet_files(directory)?;

        for file in parquet_files {
            let object_path = ObjectPath::from(file.as_str());
            let (first_ts, last_ts) = self.execute_async(async {
                min_max_from_parquet_metadata_object_store(
                    self.object_store.clone(),
                    &object_path,
                    "ts_init",
                )
                .await
            })?;

            let new_filename =
                timestamps_to_filename(UnixNanos::from(first_ts), UnixNanos::from(last_ts));
            let new_file_path = format!("{}/{}", directory, new_filename);
            let new_object_path = ObjectPath::from(new_file_path);

            self.move_file(&object_path, &new_object_path)?;
        }

        let intervals = self.get_directory_intervals(directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after resetting file names");
        }

        Ok(())
    }

    pub fn find_leaf_data_directories(&self) -> anyhow::Result<Vec<String>> {
        let data_dir = if self.base_path.is_empty() {
            "data".to_string()
        } else {
            format!("{}/data", self.base_path)
        };

        let leaf_dirs = self.execute_async(async {
            let mut all_paths = std::collections::HashSet::new();
            let mut directories = std::collections::HashSet::new();
            let mut files_in_dirs = std::collections::HashMap::new();

            // List all objects under the data directory
            let prefix = ObjectPath::from(format!("{}/", data_dir));
            let mut stream = self.object_store.list(Some(&prefix));

            while let Some(object) = stream.next().await {
                let object = object?;
                let path_str = object.location.to_string();
                all_paths.insert(path_str.clone());

                // Extract directory path
                if let Some(parent) = std::path::Path::new(&path_str).parent() {
                    let parent_str = parent.to_string_lossy().to_string();
                    directories.insert(parent_str.clone());

                    // Track files in each directory
                    files_in_dirs
                        .entry(parent_str)
                        .or_insert_with(Vec::new)
                        .push(path_str);
                }
            }

            // Find leaf directories (directories with files but no subdirectories)
            let mut leaf_dirs = Vec::new();
            for dir in &directories {
                let has_files = files_in_dirs
                    .get(dir)
                    .is_some_and(|files| !files.is_empty());
                let has_subdirs = directories
                    .iter()
                    .any(|d| d.starts_with(&format!("{}/", dir)) && d != dir);

                if has_files && !has_subdirs {
                    leaf_dirs.push(dir.clone());
                }
            }

            Ok::<Vec<String>, anyhow::Error>(leaf_dirs)
        })?;

        Ok(leaf_dirs)
    }

    /// Query data loaded in the catalog
    pub fn query<T>(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> anyhow::Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        // Register the object store with the session
        if self.original_uri.starts_with("s3://") {
            let url = url::Url::parse(&self.original_uri)?;
            let base_url =
                url::Url::parse(&format!("{}://{}", url.scheme(), url.host_str().unwrap()))?;
            self.session
                .register_object_store(&base_url, self.object_store.clone());
        }

        let files_list = self.query_files(T::path_prefix(), instrument_ids, start, end)?;

        for (idx, file_uri) in files_list.iter().enumerate() {
            let table_name = format!("{}_{}", T::path_prefix(), idx);
            let query = build_query(&table_name, start, end, where_clause);

            self.session
                .add_file::<T>(&table_name, file_uri, Some(&query))?;
        }

        Ok(self.session.get_query_result())
    }

    /// Query all parquet files for a specific data type and instrument ID
    pub fn query_files(
        &self,
        data_cls: &str,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<String>> {
        let mut files = Vec::new();

        let start_u64 = start.map(|s| s.as_u64());
        let end_u64 = end.map(|e| e.as_u64());

        let safe_ids = instrument_ids.as_ref().map(|ids| {
            ids.iter()
                .map(|id| urisafe_instrument_id(id))
                .collect::<Vec<String>>()
        });

        let base_dir = self.make_path(data_cls, None)?;

        let list_result = self.execute_async(async {
            let prefix = ObjectPath::from(format!("{}/", base_dir));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }
            Ok::<Vec<_>, anyhow::Error>(objects)
        })?;

        for object in list_result {
            let path_str = object.location.to_string();
            if path_str.ends_with(".parquet") {
                if let Some(ids) = &safe_ids {
                    let matches_any_id = ids.iter().any(|safe_id| path_str.contains(safe_id));
                    if !matches_any_id {
                        continue;
                    }
                }

                if query_intersects_filename(&path_str, start_u64, end_u64) {
                    let full_uri = self.reconstruct_full_uri(&path_str);
                    files.push(full_uri);
                }
            }
        }

        Ok(files)
    }

    /// Find the missing time intervals for a specific data type and instrument ID
    pub fn get_missing_intervals_for_request(
        &self,
        start: u64,
        end: u64,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        let intervals = self.get_intervals(data_cls, instrument_id)?;

        Ok(query_interval_diff(start, end, &intervals))
    }

    /// Get the last timestamp for a specific data type and instrument ID
    pub fn query_last_timestamp(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Option<u64>> {
        let intervals = self.get_intervals(data_cls, instrument_id)?;

        if intervals.is_empty() {
            return Ok(None);
        }

        Ok(Some(intervals.last().unwrap().1))
    }

    /// Get the time intervals covered by parquet files for a specific data type and instrument ID
    pub fn get_intervals(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        let directory = self.make_path(data_cls, instrument_id)?;

        self.get_directory_intervals(&directory)
    }

    /// Get the time intervals covered by parquet files in a directory
    fn get_directory_intervals(&self, directory: &str) -> anyhow::Result<Vec<(u64, u64)>> {
        let mut intervals = Vec::new();

        // Use object store for all operations
        let list_result = self.execute_async(async {
            let path = object_store::path::Path::from(directory);
            Ok(self
                .object_store
                .list(Some(&path))
                .collect::<Vec<_>>()
                .await)
        })?;

        for result in list_result {
            match result {
                Ok(object) => {
                    let path_str = object.location.to_string();
                    if path_str.ends_with(".parquet") {
                        if let Some(interval) = parse_filename_timestamps(&path_str) {
                            intervals.push(interval);
                        }
                    }
                }
                Err(_) => {
                    // Directory doesn't exist or is empty, which is fine
                    break;
                }
            }
        }

        intervals.sort_by_key(|&(start, _)| start);

        Ok(intervals)
    }

    /// Create a directory path for a data type and instrument ID
    fn make_path(&self, type_name: &str, instrument_id: Option<String>) -> anyhow::Result<String> {
        let mut path = if self.base_path.is_empty() {
            format!("data/{}", type_name)
        } else {
            format!("{}/data/{}", self.base_path, type_name)
        };

        if let Some(id) = instrument_id {
            path = format!("{}/{}", path, urisafe_instrument_id(&id));
        }

        Ok(path)
    }

    /// Helper method to rename a parquet file by moving it via object store operations
    fn rename_parquet_file(
        &self,
        directory: &str,
        old_start: u64,
        old_end: u64,
        new_start: u64,
        new_end: u64,
    ) -> anyhow::Result<()> {
        let old_filename =
            timestamps_to_filename(UnixNanos::from(old_start), UnixNanos::from(old_end));
        let old_path = format!("{}/{}", directory, old_filename);
        let old_object_path = self.to_object_path(&old_path);

        let new_filename =
            timestamps_to_filename(UnixNanos::from(new_start), UnixNanos::from(new_end));
        let new_path = format!("{}/{}", directory, new_filename);
        let new_object_path = self.to_object_path(&new_path);

        self.move_file(&old_object_path, &new_object_path)
    }

    /// Helper method to convert a path string to ObjectPath, handling base_path
    fn to_object_path(&self, path: &str) -> ObjectPath {
        if self.base_path.is_empty() {
            ObjectPath::from(path)
        } else {
            ObjectPath::from(
                path.strip_prefix(&format!("{}/", self.base_path))
                    .unwrap_or(path),
            )
        }
    }

    /// Helper method to move a file using object store rename operation
    fn move_file(&self, old_path: &ObjectPath, new_path: &ObjectPath) -> anyhow::Result<()> {
        self.execute_async(async {
            self.object_store
                .rename(old_path, new_path)
                .await
                .map_err(anyhow::Error::from)
        })
    }

    /// Helper method to execute async operations with a runtime
    fn execute_async<F, R>(&self, future: F) -> anyhow::Result<R>
    where
        F: std::future::Future<Output = anyhow::Result<R>>,
    {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(future)
    }
}

/// Trait for providing catalog path prefixes for different data types.
///
/// This trait enables type-safe organization of data within the catalog by providing
/// a standardized way to determine the directory structure for each data type.
/// Each data type maps to a specific subdirectory within the catalog's data folder.
///
/// # Implementation
///
/// Types implementing this trait should return a static string that represents
/// the directory name where data of that type should be stored.
///
/// # Examples
///
/// ```rust
/// use nautilus_persistence::backend::catalog::CatalogPathPrefix;
/// use nautilus_model::data::QuoteTick;
///
/// assert_eq!(QuoteTick::path_prefix(), "quotes");
/// ```
pub trait CatalogPathPrefix {
    /// Returns the path prefix (directory name) for this data type.
    ///
    /// # Returns
    ///
    /// A static string representing the directory name where this data type is stored.
    fn path_prefix() -> &'static str;
}

/// Macro for implementing [`CatalogPathPrefix`] for data types.
///
/// This macro provides a convenient way to implement the trait for multiple types
/// with their corresponding path prefixes.
///
/// # Parameters
///
/// - `$type`: The data type to implement the trait for
/// - `$path`: The path prefix string for that type
macro_rules! impl_catalog_path_prefix {
    ($type:ty, $path:expr) => {
        impl CatalogPathPrefix for $type {
            fn path_prefix() -> &'static str {
                $path
            }
        }
    };
}

// Standard implementations for financial data types
impl_catalog_path_prefix!(QuoteTick, "quotes");
impl_catalog_path_prefix!(TradeTick, "trades");
impl_catalog_path_prefix!(OrderBookDelta, "order_book_deltas");
impl_catalog_path_prefix!(OrderBookDepth10, "order_book_depths");
impl_catalog_path_prefix!(Bar, "bars");
impl_catalog_path_prefix!(IndexPriceUpdate, "index_prices");
impl_catalog_path_prefix!(MarkPriceUpdate, "mark_prices");
impl_catalog_path_prefix!(InstrumentClose, "instrument_closes");

////////////////////////////////////////////////////////////////////////////////
// Helper functions for filename operations
////////////////////////////////////////////////////////////////////////////////

/// Converts timestamps to a filename using ISO 8601 format.
///
/// This function converts two Unix nanosecond timestamps to a filename that uses
/// ISO 8601 format with filesystem-safe characters. The format matches the Python
/// implementation for consistency.
///
/// # Parameters
///
/// - `timestamp_1`: First timestamp in Unix nanoseconds
/// - `timestamp_2`: Second timestamp in Unix nanoseconds
///
/// # Returns
///
/// Returns a filename string in the format: "iso_timestamp_1_iso_timestamp_2.parquet"
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::timestamps_to_filename;
/// # use nautilus_core::UnixNanos;
/// let filename = timestamps_to_filename(
///     UnixNanos::from(1609459200000000000),
///     UnixNanos::from(1609545600000000000)
/// );
/// // Returns something like: "2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet"
/// ```
fn timestamps_to_filename(timestamp_1: UnixNanos, timestamp_2: UnixNanos) -> String {
    let datetime_1 = iso_timestamp_to_file_timestamp(&unix_nanos_to_iso8601(timestamp_1));
    let datetime_2 = iso_timestamp_to_file_timestamp(&unix_nanos_to_iso8601(timestamp_2));

    format!("{}_{}.parquet", datetime_1, datetime_2)
}

/// Converts an ISO 8601 timestamp to a filesystem-safe format.
///
/// This function replaces colons and dots with hyphens to make the timestamp
/// safe for use in filenames across different filesystems.
///
/// # Parameters
///
/// - `iso_timestamp`: ISO 8601 timestamp string (e.g., "2023-10-26T07:30:50.123456789Z")
///
/// # Returns
///
/// Returns a filesystem-safe timestamp string (e.g., "2023-10-26T07-30-50-123456789Z")
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::iso_timestamp_to_file_timestamp;
/// let safe_timestamp = iso_timestamp_to_file_timestamp("2023-10-26T07:30:50.123456789Z");
/// assert_eq!(safe_timestamp, "2023-10-26T07-30-50-123456789Z");
/// ```
fn iso_timestamp_to_file_timestamp(iso_timestamp: &str) -> String {
    iso_timestamp.replace([':', '.'], "-")
}

/// Converts a filesystem-safe timestamp back to ISO 8601 format.
///
/// This function reverses the transformation done by `iso_timestamp_to_file_timestamp`,
/// converting filesystem-safe timestamps back to standard ISO 8601 format.
///
/// # Parameters
///
/// - `file_timestamp`: Filesystem-safe timestamp string (e.g., "2023-10-26T07-30-50-123456789Z")
///
/// # Returns
///
/// Returns an ISO 8601 timestamp string (e.g., "2023-10-26T07:30:50.123456789Z")
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::file_timestamp_to_iso_timestamp;
/// let iso_timestamp = file_timestamp_to_iso_timestamp("2023-10-26T07-30-50-123456789Z");
/// assert_eq!(iso_timestamp, "2023-10-26T07:30:50.123456789Z");
/// ```
fn file_timestamp_to_iso_timestamp(file_timestamp: &str) -> String {
    let (date_part, time_part) = file_timestamp
        .split_once('T')
        .unwrap_or((file_timestamp, ""));
    let time_part = time_part.strip_suffix('Z').unwrap_or(time_part);

    // Find the last hyphen to separate nanoseconds
    if let Some(last_hyphen_idx) = time_part.rfind('-') {
        let time_with_dot_for_nanos = format!(
            "{}.{}",
            &time_part[..last_hyphen_idx],
            &time_part[last_hyphen_idx + 1..]
        );
        let final_time_part = time_with_dot_for_nanos.replace('-', ":");
        format!("{}T{}Z", date_part, final_time_part)
    } else {
        // Fallback if no nanoseconds part found
        let final_time_part = time_part.replace('-', ":");
        format!("{}T{}Z", date_part, final_time_part)
    }
}

/// Converts an ISO 8601 timestamp string to Unix nanoseconds.
///
/// This function parses an ISO 8601 timestamp and converts it to Unix nanoseconds.
/// It's used to convert parsed timestamps back to the internal representation.
///
/// # Parameters
///
/// - `iso_timestamp`: ISO 8601 timestamp string (e.g., "2023-10-26T07:30:50.123456789Z")
///
/// # Returns
///
/// Returns `Ok(u64)` with the Unix nanoseconds timestamp, or an error if parsing fails.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::iso_to_unix_nanos;
/// let nanos = iso_to_unix_nanos("2021-01-01T00:00:00.000000000Z").unwrap();
/// assert_eq!(nanos, 1609459200000000000);
/// ```
fn iso_to_unix_nanos(iso_timestamp: &str) -> Result<u64, anyhow::Error> {
    use chrono::{DateTime, Utc};

    let dt = DateTime::parse_from_rfc3339(iso_timestamp)
        .map_err(|e| anyhow::anyhow!("Failed to parse ISO timestamp '{}': {}", iso_timestamp, e))?;

    let utc_dt = dt.with_timezone(&Utc);
    let nanos = utc_dt
        .timestamp_nanos_opt()
        .ok_or_else(|| anyhow::anyhow!("Timestamp out of range: {}", iso_timestamp))?;

    u64::try_from(nanos).map_err(|_| anyhow::anyhow!("Negative timestamp: {}", nanos))
}

////////////////////////////////////////////////////////////////////////////////
// Helper functions for interval operations
////////////////////////////////////////////////////////////////////////////////

/// Converts an instrument ID to a URI-safe format by removing forward slashes.
///
/// Some instrument IDs contain forward slashes (e.g., "BTC/USD") which are not
/// suitable for use in file paths. This function removes these characters to
/// create a safe directory name.
///
/// # Parameters
///
/// - `instrument_id`: The original instrument ID string
///
/// # Returns
///
/// A URI-safe version of the instrument ID with forward slashes removed.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::urisafe_instrument_id;
/// assert_eq!(urisafe_instrument_id("BTC/USD"), "BTCUSD");
/// assert_eq!(urisafe_instrument_id("EUR-USD"), "EUR-USD");
/// ```
fn urisafe_instrument_id(instrument_id: &str) -> String {
    instrument_id.replace("/", "")
}

/// Checks if a filename's timestamp range intersects with a query interval.
///
/// This function determines whether a Parquet file (identified by its timestamp-based
/// filename) contains data that falls within the specified query time range.
///
/// # Parameters
///
/// - `filename`: The filename to check (format: "iso_timestamp_1_iso_timestamp_2.parquet")
/// - `start`: Optional start timestamp for the query range
/// - `end`: Optional end timestamp for the query range
///
/// # Returns
///
/// Returns `true` if the file's time range intersects with the query range,
/// `false` otherwise. Returns `true` if the filename cannot be parsed.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::query_intersects_filename;
/// // Example with ISO format filenames
/// assert!(query_intersects_filename("2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet", Some(1609459200000000000), Some(1609545600000000000)));
/// ```
fn query_intersects_filename(filename: &str, start: Option<u64>, end: Option<u64>) -> bool {
    if let Some((file_start, file_end)) = parse_filename_timestamps(filename) {
        (start.is_none() || start.unwrap() <= file_end)
            && (end.is_none() || file_start <= end.unwrap())
    } else {
        true
    }
}

/// Parses timestamps from a Parquet filename.
///
/// Extracts the start and end timestamps from filenames that follow the ISO 8601 format:
/// "iso_timestamp_1_iso_timestamp_2.parquet" (e.g., "2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet")
///
/// # Parameters
///
/// - `filename`: The filename to parse (can be a full path)
///
/// # Returns
///
/// Returns `Some((start_ts, end_ts))` if the filename matches the expected format,
/// `None` otherwise.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::parse_filename_timestamps;
/// assert!(parse_filename_timestamps("2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet").is_some());
/// assert_eq!(parse_filename_timestamps("invalid.parquet"), None);
/// ```
fn parse_filename_timestamps(filename: &str) -> Option<(u64, u64)> {
    let path = Path::new(filename);
    let base_name = path.file_name()?.to_str()?;
    let base_filename = base_name.strip_suffix(".parquet")?;
    let (first_part, second_part) = base_filename.split_once('_')?;

    let first_iso = file_timestamp_to_iso_timestamp(first_part);
    let second_iso = file_timestamp_to_iso_timestamp(second_part);

    let first_ts = iso_to_unix_nanos(&first_iso).ok()?;
    let second_ts = iso_to_unix_nanos(&second_iso).ok()?;

    Some((first_ts, second_ts))
}

/// Checks if a list of closed integer intervals are all mutually disjoint.
///
/// Two intervals are disjoint if they do not overlap. This function validates that
/// all intervals in the list are non-overlapping, which is a requirement for
/// maintaining data integrity in the catalog.
///
/// # Parameters
///
/// - `intervals`: A slice of timestamp intervals as (start, end) tuples
///
/// # Returns
///
/// Returns `true` if all intervals are disjoint, `false` if any overlap is found.
/// Returns `true` for empty lists or lists with a single interval.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::are_intervals_disjoint;
/// // Disjoint intervals
/// assert!(are_intervals_disjoint(&[(1, 5), (10, 15), (20, 25)]));
///
/// // Overlapping intervals
/// assert!(!are_intervals_disjoint(&[(1, 10), (5, 15)]));
/// ```
fn are_intervals_disjoint(intervals: &[(u64, u64)]) -> bool {
    let n = intervals.len();

    if n <= 1 {
        return true;
    }

    let mut sorted_intervals: Vec<(u64, u64)> = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);

    for i in 0..(n - 1) {
        let (_, end1) = sorted_intervals[i];
        let (start2, _) = sorted_intervals[i + 1];

        if end1 >= start2 {
            return false;
        }
    }

    true
}

/// Checks if intervals are contiguous (adjacent with no gaps).
///
/// Intervals are contiguous if, when sorted by start time, each interval's start
/// timestamp is exactly one more than the previous interval's end timestamp.
/// This ensures complete coverage of a time range with no gaps.
///
/// # Parameters
///
/// - `intervals`: A slice of timestamp intervals as (start, end) tuples
///
/// # Returns
///
/// Returns `true` if all intervals are contiguous, `false` if any gaps are found.
/// Returns `true` for empty lists or lists with a single interval.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::are_intervals_contiguous;
/// // Contiguous intervals
/// assert!(are_intervals_contiguous(&[(1, 5), (6, 10), (11, 15)]));
///
/// // Non-contiguous intervals (gap between 5 and 8)
/// assert!(!are_intervals_contiguous(&[(1, 5), (8, 10)]));
/// ```
fn are_intervals_contiguous(intervals: &[(u64, u64)]) -> bool {
    let n = intervals.len();
    if n <= 1 {
        return true;
    }

    let mut sorted_intervals: Vec<(u64, u64)> = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);

    for i in 0..(n - 1) {
        let (_, end1) = sorted_intervals[i];
        let (start2, _) = sorted_intervals[i + 1];

        if end1 + 1 != start2 {
            return false;
        }
    }

    true
}

/// Finds the parts of a query interval that are not covered by existing data intervals.
///
/// This function calculates the "gaps" in data coverage by comparing a requested
/// time range against the intervals covered by existing data files. It's used to
/// determine what data needs to be fetched or backfilled.
///
/// # Parameters
///
/// - `start`: Start timestamp of the query interval (inclusive)
/// - `end`: End timestamp of the query interval (inclusive)
/// - `closed_intervals`: Existing data intervals as (start, end) tuples
///
/// # Returns
///
/// Returns a vector of (start, end) tuples representing the gaps in coverage.
/// Returns an empty vector if the query range is invalid or fully covered.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::query_interval_diff;
/// // Query 1-100, have data for 10-30 and 60-80
/// let gaps = query_interval_diff(1, 100, &[(10, 30), (60, 80)]);
/// assert_eq!(gaps, vec![(1, 9), (31, 59), (81, 100)]);
/// ```
fn query_interval_diff(start: u64, end: u64, closed_intervals: &[(u64, u64)]) -> Vec<(u64, u64)> {
    if start > end {
        return Vec::new();
    }

    let interval_set = get_interval_set(closed_intervals);
    let query_range = (Bound::Included(start), Bound::Included(end));
    let query_diff = interval_set.get_interval_difference(&query_range);
    let mut result: Vec<(u64, u64)> = Vec::new();

    for interval in query_diff {
        if let Some(tuple) = interval_to_tuple(interval, start, end) {
            result.push(tuple);
        }
    }

    result
}

/// Creates an interval tree from closed integer intervals.
///
/// This function converts closed intervals [a, b] into half-open intervals [a, b+1)
/// for use with the interval tree data structure, which is used for efficient
/// interval operations and gap detection.
///
/// # Parameters
///
/// - `intervals`: A slice of closed intervals as (start, end) tuples
///
/// # Returns
///
/// Returns an [`IntervalTree`] containing the converted intervals.
///
/// # Notes
///
/// - Invalid intervals (where start > end) are skipped
/// - Uses saturating addition to prevent overflow when converting to half-open intervals
fn get_interval_set(intervals: &[(u64, u64)]) -> IntervalTree<u64> {
    let mut tree = IntervalTree::default();

    if intervals.is_empty() {
        return tree;
    }

    for &(start, end) in intervals {
        if start > end {
            continue;
        }

        tree.insert((
            Bound::Included(start),
            Bound::Excluded(end.saturating_add(1)),
        ));
    }

    tree
}

/// Converts an interval tree result back to a closed interval tuple.
///
/// This helper function converts the bounded interval representation used by
/// the interval tree back into the (start, end) tuple format used throughout
/// the catalog.
///
/// # Parameters
///
/// - `interval`: The bounded interval from the interval tree
/// - `query_start`: The start of the original query range
/// - `query_end`: The end of the original query range
///
/// # Returns
///
/// Returns `Some((start, end))` for valid intervals, `None` for empty intervals.
fn interval_to_tuple(
    interval: (Bound<&u64>, Bound<&u64>),
    query_start: u64,
    query_end: u64,
) -> Option<(u64, u64)> {
    let (bound_start, bound_end) = interval;

    let start = match bound_start {
        Bound::Included(val) => *val,
        Bound::Excluded(val) => val.saturating_add(1),
        Bound::Unbounded => query_start,
    };

    let end = match bound_end {
        Bound::Included(val) => *val,
        Bound::Excluded(val) => {
            if *val == 0 {
                return None; // Empty interval
            }
            val - 1
        }
        Bound::Unbounded => query_end,
    };

    if start <= end {
        Some((start, end))
    } else {
        None
    }
}
