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

//! Parquet data catalog for efficient storage and retrieval of financial market data.
//!
//! This module provides a data catalog implementation that uses Apache Parquet
//! format for storing financial market data with object store backends. The catalog supports
//! various data types including quotes, trades, bars, order book data, and other market events.
//!
//! # Key Features
//!
//! - **Object Store Integration**: Works with local filesystems, S3, and other object stores.
//! - **Data Type Support**: Handles all major financial data types (quotes, trades, bars, etc.).
//! - **Time-based Organization**: Organizes data by timestamp ranges for efficient querying.
//! - **Consolidation**: Merges multiple files to optimize storage and query performance.
//! - **Validation**: Ensures data integrity with timestamp ordering and interval validation.
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
//!     None,        // storage_options
//!     Some(5000),  // batch_size
//!     None,        // compression (defaults to SNAPPY)
//!     None,        // max_row_group_size (defaults to 5000)
//! );
//!
//! // Write data to the catalog
//! // catalog.write_to_parquet(data, None, None)?;
//! ```

use std::{
    borrow::Cow,
    collections::HashSet,
    fmt::Debug,
    io::Cursor,
    ops::Bound as RangeBound,
    path::{Path, PathBuf},
    sync::Arc,
};

use ahash::AHashMap;
use datafusion::arrow::record_batch::RecordBatch;
use futures::StreamExt;
use heck::ToSnakeCase;
use itertools::Itertools;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    UnixNanos,
    datetime::{iso8601_to_unix_nanos, unix_nanos_to_iso8601},
};
use nautilus_model::{
    data::{
        Bar, CustomData, Data, HasTsInit, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta,
        OrderBookDepth10, QuoteTick, TradeTick, close::InstrumentClose,
        is_monotonically_increasing_by_init, to_variant,
    },
    instruments::InstrumentAny,
};
use nautilus_serialization::arrow::{
    DecodeDataFromRecordBatch, EncodeToRecordBatch, custom::CustomDataDecoder,
};
use object_store::{ObjectStore, path::Path as ObjectPath};
use serde::Serialize;
use unbounded_interval_tree::interval_tree::IntervalTree;

use super::{
    custom::{
        custom_data_path_components, decode_batch_to_data as orchestration_decode_batch_to_data,
        decode_custom_batches_to_data as orchestration_decode_custom_batches_to_data,
        prepare_custom_data_batch,
    },
    session::{self, DataBackendSession, QueryResult, build_query},
};
use crate::parquet::{read_parquet_from_object_store, write_batches_to_object_store};

/// A high-performance data catalog for storing and retrieving financial market data using Apache Parquet format.
///
/// The `ParquetDataCatalog` provides a solution for managing large volumes of financial
/// market data with efficient storage, querying, and consolidation capabilities. It supports various
/// object store backends including local filesystems, AWS S3, and other cloud storage providers.
///
/// # Features
///
/// - **Efficient Storage**: Uses Apache Parquet format with configurable compression.
/// - **Object Store Backend**: Supports multiple storage backends through the `object_store` crate.
/// - **Time-based Organization**: Organizes data by timestamp ranges for optimal query performance.
/// - **Data Validation**: Ensures timestamp ordering and interval consistency.
/// - **Consolidation**: Merges multiple files to reduce storage overhead and improve query speed.
/// - **Type Safety**: Strongly typed data handling with compile-time guarantees.
///
/// # Data Organization
///
/// Data is organized hierarchically by data type and instrument:
/// - `data/{data_type}/{instrument_id}/{start_ts}-{end_ts}.parquet`.
/// - Files are named with their timestamp ranges for efficient range queries.
/// - Intervals are validated to be disjoint to prevent data overlap.
///
/// # Performance Considerations
///
/// - **Batch Size**: Controls memory usage during data processing.
/// - **Compression**: SNAPPY compression provides good balance of speed and size.
/// - **Row Group Size**: Affects query performance and memory usage.
/// - **File Consolidation**: Reduces the number of files for better query performance.
pub struct ParquetDataCatalog {
    /// The base path for data storage within the object store.
    pub base_path: String,
    /// The original URI provided when creating the catalog.
    pub original_uri: String,
    /// The object store backend for data persistence.
    pub object_store: Arc<dyn ObjectStore>,
    /// The DataFusion session for query execution.
    pub session: DataBackendSession,
    /// The number of records to process in each batch.
    pub batch_size: usize,
    /// The compression algorithm used for Parquet files.
    pub compression: parquet::basic::Compression,
    /// The maximum number of rows in each Parquet row group.
    pub max_row_group_size: usize,
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
    /// - `base_path`: The base directory path for data storage.
    /// - `storage_options`: Optional `HashMap` containing storage-specific configuration options.
    /// - `batch_size`: Number of records to process in each batch (default: 5000).
    /// - `compression`: Parquet compression algorithm (default: SNAPPY).
    /// - `max_row_group_size`: Maximum rows per Parquet row group (default: 5000).
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
    ///     None,        // no storage options
    ///     Some(1000),  // smaller batch size
    ///     None,        // default compression
    ///     None,        // default row group size
    /// );
    /// ```
    #[must_use]
    pub fn new(
        base_path: PathBuf,
        storage_options: Option<AHashMap<String, String>>,
        batch_size: Option<usize>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
    ) -> Self {
        let path_str = base_path.to_string_lossy().to_string();
        Self::from_uri(
            &path_str,
            storage_options,
            batch_size,
            compression,
            max_row_group_size,
        )
        .expect("Failed to create catalog from path")
    }

    /// Creates a new [`ParquetDataCatalog`] instance from a URI with optional storage options.
    ///
    /// Supports various URI schemes including local file paths and multiple cloud storage backends
    /// supported by the `object_store` crate.
    ///
    /// # Supported URI Schemes
    ///
    /// - **AWS S3**: `s3://bucket/path`.
    /// - **Google Cloud Storage**: `gs://bucket/path` or `gcs://bucket/path`.
    /// - **Azure Blob Storage**: `az://container/path` or `abfs://container@account.dfs.core.windows.net/path`.
    /// - **HTTP/WebDAV**: `http://` or `https://`.
    /// - **Local files**: `file://path` or plain paths.
    ///
    /// # Parameters
    ///
    /// - `uri`: The URI for the data storage location.
    /// - `storage_options`: Optional `HashMap` containing storage-specific configuration options:
    ///   - For S3: `endpoint_url`, region, `access_key_id`, `secret_access_key`, `session_token`, etc.
    ///   - For GCS: `service_account_path`, `service_account_key`, `project_id`, etc.
    ///   - For Azure: `account_name`, `account_key`, `sas_token`, etc.
    /// - `batch_size`: Number of records to process in each batch (default: 5000).
    /// - `compression`: Parquet compression algorithm (default: SNAPPY).
    /// - `max_row_group_size`: Maximum rows per Parquet row group (default: 5000).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The URI format is invalid or unsupported.
    /// - The object store cannot be created or accessed.
    /// - Authentication fails for cloud storage backends.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use ahash::AHashMap;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// // Local filesystem
    /// let local_catalog = ParquetDataCatalog::from_uri(
    ///     "/tmp/nautilus_data",
    ///     None, None, None, None
    /// )?;
    ///
    /// // S3 bucket
    /// let s3_catalog = ParquetDataCatalog::from_uri(
    ///     "s3://my-bucket/nautilus-data",
    ///     None, None, None, None
    /// )?;
    ///
    /// // Google Cloud Storage
    /// let gcs_catalog = ParquetDataCatalog::from_uri(
    ///     "gs://my-bucket/nautilus-data",
    ///     None, None, None, None
    /// )?;
    ///
    /// // Azure Blob Storage
    /// let azure_catalog = ParquetDataCatalog::from_uri(
    ///     "az://container/nautilus-data",
    ///     storage_options, None, None, None
    /// )?;
    ///
    /// // S3 with custom endpoint and credentials
    /// let mut storage_options = HashMap::new();
    /// storage_options.insert("endpoint_url".to_string(), "https://my-s3-endpoint.com".to_string());
    /// storage_options.insert("access_key_id".to_string(), "my-key".to_string());
    /// storage_options.insert("secret_access_key".to_string(), "my-secret".to_string());
    ///
    /// let s3_catalog = ParquetDataCatalog::from_uri(
    ///     "s3://my-bucket/nautilus-data",
    ///     Some(storage_options),
    ///     None, None, None,
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn from_uri(
        uri: &str,
        storage_options: Option<AHashMap<String, String>>,
        batch_size: Option<usize>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
    ) -> anyhow::Result<Self> {
        let batch_size = batch_size.unwrap_or(5000);
        let compression = compression.unwrap_or(parquet::basic::Compression::SNAPPY);
        let max_row_group_size = max_row_group_size.unwrap_or(5000);

        let (object_store, base_path, original_uri) =
            crate::parquet::create_object_store_from_path(uri, storage_options)?;

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

    /// Returns the base path of the catalog for testing purposes.
    #[must_use]
    pub fn get_base_path(&self) -> String {
        self.base_path.clone()
    }

    /// Resets the backend session to clear any cached table registrations.
    ///
    /// This is useful during catalog operations when files are being modified
    /// and we need to ensure fresh data is loaded.
    pub fn reset_session(&mut self) {
        self.session.clear_registered_tables();
    }

    /// Writes mixed data types to the catalog by separating them into type-specific collections.
    ///
    /// This method takes a heterogeneous collection of market data and separates it by type,
    /// then writes each type to its appropriate location in the catalog. This is useful when
    /// processing mixed data streams or bulk data imports.
    ///
    /// # Parameters
    ///
    /// - `data`: A vector of mixed [`Data`] enum variants.
    /// - `start`: Optional start timestamp to override the data's natural range.
    /// - `end`: Optional end timestamp to override the data's natural range.
    ///
    /// # Notes
    ///
    /// - Data is automatically sorted by type before writing.
    /// - Each data type is written to its own directory structure.
    /// - Instrument data handling is not yet implemented (TODO).
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
    /// catalog.write_data_enum(mixed_data, None, None)?;
    /// ```
    pub fn write_data_enum(
        &self,
        data: Vec<Data>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<()> {
        let mut deltas: Vec<OrderBookDelta> = Vec::new();
        let mut depth10s: Vec<OrderBookDepth10> = Vec::new();
        let mut quotes: Vec<QuoteTick> = Vec::new();
        let mut trades: Vec<TradeTick> = Vec::new();
        let mut bars: Vec<Bar> = Vec::new();
        let mut mark_prices: Vec<MarkPriceUpdate> = Vec::new();
        let mut index_prices: Vec<IndexPriceUpdate> = Vec::new();
        let mut closes: Vec<InstrumentClose> = Vec::new();
        // Group custom data by full DataType identity (type_name + identifier + metadata)
        // so each batch is written to the correct path with consistent schema/metadata.
        let custom_data_key = |c: &CustomData| {
            (
                c.data_type.type_name().to_string(),
                c.data_type.identifier().map(String::from),
                c.data_type.metadata_str(),
            )
        };
        let mut custom_data: AHashMap<(String, Option<String>, String), Vec<CustomData>> =
            AHashMap::new();

        for d in data.iter().cloned() {
            match d {
                Data::Deltas(_) => {}
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
                Data::Custom(c) => {
                    custom_data.entry(custom_data_key(&c)).or_default().push(c);
                }
            }
        }

        // Instruments are handled separately via write_instruments method

        self.write_to_parquet(deltas, start, end, skip_disjoint_check)?;
        self.write_to_parquet(depth10s, start, end, skip_disjoint_check)?;
        self.write_to_parquet(quotes, start, end, skip_disjoint_check)?;
        self.write_to_parquet(trades, start, end, skip_disjoint_check)?;
        self.write_to_parquet(bars, start, end, skip_disjoint_check)?;
        self.write_to_parquet(mark_prices, start, end, skip_disjoint_check)?;
        self.write_to_parquet(index_prices, start, end, skip_disjoint_check)?;
        self.write_to_parquet(closes, start, end, skip_disjoint_check)?;

        for (_, items) in custom_data {
            self.write_custom_data_batch(items, start, end, skip_disjoint_check)?;
        }

        Ok(())
    }

    /// Writes typed data to a Parquet file in the catalog.
    ///
    /// This is the core method for persisting market data to the catalog. It handles data
    /// validation, batching, compression, and ensures proper file organization with
    /// timestamp-based naming.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to write, must implement required traits for serialization and cataloging.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to write (must be in ascending timestamp order).
    /// - `start`: Optional start timestamp to override the natural data range.
    /// - `end`: Optional end timestamp to override the natural data range.
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created file, or an empty path if no data was provided.
    /// If the target file already exists, returns the path without writing (skips write).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization to Arrow record batches fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    /// - Writing would create non-disjoint timestamp intervals.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Data timestamps are not in ascending order.
    /// - Record batches are empty after conversion.
    /// - Required metadata is missing from the schema.
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
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<PathBuf>
    where
        T: HasTsInit + EncodeToRecordBatch + CatalogPathPrefix,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name)?;

        let start_ts = start.unwrap_or(data.first().unwrap().ts_init());
        let end_ts = end.unwrap_or(data.last().unwrap().ts_init());

        let batches = self.data_to_record_batches(data)?;
        let schema = batches.first().expect("Batches are empty.").schema();

        let identifier = if T::path_prefix() == "bars" {
            schema.metadata.get("bar_type").cloned()
        } else {
            schema.metadata.get("instrument_id").cloned()
        };

        let directory = self.make_path(T::path_prefix(), identifier)?;
        let filename = timestamps_to_filename(start_ts, end_ts);
        let path = PathBuf::from(format!("{directory}/{filename}"));
        let object_path = self.to_object_path(&path.to_string_lossy());

        let file_exists =
            self.execute_async(async { Ok(self.object_store.head(&object_path).await.is_ok()) })?;

        if file_exists {
            log::info!("File {} already exists, skipping write", path.display());
            return Ok(path);
        }

        if !skip_disjoint_check.unwrap_or(false) {
            let current_intervals = self.get_directory_intervals(&directory)?;
            let new_interval = (start_ts.as_u64(), end_ts.as_u64());
            let mut new_intervals = current_intervals.clone();
            new_intervals.push(new_interval);

            if !are_intervals_disjoint(&new_intervals) {
                anyhow::bail!(
                    "Writing file {filename} with interval ({start_ts}, {end_ts}) would create \
                    non-disjoint intervals. Existing intervals: {current_intervals:?}"
                );
            }
        }

        log::info!(
            "Writing {} batches of {type_name} data to {}",
            batches.len(),
            path.display(),
        );

        self.execute_async(async {
            write_batches_to_object_store(
                &batches,
                self.object_store.clone(),
                &object_path,
                Some(self.compression),
                Some(self.max_row_group_size),
                None,
            )
            .await
        })?;

        Ok(path)
    }

