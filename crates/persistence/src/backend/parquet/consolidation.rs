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

//! Period-based and bulk consolidation of parquet files in a catalog directory.

#![expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "consolidation operations forward catalog/storage errors and operate on validated batches"
)]

use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, CustomData, Data, HasTsInit, IndexPriceUpdate, MarkPriceUpdate, NautilusDataType,
    OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
};
use nautilus_serialization::arrow::{DecodeTypedFromRecordBatch, EncodeToRecordBatch};
use object_store::path::Path as ObjectPath;

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        intervals::{are_intervals_contiguous, are_intervals_disjoint},
        io::combine_parquet_files_from_object_store,
        paths::{
            extract_path_components, make_object_store_path, parse_filename_timestamps,
            timestamps_to_filename,
        },
    },
    catalog::types::{CatalogDataType, INSTRUMENT_PATH_PREFIXES, parquet_data_path_prefix},
    common::custom::group_custom_data_by_type,
};

/// Information about a consolidation query to be executed.
#[derive(Debug, Clone)]
pub struct ConsolidationQuery {
    /// Start timestamp for the query range (inclusive, in nanoseconds)
    pub query_start: u64,
    /// End timestamp for the query range (inclusive, in nanoseconds)
    pub query_end: u64,
    /// Whether to use period boundaries for file naming (true) or actual data timestamps (false)
    pub use_period_boundaries: bool,
}

impl ParquetDataCatalog {
    /// Consolidates all data files in the catalog.
    ///
    /// This method identifies all leaf directories in the catalog that contain parquet files
    /// and consolidates them. A leaf directory is one that contains files but no subdirectories.
    /// This is a convenience method that effectively calls `consolidate_data` for all data types
    /// and instrument IDs in the catalog.
    ///
    /// # Parameters
    ///
    /// - `start`: Optional start timestamp for the consolidation range. Only files with timestamps
    ///   greater than or equal to this value will be consolidated. If None, all files
    ///   from the beginning of time will be considered.
    /// - `end`: Optional end timestamp for the consolidation range. Only files with timestamps
    ///   less than or equal to this value will be consolidated. If None, all files
    ///   up to the end of time will be considered.
    /// - `ensure_contiguous_files`: Whether to validate that consolidated intervals are contiguous (default: true).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails for any directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory listing fails.
    /// - File consolidation operations fail.
    /// - Interval validation fails (when `ensure_contiguous_files` is true).
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
    /// // Consolidate all files in the catalog
    /// catalog.consolidate_catalog(None, None, None, None)?;
    ///
    /// // Consolidate only files within a specific time range
    /// catalog.consolidate_catalog(
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(true),
    ///     None,
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_catalog(
        &self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.consolidate_directory(
                &directory,
                start,
                end,
                ensure_contiguous_files,
                deduplicate,
            )?;
        }

