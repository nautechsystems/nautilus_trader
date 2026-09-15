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

//! Parquet catalog query paths and typed query wrappers.

#![expect(
    clippy::missing_errors_doc,
    clippy::used_underscore_binding,
    reason = "query methods forward DataFusion errors and underscore fields mirror SQL aliases"
)]

use nautilus_model::instruments::NautilusInstrumentType;
use nautilus_serialization::arrow::{
    catalog_identifier_from_metadata, instrument::decode_instrument_any_batch,
    record_batch_with_identifier_column,
};

use super::{
    ArrowSchemaProvider, Bar, CatalogDataType, CustomDataDecoder, Data, DecodeDataFromRecordBatch,
    DecodeTypedFromRecordBatch, FundingRateUpdate, HasTsInit, HashMap, INSTRUMENT_PATH_PREFIXES,
    InstrumentAny, InstrumentClose, NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth,
    ParquetDataCatalog, Path, QuoteTick, RecordBatch, TradeTick, UnixNanos, build_query,
    catalog_record_batch_to_display, datafusion, decode_object_store_segment,
    extract_bar_type_instrument_id, extract_identifier_from_path, extract_sql_safe_filename,
    filter_instruments_for_request_range, instrument_path_prefix,
    is_monotonically_increasing_by_init, make_object_store_path, make_sql_safe_identifier,
    parquet_data_path_prefix, parse_filename_timestamps, query_intersects_filename,
    read_parquet_from_object_store, read_parquet_schema_from_object_store,
    session::{MergedPages, TypedPages, decode_typed_pages},
    urisafe_instrument_id,
};
use crate::common::arrow::empty_display_batch_with_identifier;

impl ParquetDataCatalog {
    /// Queries one data family through the existing row iterator API.
    pub fn query<T>(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<crate::backend::session::QueryResult>
    where
        T: DecodeTypedFromRecordBatch + CatalogDataType + HasTsInit + Into<Data> + Send + 'static,
    {
        self.query_typed_pages::<T>(
            identifiers,
            start,
            end,
            where_clause,
            files,
            optimize_file_loading,
        )
        .map(crate::backend::session::QueryResult::from_typed_pages)
    }

    /// Queries instruments from the catalog.
    ///
    /// Instruments are stored under v1-compatible concrete instrument type folders:
    /// `data/{instrument_type}/{instrument_id}/`.
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
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Query all instruments
    /// let instruments = catalog.query_instruments(None)?;
    ///
    /// // Query specific instruments
    /// let instrument_ids = vec!["EUR/USD.SIM".to_string()];
    /// let instruments = catalog.query_instruments(Some(&instrument_ids))?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_instruments(
        &self,
        instrument_ids: Option<&[String]>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.query_instruments_filtered(instrument_ids, None, None)
    }

    /// Queries instruments from the catalog with optional timestamp filtering.
    ///
    /// This reads all matching parquet files under
    /// `data/{instrument_type}/{instrument_id}/`, decodes the records back to
    /// `InstrumentAny`, and filters them by `ts_init` when a range is provided.
    pub fn query_instruments_filtered(
        &self,
        instrument_ids: Option<&[String]>,
        _start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let instrument_files = self.discover_instrument_files(instrument_ids, end, None)?;
        self.decode_instrument_files(instrument_files, _start, end)
    }

    /// Queries instruments from the catalog with optional timestamp and SQL filtering.
    ///
    /// When `where_clause` is provided, the predicate is applied through DataFusion
    /// before instrument records are decoded.
    pub fn query_instruments_filtered_with_where(
        &mut self,
        instrument_ids: Option<&[String]>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.query_instruments_filtered_with_where_and_type(
            instrument_ids,
            start,
            end,
            where_clause,
            None,
        )
    }