    /// Writes custom data to a Parquet file in the catalog.
    ///
    /// This method handles writing custom data types that implement `CustomDataTrait`.
    /// Custom data is organized by type name in a `custom/{type_name}/` directory structure.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of custom data items to write (must be in ascending timestamp order).
    /// - `start`: Optional start timestamp to override the natural data range.
    /// - `end`: Optional end timestamp to override the natural data range.
    /// - `skip_disjoint_check`: Whether to skip interval disjointness validation.
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created file, or an empty path if no data was provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization to Arrow record batches fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    /// - Writing would create non-disjoint timestamp intervals (unless skipped).
    pub fn write_custom_data_batch(
        &self,
        data: Vec<CustomData>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<PathBuf> {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let (batch, type_name, identifier, start_ts, end_ts) = prepare_custom_data_batch(data)?;
        let start_ts = start.unwrap_or(start_ts);
        let end_ts = end.unwrap_or(end_ts);
        let batches = vec![batch];

        let directory = self.make_path_custom_data(&type_name, identifier)?;
        let filename = timestamps_to_filename(start_ts, end_ts);
        let path = PathBuf::from(format!("{directory}/{filename}"));
        let object_path = self.to_object_path(&path.to_string_lossy());

        let file_exists =
            self.execute_async(async { Ok(self.object_store.head(&object_path).await.is_ok()) })?;

        if file_exists {
            log::info!("File {} already exists, skipping write", path.display());
            return Ok(path);
        }

        if !skip_disjoint_check.unwrap_or(false) {
            let current_intervals = self.get_directory_intervals(&directory)?;
            let new_interval = (start_ts.as_u64(), end_ts.as_u64());
            let mut new_intervals = current_intervals.clone();
            new_intervals.push(new_interval);

            if !are_intervals_disjoint(&new_intervals) {
                anyhow::bail!(
                    "Writing file {filename} with interval ({start_ts}, {end_ts}) would create \
                    non-disjoint intervals. Existing intervals: {current_intervals:?}"
                );
            }
        }

        self.execute_async(async {
            write_batches_to_object_store(
                &batches,
                self.object_store.clone(),
                &object_path,
                Some(self.compression),
                Some(self.max_row_group_size),
                None,
            )
            .await
        })?;

        Ok(path)
    }

    /// Writes instruments to Parquet files in the catalog.
    ///
    /// Instruments are stored by instrument ID rather than timestamp ranges, since they
    /// represent metadata that doesn't change over time. Each instrument is stored in
    /// its own file: `data/instruments/{instrument_id}/instrument.parquet`
    ///
    /// # Parameters
    ///
    /// - `instruments`: Vector of instruments to write.
    ///
    /// # Returns
    ///
    /// Returns a vector of paths to the created files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::instruments::InstrumentAny;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let instruments: Vec<InstrumentAny> = vec![/* instruments */];
    ///
    /// let paths = catalog.write_instruments(instruments)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    /// Writes instruments to Parquet files in the catalog.
    ///
    /// Instruments are stored by instrument ID rather than timestamp ranges, since they
    /// represent metadata that doesn't change over time. Each instrument is stored in
    /// its own file: `data/instruments/{instrument_id}/instrument.parquet`
    ///
    /// # Parameters
    ///
    /// - `instruments`: Vector of instruments to write.
    ///
    /// # Returns
    ///
    /// Returns a vector of paths to the created files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::instruments::InstrumentAny;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let instruments: Vec<InstrumentAny> = vec![/* instruments */];
    ///
    /// let paths = catalog.write_instruments(instruments)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_instruments(
        &self,
        instruments: Vec<InstrumentAny>,
    ) -> anyhow::Result<Vec<PathBuf>> {
        use nautilus_model::instruments::Instrument;

        if instruments.is_empty() {
            return Ok(Vec::new());
        }

        // Group instruments by instrument_id
        let mut by_id: AHashMap<String, Vec<InstrumentAny>> = AHashMap::new();
        for instrument in instruments {
            let instrument_id = Instrument::id(&instrument).to_string();
            by_id.entry(instrument_id).or_default().push(instrument);
        }

        let mut paths = Vec::new();

        for (instrument_id, instrument_group) in by_id {
            // Convert to record batches
            let batches = self.data_to_record_batches(instrument_group)?;
            if batches.is_empty() {
                continue;
            }

            // Create directory path: data/instruments/{instrument_id}/
            let directory = self.make_path("instruments", Some(instrument_id.clone()))?;
            let filename = "instrument.parquet";
            let path = PathBuf::from(format!("{directory}/{filename}"));
            let object_path = self.to_object_path(&path.to_string_lossy());

            let file_exists = self
                .execute_async(async { Ok(self.object_store.head(&object_path).await.is_ok()) })?;

            if file_exists {
                log::info!(
                    "Instrument file {} already exists, skipping write",
                    path.display()
                );
                paths.push(path);
                continue;
            }

            log::info!(
                "Writing {} batches of instrument data for {instrument_id} to {}",
                batches.len(),
                path.display(),
            );

            // ArrowWriter stores the full schema (including "class" metadata) in ARROW:schema.
            // When reading, use the builder's schema for metadata (see query_instruments).
            self.execute_async(async {
                write_batches_to_object_store(
                    &batches,
                    self.object_store.clone(),
                    &object_path,
                    Some(self.compression),
                    Some(self.max_row_group_size),
                    None,
                )
                .await
            })?;

            paths.push(path);
        }

        Ok(paths)
    }

    /// Queries instruments from the catalog.
    ///
    /// Instruments are stored by instrument ID in `data/instruments/{instrument_id}/instrument.parquet`.
    /// This method reads the instrument files and deserializes them back to `InstrumentAny`.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by. If `None`, queries all instruments.
    ///
    /// # Returns
    ///
    /// Returns a vector of `InstrumentAny` instances, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File discovery fails.
    /// - File reading fails.
    /// - Data deserialization fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::instruments::InstrumentAny;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Query all instruments
    /// let instruments = catalog.query_instruments(None)?;
    ///
    /// // Query specific instruments
    /// let instruments = catalog.query_instruments(Some(vec!["EUR/USD.SIM".to_string()]))?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_instruments(
        &self,
        instrument_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        use nautilus_serialization::arrow::instrument::decode_instrument_any_batch;

        let base_dir = self.make_path("instruments", None)?;
        let mut all_instruments = Vec::new();

        // List all instrument directories
        let list_result = self.execute_async(async {
            let prefix = ObjectPath::from(format!("{base_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }
            Ok::<Vec<_>, anyhow::Error>(objects)
        })?;

        // Extract unique instrument directories
        let mut instrument_dirs: HashSet<String> = HashSet::new();
        for object in list_result {
            let path_str = object.location.to_string();
            if path_str.ends_with("instrument.parquet") {
                // Extract directory path (everything before the filename)
                if let Some(dir_path) = path_str.strip_suffix("/instrument.parquet") {
                    // Extract instrument_id from directory path (last component)
                    let path_parts: Vec<&str> = dir_path.split('/').collect();
                    if let Some(instrument_id_dir) = path_parts.last() {
                        // Apply filter if provided
                        if let Some(ref ids) = instrument_ids
                            && !ids
                                .iter()
                                .map(|id| urisafe_instrument_id(id))
                                .any(|x| x.as_str() == urisafe_instrument_id(instrument_id_dir))
                        {
                            continue;
                        }
                        instrument_dirs.insert(dir_path.to_string());
                    }
                }
            }
        }

        // Read each instrument file (written as Parquet). Use the builder's schema for
        // metadata (Arrow restores it from ARROW:schema); batch.schema() has metadata stripped.
        // Use to_object_path_parsed so paths from list() are not re-encoded by Path::from.
        for dir_path in instrument_dirs {
            let file_path = format!("{dir_path}/instrument.parquet");
            let object_path = self.to_object_path_parsed(&file_path)?;
            let (batches, builder_schema) = self.execute_async(async {
                read_parquet_from_object_store(self.object_store.clone(), &object_path).await
            })?;

            let metadata: std::collections::HashMap<String, String> =
                builder_schema.metadata().clone();

            for batch in batches {
                let instruments = decode_instrument_any_batch(&metadata, batch)?;
                all_instruments.extend(instruments);
            }
        }

        Ok(all_instruments)
    }

    /// Writes typed data to a JSON file in the catalog.
    ///
    /// This method provides an alternative to Parquet format for data export and debugging.
    /// JSON files are human-readable but less efficient for large datasets.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to write, must implement serialization and cataloging traits.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to write (must be in ascending timestamp order).
    /// - `path`: Optional custom directory path (defaults to catalog's standard structure).
    /// - `write_metadata`: Whether to write a separate metadata file alongside the data.
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - JSON serialization fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
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
        T: HasTsInit + Serialize + CatalogPathPrefix + EncodeToRecordBatch,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name)?;

        let start_ts = data.first().unwrap().ts_init();
        let end_ts = data.last().unwrap().ts_init();

        let directory =
            path.unwrap_or_else(|| PathBuf::from(self.make_path(T::path_prefix(), None).unwrap()));
        let filename = timestamps_to_filename(start_ts, end_ts).replace(".parquet", ".json");
        let json_path = directory.join(&filename);

        log::info!(
            "Writing {} records of {type_name} data to {}",
            data.len(),
            json_path.display(),
        );

