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
    reason = "consolidation operations forward catalog/storage errors and operate on validated batches"
)]

use ahash::AHashSet;
use arrow::record_batch::RecordBatch;
use nautilus_core::{ClosedInterval, UnixNanos, string::conversions::to_snake_case};
use nautilus_model::data::{
    Bar, CustomData, Data, HasTsInit, IndexPriceUpdate, MarkPriceUpdate, NautilusDataType,
    OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
};
use nautilus_serialization::arrow::{
    DecodeTypedFromRecordBatch, EncodeToRecordBatch, catalog_identifier_from_metadata,
};
use object_store::path::Path as ObjectPath;

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        intervals::{are_intervals_contiguous, are_intervals_disjoint},
        io::combine_parquet_files_from_object_store,
        paths::{
            extract_path_components, make_object_store_path, normalize_path_separators,
            parse_filename_timestamps, timestamps_to_filename,
        },
    },
    catalog::types::{
        CatalogDataType, HasCatalogDataType, parquet_catalog_data_type_path_prefixes,
        parquet_data_path_prefix,
    },
    common::coverage::merge_closed_intervals,
};

/// Information about a consolidation query to be executed.
#[derive(Debug, Clone)]
pub struct ConsolidationQuery {
    /// Start timestamp for the query range (inclusive, in nanoseconds).
    pub query_start: u64,
    /// End timestamp for the query range (inclusive, in nanoseconds).
    pub query_end: u64,
    /// Whether to use period boundaries for file naming (true) or actual data timestamps (false).
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
    /// - `data_type`: The stored family to consolidate.
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
    /// use nautilus_model::data::NautilusDataType;
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
    /// catalog.consolidate_data(
    ///     &NautilusDataType::QuoteTick.into(),
    ///     Some("BTCUSD"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// )?;
    ///
    /// // Consolidate trade files within a time range
    /// catalog.consolidate_data(
    ///     &NautilusDataType::TradeTick.into(),
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
        data_type: &CatalogDataType,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> anyhow::Result<()> {
        for type_name in parquet_catalog_data_type_path_prefixes(data_type) {
            self.consolidate_prefix_data(
                type_name.as_ref(),
                identifier,
                start,
                end,
                ensure_contiguous_files,
                deduplicate,
            )?;
        }

        Ok(())
    }

    fn consolidate_prefix_data(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> anyhow::Result<()> {
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

                self.consolidate_prefix_data_by_period(
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
        let start = start.map(|t| t.as_u64());
        let end = end.map(|t| t.as_u64());

        for file in parquet_files {
            let Some(interval) = parse_filename_timestamps(&file) else {
                continue;
            };

            if start.is_none_or(|s| interval.0 >= s) && end.is_none_or(|e| interval.1 <= e) {
                files_to_consolidate.push((file, interval));
            }
        }

        files_to_consolidate.sort_by_key(|&(_, (interval_start, _))| interval_start);
        let intervals: Vec<(u64, u64)> = files_to_consolidate
            .iter()
            .map(|&(_, interval)| interval)
            .collect();

        // Validate disjointness before merging so source files are left untouched on failure
        if ensure_contiguous_files.unwrap_or(true) && !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint before consolidating a directory");
        }

        let Some(last_end) = intervals.iter().map(|interval| interval.1).max() else {
            return Ok(());
        };

        let file_name =
            timestamps_to_filename(UnixNanos::from(intervals[0].0), UnixNanos::from(last_end));
        let path = make_object_store_path(directory, [&file_name]);

        let object_paths = files_to_consolidate
            .iter()
            .map(|(path, _)| self.to_object_path_parsed(path))
            .collect::<anyhow::Result<Vec<_>>>()?;

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
        })
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
    /// - `ensure_contiguous_files`: If true, uses period boundaries clipped to `start` and `end` for file naming.
    ///   If false, uses actual data timestamps for file naming.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails for any directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `period_nanos` is zero and a directory holds data to consolidate.
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
            let (Some(data_cls_name), identifier) =
                self.extract_data_cls_and_identifier_from_path(&directory)?
            else {
                continue;
            };