        Ok(())
    }

    /// Consolidates data files for a specific data type and identifier.
    ///
    /// This method consolidates Parquet files within a specific directory (defined by data type
    /// and optional identifier) by merging multiple files into a single file. This improves
    /// query performance and can reduce storage overhead.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars").
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    /// - `start`: Optional start timestamp to limit consolidation to files within this range.
    /// - `end`: Optional end timestamp to limit consolidation to files within this range.
    /// - `ensure_contiguous_files`: Whether to validate that consolidated intervals are contiguous (default: true).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - File consolidation operations fail.
    /// - Interval validation fails (when `ensure_contiguous_files` is true).
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
    /// // Consolidate all quote files for a specific instrument
    /// catalog.consolidate_data("quotes", Some("BTCUSD"), None, None, None, None)?;
    ///
    /// // Consolidate trade files within a time range
    /// catalog.consolidate_data(
    ///     "trades",
    ///     None,
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(true),
    ///     None,
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_data(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> anyhow::Result<()> {
        if matches!(type_name, "instrument" | "instruments") {
            for prefix in INSTRUMENT_PATH_PREFIXES {
                self.consolidate_data(
                    prefix,
                    identifier,
                    start,
                    end,
                    ensure_contiguous_files,
                    deduplicate,
                )?;
            }
            return Ok(());
        }

        let directory = self.make_path(type_name, identifier)?;
        let raw_result = self.consolidate_directory(
            &directory,
            start,
            end,
            ensure_contiguous_files,
            deduplicate,
        );

        match raw_result {
            Ok(()) => Ok(()),
            Err(raw_error)
                if can_rewrite_consolidation_by_period(type_name)
                    && is_schema_incompatibility(&raw_error) =>
            {
                if deduplicate.unwrap_or(false) {
                    anyhow::bail!(
                        "Raw parquet consolidation failed due to incompatible file schemas, \
                         but typed period consolidation cannot preserve deduplicate=true. \
                         Raw consolidation error: {raw_error}"
                    );
                }

                log::warn!(
                    "Raw parquet consolidation failed due to incompatible file schemas for \
                     {type_name}; retrying with typed period consolidation. Raw error: {raw_error}"
                );

                self.consolidate_data_by_period(
                    type_name,
                    identifier,
                    None,
                    start,
                    end,
                    ensure_contiguous_files,
                )
                .map_err(|typed_error| {
                    anyhow::anyhow!(
                        "Raw parquet consolidation failed due to incompatible file schemas, \
                         and typed period consolidation also failed. Raw error: {raw_error}; \
                         typed period error: {typed_error}"
                    )
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Consolidates Parquet files within a specific directory by merging them into a single file.
    ///
    /// This internal method performs the actual consolidation work for a single directory.
    /// It identifies files within the specified time range, validates their intervals,
    /// and combines them into a single Parquet file with optimized storage.
    ///
    /// # Parameters
    ///
    /// - `directory`: The directory path containing Parquet files to consolidate.
    /// - `start`: Optional start timestamp to limit consolidation to files within this range.
    /// - `end`: Optional end timestamp to limit consolidation to files within this range.
    /// - `ensure_contiguous_files`: Whether to validate that consolidated intervals are contiguous.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    ///
    /// # Behavior
    ///
    /// - Skips consolidation if directory contains 1 or fewer files.
    /// - Filters files by timestamp range if start/end are specified.
    /// - Sorts intervals by start timestamp before consolidation.
    /// - Creates a new file spanning the entire time range of input files.
    /// - Validates interval disjointness after consolidation (if enabled).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory listing fails.
    /// - File combination operations fail.
    /// - Interval validation fails (when `ensure_contiguous_files` is true).
    /// - Object store operations fail.
    fn consolidate_directory(
        &self,
        directory: &str,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
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
        files_to_consolidate.sort_by_key(|file| {
            parse_filename_timestamps(file).map_or(u64::MAX, |(start, _)| start)
        });

        // Validate disjointness before merging so source files are left untouched on failure
        if ensure_contiguous_files.unwrap_or(true) && !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint before consolidating a directory");
        }

        if !intervals.is_empty() {
            let file_name = timestamps_to_filename(
                UnixNanos::from(intervals[0].0),
                UnixNanos::from(intervals.iter().map(|i| i.1).max().unwrap()),
            );
            let path = make_object_store_path(directory, [&file_name]);

            // Convert string paths to ObjectPath for the function call
            let object_paths: Vec<ObjectPath> = files_to_consolidate
                .iter()
                .map(|path| ObjectPath::from(path.as_str()))
                .collect();

            self.execute_async(|| async {
                combine_parquet_files_from_object_store(
                    self.object_store.clone(),
                    object_paths,
                    &ObjectPath::from(path),
                    Some(self.compression),
                    Some(self.max_row_group_size),
                    deduplicate,
                )
                .await
            })?;
        }

        Ok(())
    }

    /// Consolidates all data files in the catalog by splitting them into fixed time periods.
    ///
    /// This method identifies all leaf directories in the catalog that contain parquet files
    /// and consolidates them by period. A leaf directory is one that contains files but no subdirectories.
    /// This is a convenience method that effectively calls `consolidate_data_by_period` for all data types
    /// and instrument IDs in the catalog.
    ///
    /// # Parameters
    ///
    /// - `period_nanos`: The period duration for consolidation in nanoseconds. Default is 1 day (86400000000000).
    ///   Examples: 3600000000000 (1 hour), 604800000000000 (7 days), 1800000000000 (30 minutes)
    /// - `start`: Optional start timestamp for the consolidation range. Only files with timestamps
    ///   greater than or equal to this value will be consolidated. If None, all files
    ///   from the beginning of time will be considered.
    /// - `end`: Optional end timestamp for the consolidation range. Only files with timestamps
    ///   less than or equal to this value will be consolidated. If None, all files
    ///   up to the end of time will be considered.
    /// - `ensure_contiguous_files`: If true, uses period boundaries for file naming.
    ///   If false, uses actual data timestamps for file naming.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails for any directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory listing fails.
    /// - Data type extraction from path fails.
    /// - Period-based consolidation operations fail.
    ///
    /// # Notes
    ///
    /// - This operation can be resource-intensive for large catalogs with many data types.
    ///   and instruments.
    /// - The consolidation process splits data into fixed time periods rather than combining.
    ///   all files into a single file per directory.
    /// - Uses the same period-based consolidation logic as `consolidate_data_by_period`.
    /// - Original files are removed and replaced with period-based consolidated files.
    /// - This method is useful for periodic maintenance of the catalog to standardize.
    ///   file organization by time periods.
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
    /// // Consolidate all files in the catalog by 1-day periods
    /// catalog.consolidate_catalog_by_period(
    ///     Some(86400000000000), // 1 day in nanoseconds
    ///     None,
    ///     None,
    ///     Some(true),
    /// )?;
    ///
    /// // Consolidate only files within a specific time range by 1-hour periods
    /// catalog.consolidate_catalog_by_period(
    ///     Some(3600000000000), // 1 hour in nanoseconds
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(false),
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_catalog_by_period(
        &mut self,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            let (data_cls, identifier) =
                self.extract_data_cls_and_identifier_from_path(&directory)?;

            if let Some(data_cls_name) = data_cls {
                let identifier_ref = identifier.as_deref();

                if !self.dispatch_consolidate_data_by_period(
                    &data_cls_name,
                    identifier_ref,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )? {
                    // Skip unknown data types
                    log::warn!("Unknown data type for consolidation: {data_cls_name}");
                }
            }
        }

        Ok(())
    }

    /// Extracts data class and identifier from a directory path.
    ///
    /// This method parses a directory path to extract the data type and optional
    /// instrument identifier. It's used to determine what type of data consolidation
    /// to perform for each directory.
    ///
    /// # Parameters
    ///
    /// - `path`: The directory path to parse.
    ///
    /// # Returns
    ///
    /// Returns a tuple of (`data_class`, identifier) where both are optional strings.
    pub fn extract_data_cls_and_identifier_from_path(
        &self,
        path: &str,
    ) -> anyhow::Result<(Option<String>, Option<String>)> {
        // Use cross-platform path parsing
        let path_components = extract_path_components(path);

        // Find the "data" directory in the path
        if let Some(data_index) = path_components.iter().position(|part| part == "data")
            && data_index + 1 < path_components.len()
        {
            let second = &path_components[data_index + 1];

            if second == "custom" {
                let Some(type_name) = path_components.get(data_index + 2) else {
                    return Ok((None, None));
                };
                let identifier = (path_components.len() > data_index + 3)
                    .then(|| path_components[data_index + 3..].join("/"));
                return Ok((Some(format!("custom/{type_name}")), identifier));
            }
            let data_cls = second.clone();
            let identifier = if data_index + 2 < path_components.len() {
                Some(path_components[data_index + 2].clone())
            } else {
                None
            };

            return Ok((Some(data_cls), identifier));
        }

        // If we can't parse the path, return None for both
        Ok((None, None))
    }

    /// Consolidates data files by splitting them into fixed time periods.
    ///
    /// This method queries data by period and writes consolidated files immediately,
    /// using efficient period-based consolidation logic. When start/end boundaries intersect existing files,
    /// the function automatically splits those files to preserve all data.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars").
    /// - `identifier`: Optional instrument ID to consolidate. If None, consolidates all instruments.
    /// - `period_nanos`: The period duration for consolidation in nanoseconds. Default is 1 day (86400000000000).
    ///   Examples: 3600000000000 (1 hour), 604800000000000 (7 days), 1800000000000 (30 minutes)
    /// - `start`: Optional start timestamp for consolidation range. If None, uses earliest available data.
    ///   If specified and intersects existing files, those files will be split to preserve
    ///   data outside the consolidation range.
    /// - `end`: Optional end timestamp for consolidation range. If None, uses latest available data.
    ///   If specified and intersects existing files, those files will be split to preserve
    ///   data outside the consolidation range.
    /// - `ensure_contiguous_files`: If true, uses period boundaries for file naming.
    ///   If false, uses actual data timestamps for file naming.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - File operations fail.
    /// - Data querying or writing fails.
    ///
    /// # Notes
    ///
    /// - Uses two-phase approach: first determines all queries, then executes them.
    /// - Groups intervals into contiguous groups to preserve holes between groups.
    /// - Allows consolidation across multiple files within each contiguous group.
    /// - Skips queries if target files already exist for efficiency.
    /// - Original files are removed immediately after querying each period.
    /// - When `ensure_contiguous_files=false`, file timestamps match actual data range.
    /// - When `ensure_contiguous_files=true`, file timestamps use period boundaries.
    /// - Uses modulo arithmetic for efficient period boundary calculation.
    /// - Preserves holes in data by preventing queries from spanning across gaps.
    /// - Automatically splits files at start/end boundaries to preserve all data.
    /// - Split operations are executed before consolidation to ensure data preservation.
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
    /// // Consolidate all quote files by 1-day periods
    /// catalog.consolidate_data_by_period(
    ///     "quotes",
    ///     None,
    ///     Some(86400000000000), // 1 day in nanoseconds
    ///     None,
    ///     None,
    ///     Some(true),
    /// )?;
    ///
    /// // Consolidate specific instrument by 1-hour periods
    /// catalog.consolidate_data_by_period(
    ///     "trades",
    ///     Some("BTCUSD"),
    ///     Some(3600000000000), // 1 hour in nanoseconds
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(false),
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_data_by_period(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        if !self.dispatch_consolidate_data_by_period(
            type_name,
            identifier,
            period_nanos,
            start,
            end,
            ensure_contiguous_files,
        )? {
            anyhow::bail!("Unknown data type for consolidation: {type_name}");
        }

        Ok(())
    }

    /// Dispatches period-based consolidation for `type_name`, returning `Ok(false)` for
    /// unknown data types so callers choose whether to warn or fail.
    fn dispatch_consolidate_data_by_period(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<bool> {
        match type_name {
            "quotes" => {
                self.consolidate_data_by_period_generic::<QuoteTick>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "trades" => {
                self.consolidate_data_by_period_generic::<TradeTick>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "order_book_deltas" => {
                self.consolidate_data_by_period_generic::<OrderBookDelta>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "order_book_depths" => {
                self.consolidate_data_by_period_generic::<OrderBookDepth>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "bars" => {
                self.consolidate_data_by_period_generic::<Bar>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "index_prices" => {
                self.consolidate_data_by_period_generic::<IndexPriceUpdate>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "mark_prices" => {
                self.consolidate_data_by_period_generic::<MarkPriceUpdate>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "instrument_closes" => {
                self.consolidate_data_by_period_generic::<InstrumentClose>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            _ => {
                if let Some(custom_type_name) = type_name.strip_prefix("custom/") {
                    self.consolidate_custom_data_by_period(
                        custom_type_name,
                        identifier,
                        period_nanos,
                        start,
                        end,
                        ensure_contiguous_files,
                    )?;
                } else {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Generic consolidate data files by splitting them into fixed time periods.
    ///
    /// This is a type-safe version of `consolidate_data_by_period` that uses generic types
    /// to ensure compile-time correctness and enable reuse across different data types.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to consolidate, must implement required traits for serialization.
    ///
    /// # Parameters
    ///
    /// - `identifier`: Optional instrument ID to target a specific instrument's data.
    /// - `period_nanos`: Optional period size in nanoseconds (default: 1 day).
    /// - `start`: Optional start timestamp for consolidation range.
    /// - `end`: Optional end timestamp for consolidation range.
    /// - `ensure_contiguous_files`: Optional flag to control file naming strategy.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    pub fn consolidate_data_by_period_generic<T>(
        &mut self,
        identifier: Option<&str>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()>
    where
        T: DecodeTypedFromRecordBatch
            + CatalogDataType
            + EncodeToRecordBatch
            + HasTsInit
            + TryFrom<Data>
            + Clone,
    {
        let period_nanos = period_nanos.unwrap_or(86_400_000_000_000); // Default: 1 day
        let ensure_contiguous_files = ensure_contiguous_files.unwrap_or(true);

        // Use get_intervals for cleaner implementation
        let data_type = T::catalog_data_type();
        let path_prefix = parquet_data_path_prefix(&data_type);
        let intervals = self.get_intervals(path_prefix.as_ref(), identifier)?;

        if intervals.is_empty() {
            return Ok(()); // No files to consolidate
        }

        // Use auxiliary function to prepare all queries for execution
        let queries_to_execute = self.prepare_consolidation_queries(
            path_prefix.as_ref(),
            identifier,
            &intervals,
            period_nanos,
            start,
            end,
            ensure_contiguous_files,
        )?;

        if queries_to_execute.is_empty() {
            return Ok(()); // No queries to execute
        }

        // Get directory for file operations
        let directory = self.make_path(path_prefix.as_ref(), identifier)?;
        let mut existing_files = self.list_parquet_files(&directory)?;
        existing_files.sort();

        // Capture the overall window's left bound before the loop consumes queries_to_execute,
        // a source file is only deleted when its interval is fully consumed by the consolidation.
        let overall_query_start = queries_to_execute[0].query_start;

        // Phase 2: Execute queries, write, and delete
        let mut file_start_ns: Option<u64> = None; // Track contiguity across periods

        for query_info in queries_to_execute {
            // Query data for this period using query_typed_data
            let instrument_ids = identifier.map(|id| vec![id.to_string()]);

            // Use optimize_file_loading=false to match Python behavior:
            // During consolidation, we want to read only the specific files being consolidated,
            // not the entire directory. This ensures precise file control during consolidation.
            let period_data = self.query_typed_data::<T>(
                instrument_ids,
                Some(UnixNanos::from(query_info.query_start)),
                Some(UnixNanos::from(query_info.query_end)),
                None,
                Some(existing_files.clone()),
                false, // optimize_file_loading=false for precise file control during consolidation
            )?;

            if period_data.is_empty() {
                // Skip if no data found, but maintain contiguity by using query start
                if file_start_ns.is_none() {
                    file_start_ns = Some(query_info.query_start);
                }
                continue;
            }

            // Determine final file timestamps
            let (final_start_ns, final_end_ns) = if query_info.use_period_boundaries {
                // Use period boundaries for file naming, maintaining contiguity
                if file_start_ns.is_none() {
                    file_start_ns = Some(query_info.query_start);
                }
                let start = file_start_ns.unwrap();
                (start, query_info.query_end)
            } else {
                // Use actual data timestamps for file naming
                let first_ts = period_data.first().unwrap().ts_init().as_u64();
                let last_ts = period_data.last().unwrap().ts_init().as_u64();
                (first_ts, last_ts)
            };

            // Check again if target file exists (in case it was created during this process)
            let target_filename = format!(
                "{}/{}",
                directory,
                timestamps_to_filename(
                    UnixNanos::from(final_start_ns),
                    UnixNanos::from(final_end_ns)
                )
            );

            if self.file_exists(&target_filename)? {
                // This period is already consolidated; do not let a later cleanup delete it.
                let target_object_path = self.to_object_path(&target_filename)?.to_string();
                existing_files.retain(|f| f != &target_object_path);
                // Reset so the next period starts a new segment after the existing file
                file_start_ns = None;
                continue;
            }

            // Write consolidated data for this period using write_to_parquet
            // Use skip_disjoint_check since we're managing file removal carefully
            let start_ts = UnixNanos::from(final_start_ns);
            let end_ts = UnixNanos::from(final_end_ns);
            self.write_to_parquet(&period_data, Some(start_ts), Some(end_ts), Some(true))?;

            // Delete files fully consumed by this period; keep straddlers so no data is lost
            for file in existing_files.clone() {
                if let Some(interval) = parse_filename_timestamps(&file)
                    && interval.1 <= query_info.query_end
                    && interval.0 >= overall_query_start
                {
                    existing_files.retain(|f| f != &file);
                    self.delete_file(&file)?;
                }
            }

            // Reset so next period starts a new contiguous segment
            file_start_ns = None;
        }

        Ok(())
    }

    /// Consolidates custom data files by splitting them into fixed time periods.
    ///
    /// This method provides consolidation for custom data types that don't have compile-time
    /// type information. It uses dynamic querying and writing methods.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The custom data type name (without "custom/" prefix).
    /// - `identifier`: Optional instrument ID to consolidate.
    /// - `period_nanos`: Optional period size in nanoseconds (default: 1 day).
    /// - `start`: Optional start timestamp for consolidation range.
    /// - `end`: Optional end timestamp for consolidation range.
    /// - `ensure_contiguous_files`: Optional flag to control file naming strategy.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    fn consolidate_custom_data_by_period(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let period_nanos = period_nanos.unwrap_or(86_400_000_000_000); // Default: 1 day
        let ensure_contiguous_files = ensure_contiguous_files.unwrap_or(true);

        // Get intervals for the custom data type
        let path_prefix = parquet_data_path_prefix(&NautilusDataType::Custom {
            type_name: type_name.to_string(),
        });
        let intervals = self.get_intervals(path_prefix.as_ref(), identifier)?;

        if intervals.is_empty() {
            return Ok(()); // No files to consolidate
        }

        // Use auxiliary function to prepare all queries for execution
        let queries_to_execute = self.prepare_consolidation_queries(
            path_prefix.as_ref(),
            identifier,
            &intervals,
            period_nanos,
            start,
            end,
            ensure_contiguous_files,
        )?;

        if queries_to_execute.is_empty() {
            return Ok(()); // No queries to execute
        }

        // Get directory for file operations
        let directory = self.make_path(path_prefix.as_ref(), identifier)?;
        let mut existing_files = self.list_parquet_files(&directory)?;
        existing_files.sort();

        // Capture the overall window's left bound before the loop consumes queries_to_execute,
        // a source file is only deleted when its interval is fully consumed by the consolidation.
        let overall_query_start = queries_to_execute[0].query_start;

        // Phase 2: Execute queries, write, and delete
        let mut file_start_ns: Option<u64> = None; // Track contiguity across periods

        for query_info in queries_to_execute {
            // Query custom data for this period using query_custom_data_dynamic
            let instrument_ids = identifier.map(|id| vec![id.to_string()]);

            let period_data = self.query_custom_data_dynamic(
                type_name,
                instrument_ids.as_deref(),
                Some(UnixNanos::from(query_info.query_start)),
                Some(UnixNanos::from(query_info.query_end)),
                None,
                Some(existing_files.clone()),
                false, // optimize_file_loading=false for precise file control during consolidation
            )?;

            if period_data.is_empty() {
                // Skip if no data found, but maintain contiguity by using query start
                if file_start_ns.is_none() {
                    file_start_ns = Some(query_info.query_start);
                }
                continue;
            }

            // Determine final file timestamps
            let (final_start_ns, final_end_ns) = if query_info.use_period_boundaries {
                // Use period boundaries for file naming, maintaining contiguity
                if file_start_ns.is_none() {
                    file_start_ns = Some(query_info.query_start);
                }
                let start = file_start_ns.unwrap();
                (start, query_info.query_end)
            } else {
                // Use actual data timestamps for file naming
                let first_ts = period_data.first().unwrap().ts_init().as_u64();
                let last_ts = period_data.last().unwrap().ts_init().as_u64();
                (first_ts, last_ts)
            };

            // Check again if target file exists (in case it was created during this process)
            let target_filename = format!(
                "{}/{}",
                directory,
                timestamps_to_filename(
                    UnixNanos::from(final_start_ns),
                    UnixNanos::from(final_end_ns)
                )
            );

            if self.file_exists(&target_filename)? {
                // This period is already consolidated; do not let a later cleanup delete it.
                let target_object_path = self.to_object_path(&target_filename)?.to_string();
                existing_files.retain(|f| f != &target_object_path);
                // Reset so the next period starts a new segment after the existing file
                file_start_ns = None;
                continue;
            }

            let custom_items: Vec<CustomData> = period_data
                .into_iter()
                .filter_map(|data| match data {
                    Data::Custom(c) => Some(c),
                    _ => None,
                })
                .collect();

            // Write consolidated data for each type
            let start_ts = UnixNanos::from(final_start_ns);
            let end_ts = UnixNanos::from(final_end_ns);

            for items in group_custom_data_by_type(custom_items.iter()) {
                self.write_custom_data_refs_batch(
                    &items,
                    Some(start_ts),
                    Some(end_ts),
                    Some(true),
                )?;
            }

            // Delete files fully consumed by this period; keep straddlers so no data is lost
            for file in existing_files.clone() {
                if let Some(interval) = parse_filename_timestamps(&file)
                    && interval.1 <= query_info.query_end
                    && interval.0 >= overall_query_start
                {
                    existing_files.retain(|f| f != &file);
                    self.delete_file(&file)?;
                }
            }

            // Reset so next period starts a new contiguous segment
            file_start_ns = None;
        }

        Ok(())
    }

    /// Prepares all queries for consolidation by filtering, grouping, and handling splits.
    ///
    /// This auxiliary function handles all the preparation logic for consolidation:
    /// 1. Filters intervals by time range.
    /// 2. Groups intervals into contiguous groups.
    /// 3. Identifies and creates split operations for data preservation.
    /// 4. Generates period-based consolidation queries.
    /// 5. Checks for existing target files.
    #[expect(
        clippy::too_many_arguments,
        reason = "Consolidation keeps its window and policy arguments explicit"
    )]
    pub fn prepare_consolidation_queries(
        &self,
        type_name: &str,
        identifier: Option<&str>,
        intervals: &[(u64, u64)],
        period_nanos: u64,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: bool,
    ) -> anyhow::Result<Vec<ConsolidationQuery>> {
        // Filter intervals by time range if specified
        let used_start = start.map(|s| s.as_u64());
        let used_end = end.map(|e| e.as_u64());

        let mut filtered_intervals = Vec::new();

        for &(interval_start, interval_end) in intervals {
            // Check if interval overlaps with the specified range
            if used_start.is_none_or(|start| start <= interval_end)
                && used_end.is_none_or(|end| interval_start <= end)
            {
                filtered_intervals.push((interval_start, interval_end));
            }
        }

        if filtered_intervals.is_empty() {
            return Ok(Vec::new()); // No intervals in the specified range
        }

        // Check contiguity of filtered intervals if required
        if ensure_contiguous_files && !are_intervals_contiguous(&filtered_intervals) {
            anyhow::bail!(
                "Intervals are not contiguous. When ensure_contiguous_files=true, \
                 all files in the consolidation range must have contiguous timestamps."
            );
        }

        // Group intervals by the target period: split only when the gap between files
        // exceeds one period, since sub-period gaps land in the same consolidated file.
        let contiguous_groups = self.group_contiguous_intervals(&filtered_intervals, period_nanos);

        let mut queries_to_execute = Vec::new();

        // Handle interval splitting by creating split operations for data preservation
        if !filtered_intervals.is_empty() {
            if let Some(start_ts) = used_start {
                let first_interval = filtered_intervals[0];
                if first_interval.0 < start_ts && start_ts <= first_interval.1 {
                    // Split before start: preserve data from interval_start to start-1
                    queries_to_execute.push(ConsolidationQuery {
                        query_start: first_interval.0,
                        query_end: start_ts - 1,
                        use_period_boundaries: false,
                    });
                }
            }

            if let Some(end_ts) = used_end {
                let last_interval = filtered_intervals[filtered_intervals.len() - 1];
                if last_interval.0 <= end_ts && end_ts < last_interval.1 {
                    // Split after end: preserve data from end+1 to interval_end
                    queries_to_execute.push(ConsolidationQuery {
                        query_start: end_ts + 1,
                        query_end: last_interval.1,
                        use_period_boundaries: false,
                    });
                }
            }
        }

        // Generate period-based consolidation queries for each contiguous group
        for group in contiguous_groups {
            let group_start = group[0].0;
            let group_end = group[group.len() - 1].1;

            // Apply start/end filtering to the group
            let effective_start = used_start.map_or(group_start, |s| s.max(group_start));
            let effective_end = used_end.map_or(group_end, |e| e.min(group_end));

            if effective_start > effective_end {
                continue; // Skip if no overlap
            }

            // Generate period-based queries within this contiguous group
            let mut current_start_ns = (effective_start / period_nanos) * period_nanos;

            // Add safety check to prevent infinite loops (match Python logic)
            let max_iterations = 10000;
            let mut iteration_count = 0;

            while current_start_ns <= effective_end {
                iteration_count += 1;
                if iteration_count > max_iterations {
                    // Safety break to prevent infinite loops
                    break;
                }
                let current_end_ns = (current_start_ns + period_nanos - 1).min(effective_end);

                // Check if target file already exists (only when ensure_contiguous_files is true)
                if ensure_contiguous_files {
                    let directory = self.make_path(type_name, identifier)?;
                    let target_filename = format!(
                        "{}/{}",
                        directory,
                        timestamps_to_filename(
                            UnixNanos::from(current_start_ns),
                            UnixNanos::from(current_end_ns)
                        )
                    );

                    if self.file_exists(&target_filename)? {
                        // Skip if target file already exists
                        current_start_ns += period_nanos;
                        continue;
                    }
                }

                // Add query to execution list
                queries_to_execute.push(ConsolidationQuery {
                    query_start: current_start_ns,
                    query_end: current_end_ns,
                    use_period_boundaries: ensure_contiguous_files,
                });

                // Move to next period
                current_start_ns += period_nanos;

                if current_start_ns > effective_end {
                    break;
                }
            }
        }

        // Sort queries by start date to enable efficient file removal
        // Files can be removed when interval[1] <= query_info["query_end"]
        // and processing in chronological order ensures optimal cleanup
        queries_to_execute.sort_by_key(|q| q.query_start);

        Ok(queries_to_execute)
    }

    /// Groups intervals for period-based consolidation.
    ///
    /// Groups adjacent intervals into the same bucket unless the gap between them exceeds
    /// `period_nanos`. Sub-period gaps land in the same consolidated file anyway, so they
    /// do not warrant a split. Gaps larger than one period represent genuine data holes.
    ///
    /// # Parameters
    ///
    /// - `intervals`: A slice of timestamp intervals as (start, end) tuples, sorted by start.
    /// - `period_nanos`: The target consolidation period; gaps larger than this split groups.
    ///
    /// # Returns
    ///
    /// Returns a vector of groups. Returns an empty vector if the input is empty.
    ///
    /// # Examples
    ///
    /// ```text
    /// Legacy chunked files with period=86_400_000_000_000 (1 day):
    ///   [(1,5), (6,10), (11,15)] -> [[(1,5), (6,10), (11,15)]]
    ///
    /// Small period=1 with mixed gaps:
    ///   [(1,5), (8,10), (12,15)] -> [[(1,5)], [(8,10)], [(12,15)]]
    /// ```
    #[must_use]
    pub fn group_contiguous_intervals(
        &self,
        intervals: &[(u64, u64)],
        period_nanos: u64,
    ) -> Vec<Vec<(u64, u64)>> {
        if intervals.is_empty() {
            return Vec::new();
        }

        // Split groups only when the gap between files exceeds one period,
        // since sub-period gaps land in the same consolidated file anyway.
        // This works for both legacy chunked files (gap ~1ns) and fragment-per-flush
        // catalogs (gap ~bar interval) without inferring spacing from the data.
        let mut contiguous_groups = Vec::new();
        let mut current_group = vec![intervals[0]];

        for i in 1..intervals.len() {
            let prev_end = intervals[i - 1].1;
            let curr_start = intervals[i].0;

            if curr_start.saturating_sub(prev_end) > period_nanos {
                contiguous_groups.push(current_group);
                current_group = vec![intervals[i]];
            } else {
                current_group.push(intervals[i]);
            }
        }

        contiguous_groups.push(current_group);

        contiguous_groups
    }
}

fn can_rewrite_consolidation_by_period(type_name: &str) -> bool {
    matches!(
        type_name,
        "quotes"
            | "trades"
            | "order_book_deltas"
            | "order_book_depths"
            | "bars"
            | "index_prices"
            | "mark_prices"
    )
}

fn is_schema_incompatibility(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_lowercase();
    [
        "schema",
        "field",
        "column",
        "data type",
        "datatype",
        "not compatible",
        "mismatch",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("quotes", true)]
    #[case("bars", true)]
    #[case("instrument_closes", false)]
    #[case("custom_signal", false)]
    fn can_rewrite_consolidation_by_period_only_for_supported_types(
        #[case] type_name: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(can_rewrite_consolidation_by_period(type_name), expected);
    }

    #[rstest]
    #[case("schema mismatch while writing record batch", true)]
    #[case("object store request timed out", false)]
    fn schema_incompatibility_detection_matches_schema_errors(
        #[case] message: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(
            is_schema_incompatibility(&anyhow::anyhow!(message.to_string())),
            expected
        );
    }
}