        if write_metadata {
            let metadata = T::chunk_metadata(&data);
            let metadata_path = json_path.with_extension("metadata.json");
            log::info!("Writing metadata to {}", metadata_path.display());

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
    /// - `data`: Slice of data records to validate.
    /// - `type_name`: Name of the data type for error messages.
    ///
    pub fn check_ascending_timestamps<T: HasTsInit>(
        data: &[T],
        type_name: &str,
    ) -> anyhow::Result<()> {
        if !data
            .array_windows()
            .all(|[a, b]| a.ts_init() <= b.ts_init())
        {
            anyhow::bail!("{type_name} timestamps must be in ascending order");
        }

        Ok(())
    }

    /// Converts data into Arrow record batches for Parquet serialization.
    ///
    /// This method chunks the data according to the configured batch size and converts
    /// each chunk into an Arrow record batch with appropriate metadata.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to convert, must implement required encoding traits.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to convert.
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
        T: HasTsInit + EncodeToRecordBatch,
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

    /// Extends the timestamp range of an existing Parquet file by renaming it.
    ///
    /// This method finds an existing file that is adjacent to the specified time range
    /// and renames it to include the new range. This is useful when appending data
    /// that extends the time coverage of existing files.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an instrument_id (e.g., "EUR/USD.SIM") or a bar_type (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    /// - `start`: Start timestamp of the new range to extend to.
    /// - `end`: End timestamp of the new range to extend to.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - No adjacent file is found to extend.
    /// - File rename operations fail.
    /// - Interval validation fails after extension.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Extend a file's range backwards or forwards
    /// catalog.extend_file_name(
    ///     "quotes",
    ///     Some("BTC/USD.SIM".to_string()),
    ///     UnixNanos::from(1609459200000000000),
    ///     UnixNanos::from(1609545600000000000)
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn extend_file_name(
        &self,
        data_cls: &str,
        identifier: Option<String>,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(data_cls, identifier)?;
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

    /// Lists all Parquet files in a specified directory.
    ///
    /// This method scans a directory and returns the full paths of all files with the `.parquet`
    /// extension. It works with both local filesystems and remote object stores, making it
    /// suitable for various storage backends.
    ///
    /// # Parameters
    ///
    /// - `directory`: The directory path to scan for Parquet files.
    ///
    /// # Returns
    ///
    /// Returns a vector of full file paths (as strings) for all Parquet files found in the directory.
    /// The paths are relative to the object store root and suitable for use with object store operations.
    /// Returns an empty vector if the directory doesn't exist or contains no Parquet files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    /// - Network issues occur (for remote object stores).
    ///
    /// # Notes
    ///
    /// - Only files ending with `.parquet` are included.
    /// - Subdirectories are not recursively scanned.
    /// - File paths are returned in the order provided by the object store.
    /// - Works with all supported object store backends (local, S3, GCS, Azure, etc.).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let files = catalog.list_parquet_files("data/quotes/EURUSD")?;
    ///
    /// for file in files {
    ///     println!("Found Parquet file: {}", file);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_parquet_files(&self, directory: &str) -> anyhow::Result<Vec<String>> {
        self.execute_async(async {
            let prefix = ObjectPath::from(format!("{directory}/"));
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

    /// Lists all instrument identifiers for a specific data type.
    ///
    /// This method scans the data directory for a given data type and extracts
    /// all unique instrument identifiers from the directory structure.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The data type directory name (e.g., "quotes", "trades", "bars").
    ///
    /// # Returns
    ///
    /// Returns a vector of instrument identifier strings.
    ///
    /// # Errors
    ///
    /// Returns an error if directory listing fails.
    pub fn list_instruments(&self, data_type: &str) -> anyhow::Result<Vec<String>> {
        self.execute_async(async {
            let prefix = ObjectPath::from(format!("data/{data_type}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut instruments = HashSet::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                let path = object.location.as_ref();
                let parts: Vec<&str> = path.split('/').collect();
                if parts.len() >= 3 {
                    instruments.insert(parts[2].to_string());
                }
            }
            Ok::<Vec<String>, anyhow::Error>(instruments.into_iter().collect())
        })
    }

    /// Lists Parquet files matching specific criteria (data type, identifiers, time range).
    ///
    /// This method finds all Parquet files that match the specified criteria by filtering
    /// files based on their directory structure and filename timestamps.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The data type directory name (e.g., "quotes", "trades", "custom/MyType").
    /// - `identifiers`: Optional list of identifiers to filter by.
    /// - `start`: Optional start timestamp to filter files by their time range.
    /// - `end`: Optional end timestamp to filter files by their time range.
    ///
    /// # Returns
    ///
    /// Returns a vector of file paths that match the criteria.
    ///
    /// # Errors
    ///
    /// Returns an error if directory listing or file filtering fails.
    pub fn list_parquet_files_with_criteria(
        &self,
        data_type: &str,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<String>> {
        let mut all_files = Vec::new();

        let start_u64 = start.map(|s| s.as_u64());
        let end_u64 = end.map(|e| e.as_u64());

        let base_dir = self.make_path(data_type, None)?;

        // Use recursive listing to match Python's glob behavior
        let list_result = self.execute_async(async {
            let prefix = ObjectPath::from(format!("{base_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }
            Ok::<Vec<_>, anyhow::Error>(objects)
        })?;

        for object in list_result {
            let path_str = object.location.to_string();

            // Filter by identifiers if provided
            if let Some(ids) = &identifiers {
                let path_components = extract_path_components(&path_str);
                let mut matches = false;
                for id in ids {
                    if path_components.iter().any(|c| c.contains(id)) {
                        matches = true;
                        break;
                    }
                }

                if !matches {
                    continue;
                }
            }

            // Filter by timestamp range if filename can be parsed
            if path_str.ends_with(".parquet")
                && query_intersects_filename(&path_str, start_u64, end_u64)
            {
                all_files.push(path_str);
            }
        }

        Ok(all_files)
    }

    /// Helper method to reconstruct full URI for remote object store paths
    #[must_use]
    pub fn reconstruct_full_uri(&self, path_str: &str) -> String {
        // Check if this is a remote URI scheme that needs reconstruction
        if self.is_remote_uri() {
            // Extract the base URL (scheme + host) from the original URI
            if let Ok(url) = url::Url::parse(&self.original_uri)
                && let Some(host) = url.host_str()
            {
                return format!("{}://{}/{}", url.scheme(), host, path_str);
            }
        }

        // For local paths, extract the directory from the original URI
        if self.original_uri.starts_with("file://") {
            // Extract the path from the file:// URI
            if let Ok(url) = url::Url::parse(&self.original_uri)
                && let Ok(base_path) = url.to_file_path()
            {
                // Use platform-appropriate path separator for display
                // but object store paths always use forward slashes
                let base_str = base_path.to_string_lossy();
                return self.join_paths(&base_str, path_str);
            }
        }

        // For local paths without file:// prefix, use the original URI as base
        if self.base_path.is_empty() {
            // If base_path is empty and not a file URI, try using original_uri as base
            if self.original_uri.contains("://") {
                // Fallback: return the path as-is
                path_str.to_string()
            } else {
                self.join_paths(self.original_uri.trim_end_matches('/'), path_str)
            }
        } else {
            let base = self.base_path.trim_end_matches('/');
            self.join_paths(base, path_str)
        }
    }

    /// Helper method to join paths using forward slashes (object store convention)
    #[must_use]
    fn join_paths(&self, base: &str, path: &str) -> String {
        make_object_store_path(base, &[path])
    }

    /// Resolves a path for use with DataFusion (avoiding Windows path doubling for file://).
    /// Returns the path as-is if it is already a full URI or absolute; otherwise builds
    /// file:// base + path for local catalogs or reconstruct_full_uri for remote.
    #[must_use]
    fn resolve_path_for_datafusion(&self, path: &str) -> String {
        if path.contains("://") {
            return path.to_string();
        }

        if path.starts_with('/') {
            return path.to_string();
        }

        if self.original_uri.starts_with("file://") {
            let base = self.original_uri.trim_end_matches('/');
            let path_trimmed = path.trim_end_matches('/');
            return format!("{base}/{path_trimmed}");
        }
        self.reconstruct_full_uri(path)
    }

    /// Like resolve_path_for_datafusion but ensures the result ends with a trailing slash.
    #[must_use]
    fn resolve_directory_for_datafusion(&self, directory: &str) -> String {
        let mut resolved = self.resolve_path_for_datafusion(directory);
        if !resolved.ends_with('/') {
            resolved.push('/');
        }
        resolved
    }

    /// Returns the path string to push in query_files result list: relative for file://,
    /// full URI for remote (so callers can pass to resolve_path_for_datafusion later).
    #[must_use]
    fn path_for_query_list(&self, path: &str) -> String {
        if self.original_uri.starts_with("file://") {
            path.to_string()
        } else {
            self.reconstruct_full_uri(path)
        }
    }

    /// Returns the native path string for the catalog root (for std::fs). Only valid when
    /// !is_remote_uri(); uses parquet's file_uri_to_native_path for file:// URIs.
    #[must_use]
    fn native_base_path_string(&self) -> String {
        if self.original_uri.starts_with("file://") {
            crate::parquet::file_uri_to_native_path(&self.original_uri)
        } else {
            self.original_uri.clone()
        }
    }

    /// Helper method to check if the original URI uses a remote object store scheme
    #[must_use]
    pub fn is_remote_uri(&self) -> bool {
        self.original_uri.starts_with("s3://")
            || self.original_uri.starts_with("gs://")
            || self.original_uri.starts_with("gcs://")
            || self.original_uri.starts_with("az://")
            || self.original_uri.starts_with("abfs://")
            || self.original_uri.starts_with("http://")
            || self.original_uri.starts_with("https://")
    }

    /// Executes a query against the catalog to retrieve market data of a specific type.
    ///
    /// This is the primary method for querying data from the catalog. It registers the appropriate
    /// object store with the DataFusion session, finds all relevant Parquet files, and executes
    /// the query across them. The method supports filtering by instrument IDs, time ranges, and
    /// custom SQL WHERE clauses.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to query, must implement required traits for deserialization and cataloging.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings (e.g., "EUR/USD.SIM")
    ///   or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"). If `None`, queries all identifiers.
    ///   For bars, partial matching is supported (e.g., "EUR/USD.SIM" will match "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    /// - `start`: Optional start timestamp for filtering (inclusive). If `None`, queries from the beginning.
    /// - `end`: Optional end timestamp for filtering (inclusive). If `None`, queries to the end.
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering (e.g., "price > 100").
    /// - `files`: Optional list of specific files to query. If provided, skips file discovery.
    /// - `optimize_file_loading`: If `true` (default), registers entire directories with DataFusion,
    ///   which is more efficient for managing many files. If `false`, registers each file individually
    ///   (needed for operations like consolidation where precise file control is required).
    ///
    /// # Returns
    ///
    /// Returns a [`QueryResult`] containing the query execution context and data.
    /// Use [`QueryResult::collect()`] to retrieve the actual data records.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store registration fails for remote URIs.
    /// - File discovery fails.
    /// - DataFusion query execution fails.
    /// - Data deserialization fails.
    ///
    /// # Performance Notes
    ///
    /// - Files are automatically filtered by timestamp ranges before querying.
    /// - DataFusion optimizes queries across multiple Parquet files.
    /// - Use specific instrument IDs and time ranges to improve performance.
    /// - WHERE clauses are pushed down to the Parquet reader when possible.
    /// - Directory-based registration (`optimize_file_loading=true`) is more efficient for queries
    ///   with many files, as it reduces the number of table registrations.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::data::QuoteTick;
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let mut catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Query all quote data (uses directory-based registration by default)
    /// let result = catalog.query::<QuoteTick>(None, None, None, None, None, true)?;
    /// let quotes = result.collect();
    ///
    /// // Query specific instruments within a time range
    /// let result = catalog.query::<QuoteTick>(
    ///     Some(vec!["EUR/USD.SIM".to_string(), "GBP/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     None,
    ///     None,
    ///     true
    /// )?;
    ///
    /// // Query with custom WHERE clause and file-based registration
    /// let result = catalog.query::<QuoteTick>(
    ///     Some(vec!["EUR/USD.SIM".to_string()]),
    ///     None,
    ///     None,
    ///     Some("bid_price > 1.2000"),
    ///     None,
    ///     false  // Use file-based registration for precise control
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query<T>(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        // Register the object store with the session for remote URIs only.
        // For local file:// we do not register: we pass full file URLs to register_parquet
        // so DataFusion's default file provider handles them (avoids path doubling on Windows
        // where a registered store would receive a path that gets prefixed again).
        if self.is_remote_uri() {
            let url = url::Url::parse(&self.original_uri)?;
            let host = url
                .host_str()
                .ok_or_else(|| anyhow::anyhow!("Remote URI missing host/bucket name"))?;
            let base_url = url::Url::parse(&format!("{}://{}", url.scheme(), host))?;
            self.session
                .register_object_store(&base_url, self.object_store.clone());
        }

        let files_list = if let Some(files) = files {
            files
        } else {
            self.query_files(T::path_prefix(), identifiers, start, end)?
        };

        if optimize_file_loading {
            // Use directory-based registration for efficiency. DataFusion handles
            // reading all files in each directory, which is more memory-efficient
            // than registering many individual file tables.
            let directories: HashSet<String> = files_list
                .iter()
                .filter_map(|file_uri| {
                    // Extract directory path (everything except the filename)
                    let path = Path::new(file_uri);
                    path.parent().map(|p| p.to_string_lossy().to_string())
                })
                .collect();

            for directory in directories {
                // Extract identifier from directory path (last component)
                let path_parts: Vec<&str> = directory.split('/').collect();
                let identifier = if path_parts.is_empty() {
                    "unknown".to_string()
                } else {
                    path_parts[path_parts.len() - 1].to_string()
                };
                let safe_sql_identifier = make_sql_safe_identifier(&identifier);

                // Create table name from path_prefix and identifier (no filename component)
                let table_name = format!("{}_{}", T::path_prefix(), safe_sql_identifier);
                let query = build_query(&table_name, start, end, where_clause);

                let resolved_path = self.resolve_directory_for_datafusion(&directory);

                self.session
                    .add_file::<T>(&table_name, &resolved_path, Some(&query), None)?;
            }
        } else {
            // Register files individually (for operations requiring precise file control)
            for file_uri in &files_list {
                // Extract identifier from file path and filename to create meaningful table names
                let identifier = extract_identifier_from_path(file_uri);
                let safe_sql_identifier = make_sql_safe_identifier(&identifier);
                let safe_filename = extract_sql_safe_filename(file_uri);

                // Create table name from path_prefix, identifier, and filename
                let table_name = format!(
                    "{}_{}_{}",
                    T::path_prefix(),
                    safe_sql_identifier,
                    safe_filename
                );
                let query = build_query(&table_name, start, end, where_clause);

                let resolved_path = self.resolve_path_for_datafusion(file_uri);
                self.session
                    .add_file::<T>(&table_name, &resolved_path, Some(&query), None)?;
            }
        }

        Ok(self.session.get_query_result())
    }

    /// Queries typed data from the catalog and returns results as a strongly-typed vector.
    ///
    /// This is a convenience method that wraps the generic `query` method and automatically
    /// collects and converts the results into a vector of the specific data type. It handles
    /// the type conversion from the generic [`Data`] enum to the concrete type `T`.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The specific data type to query and return. Must implement required traits for
    ///   deserialization, cataloging, and conversion from the [`Data`] enum.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings (e.g., "EUR/USD.SIM")
    ///   or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"). If `None`, queries all identifiers.
    ///   For bars, partial matching is supported (e.g., "EUR/USD.SIM" will match "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    /// - `start`: Optional start timestamp for filtering (inclusive). If `None`, queries from the beginning.
    /// - `end`: Optional end timestamp for filtering (inclusive). If `None`, queries to the end.
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering. Use standard SQL syntax
    ///   with column names matching the Parquet schema (e.g., "`bid_price` > 1.2000", "volume > 1000").
    ///
    /// # Returns
    ///
    /// Returns a vector of the specific data type `T`, sorted by timestamp. The vector will be
    /// empty if no data matches the query criteria.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The underlying query execution fails.
    /// - Data type conversion fails.
    /// - Object store access fails.
    /// - Invalid WHERE clause syntax is provided.
    ///
    /// # Performance Considerations
    ///
    /// - Use specific instrument IDs and time ranges to minimize data scanning.
    /// - WHERE clauses are pushed down to Parquet readers when possible.
    /// - Results are automatically sorted by timestamp during collection.
    /// - Memory usage scales with the amount of data returned.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::data::{QuoteTick, TradeTick, Bar};
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let mut catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Query all quotes for a specific instrument
    /// let quotes: Vec<QuoteTick> = catalog.query_typed_data(
    ///     Some(vec!["EUR/USD.SIM".to_string()]),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    ///     true
    /// )?;
    ///
    /// // Query trades within a specific time range
    /// let trades: Vec<TradeTick> = catalog.query_typed_data(
    ///     Some(vec!["BTC/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     None,
    ///     None,
    ///     true
    /// )?;
    ///
    /// // Query bars with volume filter (using instrument_id - partial match for bar_type)
    /// let bars: Vec<Bar> = catalog.query_typed_data(
    ///     Some(vec!["AAPL.NASDAQ".to_string()]),
    ///     None,
    ///     None,
    ///     Some("volume > 1000000"),
    ///     None,
    ///     true
    /// )?;
    ///
    /// // Query bars with specific bar_type
    /// let bars: Vec<Bar> = catalog.query_typed_data(
    ///     Some(vec!["AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL".to_string()]),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    ///     true
    /// )?;
    ///
    /// // Query multiple instruments with price filter
    /// let quotes: Vec<QuoteTick> = catalog.query_typed_data(
    ///     Some(vec!["EUR/USD.SIM".to_string(), "GBP/USD.SIM".to_string()]),
    ///     None,
    ///     None,
    ///     Some("bid_price > 1.2000 AND ask_price < 1.3000"),
    ///     None,
    ///     true
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_typed_data<T>(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix + TryFrom<Data>,
    {
        // Reset session to allow repeated queries (streams are consumed on each query)
        self.reset_session();

        let query_result = self.query::<T>(
            identifiers,
            start,
            end,
            where_clause,
            files,
            optimize_file_loading,
        )?;
        let all_data = query_result.collect();

        // Convert Data enum variants to specific type T using to_variant
        Ok(to_variant::<T>(all_data))
    }

    /// Queries custom data dynamically by type name.
    ///
    /// This method allows querying custom data types without compile-time knowledge of the type.
    /// It uses dynamic schema decoding based on the type name stored in metadata.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The name of the custom data type to query.
    /// - `identifiers`: Optional list of instrument identifiers to filter by.
    /// - `start`: Optional start timestamp for filtering.
    /// - `end`: Optional end timestamp for filtering.
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering.
    /// - `files`: Optional list of specific files to query.
    /// - `_optimize_file_loading`: Whether to optimize file loading (currently unused).
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` enum variants containing the custom data.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File discovery fails.
    /// - Data decoding fails.
    /// - Query execution fails.
    #[allow(clippy::too_many_arguments)]
    pub fn query_custom_data_dynamic(
        &mut self,
        type_name: &str,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        _optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<Data>> {
        self.reset_session();
        let path_prefix = format!("custom/{type_name}");

        let files = if let Some(f) = files {
            f.into_iter()
                .map(|p| self.to_object_path(&p).to_string())
                .collect::<Vec<_>>()
        } else {
            self.list_parquet_files_with_criteria(&path_prefix, identifiers, start, end)?
        };

        if files.is_empty() {
            return Ok(Vec::new());
        }

        let table_name = "custom_data_table";

        // Use CustomDataDecoder for all custom data. Pass type_name so decode can look up
        // the type when Parquet/DataFusion does not preserve schema metadata. Callers must
        // ensure Rust custom types are registered via ensure_custom_data_registered::<T>().
        for file in files {
            let resolved_path = self.resolve_path_for_datafusion(&file);
            let sql_query = build_query(table_name, start, end, where_clause);

            self.session
                .add_file::<CustomDataDecoder>(
                    table_name,
                    &resolved_path,
                    Some(&sql_query),
                    Some(type_name),
                )
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }

        let query_result = self.session.get_query_result();
        Ok(query_result.collect())
    }

    /// Queries all Parquet files for a specific data type and optional instrument IDs.
    ///
    /// This method finds all Parquet files that match the specified criteria and returns
    /// their full URIs. The files are filtered by data type, instrument IDs (if provided),
    /// and timestamp range (if provided).
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings
    ///   (e.g., "EUR/USD.SIM") or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported.
    /// - `start`: Optional start timestamp to filter files by their time range.
    /// - `end`: Optional end timestamp to filter files by their time range.
    ///
    /// # Returns
    ///
    /// Returns a vector of file URI strings that match the query criteria,
    /// or an error if the query fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - Object store listing operations fail.
    /// - URI reconstruction fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Query all quote files
    /// let files = catalog.query_files("quotes", None, None, None)?;
    ///
    /// // Query trade files for specific instruments within a time range
    /// let files = catalog.query_files(
    ///     "trades",
    ///     Some(vec!["BTC/USD.SIM".to_string(), "ETH/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000))
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_files(
        &self,
        data_cls: &str,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<String>> {
        let mut files = Vec::new();

        let start_u64 = start.map(|s| s.as_u64());
        let end_u64 = end.map(|e| e.as_u64());

        let base_dir = self.make_path(data_cls, None)?;

        // Use recursive listing to match Python's glob behavior
        let list_result = self.execute_async(async {
            let prefix = ObjectPath::from(format!("{base_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }
            Ok::<Vec<_>, anyhow::Error>(objects)
        })?;

        let mut file_paths: Vec<String> = list_result
            .into_iter()
            .filter_map(|object| {
                let path_str = object.location.to_string();
                if path_str.ends_with(".parquet") {
                    Some(path_str)
                } else {
                    None
                }
            })
            .collect();

        // Apply identifier filtering if provided
        if let Some(identifiers) = identifiers {
            let safe_identifiers: Vec<String> = identifiers
                .iter()
                .map(|id| urisafe_instrument_id(id))
                .collect();

            // Exact match by default for instrument_ids or bar_types
            let exact_match_file_paths: Vec<String> = file_paths
                .iter()
                .filter(|file_path| {
                    // Extract the directory name (second to last path component)
                    let path_parts: Vec<&str> = file_path.split('/').collect();
                    if path_parts.len() >= 2 {
                        let dir_name = path_parts[path_parts.len() - 2];
                        safe_identifiers.iter().any(|safe_id| safe_id == dir_name)
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();

            if exact_match_file_paths.is_empty() && data_cls == "bars" {
                file_paths.retain(|file_path| {
                    let path_parts: Vec<&str> = file_path.split('/').collect();
                    if path_parts.len() >= 2 {
                        let dir_name = path_parts[path_parts.len() - 2];
                        if let Some(bar_instrument_id) = extract_bar_type_instrument_id(dir_name) {
                            safe_identifiers.iter().any(|id| id == bar_instrument_id)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                });
            } else {
                file_paths = exact_match_file_paths;
            }
        }

        // Apply timestamp filtering
        file_paths.retain(|file_path| query_intersects_filename(file_path, start_u64, end_u64));

        for file_path in file_paths {
            files.push(self.path_for_query_list(&file_path));
        }

        Ok(files)
    }

    pub fn quote_ticks(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.query_typed_data::<QuoteTick>(instrument_ids, start, end, None, None, true)
    }

    /// Queries trade tick data for the specified instrument(s) and time range.
    pub fn trade_ticks(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.query_typed_data::<TradeTick>(instrument_ids, start, end, None, None, true)
    }

    /// Queries bar data for the specified instrument(s) and time range.
    pub fn bars(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<Bar>> {
        self.query_typed_data::<Bar>(instrument_ids, start, end, None, None, true)
    }

    /// Queries order book delta data for the specified instrument(s) and time range.
    pub fn order_book_deltas(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        self.query_typed_data::<OrderBookDelta>(instrument_ids, start, end, None, None, true)
    }

    /// Queries order book depth L10 data for the specified instrument(s) and time range.
    pub fn order_book_depth10(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<OrderBookDepth10>> {
        self.query_typed_data::<OrderBookDepth10>(instrument_ids, start, end, None, None, true)
    }

    /// Queries instrument close data for the specified instrument(s) and time range.
    pub fn instrument_closes(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<InstrumentClose>> {
        self.query_typed_data::<InstrumentClose>(instrument_ids, start, end, None, None, true)
    }

    /// Queries any instrument data for the specified instrument(s) and time range.
    pub fn instruments(
        &self,
        instrument_ids: Option<Vec<String>>,
        _start: Option<UnixNanos>,
        _end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.query_instruments(instrument_ids)
    }

    /// Retrieves a list of file paths for a given data type.
    ///
    /// This method constructs a path pattern to find all parquet files
    /// associated with the specified data type in the catalog's directory structure.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades", "bars").
    ///
    /// # Returns
    ///
    /// Returns a vector of file paths matching the data type, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let files = catalog.get_file_list_from_data_cls("quotes")?;
    ///
    /// for file in files {
    ///     println!("Found file: {}", file);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_file_list_from_data_cls(&self, data_cls: &str) -> anyhow::Result<Vec<String>> {
        let base_dir = self.make_path(data_cls, None)?;

        let list_result = self.execute_async(async {
            let prefix = ObjectPath::from(format!("{base_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }
            Ok::<Vec<_>, anyhow::Error>(objects)
        })?;

        let file_paths: Vec<String> = list_result
            .into_iter()
            .filter_map(|object| {
                let path_str = object.location.to_string();
                if path_str.ends_with(".parquet") {
                    Some(path_str)
                } else {
                    None
                }
            })
            .collect();

        Ok(file_paths)
    }

    /// Filters a list of file paths based on identifiers and time range.
    ///
    /// This method filters the provided file paths by:
    /// 1. Matching identifiers (exact match for instruments, prefix match for bars)
    /// 2. Intersecting with the specified time range
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades", "bars").
    /// - `file_paths`: List of file paths to filter.
    /// - `identifiers`: Optional list of identifiers to match against file paths.
    /// - `start`: Optional start timestamp for filtering.
    /// - `end`: Optional end timestamp for filtering.
    ///
    /// # Returns
    ///
    /// Returns a filtered vector of file paths that match the criteria.
    ///
    /// # Notes
    ///
    /// For Bar data types, if exact identifier matching fails, the function attempts
    /// partial matching by checking if the file's identifier starts with the provided identifier
    /// followed by a dash (to match bar type patterns).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let all_files = catalog.get_file_list_from_data_cls("quotes")?;
    ///
    /// let filtered = catalog.filter_files(
    ///     "quotes",
    ///     all_files,
    ///     Some(vec!["EUR/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000))
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn filter_files(
        &self,
        data_cls: &str,
        file_paths: Vec<String>,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<String>> {
        let mut filtered_paths = file_paths;

        // Apply identifier filtering if provided
        if let Some(identifiers) = identifiers {
            let safe_identifiers: Vec<String> = identifiers
                .iter()
                .map(|id| urisafe_instrument_id(id))
                .collect();

            // Extract directory names from file paths
            let file_safe_identifiers: Vec<String> = filtered_paths
                .iter()
                .map(|file_path| {
                    let path_parts: Vec<&str> = file_path.split('/').collect();
                    if path_parts.len() >= 2 {
                        path_parts[path_parts.len() - 2].to_string()
                    } else {
                        String::new()
                    }
                })
                .collect();

            // Exact match by default for instrument_ids or bar_types
            let exact_match_file_paths: Vec<String> = filtered_paths
                .iter()
                .enumerate()
                .filter_map(|(i, file_path)| {
                    let dir_name = &file_safe_identifiers[i];
                    if safe_identifiers.iter().any(|safe_id| safe_id == dir_name) {
                        Some(file_path.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if exact_match_file_paths.is_empty() && data_cls == "bars" {
                // Partial match of instrument_ids in bar_types for bars
                filtered_paths.retain(|file_path| {
                    let path_parts: Vec<&str> = file_path.split('/').collect();
                    if path_parts.len() >= 2 {
                        let dir_name = path_parts[path_parts.len() - 2];
                        safe_identifiers
                            .iter()
                            .any(|safe_id| dir_name.starts_with(&format!("{safe_id}-")))
                    } else {
                        false
                    }
                });
            } else {
                filtered_paths = exact_match_file_paths;
            }
        }

        // Apply timestamp filtering
        let start_u64 = start.map(|s| s.as_u64());
        let end_u64 = end.map(|e| e.as_u64());
        filtered_paths.retain(|file_path| query_intersects_filename(file_path, start_u64, end_u64));

        Ok(filtered_paths)
    }

    /// Finds the missing time intervals for a specific data type and instrument ID.
    ///
    /// This method compares a requested time range against the existing data coverage
    /// and returns the gaps that need to be filled. This is useful for determining
    /// what data needs to be fetched or backfilled.
    ///
    /// # Parameters
    ///
    /// - `start`: Start timestamp of the requested range (Unix nanoseconds).
    /// - `end`: End timestamp of the requested range (Unix nanoseconds).
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `instrument_id`: Optional instrument ID to target a specific instrument's data.
    ///
    /// # Returns
    ///
    /// Returns a vector of (start, end) tuples representing the missing intervals,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - Interval retrieval fails.
    /// - Gap calculation fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Find missing intervals for quote data
    /// let missing = catalog.get_missing_intervals_for_request(
    ///     1609459200000000000,  // start
    ///     1609545600000000000,  // end
    ///     "quotes",
    ///     Some("BTCUSD".to_string())
    /// )?;
    ///
    /// for (start, end) in missing {
    ///     println!("Missing data from {} to {}", start, end);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_missing_intervals_for_request(
        &self,
        start: u64,
        end: u64,
        data_cls: &str,
        identifier: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        let intervals = self.get_intervals(data_cls, identifier)?;

        Ok(query_interval_diff(start, end, &intervals))
    }

    /// Gets the first (earliest) timestamp for a specific data type and identifier.
    ///
    /// This method finds the earliest timestamp covered by existing data files for
    /// the specified data type and identifier. This is useful for determining
    /// the oldest data available or for incremental data updates.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an instrument_id (e.g., "EUR/USD.SIM") or a bar_type (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///
    /// # Returns
    ///
    /// Returns `Some(timestamp)` if data exists, `None` if no data is found,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - Interval retrieval fails.
    ///
    /// # Note
    ///
    /// Unlike the Python implementation, this method does not check subclasses of the
    /// data type. The Python version checks `[data_cls, *data_cls.__subclasses__()]` to
    /// handle cases where subclasses might use different directory names. Since Rust
    /// works with string names rather than types, subclass checking is not possible.
    /// In practice, most subclasses map to the same directory name via `class_to_filename`,
    /// so this difference is typically not significant.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Get the first timestamp for quote data
    /// if let Some(first_ts) = catalog.query_first_timestamp("quotes", Some("BTCUSD".to_string()))? {
    ///     println!("First quote timestamp: {}", first_ts);
    /// } else {
    ///     println!("No quote data found");
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_first_timestamp(
        &self,
        data_cls: &str,
        identifier: Option<String>,
    ) -> anyhow::Result<Option<u64>> {
        let intervals = self.get_intervals(data_cls, identifier)?;

        if intervals.is_empty() {
            return Ok(None);
        }

        Ok(Some(intervals.first().unwrap().0))
    }

    /// Gets the last (most recent) timestamp for a specific data type and identifier.
    ///
    /// This method finds the latest timestamp covered by existing data files for
    /// the specified data type and identifier. This is useful for determining
    /// the most recent data available or for incremental data updates.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an instrument_id (e.g., "EUR/USD.SIM") or a bar_type (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///
    /// # Returns
    ///
    /// Returns `Some(timestamp)` if data exists, `None` if no data is found,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - Interval retrieval fails.
    ///
    /// # Note
    ///
    /// Unlike the Python implementation, this method does not check subclasses of the
    /// data type. The Python version checks `[data_cls, *data_cls.__subclasses__()]` to
    /// handle cases where subclasses might use different directory names. Since Rust
    /// works with string names rather than types, subclass checking is not possible.
    /// In practice, most subclasses map to the same directory name via `class_to_filename`,
    /// so this difference is typically not significant.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Get the last timestamp for quote data
    /// if let Some(last_ts) = catalog.query_last_timestamp("quotes", Some("BTCUSD".to_string()))? {
    ///     println!("Last quote timestamp: {}", last_ts);
    /// } else {
    ///     println!("No quote data found");
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_last_timestamp(
        &self,
        data_cls: &str,
        identifier: Option<String>,
    ) -> anyhow::Result<Option<u64>> {
        let intervals = self.get_intervals(data_cls, identifier)?;

        if intervals.is_empty() {
            return Ok(None);
        }

        Ok(Some(intervals.last().unwrap().1))
    }

    /// Gets the time intervals covered by Parquet files for a specific data type and identifier.
    ///
    /// This method returns all time intervals covered by existing data files for the
    /// specified data type and identifier. The intervals are sorted by start time and
    /// represent the complete data coverage available.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an instrument_id (e.g., "EUR/USD.SIM") or a bar_type (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///
    /// # Returns
    ///
    /// Returns a vector of (start, end) tuples representing the covered intervals,
    /// sorted by start time, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - Directory listing fails.
    /// - Filename parsing fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Get all intervals for quote data
    /// let intervals = catalog.get_intervals("quotes", Some("BTCUSD".to_string()))?;
    /// for (start, end) in intervals {
    ///     println!("Data available from {} to {}", start, end);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_intervals(
        &self,
        data_cls: &str,
        identifier: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        let directory = self.make_path(data_cls, identifier.clone())?;
        let intervals = self.get_directory_intervals(&directory)?;

        // For bars, fall back to partial matching when the exact directory
        // doesn't exist (callers may pass an instrument_id like "EUR/USD.SIM"
        // but bars are stored under bar_type dirs like "EURUSD.SIM-1-MINUTE-...")

        if !intervals.is_empty() || data_cls != "bars" || identifier.is_none() {
            return Ok(intervals);
        }

        let safe_id = urisafe_instrument_id(&identifier.unwrap());

        // Use relative path so list_directory_stems doesn't double-prefix
        // for remote catalogs (make_path already includes base_path)
        let bars_subdir = format!("data/{data_cls}");
        let subdirs = self.list_directory_stems(&bars_subdir)?;

        let mut all_intervals = Vec::new();

        for subdir in &subdirs {
            let decoded = urlencoding::decode(subdir).unwrap_or(Cow::Borrowed(subdir));

            if extract_bar_type_instrument_id(&decoded) == Some(safe_id.as_str()) {
                // Use decoded name to avoid double percent-encoding
                // (to_object_path uses Path::from which re-encodes)
                let subdir_path = self.make_path(data_cls, Some(decoded.into_owned()))?;
                all_intervals.extend(self.get_directory_intervals(&subdir_path)?);
            }
        }

        all_intervals.sort_by_key(|&(start, _)| start);

        // Merge overlapping intervals from different bar types so that
        // last().1 reliably gives the maximum end timestamp
        let mut merged: Vec<(u64, u64)> = Vec::new();

        for interval in all_intervals {
            if let Some(last) = merged.last_mut()
                && interval.0 <= last.1
            {
                last.1 = last.1.max(interval.1);
                continue;
            }
            merged.push(interval);
        }

        Ok(merged)
    }

    /// Gets the time intervals covered by Parquet files in a specific directory.
    ///
    /// This method scans a directory for Parquet files and extracts the timestamp ranges
    /// from their filenames. It's used internally by other methods to determine data coverage
    /// and is essential for interval-based operations like gap detection and consolidation.
    ///
    /// # Parameters
    ///
    /// - `directory`: The directory path to scan for Parquet files.
    ///
    /// # Returns
    ///
    /// Returns a vector of (start, end) tuples representing the time intervals covered
    /// by files in the directory, sorted by start timestamp. Returns an empty vector
    /// if the directory doesn't exist or contains no valid Parquet files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Notes
    ///
    /// - Only files with valid timestamp-based filenames are included.
    /// - Files with unparsable names are silently ignored.
    /// - The method works with both local and remote object stores.
    /// - Results are automatically sorted by start timestamp.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    /// let intervals = catalog.get_directory_intervals("data/quotes/EURUSD")?;
    ///
    /// for (start, end) in intervals {
    ///     println!("File covers {} to {}", start, end);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_directory_intervals(&self, directory: &str) -> anyhow::Result<Vec<(u64, u64)>> {
        // Use object store for all operations
        // Convert directory to object path format (consistent with how files are written)
        // For local stores with empty base_path, to_object_path returns path as-is
        // For remote stores, to_object_path strips the base_path prefix
        let object_dir = self.to_object_path(directory);
        let list_result = self.execute_async(async {
            // Ensure trailing slash for directory listing
            let dir_str = format!("{}/", object_dir.as_ref());
            let prefix = ObjectPath::from(dir_str);
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }
            Ok::<Vec<_>, anyhow::Error>(objects)
        })?;

        let mut intervals = Vec::new();
        for object in list_result {
            let path_str = object.location.to_string();
            if path_str.ends_with(".parquet")
                && let Some(interval) = parse_filename_timestamps(&path_str)
            {
                intervals.push(interval);
            }
        }

        intervals.sort_by_key(|&(start, _)| start);

        Ok(intervals)
    }

    /// Constructs a directory path for storing data of a specific type and instrument.
    ///
    /// This method builds the hierarchical directory structure used by the catalog to organize
    /// data by type and instrument. The path follows the pattern: `{base_path}/data/{type_name}/{instrument_id}`.
    /// Instrument IDs are automatically converted to URI-safe format by removing forward slashes.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars").
    /// - `identifier`: Optional identifier. Can be an instrument_id (e.g., "EUR/USD.SIM") or a bar_type (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"). If provided, creates a subdirectory for the identifier. If `None`, returns the path to the data type directory.
    ///
    /// # Returns
    ///
    /// Returns the constructed directory path as a string, or an error if path construction fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument ID contains invalid characters that cannot be made URI-safe.
    /// - Path construction fails due to system limitations.
    ///
    /// # Path Structure
    ///
    /// - Without identifier: `{base_path}/data/{type_name}`.
    /// - With identifier: `{base_path}/data/{type_name}/{safe_identifier}`.
    /// - If `base_path` is empty: `data/{type_name}[/{safe_identifier}]`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Path for all quote data
    /// let quotes_path = catalog.make_path("quotes", None)?;
    /// // Returns: "/base/path/data/quotes"
    ///
    /// // Path for specific instrument quotes
    /// let eurusd_quotes = catalog.make_path("quotes", Some("EUR/USD".to_string()))?;
    /// // Returns: "/base/path/data/quotes/EURUSD" (slash removed)
    ///
    /// // Path for bar data with complex instrument ID
    /// let bars_path = catalog.make_path("bars", Some("BTC/USD-1H".to_string()))?;
    /// // Returns: "/base/path/data/bars/BTCUSD-1H"
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn make_path(&self, type_name: &str, identifier: Option<String>) -> anyhow::Result<String> {
        let mut components = vec!["data".to_string(), type_name.to_string()];

        if let Some(id) = identifier {
            let safe_id = urisafe_instrument_id(&id);
            components.push(safe_id);
        }

        let path = make_object_store_path_owned(&self.base_path, components);
        Ok(path)
    }

    /// Builds the directory path for custom data: `data/custom/{type_name}[/{identifier segments}]`.
    /// Identifier can contain `//` for subdirectories (normalized to `/`); path is safe for writing.
    pub fn make_path_custom_data(
        &self,
        type_name: &str,
        identifier: Option<String>,
    ) -> anyhow::Result<String> {
        let components = custom_data_path_components(type_name, identifier.as_deref());
        let path = make_object_store_path_owned(&self.base_path, components);
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
        let old_path = format!("{directory}/{old_filename}");
        let old_object_path = self.to_object_path(&old_path);

        let new_filename =
            timestamps_to_filename(UnixNanos::from(new_start), UnixNanos::from(new_end));
        let new_path = format!("{directory}/{new_filename}");
        let new_object_path = self.to_object_path(&new_path);

        self.move_file(&old_object_path, &new_object_path)
    }

    /// Converts a catalog path string to an [`ObjectPath`] for object store operations.
    ///
    /// This method handles the conversion between catalog-relative paths and object store paths,
    /// taking into account the catalog's base path configuration. It automatically strips the
    /// base path prefix when present to create the correct object store path.
    ///
    /// # Parameters
    ///
    /// - `path`: The catalog path string to convert. Can be absolute or relative.
    ///
    /// # Returns
    ///
    /// Returns an [`ObjectPath`] suitable for use with object store operations.
    ///
    /// # Path Handling
    ///
    /// - If `base_path` is empty, the path is used as-is.
    /// - If `base_path` is set, it's stripped from the path if present.
    /// - Trailing slashes and backslashes are automatically handled.
    /// - The resulting path is relative to the object store root.
    /// - All paths are normalized to use forward slashes (object store convention).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Convert a full catalog path
    /// let object_path = catalog.to_object_path("/base/data/quotes/file.parquet");
    /// // Returns: ObjectPath("data/quotes/file.parquet") if base_path is "/base"
    ///
    /// // Convert a relative path
    /// let object_path = catalog.to_object_path("data/trades/file.parquet");
    /// // Returns: ObjectPath("data/trades/file.parquet")
    /// ```
    #[must_use]
    pub fn to_object_path(&self, path: &str) -> ObjectPath {
        // Normalize path separators to forward slashes for object store
        let normalized_path = path.replace('\\', "/");

        if self.base_path.is_empty() {
            return ObjectPath::from(normalized_path);
        }

        // Normalize base path separators as well
        let normalized_base = self.base_path.replace('\\', "/");
        let base = normalized_base.trim_end_matches('/');

        // Remove the catalog base prefix if present
        let without_base = normalized_path
            .strip_prefix(&format!("{base}/"))
            .or_else(|| normalized_path.strip_prefix(base))
            .unwrap_or(&normalized_path);

        ObjectPath::from(without_base)
    }

    /// Converts a path string to [`ObjectPath`] using parse (no percent-encoding).
    ///
    /// Use this for paths that were returned by the object store (e.g. from `list()`),
    /// which may already be percent-encoded. Using [`Self::to_object_path`] (which uses
    /// `Path::from`) on such paths would double-encode (e.g. `%5E` -> `%255E`).
    pub fn to_object_path_parsed(&self, path: &str) -> anyhow::Result<ObjectPath> {
        let normalized_path = path.replace('\\', "/");

        let to_parse = if self.base_path.is_empty() {
            normalized_path.as_str()
        } else {
            let normalized_base = self.base_path.replace('\\', "/");
            let base = normalized_base.trim_end_matches('/');
            normalized_path
                .strip_prefix(&format!("{base}/"))
                .or_else(|| normalized_path.strip_prefix(base))
                .unwrap_or(normalized_path.as_str())
        };

        ObjectPath::parse(to_parse).map_err(anyhow::Error::from)
    }

    #[allow(dead_code)]
    fn to_file_path(&self, path: &ObjectPath) -> String {
        path.to_string()
    }

    /// Helper method to move a file using object store rename operation
    pub fn move_file(&self, old_path: &ObjectPath, new_path: &ObjectPath) -> anyhow::Result<()> {
        self.execute_async(async {
            self.object_store
                .rename(old_path, new_path)
                .await
                .map_err(anyhow::Error::from)
        })
    }

    /// Helper method to execute async operations with a runtime
    pub fn execute_async<F, R>(&self, future: F) -> anyhow::Result<R>
    where
        F: std::future::Future<Output = anyhow::Result<R>>,
    {
        let rt = get_runtime();
        rt.block_on(future)
    }

    /// Lists directory stems (directory names without path) in a subdirectory.
    ///
    /// This method scans a subdirectory and returns the names of all immediate
    /// subdirectories. It's used to list data types, backtest runs, and live runs.
    ///
    /// # Parameters
    ///
    /// - `subdirectory`: The subdirectory path to scan (e.g., "data", "backtest", "live").
    ///
    /// # Returns
    ///
    /// Returns a vector of directory names (stems) found in the subdirectory,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // List all data types
    /// let data_types = catalog.list_directory_stems("data")?;
    /// for data_type in data_types {
    ///     println!("Found data type: {}", data_type);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_directory_stems(&self, subdirectory: &str) -> anyhow::Result<Vec<String>> {
        // For local filesystem paths, use filesystem operations to detect empty directories
        // For remote object stores, we can only list directories that contain files
        if !self.is_remote_uri() {
            let directory = PathBuf::from(self.native_base_path_string()).join(subdirectory);

            // Check if directory exists
            if !directory.exists() {
                return Ok(Vec::new());
            }

            // List all entries in the directory
            let mut directories = Vec::new();

            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type()
                        && file_type.is_dir()
                    {
                        // Use file_name() to get the directory name (not file_stem which removes extension)
                        if let Some(name) = entry.path().file_name() {
                            directories.push(name.to_string_lossy().to_string());
                        }
                    }
                }
            }
            directories.sort();
            return Ok(directories);
        }

        // For remote URIs, use object store listing (only lists directories with files)
        let directory = make_object_store_path(&self.base_path, &[subdirectory]);

        let list_result = self.execute_async(async {
            let prefix = ObjectPath::from(format!("{directory}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut directories = Vec::new();
            let mut seen_dirs = std::collections::HashSet::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                let path_str = object.location.to_string();

                // Extract the immediate subdirectory name
                if let Some(relative_path) = path_str.strip_prefix(&format!("{directory}/")) {
                    let parts: Vec<&str> = relative_path.split('/').collect();
                    if let Some(first_part) = parts.first()
                        && !first_part.is_empty()
                        && !seen_dirs.contains(*first_part)
                    {
                        seen_dirs.insert(first_part.to_string());
                        directories.push(first_part.to_string());
                    }
                }
            }

            Ok::<Vec<String>, anyhow::Error>(directories)
        })?;

        Ok(list_result)
    }

    /// Lists all data types available in the catalog.
    ///
    /// This method returns the names of all data type directories in the catalog.
    /// Data types correspond to different kinds of market data (e.g., "quotes", "trades", "bars").
    ///
    /// # Returns
    ///
    /// Returns a vector of data type names, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // List all data types
    /// let data_types = catalog.list_data_types()?;
    /// for data_type in data_types {
    ///     println!("Available data type: {}", data_type);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    ///
    /// # Note
    ///
    /// FundingRateUpdate is intentionally excluded from the catalog and feather writer;
    /// directories such as "funding_rate_update" or "funding_rates" are filtered out.
    pub fn list_data_types(&self) -> anyhow::Result<Vec<String>> {
        let stems = self.list_directory_stems("data")?;
        Ok(stems
            .into_iter()
            .filter(|s| !Self::is_excluded_stream_data_type(s))
            .collect())
    }

    /// Data types that are not persisted by the Rust feather writer or catalog (e.g. FundingRateUpdate).
    fn is_excluded_stream_data_type(name: &str) -> bool {
        matches!(name, "funding_rate_update" | "funding_rates")
    }

    /// Lists all backtest run IDs available in the catalog.
    ///
    /// This method returns the names of all backtest run directories in the catalog.
    /// Each backtest run corresponds to a specific backtest execution instance.
    ///
    /// # Returns
    ///
    /// Returns a vector of backtest run IDs, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // List all backtest runs
    /// let runs = catalog.list_backtest_runs()?;
    /// for run_id in runs {
    ///     println!("Backtest run: {}", run_id);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_backtest_runs(&self) -> anyhow::Result<Vec<String>> {
        self.list_directory_stems("backtest")
    }

    /// Lists all live run IDs available in the catalog.
    ///
    /// This method returns the names of all live run directories in the catalog.
    /// Each live run corresponds to a specific live trading execution instance.
    ///
    /// # Returns
    ///
    /// Returns a vector of live run IDs, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // List all live runs
    /// let runs = catalog.list_live_runs()?;
    /// for run_id in runs {
    ///     println!("Live run: {}", run_id);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_live_runs(&self) -> anyhow::Result<Vec<String>> {
        self.list_directory_stems("live")
    }

    /// Reads data from a live run instance.
    ///
    /// This method reads all data associated with a specific live run instance
    /// from feather files stored in the catalog.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the live run instance to read.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` objects from the live run, sorted by timestamp,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instance ID doesn't exist.
    /// - Feather file reading fails.
    /// - Data deserialization fails.
    ///
    /// # Note
    ///
    /// This method is currently not fully implemented. Feather file reading
    /// requires complex deserialization logic that needs to be added.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Read data from a live run
    /// let data = catalog.read_live_run("instance-123")?;
    /// for item in data {
    ///     println!("Data: {:?}", item);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn read_live_run(&self, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        self.read_run_data("live", instance_id)
    }

    /// Reads data from a backtest run instance.
    ///
    /// This method reads all data associated with a specific backtest run instance
    /// from feather files stored in the catalog.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the backtest run instance to read.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` objects from the backtest run, sorted by timestamp,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instance ID doesn't exist.
    /// - Feather file reading fails.
    /// - Data deserialization fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Read data from a backtest run
    /// let data = catalog.read_backtest("instance-123")?;
    /// for item in data {
    ///     println!("Data: {:?}", item);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn read_backtest(&self, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        self.read_run_data("backtest", instance_id)
    }

    /// Helper function to read data from a run instance (backtest or live).
    ///
    /// This function reads all data associated with a specific run instance
    /// from feather files stored in the catalog.
    ///
    /// # Parameters
    ///
    /// - `subdirectory`: The subdirectory name ("backtest" or "live").
    /// - `instance_id`: The ID of the run instance to read.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` objects from the run, sorted by timestamp,
    /// or an error if the operation fails.
    fn read_run_data(&self, subdirectory: &str, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        // List all data types in the instance directory
        let instance_dir = make_object_store_path(&self.base_path, &[subdirectory, instance_id]);

        // List directories under the instance directory
        let data_types = if self.is_remote_uri() {
            // For remote URIs, use object store listing

            self.execute_async(async {
                let prefix = ObjectPath::from(format!("{instance_dir}/"));
                let mut stream = self.object_store.list(Some(&prefix));
                let mut directories = Vec::new();
                let mut seen_dirs = std::collections::HashSet::new();

                while let Some(object) = stream.next().await {
                    let object = object?;
                    let path_str = object.location.to_string();

                    // Extract the immediate subdirectory name
                    if let Some(relative_path) = path_str.strip_prefix(&format!("{instance_dir}/"))
                    {
                        let parts: Vec<&str> = relative_path.split('/').collect();
                        if let Some(first_part) = parts.first()
                            && !first_part.is_empty()
                            && !seen_dirs.contains(*first_part)
                        {
                            seen_dirs.insert(first_part.to_string());
                            directories.push(first_part.to_string());
                        }
                    }
                }

                Ok::<Vec<String>, anyhow::Error>(directories)
            })?
        } else {
            // For local filesystem paths
            let directory = PathBuf::from(self.native_base_path_string())
                .join(subdirectory)
                .join(instance_id);

            if !directory.exists() {
                return Ok(Vec::new());
            }

            let mut directories = Vec::new();

            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type()
                        && file_type.is_dir()
                        && let Some(name) = entry.path().file_name()
                    {
                        directories.push(name.to_string_lossy().to_string());
                    }
                }
            }
            directories.sort();
            directories
        };

        if data_types.is_empty() {
            // No data types found - return empty vector
            return Ok(Vec::new());
        }

        let mut all_data: Vec<Data> = Vec::new();

        // Process each data type (FundingRateUpdate excluded - see is_excluded_stream_data_type)
        for data_cls in data_types
            .into_iter()
            .filter(|s| !Self::is_excluded_stream_data_type(s))
        {
            // List all feather files for this data type
            let feather_files = self.list_feather_files(
                subdirectory,
                instance_id,
                &data_cls,
                None, // No identifier filtering - read all
            )?;

            if feather_files.is_empty() {
                continue; // Skip if no files found
            }

            // Process each feather file
            for file_path in feather_files {
                // Read the feather file (may contain multiple batches)
                let batches = self.read_feather_file(&file_path)?;

                if batches.is_empty() {
                    continue; // Skip empty or invalid files
                }

                // Convert RecordBatches to Data objects based on data_cls
                let file_data: Vec<Data> = match data_cls.as_str() {
                    "quotes" => {
                        let quotes: Vec<QuoteTick> =
                            self.convert_record_batches_to_data(batches, false)?;
                        quotes.into_iter().map(Data::from).collect()
                    }
                    "trades" => {
                        let trades: Vec<TradeTick> =
                            self.convert_record_batches_to_data(batches, false)?;
                        trades.into_iter().map(Data::from).collect()
                    }
                    "order_book_deltas" => {
                        let deltas: Vec<OrderBookDelta> =
                            self.convert_record_batches_to_data(batches, false)?;
                        deltas.into_iter().map(Data::from).collect()
                    }
                    "order_book_depths" => {
                        let depths: Vec<OrderBookDepth10> =
                            self.convert_record_batches_to_data(batches, false)?;
                        depths.into_iter().map(Data::from).collect()
                    }
                    "bars" => {
                        let bars: Vec<Bar> = self.convert_record_batches_to_data(batches, false)?;
                        bars.into_iter().map(Data::from).collect()
                    }
                    "index_prices" => {
                        let prices: Vec<IndexPriceUpdate> =
                            self.convert_record_batches_to_data(batches, false)?;
                        prices.into_iter().map(Data::from).collect()
                    }
                    "mark_prices" => {
                        let prices: Vec<MarkPriceUpdate> =
                            self.convert_record_batches_to_data(batches, false)?;
                        prices.into_iter().map(Data::from).collect()
                    }
                    "instrument_closes" => {
                        let closes: Vec<InstrumentClose> =
                            self.convert_record_batches_to_data(batches, false)?;
                        closes.into_iter().map(Data::from).collect()
                    }
                    _ => {
                        if data_cls.starts_with("custom/") {
                            self.decode_custom_batches_to_data(batches, false)?
                        } else {
                            // Unknown data type - skip it
                            continue;
                        }
                    }
                };

                all_data.extend(file_data);
            }
        }

        // Sort all data by timestamp (ts_init)
        all_data.sort_by(|a, b| {
            let ts_a = a.ts_init();
            let ts_b = b.ts_init();
            ts_a.cmp(&ts_b)
        });

        Ok(all_data)
    }

    /// Decodes multiple record batches of custom data (data_cls starts with "custom/") into a single
    /// `Vec<Data>`. Optionally replaces `ts_init` column with `ts_event` before decoding.
    ///
    /// # Errors
    ///
    /// Returns an error if any batch fails to decode.
    fn decode_custom_batches_to_data(
        &self,
        batches: Vec<RecordBatch>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<Vec<Data>> {
        orchestration_decode_custom_batches_to_data(batches, use_ts_event_for_ts_init)
    }

    /// Decodes a RecordBatch to Data objects based on metadata.
    ///
    /// This method determines the data type from metadata and decodes the batch accordingly.
    /// It supports both standard data types and custom data types when `allow_custom_fallback`
    /// is true (e.g. when called from `decode_custom_batches_to_data` for files under
    /// `custom/`). When false, unknown type names produce an error instead of attempting
    /// custom decode, so malformed or typo'd built-in metadata fails explicitly.
    ///
    /// # Parameters
    ///
    /// - `metadata`: Schema metadata containing type information.
    /// - `batch`: The RecordBatch to decode.
    /// - `allow_custom_fallback`: If true, unknown type_name is decoded via custom data
    ///   registry; if false, unknown type_name returns an error.
    ///
    /// # Returns
    ///
    /// Returns a vector of Data enum variants.
    ///
    /// # Errors
    ///
    /// Returns an error if decoding fails or the type is unknown (and custom fallback not allowed).
    #[allow(dead_code)] // used by tests
    fn decode_batch_to_data(
        &self,
        metadata: &std::collections::HashMap<String, String>,
        batch: RecordBatch,
        allow_custom_fallback: bool,
    ) -> anyhow::Result<Vec<Data>> {
        orchestration_decode_batch_to_data(metadata, batch, allow_custom_fallback)
    }

    /// Converts stream data from feather files to parquet files.
    ///
    /// This method reads data from feather files generated during a backtest or live run
    /// and writes it to the catalog in parquet format. It's useful for converting temporary
    /// stream data into a more permanent and queryable format.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the backtest or live run instance.
    /// - `data_cls`: The data class name (e.g., "quotes", "trades", "bars").
    /// - `subdirectory`: The subdirectory containing the feather files. Either "backtest" or "live" (default: "backtest").
    /// - `identifiers`: Optional list of identifiers to filter by (instrument IDs or bar types).
    /// - `use_ts_event_for_ts_init`: If true, replaces the `ts_init` column with `ts_event` column values before deserializing.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instance ID doesn't exist.
    /// - Feather file listing fails.
    /// - Feather file reading fails.
    /// - Data deserialization fails.
    /// - Writing to parquet fails.
    ///
    /// # Note
    ///
    /// This method is currently not fully implemented. It requires:
    /// - Listing feather files in the specified subdirectory
    /// - Reading feather files (Arrow IPC stream reading)
    /// - Converting Arrow tables to Nautilus data objects
    /// - Writing data to the catalog using existing write methods
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Convert backtest stream data to parquet
    /// catalog.convert_stream_to_data(
    ///     "instance-123",
    ///     "quotes",
    ///     Some("backtest"),
    ///     None,
    ///     false
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    /// Lists feather files for a specific data class in a subdirectory.
    ///
    /// This helper function finds all `.feather` files in the specified subdirectory
    /// (backtest or live) for the given instance ID and data class.
    fn list_feather_files(
        &self,
        subdirectory: &str,
        instance_id: &str,
        data_name: &str,
        identifiers: Option<&[String]>,
    ) -> anyhow::Result<Vec<String>> {
        // Construct the base directory path: {subdirectory}/{instance_id}/{data_name}
        let base_dir = make_object_store_path(&self.base_path, &[subdirectory, instance_id]);
        let data_dir = make_object_store_path(&base_dir, &[data_name]);

        let mut files = Vec::new();

        // Try to list files in the data directory (for per-instrument subdirectories)
        let subdir_prefix = ObjectPath::from(format!("{data_dir}/"));
        let list_result = self.execute_async(async {
            let mut stream = self.object_store.list(Some(&subdir_prefix));
            let mut subdirs = Vec::new();
            let mut flat_files = Vec::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                let path_str = object.location.to_string();

                // Check if this is a subdirectory (per-instrument) or a flat file
                if let Some(relative_path) = path_str.strip_prefix(&format!("{data_dir}/")) {
                    if relative_path.ends_with(".feather") {
                        // Flat file format: {data_name}_*.feather
                        if path_str.contains(&format!("{data_name}_")) {
                            flat_files.push(path_str);
                        }
                    } else {
                        // This might be a subdirectory - check if it contains feather files
                        let subdir_path = format!("{path_str}/");
                        let mut subdir_stream = self
                            .object_store
                            .list(Some(&ObjectPath::from(subdir_path.as_str())));

                        while let Some(subdir_object) = subdir_stream.next().await {
                            let subdir_object = subdir_object?;
                            let subdir_file_path = subdir_object.location.to_string();

                            if subdir_file_path.ends_with(".feather") {
                                // Check identifier filter if provided
                                if let Some(identifiers) = identifiers {
                                    let subdir_name = relative_path.split('/').next().unwrap_or("");
                                    if !identifiers.iter().any(|id| subdir_name.contains(id)) {
                                        continue;
                                    }
                                }
                                subdirs.push(subdir_file_path);
                            }
                        }
                    }
                }
            }

            Ok::<Vec<String>, anyhow::Error>([subdirs, flat_files].concat())
        })?;

        files.extend(list_result);
        files.sort();
        Ok(files)
    }

    /// Reads a feather file and returns all RecordBatches.
    ///
    /// This function reads an Arrow IPC stream file from the object store
    /// and returns all RecordBatches contained within it.
    fn read_feather_file(&self, file_path: &str) -> anyhow::Result<Vec<RecordBatch>> {
        use datafusion::arrow::ipc::reader::StreamReader;

        let bytes = self.execute_async(async {
            let path = ObjectPath::from(file_path);
            let result = self.object_store.get(&path).await?;
            let bytes = result.bytes().await?;
            Ok::<_, anyhow::Error>(bytes)
        })?;

        if bytes.is_empty() {
            return Ok(Vec::new());
        }

        // Read the Arrow IPC stream
        let cursor = Cursor::new(bytes.as_ref());
        let reader = StreamReader::try_new(cursor, None)
            .map_err(|e| anyhow::anyhow!("Failed to create StreamReader: {e}"))?;

        // Read all batches
        let mut batches = Vec::new();
        for batch_result in reader {
            let batch = batch_result.map_err(|e| anyhow::anyhow!("Failed to read batch: {e}"))?;
            batches.push(batch);
        }

        Ok(batches)
    }

    /// Converts RecordBatches to Data objects, optionally replacing ts_init with ts_event.
    fn convert_record_batches_to_data<T>(
        &self,
        batches: Vec<RecordBatch>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeDataFromRecordBatch + TryFrom<Data>,
    {
        self.convert_record_batches_to_data_with_bar_type_conversion(
            batches,
            use_ts_event_for_ts_init,
            false,
        )
    }

    /// Converts RecordBatches to Data objects with optional transforms for stream conversion.
    fn convert_record_batches_to_data_with_bar_type_conversion<T>(
        &self,
        batches: Vec<RecordBatch>,
        use_ts_event_for_ts_init: bool,
        convert_bar_type_to_external: bool,
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeDataFromRecordBatch + TryFrom<Data>,
    {
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        // Get schema and metadata from first batch
        let schema = batches[0].schema();
        let mut metadata = schema.metadata().clone();

        // Convert bar_type from INTERNAL to EXTERNAL if requested
        if convert_bar_type_to_external
            && let Some(bar_type_str) = metadata.get("bar_type").cloned()
            && bar_type_str.ends_with("-INTERNAL")
        {
            let external = bar_type_str.replace("-INTERNAL", "-EXTERNAL");
            metadata.insert("bar_type".to_string(), external);
        }

        // Process each batch
        let mut all_data = Vec::new();
        for mut batch in batches {
            // Handle ts_event/ts_init replacement if requested
            if use_ts_event_for_ts_init {
                let column_names: Vec<String> =
                    schema.fields().iter().map(|f| f.name().clone()).collect();

                let ts_event_idx = column_names
                    .iter()
                    .position(|n| n == "ts_event")
                    .ok_or_else(|| anyhow::anyhow!("ts_event column not found"))?;
                let ts_init_idx = column_names
                    .iter()
                    .position(|n| n == "ts_init")
                    .ok_or_else(|| anyhow::anyhow!("ts_init column not found"))?;

                // Create new arrays with ts_init replaced by ts_event
                let mut new_columns = batch.columns().to_vec();
                new_columns[ts_init_idx] = new_columns[ts_event_idx].clone();

                // Create new batch with updated columns
                batch = RecordBatch::try_new(schema.clone(), new_columns)
                    .map_err(|e| anyhow::anyhow!("Failed to create new batch: {e}"))?;
            }

            // Decode the batch to Data objects
            let data_vec = T::decode_data_batch(&metadata, batch)
                .map_err(|e| anyhow::anyhow!("Failed to decode batch: {e}"))?;

            all_data.extend(data_vec);
        }

        // Convert Data enum to specific type T
        Ok(to_variant::<T>(all_data))
    }

    pub fn convert_stream_to_data(
        &mut self,
        instance_id: &str,
        data_cls: &str,
        subdirectory: Option<&str>,
        identifiers: Option<Vec<String>>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<()> {
        let subdirectory = subdirectory.unwrap_or("backtest");

        // FundingRateUpdate is not persisted in Rust feather/catalog; skip without error
        if Self::is_excluded_stream_data_type(data_cls) {
            return Ok(());
        }

        // Convert data class name to filename (e.g., "quotes" -> "quotes")
        // The data_cls should already be in the correct format (snake_case)
        let data_name = data_cls.to_snake_case();

        // List all feather files for this data class
        let feather_files = self.list_feather_files(
            subdirectory,
            instance_id,
            &data_name,
            identifiers.as_deref(),
        )?;

        if feather_files.is_empty() {
            return Ok(());
        }

        // Process each feather file independently so that each file's identifier
        // (instrument_id or bar_type from schema metadata) is preserved when writing
        // to parquet. This matches the Python _convert_feather_table_to_parquet approach.
        let convert_bar_type = data_cls == "bars";

        for file_path in feather_files {
            let batches = self.read_feather_file(&file_path)?;

            if batches.is_empty() {
                continue;
            }

            match data_cls {
                "quotes" => {
                    let mut data: Vec<QuoteTick> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "trades" => {
                    let mut data: Vec<TradeTick> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "order_book_deltas" => {
                    let mut data: Vec<OrderBookDelta> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "order_book_depths" => {
                    let mut data: Vec<OrderBookDepth10> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "bars" => {
                    let mut data: Vec<Bar> = self
                        .convert_record_batches_to_data_with_bar_type_conversion(
                            batches,
                            use_ts_event_for_ts_init,
                            convert_bar_type,
                        )?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "index_prices" => {
                    let mut data: Vec<IndexPriceUpdate> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "mark_prices" => {
                    let mut data: Vec<MarkPriceUpdate> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                "instrument_closes" => {
                    let mut data: Vec<InstrumentClose> =
                        self.convert_record_batches_to_data(batches, use_ts_event_for_ts_init)?;

                    if !is_monotonically_increasing_by_init(&data) {
                        data.sort_by_key(|d| d.ts_init);
                    }
                    self.write_to_parquet(data, None, None, None)?;
                }
                _ => {
                    if data_cls.starts_with("custom/") {
                        let data =
                            self.decode_custom_batches_to_data(batches, use_ts_event_for_ts_init)?;
                        let custom_items: Vec<CustomData> = data
                            .into_iter()
                            .filter_map(|d| match d {
                                Data::Custom(c) => Some(c),
                                _ => None,
                            })
                            .collect();

                        if !custom_items.is_empty() {
                            self.write_custom_data_batch(custom_items, None, None, None)?;
                        }
                    } else {
                        anyhow::bail!("Unknown data class: {data_cls}");
                    }
                }
            }
        }

        Ok(())
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
/// - `$type`: The data type to implement the trait for.
/// - `$path`: The path prefix string for that type.
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
impl_catalog_path_prefix!(InstrumentAny, "instruments");

/// Converts timestamps to a filename using ISO 8601 format.
///
/// This function converts two Unix nanosecond timestamps to a filename that uses
/// ISO 8601 format with filesystem-safe characters. The format matches the Python
/// implementation for consistency.
///
/// # Parameters
///
/// - `timestamp_1`: First timestamp in Unix nanoseconds.
/// - `timestamp_2`: Second timestamp in Unix nanoseconds.
///
/// # Returns
///
/// Returns a filename string in the format: "`iso_timestamp_1_iso_timestamp_2.parquet`".
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
#[must_use]
pub fn timestamps_to_filename(timestamp_1: UnixNanos, timestamp_2: UnixNanos) -> String {
    let datetime_1 = iso_timestamp_to_file_timestamp(&unix_nanos_to_iso8601(timestamp_1));
    let datetime_2 = iso_timestamp_to_file_timestamp(&unix_nanos_to_iso8601(timestamp_2));

    format!("{datetime_1}_{datetime_2}.parquet")
}

/// Converts an ISO 8601 timestamp to a filesystem-safe format.
///
/// This function replaces colons and dots with hyphens to make the timestamp
/// safe for use in filenames across different filesystems.
///
/// # Parameters
///
/// - `iso_timestamp`: ISO 8601 timestamp string (e.g., "2023-10-26T07:30:50.123456789Z").
///
/// # Returns
///
/// Returns a filesystem-safe timestamp string (e.g., "2023-10-26T07-30-50-123456789Z").
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
/// - `file_timestamp`: Filesystem-safe timestamp string (e.g., "2023-10-26T07-30-50-123456789Z").
///
/// # Returns
///
/// Returns an ISO 8601 timestamp string (e.g., "2023-10-26T07:30:50.123456789Z").
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
        format!("{date_part}T{final_time_part}Z")
    } else {
        // Fallback if no nanoseconds part found
        let final_time_part = time_part.replace('-', ":");
        format!("{date_part}T{final_time_part}Z")
    }
}

/// Converts an ISO 8601 timestamp string to Unix nanoseconds.
///
/// This function parses an ISO 8601 timestamp and converts it to Unix nanoseconds.
/// It's used to convert parsed timestamps back to the internal representation.
///
/// # Parameters
///
/// - `iso_timestamp`: ISO 8601 timestamp string (e.g., "2023-10-26T07:30:50.123456789Z").
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
fn iso_to_unix_nanos(iso_timestamp: &str) -> anyhow::Result<u64> {
    Ok(iso8601_to_unix_nanos(iso_timestamp.to_string())?.into())
}

/// Converts an instrument ID to a URI-safe format by removing forward slashes
/// and replacing carets with underscores.
///
/// Some instrument IDs contain forward slashes (e.g., "BTC/USD") which are not
/// suitable for use in file paths. This function transforms these characters to
/// create a safe directory name.
///
/// # Parameters
///
/// - `instrument_id`: The original instrument ID string.
///
/// # Returns
///
/// A URI-safe version of the instrument ID with forward slashes removed and carets replaced.
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::urisafe_instrument_id;
/// assert_eq!(urisafe_instrument_id("BTC/USD"), "BTCUSD");
/// assert_eq!(urisafe_instrument_id("EUR-USD"), "EUR-USD");
/// assert_eq!(urisafe_instrument_id("^SPX.CBOE"), "_SPX.CBOE");
/// ```
pub fn urisafe_instrument_id(instrument_id: &str) -> String {
    instrument_id.replace('/', "").replace('^', "_")
}

// Extract the instrument ID portion from a bar type directory name.
// Handles both standard and composite formats:
//   {id}-{step}-{agg}-{price}-{source}
//   {id}-{step}-{agg}-{price}-{source}@{step}-{agg}-{source}
// Strips the composite suffix before parsing with rsplitn(5, '-').
fn extract_bar_type_instrument_id(bar_type_dir: &str) -> Option<&str> {
    let standard = bar_type_dir.split('@').next().unwrap_or(bar_type_dir);
    let pieces: Vec<&str> = standard.rsplitn(5, '-').collect();
    // pieces (reversed): [source, price_type, agg, step, instrument_id]
    if pieces.len() == 5 && pieces[3].chars().all(|c| c.is_ascii_digit()) {
        Some(pieces[4])
    } else {
        None
    }
}

/// Normalizes a custom data identifier for use in directory paths.
/// Replaces `//` with `/`, and filters out empty segments and `..` to prevent path traversal.
#[must_use]
pub fn safe_directory_identifier(identifier: &str) -> String {
    let normalized = identifier.replace("//", "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.is_empty() && *s != "..")
        .collect();
    segments.join("/")
}

/// Extracts the identifier from a file path.
///
/// The identifier is typically the second-to-last path component (directory name).
/// For example, from "`data/quote_tick/EURUSD/file.parquet`", extracts "EURUSD".
#[must_use]
pub fn extract_identifier_from_path(file_path: &str) -> String {
    let path_parts: Vec<&str> = file_path.split('/').collect();
    if path_parts.len() >= 2 {
        path_parts[path_parts.len() - 2].to_string()
    } else {
        "unknown".to_string()
    }
}

/// Makes an identifier safe for use in SQL table names.
///
/// Removes forward slashes, replaces dots, hyphens, and spaces with underscores, and converts to lowercase.
#[must_use]
pub fn make_sql_safe_identifier(identifier: &str) -> String {
    urisafe_instrument_id(identifier)
        .replace(['.', '-', ' ', '%'], "_")
        .to_lowercase()
}

/// Extracts the filename from a file path and makes it SQL-safe.
///
/// For example, from "data/quote_tick/EURUSD/2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet",
/// extracts "`2021_01_01t00_00_00_000000000z_2021_01_02t00_00_00_000000000z`".
#[must_use]
pub fn extract_sql_safe_filename(file_path: &str) -> String {
    if file_path.is_empty() {
        return "unknown_file".to_string();
    }

    let filename = file_path.split('/').next_back().unwrap_or("unknown_file");

    // Remove .parquet extension
    let name_without_ext = if let Some(dot_pos) = filename.rfind(".parquet") {
        &filename[..dot_pos]
    } else {
        filename
    };

    // Remove characters that can pose problems: hyphens, colons, etc.
    name_without_ext
        .replace(['-', ':', '.'], "_")
        .to_lowercase()
}

/// Creates a platform-appropriate local path using `PathBuf`.
///
/// This function constructs file system paths using the platform's native path separators.
/// Use this for local file operations that need to work with the actual file system.
///
/// # Arguments
///
/// * `base_path` - The base directory path
/// * `components` - Path components to join
///
/// # Returns
///
/// A `PathBuf` with platform-appropriate separators
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::make_local_path;
/// let path = make_local_path("/base", &["data", "quotes", "EURUSD"]);
/// // On Unix: "/base/data/quotes/EURUSD"
/// // On Windows: "\base\data\quotes\EURUSD"
/// ```
pub fn make_local_path<P: AsRef<Path>>(base_path: P, components: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(base_path.as_ref());
    for component in components {
        path.push(component);
    }
    path
}

/// Creates an object store path using forward slashes.
///
/// Object stores (S3, GCS, etc.) always expect forward slashes regardless of platform.
/// Use this when creating paths for object store operations.
///
/// # Arguments
///
/// * `base_path` - The base path (can be empty)
/// * `components` - Path components to join
///
/// # Returns
///
/// A string path with forward slash separators
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::make_object_store_path;
/// let path = make_object_store_path("base", &["data", "quotes", "EURUSD"]);
/// assert_eq!(path, "base/data/quotes/EURUSD");
/// ```
#[must_use]
pub fn make_object_store_path(base_path: &str, components: &[&str]) -> String {
    let mut parts = Vec::new();

    if !base_path.is_empty() {
        let normalized_base = base_path
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();

        if !normalized_base.is_empty() {
            parts.push(normalized_base);
        }
    }

    for component in components {
        let normalized_component = component
            .replace('\\', "/")
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();

        if !normalized_component.is_empty() {
            parts.push(normalized_component);
        }
    }

    parts.join("/")
}

/// Creates an object store path using forward slashes with owned strings.
///
/// This variant accepts owned strings to avoid lifetime issues.
///
/// # Arguments
///
/// * `base_path` - The base path (can be empty)
/// * `components` - Path components to join (owned strings)
///
/// # Returns
///
/// A string path with forward slash separators
#[must_use]
pub fn make_object_store_path_owned(base_path: &str, components: Vec<String>) -> String {
    let mut parts = Vec::new();

    if !base_path.is_empty() {
        let normalized_base = base_path
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();

        if !normalized_base.is_empty() {
            parts.push(normalized_base);
        }
    }

    for component in components {
        let normalized_component = component
            .replace('\\', "/")
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();

        if !normalized_component.is_empty() {
            parts.push(normalized_component);
        }
    }

    parts.join("/")
}

/// Converts a local `PathBuf` to an object store path string.
///
/// This function normalizes a local file system path to the forward-slash format
/// expected by object stores, handling platform differences.
///
/// # Arguments
///
/// * `local_path` - The local `PathBuf` to convert
///
/// # Returns
///
/// A string with forward slash separators suitable for object store operations
///
/// # Examples
///
/// ```rust
/// # use std::path::PathBuf;
/// # use nautilus_persistence::backend::catalog::local_to_object_store_path;
/// let local_path = PathBuf::from("data").join("quotes").join("EURUSD");
/// let object_path = local_to_object_store_path(&local_path);
/// assert_eq!(object_path, "data/quotes/EURUSD");
/// ```
#[must_use]
pub fn local_to_object_store_path(local_path: &Path) -> String {
    local_path.to_string_lossy().replace('\\', "/")
}

/// Extracts path components using platform-appropriate path parsing.
///
/// This function safely parses a path into its components, handling both
/// local file system paths and object store paths correctly.
///
/// # Arguments
///
/// * `path_str` - The path string to parse
///
/// # Returns
///
/// A vector of path components
///
/// # Examples
///
/// ```rust
/// # use nautilus_persistence::backend::catalog::extract_path_components;
/// let components = extract_path_components("data/quotes/EURUSD");
/// assert_eq!(components, vec!["data", "quotes", "EURUSD"]);
///
/// // Works with both separators
/// let components = extract_path_components("data\\quotes\\EURUSD");
/// assert_eq!(components, vec!["data", "quotes", "EURUSD"]);
/// ```
#[must_use]
pub fn extract_path_components(path_str: &str) -> Vec<String> {
    // Normalize separators and split
    let normalized = path_str.replace('\\', "/");
    normalized
        .split('/')
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Checks if a filename's timestamp range intersects with a query interval.
///
/// This function determines whether a Parquet file (identified by its timestamp-based
/// filename) contains data that falls within the specified query time range.
///
/// # Parameters
///
/// - `filename`: The filename to check (format: "`iso_timestamp_1_iso_timestamp_2.parquet`").
/// - `start`: Optional start timestamp for the query range.
/// - `end`: Optional end timestamp for the query range.
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
/// assert!(query_intersects_filename(
///     "2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet",
///     Some(1609459200000000000),
///     Some(1609545600000000000)
/// ));
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
/// "`iso_timestamp_1_iso_timestamp_2.parquet`" (e.g., "2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet")
///
/// # Parameters
///
/// - `filename`: The filename to parse (can be a full path).
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
#[must_use]
pub fn parse_filename_timestamps(filename: &str) -> Option<(u64, u64)> {
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
/// - `intervals`: A slice of timestamp intervals as (start, end) tuples.
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
#[must_use]
pub fn are_intervals_disjoint(intervals: &[(u64, u64)]) -> bool {
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
/// - `intervals`: A slice of timestamp intervals as (start, end) tuples.
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
#[must_use]
pub fn are_intervals_contiguous(intervals: &[(u64, u64)]) -> bool {
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
/// - `start`: Start timestamp of the query interval (inclusive).
/// - `end`: End timestamp of the query interval (inclusive).
/// - `closed_intervals`: Existing data intervals as (start, end) tuples.
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
    let query_range = (RangeBound::Included(start), RangeBound::Included(end));
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
/// - `intervals`: A slice of closed intervals as (start, end) tuples.
///
/// # Returns
///
/// Returns an [`IntervalTree`] containing the converted intervals.
///
/// # Notes
///
/// - Invalid intervals (where start > end) are skipped.
/// - Uses saturating addition to prevent overflow when converting to half-open intervals.
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
            RangeBound::Included(start),
            RangeBound::Excluded(end.saturating_add(1)),
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
/// - `interval`: The bounded interval from the interval tree.
/// - `query_start`: The start of the original query range.
/// - `query_end`: The end of the original query range.
///
/// # Returns
///
/// Returns `Some((start, end))` for valid intervals, `None` for empty intervals.
fn interval_to_tuple(
    interval: (RangeBound<&u64>, RangeBound<&u64>),
    query_start: u64,
    query_end: u64,
) -> Option<(u64, u64)> {
    let (bound_start, bound_end) = interval;

    let start = match bound_start {
        RangeBound::Included(val) => *val,
        RangeBound::Excluded(val) => val.saturating_add(1),
        RangeBound::Unbounded => query_start,
    };

    let end = match bound_end {
        RangeBound::Included(val) => *val,
        RangeBound::Excluded(val) => {
            if *val == 0 {
                return None; // Empty interval
            }
            val - 1
        }
        RangeBound::Unbounded => query_end,
    };

    if start <= end {
        Some((start, end))
    } else {
        None
    }
}