            if !self.dispatch_consolidate_data_by_period(
                &data_cls_name,
                identifier.as_deref(),
                period_nanos,
                start,
                end,
                ensure_contiguous_files,
            )? {
                // Skip unknown data types
                log::warn!("Unknown data type for consolidation: {data_cls_name}");
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
        // Parse from the catalog root so a base path containing a `data` segment is skipped
        let path = self.path_without_local_base(&normalize_path_separators(path));
        let path_components = extract_path_components(&path);

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
            let identifier = path_components.get(data_index + 2).cloned();

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
    /// - `data_type`: The stored family to consolidate.
    /// - `identifier`: Optional instrument ID to consolidate. If None, consolidates all instruments.
    /// - `period_nanos`: The period duration for consolidation in nanoseconds. Default is 1 day (86400000000000).
    ///   Examples: 3600000000000 (1 hour), 604800000000000 (7 days), 1800000000000 (30 minutes)
    /// - `start`: Optional start timestamp for consolidation range. If None, uses earliest available data.
    ///   If specified and intersects existing files, those files will be split to preserve
    ///   data outside the consolidation range.
    /// - `end`: Optional end timestamp for consolidation range. If None, uses latest available data.
    ///   If specified and intersects existing files, those files will be split to preserve
    ///   data outside the consolidation range.
    /// - `ensure_contiguous_files`: If true, uses period boundaries clipped to `start` and `end` for file naming.
    ///   If false, uses actual data timestamps for file naming.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `period_nanos` is zero and the directory holds data to consolidate.
    /// - `start` exceeds `end` and the directory holds data to consolidate.
    /// - `data_type` is a record family or an instrument selector, which have no
    ///   period-typed rewrite; use [`Self::consolidate_data`] for those.
    /// - The directory path cannot be constructed.
    /// - File operations fail.
    /// - Data querying or writing fails.
    ///
    /// # Notes
    ///
    /// - Uses two-phase approach: first determines all queries, then executes them.
    /// - Groups intervals into contiguous groups to preserve holes between groups.
    /// - Allows consolidation across multiple files within each contiguous group.
    /// - Validates existing targets before keeping them and removing their source fragments.
    /// - Removes source files only after processing their entire time range.
    /// - When `ensure_contiguous_files=false`, file timestamps match actual data range.
    /// - When `ensure_contiguous_files=true`, file timestamps use period boundaries clipped to `start` and `end`.
    /// - Existing-target checks compare row values without counting identical copies.
    /// - Uses modulo arithmetic for efficient period boundary calculation.
    /// - Preserves holes in data by preventing queries from spanning across gaps.
    /// - Automatically splits files at start/end boundaries to preserve all data.
    /// - Split operations are executed before consolidation to ensure data preservation.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_core::UnixNanos;
    /// use nautilus_model::data::NautilusDataType;
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
    ///     &NautilusDataType::QuoteTick.into(),
    ///     None,
    ///     Some(86400000000000), // 1 day in nanoseconds
    ///     None,
    ///     None,
    ///     Some(true),
    /// )?;
    ///
    /// // Consolidate specific instrument by 1-hour periods
    /// catalog.consolidate_data_by_period(
    ///     &NautilusDataType::TradeTick.into(),
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
        data_type: &CatalogDataType,
        identifier: Option<&str>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                data_type,
                CatalogDataType::Data(data_type) if *data_type != NautilusDataType::Instrument
            ),
            "Period consolidation applies to data families only, not {data_type}; \
             use consolidate_data",
        );

        for type_name in parquet_catalog_data_type_path_prefixes(data_type) {
            self.consolidate_prefix_data_by_period(
                type_name.as_ref(),
                identifier,
                period_nanos,
                start,
                end,
                ensure_contiguous_files,
            )?;
        }

        Ok(())
    }

    fn consolidate_prefix_data_by_period(
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
            + HasCatalogDataType
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
        let intervals =
            self.get_intervals(&CatalogDataType::Data(data_type.clone()), identifier)?;

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

        // Simulate source removals and target creation before mutating storage, so validation
        // observes the same files as execution while holding only one period's rows.
        let type_name = to_snake_case(std::any::type_name::<T>());

        for write_files in [false, true] {
            let mut files = ConsolidationFiles::new(&existing_files, start, end);
            let mut file_start_ns: Option<u64> = None;

            for query_info in &queries_to_execute {
                let period_data = self.query_period::<T>(identifier, query_info, &files.sources)?;

                let (Some(first), Some(last)) = (period_data.first(), period_data.last()) else {
                    if query_info.use_period_boundaries {
                        file_start_ns.get_or_insert(query_info.query_start);
                    }

                    files.complete(self, query_info, write_files)?;
                    continue;
                };

                let (start_ts, end_ts) =
                    query_info.file_range(&mut file_start_ns, first.ts_init(), last.ts_init());

                Self::check_ascending_timestamps(&period_data, &type_name)?;
                Self::check_chunk_identity(&period_data, &type_name)?;
                let batches = self.data_to_record_batches(&period_data)?;
                let metadata = T::chunk_metadata(&period_data);
                let target_identifier = catalog_identifier_from_metadata(&metadata);
                let target_directory =
                    self.make_path(path_prefix.as_ref(), target_identifier.as_deref())?;
                let target_filename = make_object_store_path(
                    &target_directory,
                    [timestamps_to_filename(start_ts, end_ts)],
                );
                let target = self.to_object_path(&target_filename)?.to_string();

                if files.prepare_target(self, &target, &batches)? && write_files {
                    self.write_parquet_file_checked(
                        &target_directory,
                        start_ts,
                        end_ts,
                        &batches,
                        true,
                        "File",
                        Some(&format!("{type_name} data")),
                        None,
                    )?;
                }

                files.complete(self, query_info, write_files)?;

                // Reset so next period starts a new contiguous segment
                file_start_ns = None;
            }
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
        let data_type = NautilusDataType::Custom {
            type_name: type_name.to_string(),
        };

        let path_prefix = parquet_data_path_prefix(&data_type);
        let intervals = self.get_intervals(&CatalogDataType::Data(data_type), identifier)?;

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

        // Simulate source removals and target creation before mutating storage, so validation
        // observes the same files as execution while holding only one period's rows.
        for write_files in [false, true] {
            let mut files = ConsolidationFiles::new(&existing_files, start, end);
            let mut file_start_ns: Option<u64> = None;

            for query_info in &queries_to_execute {
                let custom_items =
                    self.query_custom_period(type_name, identifier, query_info, &files.sources)?;

                let (Some(first), Some(last)) = (custom_items.first(), custom_items.last()) else {
                    if query_info.use_period_boundaries {
                        file_start_ns.get_or_insert(query_info.query_start);
                    }

                    files.complete(self, query_info, write_files)?;
                    continue;
                };

                let (start_ts, end_ts) = query_info.file_range(
                    &mut file_start_ns,
                    first.data.ts_init(),
                    last.data.ts_init(),
                );

                self.consolidate_custom_period(
                    &mut files,
                    &custom_items,
                    start_ts,
                    end_ts,
                    write_files,
                )?;
                files.complete(self, query_info, write_files)?;

                // Reset so next period starts a new contiguous segment
                file_start_ns = None;
            }
        }

        Ok(())
    }

    fn consolidate_custom_period(
        &self,
        files: &mut ConsolidationFiles,
        rows: &[CustomData],
        start: UnixNanos,
        end: UnixNanos,
        write_files: bool,
    ) -> anyhow::Result<()> {
        let writes = self.prepare_custom_data_groups(rows, Some(start), Some(end), Some(true))?;

        for write in &writes {
            let target = self
                .to_object_path(&write.path().to_string_lossy())?
                .to_string();

            if files.prepare_target(self, &target, &write.batches)? && write_files {
                self.write_custom_data_prepared(write, Some(true))?;
            }
        }

        Ok(())
    }

    // Queries the data of one consolidation period from `files` only.
    fn query_period<T>(
        &mut self,
        identifier: Option<&str>,
        query_info: &ConsolidationQuery,
        files: &[String],
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeTypedFromRecordBatch + HasCatalogDataType + HasTsInit,
    {
        let instrument_ids = identifier.map(|id| vec![id.to_string()]);

        // Use optimize_file_loading=false to match Python behavior:
        // During consolidation, we want to read only the specific files being consolidated,
        // not the entire directory. This ensures precise file control during consolidation.
        self.query::<T>(
            instrument_ids,
            Some(UnixNanos::from(query_info.query_start)),
            Some(UnixNanos::from(query_info.query_end)),
            None,
            Some(files.to_vec()),
            false, // optimize_file_loading=false for precise file control during consolidation
        )
    }

    // Queries the custom data of one consolidation period from `files` only.
    fn query_custom_period(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        query_info: &ConsolidationQuery,
        files: &[String],
    ) -> anyhow::Result<Vec<CustomData>> {
        let instrument_ids = identifier.map(|id| vec![id.to_string()]);
        let period_data = self.query_custom_data_dynamic(
            type_name,
            instrument_ids.as_deref(),
            Some(UnixNanos::from(query_info.query_start)),
            Some(UnixNanos::from(query_info.query_end)),
            None,
            Some(files.to_vec()),
            false, // optimize_file_loading=false for precise file control during consolidation
        )?;

        Ok(period_data
            .into_iter()
            .filter_map(|data| match data {
                Data::Custom(c) => Some(c),
                _ => None,
            })
            .collect())
    }

    /// Prepares all queries for consolidation by filtering, grouping, and handling splits.
    ///
    /// Includes every period and the segments outside the requested bounds. Existing targets
    /// remain in the plan so execution can verify their rows before removing source files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `period_nanos` is zero.
    /// - `start` exceeds `end`.
    /// - The intervals are not contiguous when `ensure_contiguous_files` is true.
    #[expect(
        clippy::too_many_arguments,
        reason = "Consolidation keeps its window and policy arguments explicit"
    )]
    pub fn prepare_consolidation_queries(
        &self,
        _type_name: &str,
        _identifier: Option<&str>,
        intervals: &[(u64, u64)],
        period_nanos: u64,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: bool,
    ) -> anyhow::Result<Vec<ConsolidationQuery>> {
        anyhow::ensure!(period_nanos > 0, "period_nanos must be positive");

        if let (Some(start), Some(end)) = (start, end) {
            anyhow::ensure!(start <= end, "consolidation start must not exceed end");
        }

        // Filter intervals by time range if specified
        let used_start = start.map(|s| s.as_u64());
        let used_end = end.map(|e| e.as_u64());

        let mut filtered_intervals = Vec::new();

        for &(interval_start, interval_end) in intervals {
            // Check if interval overlaps with the specified range
            if interval_intersects_range((interval_start, interval_end), start, end) {
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
                let last_end = filtered_intervals
                    .iter()
                    .map(|interval| interval.1)
                    .max()
                    .unwrap_or(end_ts);
                if end_ts < last_end {
                    // Split after end: preserve data from end+1 to interval_end
                    queries_to_execute.push(ConsolidationQuery {
                        query_start: end_ts + 1,
                        query_end: last_end,
                        use_period_boundaries: false,
                    });
                }
            }
        }

        // Generate period-based consolidation queries for each contiguous group
        for group in contiguous_groups {
            let group_start = group[0].0;
            let group_end = group
                .iter()
                .fold(group[0].1, |end, interval| end.max(interval.1));

            // Apply start/end filtering to the group
            let effective_start = used_start.map_or(group_start, |s| s.max(group_start));
            let effective_end = used_end.map_or(group_end, |e| e.min(group_end));

            if effective_start > effective_end {
                continue; // Skip if no overlap
            }

            // Generate period-based queries within this contiguous group
            let mut current_start_ns = (effective_start / period_nanos) * period_nanos;

            loop {
                let current_end_ns = current_start_ns
                    .saturating_add(period_nanos - 1)
                    .min(effective_end);

                queries_to_execute.push(ConsolidationQuery {
                    query_start: current_start_ns.max(used_start.unwrap_or(current_start_ns)),
                    query_end: current_end_ns,
                    use_period_boundaries: ensure_contiguous_files,
                });

                match current_start_ns.checked_add(period_nanos) {
                    Some(next_start_ns) if next_start_ns <= effective_end => {
                        current_start_ns = next_start_ns;
                    }
                    _ => break,
                }
            }
        }

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
        let mut current_end = intervals[0].1;
        for &(curr_start, curr_end) in intervals.iter().skip(1) {
            if curr_start.saturating_sub(current_end) > period_nanos {
                contiguous_groups.push(current_group);
                current_group = vec![(curr_start, curr_end)];
                current_end = curr_end;
            } else {
                current_group.push((curr_start, curr_end));
                current_end = current_end.max(curr_end);
            }
        }

        contiguous_groups.push(current_group);

        contiguous_groups
    }
}

