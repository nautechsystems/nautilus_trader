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
//! use std::path::Path;
//!
//! use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
//!
//! // Create a new catalog
//! let catalog = ParquetDataCatalog::new(
//!     Path::new("/path/to/data"),
//!     None,       // storage_options
//!     Some(5000), // batch_size
//!     None,       // compression (defaults to SNAPPY)
//!     None,       // max_row_group_size (defaults to 131,072)
//! );
//!
//! // Write data to the catalog
//! // catalog.write_to_parquet(&data, None, None, None)?;
//! ```

#![expect(
    clippy::missing_fields_in_debug,
    reason = "catalog Debug redacts internal caches"
)]

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};

use ahash::AHashMap;
use arrow::record_batch::RecordBatch;
use futures::StreamExt;
use nautilus_core::{
    Params, UnixNanos,
    string::{conversions::to_snake_case, urlencoding},
};
use nautilus_model::{
    data::{
        Bar, CustomData, Data, DataBatch, FundingRateUpdate, HasTsInit, IndexPriceUpdate,
        InstrumentStatus, MarkPriceUpdate, NautilusDataType, NautilusRecordType, OptionGreeks,
        OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
        is_monotonically_increasing_by_init,
    },
    instruments::{Instrument, InstrumentAny},
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, DecodeTypedFromRecordBatch,
    EncodeToRecordBatch, catalog_display::catalog_record_batch_to_display,
    custom::CustomDataDecoder, display::instrument::encode_instruments,
    record_batch_without_identifier_column,
};
use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjectPath};
use serde::Serialize;

use crate::{
    backend::parquet::{
        intervals::query_interval_diff,
        io::{
            append_path_to_file_uri, decode_object_store_segment, is_remote_uri_scheme,
            read_parquet_from_object_store, read_parquet_schema_from_object_store, remote_full_uri,
            remote_store_root_url, write_batches_to_object_store,
        },
        paths::{extract_bar_type_instrument_id, query_intersects_filename},
    },
    catalog::{
        session::{DEFAULT_DATA_BATCH_CHUNK_SIZE, DataBatchQueryResult, TypedDataBatchSession},
        traits::{
            CatalogInstrumentQuery, CatalogMetadata, CatalogQuery, CatalogReader,
            CatalogRecordQuery, CatalogWriter, DataCatalogBox, filter_instrument_query_result,
            filter_instruments_for_request_range,
        },
        types::{
            CatalogAsOf, CatalogDataType, INSTRUMENT_PATH_PREFIXES, instrument_any_type,
            instrument_path_prefix, parquet_data_path_prefix, record_path_prefix,
        },
    },
    common::{
        custom::prepare_custom_data_batch,
        datafusion::{self as datafusion, DataBackendSession, build_query},
    },
};

/// Optional Parquet query file-loading hint parameter key.
pub const QUERY_OPTIMIZE_FILE_LOADING: &str = "optimize_file_loading";

/// Optional Parquet write overlap bypass parameter key.
pub const WRITE_SKIP_DISJOINT_CHECK: &str = "skip_disjoint_check";

macro_rules! define_builtin_data_dispatch {
    (
        (Instrument, InstrumentAny, Instrument, Instrument, $instrument_prefix:literal),
        $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?
    ) => {


        fn query_builtin_batch(
            catalog: &mut ParquetDataCatalog,
            data_type: &NautilusDataType,
            identifiers: Option<Vec<String>>,
            start: Option<UnixNanos>,
            end: Option<UnixNanos>,
            where_clause: Option<&str>,
            optimize_file_loading: bool,
        ) -> Option<anyhow::Result<DataBatch>> {
            match data_type {
                $(
                    NautilusDataType::$variant => Some(
                        catalog
                            .query_typed_data::<$type>(
                                identifiers,
                                start,
                                end,
                                where_clause,
                                None,
                                optimize_file_loading,
                            )
                            .map(|data| DataBatch::$batch(data.into())),
                    ),
                )+
                _ => None,
            }
        }

        fn write_catalog_batch(
            catalog: &ParquetDataCatalog,
            batch: &DataBatch,
            start: Option<UnixNanos>,
            end: Option<UnixNanos>,
            skip_disjoint_check: Option<bool>,
        ) -> anyhow::Result<()> {
            #[allow(unreachable_patterns, reason = "reject unsupported variants introduced by feature unification")]
            match batch {
                DataBatch::BookDeltas(data) => {
                    let deltas = data.iter().flat_map(|batch| batch.deltas.iter().copied()).collect::<Vec<_>>();
                    catalog.write_grouped_to_parquet(&deltas, start, end, skip_disjoint_check)
                }
                DataBatch::Custom(data) => catalog.write_custom_data_batch(data.as_ref(), start, end, skip_disjoint_check).map(|_| ()),
                DataBatch::Instrument(data) => catalog.write_instruments(data.as_ref().to_vec()).map(|_| ()),
                $(DataBatch::$batch(data) => catalog.write_grouped_to_parquet(data.as_ref(), start, end, skip_disjoint_check),)+
                _ => anyhow::bail!("Unsupported catalog data batch: {}", batch.data_type_name()),
            }
        }
    };
}