    pub fn query_instruments_filtered_with_where_and_type(
        &mut self,
        instrument_ids: Option<&[String]>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        instrument_type: Option<&NautilusInstrumentType>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let Some(where_clause) = where_clause else {
            let instrument_files =
                self.discover_instrument_files(instrument_ids, end, instrument_type)?;
            return self.decode_instrument_files(instrument_files, start, end);
        };

        self.session.clear_registered_tables();
        self.register_remote_object_store()?;

        let mut all_instruments = Vec::new();
        let instrument_files =
            self.discover_instrument_files(instrument_ids, end, instrument_type)?;

        for (index, file_path) in instrument_files.into_iter().enumerate() {
            let object_path = self.to_object_path_parsed(&file_path)?;
            let (_, builder_schema) = self.execute_async(|| async {
                read_parquet_from_object_store(self.object_store.clone(), &object_path).await
            })?;
            let metadata: std::collections::HashMap<String, String> =
                builder_schema.metadata().clone();
            let target_schema = InstrumentAny::get_schema(Some(metadata.clone()));

            let table_name = format!(
                "instruments_{}_{}",
                index,
                extract_sql_safe_filename(&file_path)
            );
            let query = build_query(&table_name, start, end, Some(where_clause));
            let resolved_path = self.resolve_path_for_datafusion(&file_path);
            let batches = self.session.collect_parquet_files_batches(
                &table_name,
                vec![resolved_path],
                Some(&query),
            )?;

            for batch in batches {
                let batch = datafusion::cast_record_batch_to_schema(&batch, &target_schema)?;
                all_instruments.extend(decode_instrument_any_batch(&metadata, &batch)?);
            }
        }

        Ok(filter_instruments_for_request_range(
            all_instruments,
            start,
            end,
        ))
    }

    /// Discovers instrument parquet files under `data/{instrument_type}/{instrument_id}/`,
    /// filtered by instrument IDs and an optional `end` timestamp, sorted by path.
    fn discover_instrument_files(
        &self,
        instrument_ids: Option<&[String]>,
        end: Option<UnixNanos>,
        instrument_type: Option<&NautilusInstrumentType>,
    ) -> anyhow::Result<Vec<String>> {
        let base_dir = make_object_store_path(&self.base_path, ["data"]);
        let end_u64 = end.map(|ts| ts.as_u64());
        let list_result = self.list_objects(&base_dir)?;

        let mut instrument_files = Vec::new();

        for object in list_result {
            let path_str = object.location.to_string();
            if !path_str.ends_with(".parquet") {
                continue;
            }

            let path_parts: Vec<&str> = path_str.split('/').collect();
            let Some(data_index) = path_parts.iter().position(|part| *part == "data") else {
                continue;
            };
            let Some(type_dir) = path_parts.get(data_index + 1) else {
                continue;
            };

            let type_dir = decode_object_store_segment(type_dir);
            if !is_parquet_instrument_type_prefix(&type_dir)
                || instrument_type.is_some_and(|value| instrument_path_prefix(value) != type_dir)
            {
                continue;
            }

            if path_parts.len() < data_index + 4 {
                continue;
            }

            let instrument_id_dir = decode_object_store_segment(path_parts[path_parts.len() - 2]);

            if let Some(ids) = instrument_ids
                && !ids
                    .iter()
                    .map(|id| urisafe_instrument_id(id))
                    .any(|x| x.as_str() == urisafe_instrument_id(&instrument_id_dir))
            {
                continue;
            }

            let include_file = if path_str.ends_with("/instrument.parquet") {
                true
            } else if let Some((file_start, _)) = parse_filename_timestamps(&path_str) {
                end_u64.is_none_or(|end| file_start <= end)
            } else {
                // Include files with nonstandard names rather than silently dropping
                // instruments written by external or older tooling.
                log::warn!(
                    "Including instrument file with unparsable interval filename: {path_str}"
                );
                true
            };

            if include_file {
                instrument_files.push(path_str);
            }
        }

        instrument_files.sort();
        Ok(instrument_files)
    }

