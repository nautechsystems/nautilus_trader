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

//! Interval coverage and missing-interval checks for the Parquet catalog.

#![expect(
    clippy::missing_panics_doc,
    reason = "coverage functions use checked schema assumptions from catalog-controlled batches"
)]

use super::{
    Cow, ParquetDataCatalog, extract_bar_type_instrument_id, parse_filename_timestamps,
    query::is_parquet_bar_prefix, query_interval_diff, urisafe_instrument_id, urlencoding,
};
use crate::catalog::types::INSTRUMENT_PATH_PREFIXES;

impl ParquetDataCatalog {
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
    /// // Find missing intervals for quote data
    /// let missing = catalog.get_missing_intervals_for_request(
    ///     1609459200000000000, // start
    ///     1609545600000000000, // end
    ///     "quotes",
    ///     Some("BTCUSD"),
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
        identifier: Option<&str>,
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
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
    /// // Get the first timestamp for quote data
    /// if let Some(first_ts) = catalog.query_first_timestamp("quotes", Some("BTCUSD"))? {
    ///     println!("First quote timestamp: {}", first_ts);
    /// } else {
    ///     println!("No quote data found");
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_first_timestamp(
        &self,
        data_cls: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<Option<u64>> {
        let intervals = self.get_intervals(data_cls, identifier)?;

        Ok(intervals.first().map(|interval| interval.0))
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
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
    /// // Get the last timestamp for quote data
    /// if let Some(last_ts) = catalog.query_last_timestamp("quotes", Some("BTCUSD"))? {
    ///     println!("Last quote timestamp: {}", last_ts);
    /// } else {
    ///     println!("No quote data found");
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn query_last_timestamp(
        &self,
        data_cls: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<Option<u64>> {
        let intervals = self.get_intervals(data_cls, identifier)?;

        Ok(intervals.last().map(|interval| interval.1))
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
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
    /// // Get all intervals for quote data
    /// let intervals = catalog.get_intervals("quotes", Some("BTCUSD"))?;
    /// for (start, end) in intervals {
    ///     println!("Data available from {} to {}", start, end);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_intervals(
        &self,
        data_cls: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        if data_cls == "instruments" {
            let mut intervals = Vec::new();

            for prefix in INSTRUMENT_PATH_PREFIXES {
                let directory = self.make_path(prefix, identifier)?;
                intervals.extend(self.get_directory_intervals(&directory)?);
            }
            return Ok(merge_overlapping(intervals));
        }
        let directory = self.make_path(data_cls, identifier)?;
        let intervals = self.get_directory_intervals(&directory)?;

        if identifier.is_none() {
            // `get_directory_intervals` already recursed through every per-identifier
            // subdirectory via `object_store.list`, so intervals from different
            // identifiers can overlap. Merge overlaps into a disjoint sorted union
            // so callers like `query_last_timestamp` see the true max end and
            // `consolidate_data_by_period` sees contiguous coverage.
            return Ok(merge_overlapping(intervals));
        }

        // For bars, fall back to partial matching when the exact directory
        // doesn't exist (callers may pass an instrument_id like "EUR/USD.SIM"
        // but bars are stored under bar_type dirs like "EURUSD.SIM-1-MINUTE-...")

        if !intervals.is_empty() || !is_parquet_bar_prefix(data_cls) {
            return Ok(intervals);
        }

        let safe_id = urisafe_instrument_id(identifier.unwrap());

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
                let subdir_path = self.make_path(data_cls, Some(&decoded))?;
                all_intervals.extend(self.get_directory_intervals(&subdir_path)?);
            }
        }

        all_intervals.sort_by_key(|&(start, _)| start);

        // Merge overlapping intervals from different bar types so that
        // last().1 reliably gives the maximum end timestamp
        Ok(merge_overlapping(all_intervals))
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
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
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
        // For local stores with empty base_path, to_object_path returns path as-is.
        // For remote stores, to_object_path preserves or prepends the catalog base path.
        let object_dir = self.to_object_path(directory)?;
        let list_result = self.list_objects(object_dir.as_ref())?;

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
}

/// Merges overlapping intervals (sorted by start) into a disjoint sorted union.
///
/// Adjacent intervals stay separate, unlike [`crate::common::coverage::merge_closed_intervals`]:
/// these intervals describe stored files, and `are_intervals_contiguous` checks that consecutive
/// files abut exactly, which merging them away would hide.
fn merge_overlapping(intervals: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    let mut merged: Vec<(u64, u64)> = Vec::new();

    for interval in intervals {
        if let Some(last) = merged.last_mut()
            && interval.0 <= last.1
        {
            last.1 = last.1.max(interval.1);
            continue;
        }
        merged.push(interval);
    }

    merged
}
