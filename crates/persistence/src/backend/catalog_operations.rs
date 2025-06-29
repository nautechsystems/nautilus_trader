//! Catalog operations for data consolidation and reset functionality.
//!
//! This module contains the consolidation and reset operations for the `ParquetDataCatalog`.
//! These operations are separated into their own module for better organization and maintainability.

use std::collections::HashSet;

use anyhow::Result;
use futures::StreamExt;
use nautilus_core::UnixNanos;
use nautilus_model::data::{Data, HasTsInit};
use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};
use object_store::path::Path as ObjectPath;

use crate::{
    backend::catalog::{
        CatalogPathPrefix, ParquetDataCatalog, are_intervals_contiguous, are_intervals_disjoint,
        parse_filename_timestamps, timestamps_to_filename,
    },
    parquet::{
        combine_parquet_files_from_object_store, min_max_from_parquet_metadata_object_store,
    },
};

/// Information about a consolidation query to be executed.
///
/// This struct encapsulates all the information needed to execute a single consolidation
/// operation, including the data range to query and file naming strategy.
///
/// # Fields
///
/// - `query_start`: Start timestamp for the data query range (inclusive, in nanoseconds)
/// - `query_end`: End timestamp for the data query range (inclusive, in nanoseconds)
/// - `use_period_boundaries`: If true, uses period boundaries for file naming; if false, uses actual data timestamps
///
/// # Usage
///
/// This struct is used internally by the consolidation system to plan and execute
/// data consolidation operations. It allows the system to:
/// - Separate query planning from execution
/// - Handle complex scenarios like data splitting
/// - Optimize file naming strategies
/// - Batch multiple operations efficiently
/// - Maintain file contiguity across periods
///
/// # Examples
///
/// ```rust,no_run
/// use nautilus_persistence::backend::catalog_operations::ConsolidationQuery;
///
/// // Regular consolidation query
/// let query = ConsolidationQuery {
///     query_start: 1609459200000000000,
///     query_end: 1609545600000000000,
///     use_period_boundaries: true,
/// };
///
/// // Split operation to preserve data
/// let split_query = ConsolidationQuery {
///     query_start: 1609459200000000000,
///     query_end: 1609462800000000000,
///     use_period_boundaries: false,
/// };
/// ```
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
    /// This function will return an error if:
    /// - Directory listing fails.
    /// - File consolidation operations fail.
    /// - Interval validation fails (when `ensure_contiguous_files` is true).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Consolidate all files in the catalog
    /// catalog.consolidate_catalog(None, None, None)?;
    ///
    /// // Consolidate only files within a specific time range
    /// catalog.consolidate_catalog(
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(true)
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_catalog(
        &self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.consolidate_directory(&directory, start, end, ensure_contiguous_files)?;
        }

        Ok(())
    }

    /// Consolidates data files for a specific data type and instrument.
    ///
    /// This method consolidates Parquet files within a specific directory (defined by data type
    /// and optional instrument ID) by merging multiple files into a single file. This improves
    /// query performance and can reduce storage overhead.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars").
    /// - `instrument_id`: Optional instrument ID to target a specific instrument's data.
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
    /// This function will return an error if:
    /// - The directory path cannot be constructed
    /// - File consolidation operations fail
    /// - Interval validation fails (when `ensure_contiguous_files` is true)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Consolidate all quote files for a specific instrument
    /// catalog.consolidate_data(
    ///     "quotes",
    ///     Some("BTCUSD".to_string()),
    ///     None,
    ///     None,
    ///     None
    /// )?;
    ///
    /// // Consolidate trade files within a time range
    /// catalog.consolidate_data(
    ///     "trades",
    ///     None,
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(true)
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_data(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> Result<()> {
        let directory = self.make_path(type_name, instrument_id)?;
        self.consolidate_directory(&directory, start, end, ensure_contiguous_files)
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
    /// - Skips consolidation if directory contains 1 or fewer files
    /// - Filters files by timestamp range if start/end are specified
    /// - Sorts intervals by start timestamp before consolidation
    /// - Creates a new file spanning the entire time range of input files
    /// - Validates interval disjointness after consolidation (if enabled)
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Directory listing fails
    /// - File combination operations fail
    /// - Interval validation fails (when `ensure_contiguous_files` is true)
    /// - Object store operations fail
    fn consolidate_directory(
        &self,
        directory: &str,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> Result<()> {
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
            let path = format!("{directory}/{file_name}");

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

        if ensure_contiguous_files.unwrap_or(true) && !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after consolidating a directory");
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
    /// This function will return an error if:
    /// - Directory listing fails
    /// - Data type extraction from path fails
    /// - Period-based consolidation operations fail
    ///
    /// # Notes
    ///
    /// - This operation can be resource-intensive for large catalogs with many data types
    ///   and instruments.
    /// - The consolidation process splits data into fixed time periods rather than combining
    ///   all files into a single file per directory.
    /// - Uses the same period-based consolidation logic as `consolidate_data_by_period`.
    /// - Original files are removed and replaced with period-based consolidated files.
    /// - This method is useful for periodic maintenance of the catalog to standardize
    ///   file organization by time periods.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Consolidate all files in the catalog by 1-day periods
    /// catalog.consolidate_catalog_by_period(
    ///     Some(86400000000000), // 1 day in nanoseconds
    ///     None,
    ///     None,
    ///     Some(true)
    /// )?;
    ///
    /// // Consolidate only files within a specific time range by 1-hour periods
    /// catalog.consolidate_catalog_by_period(
    ///     Some(3600000000000), // 1 hour in nanoseconds
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(false)
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_catalog_by_period(
        &mut self,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            let (data_cls, identifier) =
                self.extract_data_cls_and_identifier_from_path(&directory)?;

            if let Some(data_cls_name) = data_cls {
                // Use match statement to call the generic consolidate_data_by_period for various types
                match data_cls_name.as_str() {
                    "quotes" => {
                        use nautilus_model::data::QuoteTick;
                        self.consolidate_data_by_period_generic::<QuoteTick>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "trades" => {
                        use nautilus_model::data::TradeTick;
                        self.consolidate_data_by_period_generic::<TradeTick>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "order_book_deltas" => {
                        use nautilus_model::data::OrderBookDelta;
                        self.consolidate_data_by_period_generic::<OrderBookDelta>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "order_book_depths" => {
                        use nautilus_model::data::OrderBookDepth10;
                        self.consolidate_data_by_period_generic::<OrderBookDepth10>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "bars" => {
                        use nautilus_model::data::Bar;
                        self.consolidate_data_by_period_generic::<Bar>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "index_prices" => {
                        use nautilus_model::data::IndexPriceUpdate;
                        self.consolidate_data_by_period_generic::<IndexPriceUpdate>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "mark_prices" => {
                        use nautilus_model::data::MarkPriceUpdate;
                        self.consolidate_data_by_period_generic::<MarkPriceUpdate>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    "instrument_closes" => {
                        use nautilus_model::data::close::InstrumentClose;
                        self.consolidate_data_by_period_generic::<InstrumentClose>(
                            identifier,
                            period_nanos,
                            start,
                            end,
                            ensure_contiguous_files,
                        )?;
                    }
                    _ => {
                        // Skip unknown data types
                        log::warn!("Unknown data type for consolidation: {data_cls_name}");
                        continue;
                    }
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
    /// - `path`: The directory path to parse
    ///
    /// # Returns
    ///
    /// Returns a tuple of (`data_class`, identifier) where both are optional strings.
    pub fn extract_data_cls_and_identifier_from_path(
        &self,
        path: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        // Split the path and look for the data directory structure
        let path_parts: Vec<&str> = path.split('/').collect();

        // Find the "data" directory in the path
        if let Some(data_index) = path_parts.iter().position(|&part| part == "data")
            && data_index + 1 < path_parts.len()
        {
            let data_cls = path_parts[data_index + 1].to_string();

            // Check if there's an identifier (instrument ID) after the data class
            let identifier = if data_index + 2 < path_parts.len() {
                Some(path_parts[data_index + 2].to_string())
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
    /// This function will return an error if:
    /// - The directory path cannot be constructed
    /// - File operations fail
    /// - Data querying or writing fails
    ///
    /// # Notes
    ///
    /// - Uses two-phase approach: first determines all queries, then executes them
    /// - Groups intervals into contiguous groups to preserve holes between groups
    /// - Allows consolidation across multiple files within each contiguous group
    /// - Skips queries if target files already exist for efficiency
    /// - Original files are removed immediately after querying each period
    /// - When `ensure_contiguous_files=false`, file timestamps match actual data range
    /// - When `ensure_contiguous_files=true`, file timestamps use period boundaries
    /// - Uses modulo arithmetic for efficient period boundary calculation
    /// - Preserves holes in data by preventing queries from spanning across gaps
    /// - Automatically splits files at start/end boundaries to preserve all data
    /// - Split operations are executed before consolidation to ensure data preservation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    /// use nautilus_core::UnixNanos;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Consolidate all quote files by 1-day periods
    /// catalog.consolidate_data_by_period(
    ///     "quotes",
    ///     None,
    ///     Some(86400000000000), // 1 day in nanoseconds
    ///     None,
    ///     None,
    ///     Some(true)
    /// )?;
    ///
    /// // Consolidate specific instrument by 1-hour periods
    /// catalog.consolidate_data_by_period(
    ///     "trades",
    ///     Some("BTCUSD".to_string()),
    ///     Some(3600000000000), // 1 hour in nanoseconds
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    ///     Some(false)
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn consolidate_data_by_period(
        &mut self,
        type_name: &str,
        identifier: Option<String>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> Result<()> {
        // Use match statement to call the generic consolidate_data_by_period for various types
        match type_name {
            "quotes" => {
                use nautilus_model::data::QuoteTick;
                self.consolidate_data_by_period_generic::<QuoteTick>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "trades" => {
                use nautilus_model::data::TradeTick;
                self.consolidate_data_by_period_generic::<TradeTick>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "order_book_deltas" => {
                use nautilus_model::data::OrderBookDelta;
                self.consolidate_data_by_period_generic::<OrderBookDelta>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "order_book_depths" => {
                use nautilus_model::data::OrderBookDepth10;
                self.consolidate_data_by_period_generic::<OrderBookDepth10>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "bars" => {
                use nautilus_model::data::Bar;
                self.consolidate_data_by_period_generic::<Bar>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "index_prices" => {
                use nautilus_model::data::IndexPriceUpdate;
                self.consolidate_data_by_period_generic::<IndexPriceUpdate>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "mark_prices" => {
                use nautilus_model::data::MarkPriceUpdate;
                self.consolidate_data_by_period_generic::<MarkPriceUpdate>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            "instrument_closes" => {
                use nautilus_model::data::close::InstrumentClose;
                self.consolidate_data_by_period_generic::<InstrumentClose>(
                    identifier,
                    period_nanos,
                    start,
                    end,
                    ensure_contiguous_files,
                )?;
            }
            _ => {
                anyhow::bail!("Unknown data type for consolidation: {}", type_name);
            }
        }

        Ok(())
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
    /// - `identifier`: Optional instrument ID to target a specific instrument's data
    /// - `period_nanos`: Optional period size in nanoseconds (default: 1 day)
    /// - `start`: Optional start timestamp for consolidation range
    /// - `end`: Optional end timestamp for consolidation range
    /// - `ensure_contiguous_files`: Optional flag to control file naming strategy
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if consolidation fails.
    pub fn consolidate_data_by_period_generic<T>(
        &mut self,
        identifier: Option<String>,
        period_nanos: Option<u64>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> Result<()>
    where
        T: DecodeDataFromRecordBatch
            + CatalogPathPrefix
            + EncodeToRecordBatch
            + HasTsInit
            + TryFrom<Data>
            + Clone,
    {
        let period_nanos = period_nanos.unwrap_or(86400000000000); // Default: 1 day
        let ensure_contiguous_files = ensure_contiguous_files.unwrap_or(true);

        // Use get_intervals for cleaner implementation
        let intervals = self.get_intervals(T::path_prefix(), identifier.clone())?;

        if intervals.is_empty() {
            return Ok(()); // No files to consolidate
        }

        // Use auxiliary function to prepare all queries for execution
        let queries_to_execute = self.prepare_consolidation_queries(
            T::path_prefix(),
            identifier.clone(),
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
        let directory = self.make_path(T::path_prefix(), identifier.clone())?;
        let mut existing_files = self.list_parquet_files(&directory)?;
        existing_files.sort();

        // Track files to remove and maintain existing_files list
        let mut files_to_remove = HashSet::new();
        let original_files_count = existing_files.len();

        // Phase 2: Execute queries, write, and delete
        let mut file_start_ns: Option<u64> = None; // Track contiguity across periods

        for query_info in queries_to_execute {
            // Query data for this period using query_typed_data
            let instrument_ids = identifier.as_ref().map(|id| vec![id.clone()]);

            let period_data = self.query_typed_data::<T>(
                instrument_ids,
                Some(UnixNanos::from(query_info.query_start)),
                Some(UnixNanos::from(query_info.query_end)),
                None,
                Some(existing_files.clone()),
            )?;

            if period_data.is_empty() {
                // Skip if no data found, but maintain contiguity by using query start
                if file_start_ns.is_none() {
                    file_start_ns = Some(query_info.query_start);
                }
                continue;
            }
            file_start_ns = None;

            // Determine final file timestamps
            let (final_start_ns, final_end_ns) = if query_info.use_period_boundaries {
                // Use period boundaries for file naming, maintaining contiguity
                if file_start_ns.is_none() {
                    file_start_ns = Some(query_info.query_start);
                }
                (file_start_ns.unwrap(), query_info.query_end)
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
                // Skip if target file already exists
                continue;
            }

            // Write consolidated data for this period using write_to_parquet
            // Use skip_disjoint_check since we're managing file removal carefully
            let start_ts = UnixNanos::from(final_start_ns);
            let end_ts = UnixNanos::from(final_end_ns);
            self.write_to_parquet(period_data, Some(start_ts), Some(end_ts), Some(true))?;

            // Identify files that are completely covered by this period
            // Only remove files AFTER successfully writing a new file
            // Use slice copy to avoid modification during iteration (match Python logic)
            for file in existing_files.clone() {
                if let Some(interval) = parse_filename_timestamps(&file)
                    && interval.1 <= query_info.query_end
                {
                    files_to_remove.insert(file.clone());
                    existing_files.retain(|f| f != &file);
                }
            }

            // Remove files as soon as we have some to remove
            if !files_to_remove.is_empty() {
                for file in files_to_remove.drain() {
                    self.delete_file(&file)?;
                }
            }
        }

        // Remove any remaining files that weren't removed in the loop
        // This matches the Python implementation's final cleanup step
        // Only remove files if any consolidation actually happened (i.e., files were processed)
        let files_were_processed = existing_files.len() < original_files_count;
        if files_were_processed {
            for file in existing_files {
                self.delete_file(&file)?;
            }
        }

        Ok(())
    }

    /// Prepares all queries for consolidation by filtering, grouping, and handling splits.
    ///
    /// This auxiliary function handles all the preparation logic for consolidation:
    /// 1. Filters intervals by time range
    /// 2. Groups intervals into contiguous groups
    /// 3. Identifies and creates split operations for data preservation
    /// 4. Generates period-based consolidation queries
    /// 5. Checks for existing target files
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_consolidation_queries(
        &self,
        type_name: &str,
        identifier: Option<String>,
        intervals: &[(u64, u64)],
        period_nanos: u64,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: bool,
    ) -> Result<Vec<ConsolidationQuery>> {
        // Filter intervals by time range if specified
        let used_start = start.map(|s| s.as_u64());
        let used_end = end.map(|e| e.as_u64());

        let mut filtered_intervals = Vec::new();
        for &(interval_start, interval_end) in intervals {
            // Check if interval overlaps with the specified range
            if (used_start.is_none() || used_start.unwrap() <= interval_end)
                && (used_end.is_none() || interval_start <= used_end.unwrap())
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

        // Group intervals into contiguous groups to preserve holes between groups
        // but allow consolidation within each contiguous group
        let contiguous_groups = self.group_contiguous_intervals(&filtered_intervals);

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
                    let directory = self.make_path(type_name, identifier.clone())?;
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

    /// Groups intervals into contiguous groups for efficient consolidation.
    ///
    /// This method analyzes a list of time intervals and groups them into contiguous sequences.
    /// Intervals are considered contiguous if the end of one interval is exactly one nanosecond
    /// before the start of the next interval. This grouping preserves data gaps while allowing
    /// consolidation within each contiguous group.
    ///
    /// # Parameters
    ///
    /// - `intervals`: A slice of timestamp intervals as (start, end) tuples.
    ///
    /// # Returns
    ///
    /// Returns a vector of groups, where each group is a vector of contiguous intervals.
    /// Returns an empty vector if the input is empty.
    ///
    /// # Algorithm
    ///
    /// 1. Starts with the first interval in a new group
    /// 2. For each subsequent interval, checks if it's contiguous with the previous
    /// 3. If contiguous (`prev_end` + 1 == `curr_start`), adds to current group
    /// 4. If not contiguous, starts a new group
    /// 5. Returns all groups
    ///
    /// # Examples
    ///
    /// ```text
    /// Contiguous intervals: [(1,5), (6,10), (11,15)]
    /// Returns: [[(1,5), (6,10), (11,15)]]
    ///
    /// Non-contiguous intervals: [(1,5), (8,10), (12,15)]
    /// Returns: [[(1,5)], [(8,10)], [(12,15)]]
    /// ```
    ///
    /// # Notes
    ///
    /// - Input intervals should be sorted by start timestamp
    /// - Gaps between groups are preserved and not consolidated
    /// - Used internally by period-based consolidation methods
    #[must_use]
    pub fn group_contiguous_intervals(&self, intervals: &[(u64, u64)]) -> Vec<Vec<(u64, u64)>> {
        if intervals.is_empty() {
            return Vec::new();
        }

        let mut contiguous_groups = Vec::new();
        let mut current_group = vec![intervals[0]];

        for i in 1..intervals.len() {
            let prev_interval = intervals[i - 1];
            let curr_interval = intervals[i];

            // Check if current interval is contiguous with previous (end + 1 == start)
            if prev_interval.1 + 1 == curr_interval.0 {
                current_group.push(curr_interval);
            } else {
                // Gap found, start new group
                contiguous_groups.push(current_group);
                current_group = vec![curr_interval];
            }
        }

        // Add the last group
        contiguous_groups.push(current_group);

        contiguous_groups
    }

    /// Checks if a file exists in the object store.
    ///
    /// This method performs a HEAD operation on the object store to determine if a file
    /// exists without downloading its content. It works with both local and remote object stores.
    ///
    /// # Parameters
    ///
    /// - `path`: The file path to check, relative to the catalog structure.
    ///
    /// # Returns
    ///
    /// Returns `true` if the file exists, `false` if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the object store operation fails due to network issues,
    /// authentication problems, or other I/O errors.
    fn file_exists(&self, path: &str) -> Result<bool> {
        let object_path = self.to_object_path(path);
        let exists =
            self.execute_async(async { Ok(self.object_store.head(&object_path).await.is_ok()) })?;
        Ok(exists)
    }

    /// Deletes a file from the object store.
    ///
    /// This method removes a file from the object store. The operation is permanent
    /// and cannot be undone. It works with both local filesystems and remote object stores.
    ///
    /// # Parameters
    ///
    /// - `path`: The file path to delete, relative to the catalog structure.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful deletion.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file doesn't exist
    /// - Permission is denied
    /// - Network issues occur (for remote stores)
    /// - The object store operation fails
    ///
    /// # Safety
    ///
    /// This operation is irreversible. Ensure the file is no longer needed before deletion.
    fn delete_file(&self, path: &str) -> Result<()> {
        let object_path = self.to_object_path(path);
        self.execute_async(async {
            self.object_store
                .delete(&object_path)
                .await
                .map_err(anyhow::Error::from)
        })?;
        Ok(())
    }

    /// Resets the filenames of all Parquet files in the catalog to match their actual content timestamps.
    ///
    /// This method scans all leaf data directories in the catalog and renames files based on
    /// the actual timestamp range of their content. This is useful when files have been
    /// modified or when filename conventions have changed.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Directory listing fails
    /// - File metadata reading fails
    /// - File rename operations fail
    /// - Interval validation fails after renaming
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Reset all filenames in the catalog
    /// catalog.reset_catalog_file_names()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn reset_catalog_file_names(&self) -> Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.reset_file_names(&directory)?;
        }

        Ok(())
    }

    /// Resets the filenames of Parquet files for a specific data type and instrument ID.
    ///
    /// This method renames files in a specific directory based on the actual timestamp
    /// range of their content. This is useful for correcting filenames after data
    /// modifications or when filename conventions have changed.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data type directory name (e.g., "quotes", "trades").
    /// - `instrument_id`: Optional instrument ID to target a specific instrument's data.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The directory path cannot be constructed
    /// - File metadata reading fails
    /// - File rename operations fail
    /// - Interval validation fails after renaming
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// // Reset filenames for all quote files
    /// catalog.reset_data_file_names("quotes", None)?;
    ///
    /// // Reset filenames for a specific instrument's trade files
    /// catalog.reset_data_file_names("trades", Some("BTCUSD".to_string()))?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn reset_data_file_names(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> Result<()> {
        let directory = self.make_path(data_cls, instrument_id)?;
        self.reset_file_names(&directory)
    }

    /// Resets the filenames of Parquet files in a directory to match their actual content timestamps.
    ///
    /// This internal method scans all Parquet files in a directory, reads their metadata to
    /// determine the actual timestamp range of their content, and renames the files accordingly.
    /// This ensures that filenames accurately reflect the data they contain.
    ///
    /// # Parameters
    ///
    /// - `directory`: The directory path containing Parquet files to rename.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Process
    ///
    /// 1. Lists all Parquet files in the directory
    /// 2. For each file, reads metadata to extract min/max timestamps
    /// 3. Generates a new filename based on actual timestamp range
    /// 4. Moves the file to the new name using object store operations
    /// 5. Validates that intervals remain disjoint after renaming
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Directory listing fails
    /// - Metadata reading fails for any file
    /// - File move operations fail
    /// - Interval validation fails after renaming
    /// - Object store operations fail
    ///
    /// # Notes
    ///
    /// - This operation can be time-consuming for directories with many files
    /// - Files are processed sequentially to avoid conflicts
    /// - The operation is atomic per file but not across the entire directory
    fn reset_file_names(&self, directory: &str) -> Result<()> {
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
            let new_file_path = format!("{directory}/{new_filename}");
            let new_object_path = ObjectPath::from(new_file_path);

            self.move_file(&object_path, &new_object_path)?;
        }

        let intervals = self.get_directory_intervals(directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after resetting file names");
        }

        Ok(())
    }

    /// Finds all leaf data directories in the catalog.
    ///
    /// A leaf directory is one that contains data files but no subdirectories.
    /// This method is used to identify directories that can be processed for
    /// consolidation or other operations.
    ///
    /// # Returns
    ///
    /// Returns a vector of directory path strings representing leaf directories,
    /// or an error if directory traversal fails.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Object store listing operations fail
    /// - Directory structure cannot be analyzed
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    ///
    /// let catalog = ParquetDataCatalog::new(/* ... */);
    ///
    /// let leaf_dirs = catalog.find_leaf_data_directories()?;
    /// for dir in leaf_dirs {
    ///     println!("Found leaf directory: {}", dir);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
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
            let prefix = ObjectPath::from(format!("{data_dir}/"));
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
                    .any(|d| d.starts_with(&format!("{dir}/")) && d != dir);

                if has_files && !has_subdirs {
                    leaf_dirs.push(dir.clone());
                }
            }

            Ok::<Vec<String>, anyhow::Error>(leaf_dirs)
        })?;

        Ok(leaf_dirs)
    }
}