    fn decode_instrument_files(
        &self,
        instrument_files: Vec<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut instruments = Vec::new();

        for file_path in instrument_files {
            let object_path = self.to_object_path_parsed(&file_path)?;
            let (batches, builder_schema) = self.execute_async(|| async {
                read_parquet_from_object_store(self.object_store.clone(), &object_path).await
            })?;
            let metadata = builder_schema.metadata().clone();
            let target_schema = InstrumentAny::get_schema(Some(metadata.clone()));

            for batch in batches {
                let batch = datafusion::cast_record_batch_to_schema(&batch, &target_schema)?;
                instruments.extend(decode_instrument_any_batch(&metadata, &batch)?);
            }
        }

        Ok(filter_instruments_for_request_range(
            instruments,
            start,
            end,
        ))
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
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings (e.g., "EUR/USD.SIM")
    ///   or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"). If `None`, queries all identifiers.
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
    /// use nautilus_core::UnixNanos;
    /// use nautilus_model::data::{Bar, QuoteTick, TradeTick};
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Query all quotes for a specific instrument
    /// let quotes: Vec<QuoteTick> = catalog.query_typed_data(
    ///     Some(vec!["EUR/USD.SIM".to_string()]),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    ///     true,
    /// )?;
    ///
    /// // Query trades within a specific time range
    /// let trades: Vec<TradeTick> = catalog.query_typed_data(
    ///     Some(vec!["BTC/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     None,
    ///     None,
    ///     true,
    /// )?;
    ///
    /// // Query bars with volume filter (using instrument_id - partial match for bar_type)
    /// let bars: Vec<Bar> = catalog.query_typed_data(
    ///     Some(vec!["AAPL.NASDAQ".to_string()]),
    ///     None,
    ///     None,
    ///     Some("volume > 1000000"),
    ///     None,
    ///     true,
    /// )?;
    ///
    /// // Query bars with specific bar_type
    /// let bars: Vec<Bar> = catalog.query_typed_data(
    ///     Some(vec!["AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL".to_string()]),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    ///     true,
    /// )?;
    ///
    /// // Query multiple instruments with price filter
    /// let quotes: Vec<QuoteTick> = catalog.query_typed_data(
    ///     Some(vec!["EUR/USD.SIM".to_string(), "GBP/USD.SIM".to_string()]),
    ///     None,
    ///     None,
    ///     Some("bid_price > 1.2000 AND ask_price < 1.3000"),
    ///     None,
    ///     true,
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
        T: DecodeTypedFromRecordBatch + CatalogDataType + HasTsInit,
    {
        self.query_typed::<T>(
            identifiers,
            start,
            end,
            where_clause,
            files,
            optimize_file_loading,
        )
    }

    pub(super) fn query_typed_pages<T>(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<TypedPages<T>>
    where
        T: DecodeTypedFromRecordBatch + CatalogDataType + HasTsInit + Send + 'static,
    {
        self.clear_session_tables();
        self.register_remote_object_store()?;
        let data_type = T::catalog_data_type();
        let prefix = parquet_data_path_prefix(&data_type);
        let files = match files {
            Some(files) => files,
            None => self.query_files(prefix.as_ref(), identifiers, start, end)?,
        };
        let paths = if optimize_file_loading {
            parent_directories(&files)
                .into_iter()
                .map(|directory| self.resolve_directory_for_datafusion(&directory))
                .collect::<Vec<_>>()
        } else {
            files
                .iter()
                .map(|file| self.resolve_path_for_datafusion(file))
                .collect()
        };
        let mut sources = Vec::with_capacity(paths.len());
        for (index, path) in paths.into_iter().enumerate() {
            let table = format!("parquet_{index}");
            let sql = build_query(&table, start, end, where_clause);
            let stream = self
                .session
                .parquet_files_batch_stream(&table, vec![path], Some(&sql))?;
            let pages = decode_typed_pages::<T>(stream);
            sources.push(
                Box::new(datafusion::BlockingBatchStream::from_stream_with_runtime(
                    pages,
                    &self.session.runtime,
                )) as TypedPages<T>,
            );
        }
        Ok(Box::new(MergedPages::new(sources, self.batch_size)))
    }

    /// Queries typed records that are not represented by the [`Data`] enum.
    pub fn query_typed<T>(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeTypedFromRecordBatch + CatalogDataType + HasTsInit,
    {
        self.clear_session_tables();

        self.register_remote_object_store()?;

        let data_type = T::catalog_data_type();
        let path_prefix = parquet_data_path_prefix(&data_type);

        let files_list = if let Some(files) = files {
            files
        } else {
            self.query_files(path_prefix.as_ref(), identifiers, start, end)?
        };

        let mut all_records = Vec::new();

        if optimize_file_loading {
            for directory in parent_directories(&files_list) {
                let identifier = dir_identifier(&directory);
                let safe_sql_identifier = make_sql_safe_identifier(&identifier);
                let table_name = format!("{}_{}", path_prefix.as_ref(), safe_sql_identifier);
                let query = build_query(&table_name, start, end, where_clause);
                let resolved_path = self.resolve_directory_for_datafusion(&directory);
                let batches = self.session.collect_parquet_files_batches(
                    &table_name,
                    vec![resolved_path],
                    Some(&query),
                )?;

                all_records.extend(self.convert_record_batches_to_typed::<T>(batches)?);
            }
        } else {
            for file_uri in &files_list {
                let identifier = extract_identifier_from_path(file_uri).ok_or_else(|| {
                    anyhow::anyhow!("Cannot extract identifier from path '{file_uri}'")
                })?;
                let safe_sql_identifier = make_sql_safe_identifier(identifier);
                let safe_filename = extract_sql_safe_filename(file_uri);
                let table_name = format!(
                    "{}_{}_{}",
                    path_prefix.as_ref(),
                    safe_sql_identifier,
                    safe_filename
                );
                let query = build_query(&table_name, start, end, where_clause);
                let resolved_path = self.resolve_path_for_datafusion(file_uri);
                let batches = self.session.collect_parquet_files_batches(
                    &table_name,
                    vec![resolved_path],
                    Some(&query),
                )?;

                all_records.extend(self.convert_record_batches_to_typed::<T>(batches)?);
            }
        }

        if !is_monotonically_increasing_by_init(&all_records) {
            all_records.sort_by_key(HasTsInit::ts_init);
        }

        Ok(all_records)
    }

    /// Queries raw catalog Arrow record batches for any supported record table.
    ///
    /// # Errors
    ///
    /// Returns an error if file discovery or DataFusion query execution fails.
    pub fn query_record_batches(
        &mut self,
        record_type: &str,
        identifier: Option<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        self.clear_session_tables();
        self.register_remote_object_store()?;

        let identifiers = identifier.map(|value| vec![value]);
        let files_list = self.query_files(record_type, identifiers, start, end)?;
        let mut record_batches = Vec::new();
        let table_prefix = make_sql_safe_identifier(record_type);

        if optimize_file_loading {
            // Deterministic registration order so equal-ts_init tie order is reproducible.
            for directory in parent_directories(&files_list) {
                let identifier = decode_object_store_segment(&dir_identifier(&directory));
                let safe_sql_identifier = make_sql_safe_identifier(&identifier);
                let table_name = format!("{table_prefix}_{safe_sql_identifier}");
                let query = build_query(&table_name, start, end, where_clause);
                let resolved_path = self.resolve_directory_for_datafusion(&directory);
                record_batches.extend(self.session.collect_parquet_files_batches(
                    &table_name,
                    vec![resolved_path],
                    Some(&query),
                )?);
            }
        } else {
            for file_uri in &files_list {
                let identifier = extract_identifier_from_path(file_uri).ok_or_else(|| {
                    anyhow::anyhow!("Cannot extract identifier from path '{file_uri}'")
                })?;
                let safe_sql_identifier = make_sql_safe_identifier(identifier);
                let safe_filename = extract_sql_safe_filename(file_uri);
                let table_name = format!("{table_prefix}_{safe_sql_identifier}_{safe_filename}");
                let query = build_query(&table_name, start, end, where_clause);
                let resolved_path = self.resolve_path_for_datafusion(file_uri);
                record_batches.extend(self.session.collect_parquet_files_batches(
                    &table_name,
                    vec![resolved_path],
                    Some(&query),
                )?);
            }
        }

        Ok(record_batches)
    }

    /// Queries raw catalog batches and converts them to display-friendly Arrow batches.
    ///
    /// # Errors
    ///
    /// Returns an error if file discovery, DataFusion query execution, or catalog display
    /// conversion fails.
    pub fn query_display_record_batches(
        &mut self,
        data_type: &NautilusDataType,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        self.clear_session_tables();
        self.register_remote_object_store()?;

        let data_path_prefix = parquet_data_path_prefix(data_type);
        let files_list = self.query_files(data_path_prefix.as_ref(), identifiers, start, end)?;
        let mut display_batches = Vec::new();
        let table_prefix = make_sql_safe_identifier(data_path_prefix.as_ref());

        if optimize_file_loading {
            // Deterministic registration order so equal-ts_init tie order is reproducible.
            for directory in parent_directories(&files_list) {
                let path_identifier = display_identifier(data_type, &directory);
                let safe_sql_identifier = make_sql_safe_identifier(
                    path_identifier
                        .as_deref()
                        .unwrap_or(data_path_prefix.as_ref()),
                );
                let table_name = format!("{table_prefix}_{safe_sql_identifier}");
                let query = build_query(&table_name, start, end, where_clause);
                let resolved_path = self.resolve_directory_for_datafusion(&directory);
                let batches = self.session.collect_parquet_files_batches(
                    &table_name,
                    vec![resolved_path],
                    Some(&query),
                )?;

                for batch in batches {
                    let identifier =
                        display_batch_identifier(data_type, &batch, path_identifier.as_deref());
                    let batch = record_batch_with_identifier_column(batch, identifier.as_deref())?;
                    let metadata = batch.schema().metadata().clone();
                    display_batches.push(catalog_record_batch_to_display(
                        data_type, &metadata, &batch,
                    )?);
                }
            }
        } else {
            for file_uri in &files_list {
                let directory = Path::new(file_uri)
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Cannot extract directory from '{file_uri}'"))?
                    .to_string_lossy();
                let path_identifier = display_identifier(data_type, &directory);
                let safe_sql_identifier = make_sql_safe_identifier(
                    path_identifier
                        .as_deref()
                        .unwrap_or(data_path_prefix.as_ref()),
                );
                let safe_filename = extract_sql_safe_filename(file_uri);
                let table_name = format!("{table_prefix}_{safe_sql_identifier}_{safe_filename}");
                let query = build_query(&table_name, start, end, where_clause);
                let resolved_path = self.resolve_path_for_datafusion(file_uri);
                let batches = self.session.collect_parquet_files_batches(
                    &table_name,
                    vec![resolved_path],
                    Some(&query),
                )?;

                for batch in batches {
                    let identifier =
                        display_batch_identifier(data_type, &batch, path_identifier.as_deref());
                    let batch = record_batch_with_identifier_column(batch, identifier.as_deref())?;
                    let metadata = batch.schema().metadata().clone();
                    display_batches.push(catalog_record_batch_to_display(
                        data_type, &metadata, &batch,
                    )?);
                }
            }
        }

        if display_batches.is_empty() {
            display_batches.push(empty_display_batch_with_identifier(data_type)?);
        }
        Ok(display_batches)
    }

    /// Queries concrete catalog identifiers for matching data rows.
    pub fn query_identifiers(
        &mut self,
        data_type: &str,
        identifiers: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        _optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<String>> {
        self.clear_session_tables();
        self.register_remote_object_store()?;

        let files_list = self.query_files(data_type, identifiers, start, end)?;
        let table_prefix = make_sql_safe_identifier(data_type);
        let mut identifiers = Vec::new();

        for directory in parent_directories(&files_list) {
            let identifier = dir_identifier(&directory);
            let safe_identifier = make_sql_safe_identifier(&identifier);
            let table_name = format!("{table_prefix}_{safe_identifier}_identifier_check");
            let query = format!(
                "{} LIMIT 1",
                build_query(&table_name, start, end, where_clause)
            );
            let resolved_path = self.resolve_directory_for_datafusion(&directory);
            let batches = self.session.collect_parquet_files_batches(
                &table_name,
                vec![resolved_path],
                Some(&query),
            )?;

            if batches.iter().any(|batch| batch.num_rows() != 0) {
                identifiers.push(decode_object_store_segment(&identifier));
            }
        }

        identifiers.sort();
        identifiers.dedup();
        Ok(identifiers)
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
    #[expect(clippy::too_many_arguments)]
    pub fn query_custom_data_dynamic(
        &mut self,
        type_name: &str,
        identifiers: Option<&[String]>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        _optimize_file_loading: bool,
    ) -> anyhow::Result<Vec<Data>> {
        self.clear_session_tables();

        self.register_remote_object_store()?;

        let path_prefix = parquet_data_path_prefix(&NautilusDataType::Custom {
            type_name: type_name.to_string(),
        });

        let files = if let Some(f) = files {
            f.into_iter()
                .map(|p| self.to_object_path(&p).map(|op| op.to_string()))
                .collect::<anyhow::Result<Vec<_>>>()?
        } else {
            self.list_parquet_files_with_criteria(path_prefix.as_ref(), identifiers, start, end)?
        };

        if files.is_empty() {
            return Ok(Vec::new());
        }

        // Use CustomDataDecoder for all custom data. Pass type_name so decode can look up
        // the type when Parquet/DataFusion does not preserve schema metadata. Callers must
        // ensure Rust custom types are registered via ensure_custom_data_registered::<T>().
        let mut lookup_metadata = HashMap::new();
        lookup_metadata.insert("type_name".to_string(), type_name.to_string());
        let registered_schema = CustomDataDecoder::get_schema(Some(lookup_metadata.clone()));
        registered_schema.field_with_name("ts_init").map_err(|_| {
            anyhow::anyhow!(
                "custom data type '{type_name}' is not registered with an Arrow schema containing ts_init; \
                 call ensure_custom_data_registered::<T>() before querying"
            )
        })?;

        let mut all_data = Vec::new();

        for file in files {
            let object_path = self.to_object_path_parsed(&file)?;
            let mut decode_metadata = self.execute_async(|| async {
                let schema =
                    read_parquet_schema_from_object_store(self.object_store.clone(), &object_path)
                        .await?;
                Ok::<HashMap<String, String>, anyhow::Error>(schema.metadata().clone())
            })?;
            decode_metadata.extend(lookup_metadata.clone());
            let identifier = extract_identifier_from_path(&file)
                .ok_or_else(|| anyhow::anyhow!("Cannot extract identifier from path '{file}'"))?;
            let safe_type_name = make_sql_safe_identifier(type_name);
            let safe_sql_identifier = make_sql_safe_identifier(identifier);
            let safe_filename = extract_sql_safe_filename(&file);
            let table_name =
                format!("custom_{safe_type_name}_{safe_sql_identifier}_{safe_filename}");
            let resolved_path = self.resolve_path_for_datafusion(&file);
            let sql_query = build_query(&table_name, start, end, where_clause);

            // Use schemaless registration so DataFusion preserves the parquet file's
            // schema metadata (e.g. `bar_type`) on output batches, since the
            // explicit-schema variant strips per-batch metadata that decoders rely on.
            let batches = self.session.collect_parquet_files_batches(
                &table_name,
                vec![resolved_path],
                Some(&sql_query),
            )?;

            for batch in batches {
                all_data.extend(CustomDataDecoder::decode_data_batch(
                    &decode_metadata,
                    batch,
                )?);
            }
        }
        all_data.sort_by_key(HasTsInit::ts_init);
        Ok(all_data)
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
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings
    ///   (e.g., "EUR/USD.SIM") or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
    /// use nautilus_core::UnixNanos;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Query all quote files
    /// let files = catalog.query_files("quotes", None, None, None)?;
    ///
    /// // Query trade files for specific instruments within a time range
    /// let files = catalog.query_files(
    ///     "trades",
    ///     Some(vec!["BTC/USD.SIM".to_string(), "ETH/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
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
        let list_result = self.list_objects(&base_dir)?;

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
                        let dir_name =
                            decode_object_store_segment(path_parts[path_parts.len() - 2]);
                        safe_identifiers.iter().any(|safe_id| safe_id == &dir_name)
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();

            if exact_match_file_paths.is_empty() && is_parquet_bar_prefix(data_cls) {
                file_paths.retain(|file_path| {
                    let path_parts: Vec<&str> = file_path.split('/').collect();
                    if path_parts.len() >= 2 {
                        let dir_name =
                            decode_object_store_segment(path_parts[path_parts.len() - 2]);

                        if let Some(bar_instrument_id) = extract_bar_type_instrument_id(&dir_name) {
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
        files.sort();

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

    /// Queries order book depth data for the specified instrument(s) and time range.
    pub fn order_book_depths(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<OrderBookDepth>> {
        self.query_typed_data::<OrderBookDepth>(instrument_ids, start, end, None, None, true)
    }

    /// Queries funding rate updates for the specified instrument(s) and time range.
    pub fn funding_rates(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<FundingRateUpdate>> {
        self.query_typed::<FundingRateUpdate>(instrument_ids, start, end, None, None, true)
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

    /// Queries option greeks data for the specified instrument(s) and time range.
    pub fn option_greeks(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<OptionGreeks>> {
        self.query_typed_data::<OptionGreeks>(instrument_ids, start, end, None, None, true)
    }

    /// Queries any instrument data for the specified instrument(s) and time range.
    pub fn instruments(
        &self,
        instrument_ids: Option<&[String]>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.query_instruments_filtered(instrument_ids, start, end)
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
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let files = catalog.get_file_list_from_data_cls("quotes")?;
    ///
    /// for file in files {
    ///     println!("Found file: {}", file);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_file_list_from_data_cls(&self, data_cls: &str) -> anyhow::Result<Vec<String>> {
        let base_dir = self.make_path(data_cls, None)?;

        let list_result = self.list_objects(&base_dir)?;

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
    /// use nautilus_core::UnixNanos;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let all_files = catalog.get_file_list_from_data_cls("quotes")?;
    ///
    /// let filtered = catalog.filter_files(
    ///     "quotes",
    ///     all_files,
    ///     Some(vec!["EUR/USD.SIM".to_string()]),
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
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
                        decode_object_store_segment(path_parts[path_parts.len() - 2])
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

            if exact_match_file_paths.is_empty() && is_parquet_bar_prefix(data_cls) {
                // Partial match of instrument_ids in bar_types for bars
                filtered_paths.retain(|file_path| {
                    let path_parts: Vec<&str> = file_path.split('/').collect();
                    if path_parts.len() >= 2 {
                        let dir_name =
                            decode_object_store_segment(path_parts[path_parts.len() - 2]);
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
}

fn is_parquet_instrument_type_prefix(prefix: &str) -> bool {
    INSTRUMENT_PATH_PREFIXES.contains(&prefix)
}

pub(super) fn is_parquet_bar_prefix(data_cls: &str) -> bool {
    data_cls == parquet_data_path_prefix(&NautilusDataType::Bar).as_ref()
}

/// Returns the sorted, deduplicated parent directories (everything except the filename)
/// of the given file URIs.
fn parent_directories(files: &[String]) -> Vec<String> {
    let mut directories: Vec<String> = files
        .iter()
        .filter_map(|file_uri| {
            Path::new(file_uri)
                .parent()
                .map(|path| path.to_string_lossy().to_string())
        })
        .collect();
    directories.sort();
    directories.dedup();
    directories
}

/// Extracts the identifier from a directory path (last component).
fn dir_identifier(directory: &str) -> String {
    directory
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

fn display_identifier(data_type: &NautilusDataType, directory: &str) -> Option<String> {
    let identifier = dir_identifier(directory);
    let is_unpartitioned_custom = matches!(data_type, NautilusDataType::Custom { .. })
        && Path::new(directory)
            .ends_with(Path::new("data").join(parquet_data_path_prefix(data_type).as_ref()));

    (!is_unpartitioned_custom).then(|| decode_object_store_segment(&identifier))
}

fn display_batch_identifier(
    data_type: &NautilusDataType,
    batch: &RecordBatch,
    path_identifier: Option<&str>,
) -> Option<String> {
    if matches!(data_type, NautilusDataType::Custom { .. }) {
        path_identifier.map(str::to_string)
    } else {
        catalog_identifier_from_metadata(batch.schema().metadata())
            .or_else(|| path_identifier.map(str::to_string))
    }
}