nautilus_model::for_each_data_type!(define_builtin_data_dispatch);

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

mod coverage;
mod query;
mod session;
mod store;
mod write;

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
    /// - `max_row_group_size`: Maximum rows per Parquet row group (default: 131,072).
    ///
    /// # Panics
    ///
    /// Panics if the path cannot be converted to a valid URI or if the object store
    /// cannot be created from the path.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::path::Path;
    ///
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(
    ///     Path::new("/tmp/nautilus_data"),
    ///     None,       // no storage options
    ///     Some(1000), // smaller batch size
    ///     None,       // default compression
    ///     None,       // default row group size
    /// );
    /// ```
    #[must_use]
    pub fn new(
        base_path: &Path,
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
    /// - `max_row_group_size`: Maximum rows per Parquet row group (default: 131,072).
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
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// // Local filesystem
    /// let local_catalog = ParquetDataCatalog::from_uri("/tmp/nautilus_data", None, None, None, None)?;
    ///
    /// // S3 bucket
    /// let s3_catalog =
    ///     ParquetDataCatalog::from_uri("s3://my-bucket/nautilus-data", None, None, None, None)?;
    ///
    /// // Google Cloud Storage
    /// let gcs_catalog =
    ///     ParquetDataCatalog::from_uri("gs://my-bucket/nautilus-data", None, None, None, None)?;
    ///
    /// // Azure Blob Storage
    /// let azure_catalog =
    ///     ParquetDataCatalog::from_uri("az://container/nautilus-data", None, None, None, None)?;
    ///
    /// // S3 with custom endpoint and credentials
    /// let mut storage_options = AHashMap::new();
    /// storage_options.insert(
    ///     "endpoint_url".to_string(),
    ///     "https://my-s3-endpoint.com".to_string(),
    /// );
    /// storage_options.insert("access_key_id".to_string(), "my-key".to_string());
    /// storage_options.insert("secret_access_key".to_string(), "my-secret".to_string());
    ///
    /// let custom_s3_catalog = ParquetDataCatalog::from_uri(
    ///     "s3://my-bucket/nautilus-data",
    ///     Some(storage_options),
    ///     None,
    ///     None,
    ///     None,
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
        let batch_size = batch_size.unwrap_or(DEFAULT_DATA_BATCH_CHUNK_SIZE);
        let compression = compression.unwrap_or(parquet::basic::Compression::ZSTD(
            parquet::basic::ZstdLevel::default(),
        ));
        let max_row_group_size =
            max_row_group_size.unwrap_or(crate::backend::parquet::DEFAULT_ROW_GROUP_SIZE);

        let location = crate::backend::parquet::io::create_object_store_location_from_path(
            uri,
            storage_options,
        )?;

        Ok(Self {
            base_path: location.base_path,
            original_uri: location.original_uri,
            object_store: location.object_store,
            session: DataBackendSession::new(batch_size),
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

    /// Clears cached table registrations so a later query re-reads files that changed.
    ///
    /// Catalog operations that modify files call this before querying again.
    pub(crate) fn clear_session_tables(&mut self) {
        self.session.clear_registered_tables();
    }
}

impl CatalogReader for ParquetDataCatalog {
    fn fork_query_catalog(&self) -> anyhow::Result<Option<DataCatalogBox>> {
        Ok(Some(Box::new(Self {
            base_path: self.base_path.clone(),
            original_uri: self.original_uri.clone(),
            object_store: self.object_store.clone(),
            session: DataBackendSession::new(self.batch_size),
            batch_size: self.batch_size,
            compression: self.compression,
            max_row_group_size: self.max_row_group_size,
        })))
    }

    fn query_batch_session(
        &mut self,
        query: &CatalogQuery,
        chunk_size: Option<usize>,
    ) -> anyhow::Result<DataBatchQueryResult> {
        ensure_latest_query(query)?;
        macro_rules! batch_session {
            ((Instrument, InstrumentAny, Instrument, Instrument, $instrument_prefix:literal), $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
                match &query.data_type {
                    $(NautilusDataType::$variant => {
                        let pages = self.query_typed_pages::<$type>(
                            query.identifiers.clone(), query.start, query.end, query.where_clause.as_deref(), None,
                            query.params.as_ref().and_then(|params| params.get_bool(QUERY_OPTIMIZE_FILE_LOADING)).unwrap_or(true),
                        )?;
                        Ok(Box::new(TypedDataBatchSession::new(pages, chunk_size)) as DataBatchQueryResult)
                    },)+
                    _ => match self.query_batch(query)? {
                        DataBatch::Instrument(data) => Ok(Box::new(TypedDataBatchSession::from_vec(data.as_ref().to_vec(), chunk_size))),
                        DataBatch::Custom(data) => Ok(Box::new(TypedDataBatchSession::from_vec(data.as_ref().to_vec(), chunk_size))),
                        _ => anyhow::bail!("Unsupported Parquet query family"),
                    },
                }
            };
        }
        nautilus_model::for_each_data_type!(batch_session)
    }

    fn reset_session(&mut self) {
        self.clear_session_tables();
    }

    fn instruments(
        &mut self,
        query: &CatalogInstrumentQuery,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let CatalogInstrumentQuery {
            instrument_ids,
            start,
            end,
            where_clause,
        } = query.clone();
        let instrument_ids = instrument_ids.as_deref();
        self.query_instruments_filtered_with_where(
            instrument_ids,
            start,
            end,
            where_clause.as_deref(),
        )
    }

    fn query_batch(&mut self, query: &CatalogQuery) -> anyhow::Result<DataBatch> {
        ensure_latest_query(query)?;
        let CatalogQuery {
            data_type,
            identifiers,
            start,
            end,
            where_clause,
            params,
            ..
        } = query.clone();
        let where_clause = where_clause.as_deref();
        let optimize_file_loading = params
            .as_ref()
            .and_then(|params| params.get_bool(QUERY_OPTIMIZE_FILE_LOADING))
            .unwrap_or(true);

        match data_type {
            NautilusDataType::Instrument => {
                let data = self.query_instruments_filtered_with_where(
                    identifiers.as_deref(),
                    start,
                    end,
                    where_clause,
                )?;
                Ok(DataBatch::Instrument(
                    filter_instrument_query_result(data, start, params.as_ref()).into(),
                ))
            }
            NautilusDataType::Custom { type_name } => {
                let data = self.query_custom_data_dynamic(
                    &type_name,
                    identifiers.as_deref(),
                    start,
                    end,
                    where_clause,
                    None,
                    optimize_file_loading,
                )?;
                Ok(DataBatch::Custom(
                    data.into_iter()
                        .filter_map(|item| match item {
                            Data::Custom(custom) => Some(custom),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .into(),
                ))
            }
            #[cfg(feature = "defi")]
            NautilusDataType::Defi => Err(anyhow::Error::from(
                crate::errors::PersistenceError::unsupported("Parquet catalog DeFi data"),
            )),
            data_type => query_builtin_batch(
                self,
                &data_type,
                identifiers,
                start,
                end,
                where_clause,
                optimize_file_loading,
            )
            .expect("built-in data type dispatch is exhaustive"),
        }
    }

    fn query_identifiers(&mut self, query: &CatalogQuery) -> anyhow::Result<Vec<String>> {
        ensure_latest_query(query)?;
        let CatalogQuery {
            data_type,
            identifiers,
            start,
            end,
            where_clause,
            params,
            ..
        } = query.clone();

        if data_type == NautilusDataType::Instrument {
            let mut identifiers = self
                .query_instruments_filtered_with_where(
                    identifiers.as_deref(),
                    start,
                    end,
                    where_clause.as_deref(),
                )?
                .into_iter()
                .map(|instrument| instrument.id().to_string())
                .collect::<Vec<_>>();
            identifiers.sort();
            identifiers.dedup();
            return Ok(identifiers);
        }

        let optimize_file_loading = params
            .as_ref()
            .and_then(|params| params.get_bool(QUERY_OPTIMIZE_FILE_LOADING))
            .unwrap_or(true);
        let type_name = parquet_data_path_prefix(&data_type);

        Self::query_identifiers(
            self,
            type_name.as_ref(),
            identifiers,
            start,
            end,
            where_clause.as_deref(),
            optimize_file_loading,
        )
    }

    fn query_display_record_batches(
        &mut self,
        query: &CatalogQuery,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        ensure_latest_query(query)?;
        let CatalogQuery {
            data_type,
            identifiers,
            start,
            end,
            where_clause,
            params,
            ..
        } = query.clone();

        if data_type == NautilusDataType::Instrument {
            let instruments = self.query_instruments_filtered_with_where(
                identifiers.as_deref(),
                start,
                end,
                where_clause.as_deref(),
            )?;
            return if instruments.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![encode_instruments(&instruments)?])
            };
        }

        let optimize_file_loading = params
            .as_ref()
            .and_then(|params| params.get_bool(QUERY_OPTIMIZE_FILE_LOADING))
            .unwrap_or(true);
        Self::query_display_record_batches(
            self,
            &data_type,
            identifiers,
            start,
            end,
            where_clause.as_deref(),
            optimize_file_loading,
        )
    }

    fn query_record_batches(
        &mut self,
        query: &CatalogRecordQuery,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        anyhow::ensure!(
            query.as_of == CatalogAsOf::Latest,
            "Parquet catalog does not support historical queries"
        );
        let CatalogRecordQuery {
            record_type,
            identifier,
            start,
            end,
            where_clause,
            params,
            ..
        } = query.clone();
        let optimize_file_loading = params
            .as_ref()
            .and_then(|params| params.get_bool(QUERY_OPTIMIZE_FILE_LOADING))
            .unwrap_or(true);
        let type_name = record_path_prefix(&record_type);

        Self::query_record_batches(
            self,
            type_name.as_ref(),
            identifier,
            start,
            end,
            where_clause.as_deref(),
            optimize_file_loading,
        )
    }

    fn query_record_display_batches(
        &mut self,
        query: &CatalogRecordQuery,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        anyhow::ensure!(
            query.as_of == CatalogAsOf::Latest,
            "Parquet catalog does not support historical queries"
        );
        CatalogReader::query_record_batches(self, query)
    }

    fn query_metadata(&mut self, query: &CatalogQuery) -> anyhow::Result<Vec<CatalogMetadata>> {
        ensure_latest_query(query)?;
        let CatalogQuery {
            data_type,
            identifiers,
            start,
            end,
            where_clause,
            ..
        } = query.clone();

        if data_type == NautilusDataType::Instrument {
            let mut metadata = Vec::new();
            for prefix in INSTRUMENT_PATH_PREFIXES {
                metadata.extend(Self::query_metadata(
                    self,
                    prefix,
                    identifiers.clone(),
                    start,
                    end,
                    where_clause.as_deref(),
                )?);
            }
            metadata.sort_by_key(|entry| entry.first_ts_init);
            return Ok(metadata);
        }

        let type_name = parquet_data_path_prefix(&data_type);

        Self::query_metadata(
            self,
            type_name.as_ref(),
            identifiers,
            start,
            end,
            where_clause.as_deref(),
        )
    }

    fn get_missing_intervals_for_request(
        &mut self,
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifier: Option<&str>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        match data_type {
            NautilusDataType::Instrument => {
                let identifiers = identifier.map(|value| vec![value.to_string()]);
                let data = self.query_instruments_filtered(
                    identifiers.as_deref(),
                    Some(start),
                    Some(end),
                )?;
                Ok(if data.is_empty() {
                    vec![(start.as_u64(), end.as_u64())]
                } else {
                    Vec::new()
                })
            }
            NautilusDataType::Custom { type_name } => {
                if let Some(identifier) = identifier {
                    let directory = self.make_path_custom_data(&type_name, Some(identifier))?;
                    let intervals = self.get_directory_intervals(&directory)?;

                    Ok(query_interval_diff(
                        start.as_u64(),
                        end.as_u64(),
                        &intervals,
                    ))
                } else {
                    let data_cls =
                        parquet_data_path_prefix(&NautilusDataType::Custom { type_name });
                    Self::get_missing_intervals_for_request(
                        self,
                        start.as_u64(),
                        end.as_u64(),
                        data_cls.as_ref(),
                        None,
                    )
                }
            }
            _ => Self::get_missing_intervals_for_request(
                self,
                start.as_u64(),
                end.as_u64(),
                parquet_data_path_prefix(&data_type).as_ref(),
                identifier,
            ),
        }
    }

    fn query_last_timestamp(
        &mut self,
        data_type: NautilusDataType,
        identifier: Option<&str>,
    ) -> anyhow::Result<Option<u64>> {
        match data_type {
            NautilusDataType::Instrument => {
                let identifiers = identifier.map(|value| vec![value.to_string()]);
                Ok(self
                    .query_instruments(identifiers.as_deref())?
                    .into_iter()
                    .map(|instrument| HasTsInit::ts_init(&instrument).as_u64())
                    .max())
            }
            NautilusDataType::Custom { type_name } => {
                if let Some(identifier) = identifier {
                    let directory = self.make_path_custom_data(&type_name, Some(identifier))?;
                    let intervals = self.get_directory_intervals(&directory)?;

                    Ok(intervals.into_iter().map(|(_, end)| end).max())
                } else {
                    let data_cls =
                        parquet_data_path_prefix(&NautilusDataType::Custom { type_name });
                    Self::query_last_timestamp(self, data_cls.as_ref(), None)
                }
            }
            _ => Self::query_last_timestamp(
                self,
                parquet_data_path_prefix(&data_type).as_ref(),
                identifier,
            ),
        }
    }
}

impl CatalogWriter for ParquetDataCatalog {
    fn write_instruments(&mut self, instruments: &[InstrumentAny]) -> anyhow::Result<()> {
        Self::write_instruments(self, instruments.to_vec()).map(|_| ())
    }

    fn write_data(
        &mut self,
        data: &[Data],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let skip_disjoint_check = params
            .as_ref()
            .and_then(|params| params.get_bool(WRITE_SKIP_DISJOINT_CHECK));
        self.write_data_enum(data, start, end, skip_disjoint_check)
    }

    fn write_data_batch(
        &mut self,
        batch: &DataBatch,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        let skip_disjoint_check = params
            .as_ref()
            .and_then(|params| params.get_bool(WRITE_SKIP_DISJOINT_CHECK));

        write_catalog_batch(self, batch, start, end, skip_disjoint_check)
    }

    fn write_records(
        &mut self,
        record_type: NautilusRecordType,
        batches: &[RecordBatch],
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        let params = params.unwrap_or_default();
        let identifier = params.get_str("identifier").map(str::to_owned);
        self.write_record_batches(&record_type, identifier.as_deref(), batches, &params)
    }

    fn record_empty_coverage(
        &mut self,
        data_type: NautilusDataType,
        identifier: Option<&str>,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()> {
        match &data_type {
            NautilusDataType::Instrument => {
                anyhow::bail!(
                    "Cannot record empty instrument coverage without a concrete instrument type"
                )
            }
            NautilusDataType::Custom { type_name } => {
                let directory = self.make_path_custom_data(type_name, identifier)?;
                self.extend_file_name_in_directory(&directory, start, end)
            }
            _ => {
                let data_cls = parquet_data_path_prefix(&data_type);
                self.extend_file_name(data_cls.as_ref(), identifier, start, end)
            }
        }
    }
}

// Re-export public items from sibling modules so historical
// `crate::backend::parquet::catalog::...` imports continue to resolve.
pub use crate::backend::parquet::{
    intervals::{are_intervals_contiguous, are_intervals_disjoint},
    paths::{
        CatalogPathPrefix, extract_identifier_from_path, extract_path_components,
        extract_sql_safe_filename, local_to_object_store_path, make_local_path,
        make_object_store_path, make_sql_safe_identifier, parse_filename_timestamps,
        safe_directory_identifier, timestamps_to_filename, urisafe_instrument_id,
    },
};

fn ensure_latest_query(query: &CatalogQuery) -> anyhow::Result<()> {
    anyhow::ensure!(
        query.as_of == CatalogAsOf::Latest,
        "Parquet catalog does not support historical queries"
    );
    anyhow::ensure!(
        query.data_type != NautilusDataType::OrderBook,
        "Parquet catalog does not support order book snapshots"
    );
    Ok(())
}