impl ConsolidationQuery {
    fn file_range(
        &self,
        start_ns: &mut Option<u64>,
        first: UnixNanos,
        last: UnixNanos,
    ) -> (UnixNanos, UnixNanos) {
        if self.use_period_boundaries {
            let start = *start_ns.get_or_insert(self.query_start);
            (UnixNanos::from(start), UnixNanos::from(self.query_end))
        } else {
            (first, last)
        }
    }
}

struct ConsolidationFiles {
    sources: Vec<String>,
    paths: AHashSet<String>,
    completed: Vec<ClosedInterval>,
}

impl ConsolidationFiles {
    fn new(files: &[String], start: Option<UnixNanos>, end: Option<UnixNanos>) -> Self {
        Self {
            sources: files
                .iter()
                .filter(|file| {
                    parse_filename_timestamps(file)
                        .is_some_and(|interval| interval_intersects_range(interval, start, end))
                })
                .cloned()
                .collect(),
            paths: files.iter().cloned().collect(),
            completed: Vec::new(),
        }
    }

    fn prepare_target(
        &mut self,
        catalog: &ParquetDataCatalog,
        target: &str,
        batches: &[RecordBatch],
    ) -> anyhow::Result<bool> {
        if !self.paths.contains(target) {
            self.paths.insert(target.to_string());
            return Ok(true);
        }

        anyhow::ensure!(
            catalog.target_holds_rows(target, batches)?,
            "Cannot consolidate into existing target {target}: it does not hold every source row",
        );
        self.sources.retain(|file| file != target);
        Ok(false)
    }

    fn complete(
        &mut self,
        catalog: &ParquetDataCatalog,
        query: &ConsolidationQuery,
        write_files: bool,
    ) -> anyhow::Result<()> {
        self.completed.push(ClosedInterval {
            start: query.query_start,
            end: query.query_end,
        });

        self.completed = merge_closed_intervals(&self.completed);

        for file in self.sources.clone() {
            if let Some((start, end)) = parse_filename_timestamps(&file)
                && self
                    .completed
                    .iter()
                    .any(|covered| covered.start <= start && end <= covered.end)
            {
                self.sources.retain(|source| source != &file);
                self.paths.remove(&file);
                if write_files {
                    catalog.delete_file(&file)?;
                }
            }
        }

        Ok(())
    }
}

fn interval_intersects_range(
    (interval_start, interval_end): (u64, u64),
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> bool {
    start.is_none_or(|start| start.as_u64() <= interval_end)
        && end.is_none_or(|end| interval_start <= end.as_u64())
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
    #[case("custom/signal", false)]
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
