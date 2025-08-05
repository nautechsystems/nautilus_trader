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

use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
};
use pyo3::{exceptions::PyIOError, prelude::*};

use crate::backend::catalog::ParquetDataCatalog;

/// A catalog for writing data to Parquet files.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.persistence")
)]
pub struct ParquetDataCatalogV2 {
    inner: ParquetDataCatalog,
}

#[pymethods]
impl ParquetDataCatalogV2 {
    /// Create a new `ParquetCatalog` with the given base path and optional parameters.
    ///
    /// # Parameters
    ///
    /// - `base_path`: The base path for the catalog
    /// - `storage_options`: Optional storage configuration for cloud backends
    /// - `batch_size`: Optional batch size for processing (default: 5000)
    /// - `compression`: Optional compression type (0=UNCOMPRESSED, 1=SNAPPY, 2=GZIP, 3=LZO, 4=BROTLI, 5=LZ4, 6=ZSTD)
    /// - `max_row_group_size`: Optional maximum row group size (default: 5000)
    #[new]
    #[pyo3(signature = (base_path, storage_options=None, batch_size=None, compression=None, max_row_group_size=None))]
    #[must_use]
    pub fn new(
        base_path: String,
        storage_options: Option<std::collections::HashMap<String, String>>,
        batch_size: Option<usize>,
        compression: Option<u8>,
        max_row_group_size: Option<usize>,
    ) -> Self {
        let compression = compression.map(|c| match c {
            0 => parquet::basic::Compression::UNCOMPRESSED,
            1 => parquet::basic::Compression::SNAPPY,
            // For GZIP, LZO, BROTLI, LZ4, ZSTD we need to use the default level
            // since we can't pass the level parameter through PyO3
            2 => {
                let level = Default::default();
                parquet::basic::Compression::GZIP(level)
            }
            3 => parquet::basic::Compression::LZO,
            4 => {
                let level = Default::default();
                parquet::basic::Compression::BROTLI(level)
            }
            5 => parquet::basic::Compression::LZ4,
            6 => {
                let level = Default::default();
                parquet::basic::Compression::ZSTD(level)
            }
            _ => parquet::basic::Compression::SNAPPY,
        });

        Self {
            inner: ParquetDataCatalog::from_uri(
                &base_path,
                storage_options,
                batch_size,
                compression,
                max_row_group_size,
            )
            .expect("Failed to create ParquetDataCatalog"),
        }
    }

    // TODO: Cannot pass mixed data across pyo3 as a single type
    // pub fn write_data(mut slf: PyRefMut<'_, Self>, data_type: NautilusDataType, data: Vec<Data>) {}

    /// Write quote tick data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of quote ticks to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_quote_ticks(
        &self,
        data: Vec<QuoteTick>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write quote ticks: {e}")))
    }

    /// Write trade tick data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of trade ticks to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_trade_ticks(
        &self,
        data: Vec<TradeTick>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write trade ticks: {e}")))
    }

    /// Write order book delta data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of order book deltas to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_order_book_deltas(
        &self,
        data: Vec<OrderBookDelta>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write order book deltas: {e}")))
    }

    /// Write bar data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of bars to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_bars(
        &self,
        data: Vec<Bar>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write bars: {e}")))
    }

    /// Write order book depth data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of order book depths to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_order_book_depths(
        &self,
        data: Vec<OrderBookDepth10>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write order book depths: {e}")))
    }

    /// Write mark price update data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of mark price updates to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_mark_price_updates(
        &self,
        data: Vec<MarkPriceUpdate>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write mark price updates: {e}")))
    }

    /// Write index price update data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of index price updates to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_index_price_updates(
        &self,
        data: Vec<IndexPriceUpdate>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos, Some(skip_disjoint_check))
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write index price updates: {e}")))
    }

    /// Extend file names in the catalog with additional timestamp information.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name
    /// - `instrument_id`: Optional instrument ID filter
    /// - `start`: Start timestamp (nanoseconds since Unix epoch)
    /// - `end`: End timestamp (nanoseconds since Unix epoch)
    #[pyo3(signature = (data_cls, instrument_id=None, *, start, end))]
    pub fn extend_file_name(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
        start: u64,
        end: u64,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = UnixNanos::from(start);
        let end_nanos = UnixNanos::from(end);

        self.inner
            .extend_file_name(data_cls, instrument_id, start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to extend file name: {e}")))
    }

    /// Consolidate all data files in the catalog within the specified time range.
    ///
    /// # Parameters
    ///
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to ensure files are contiguous
    #[pyo3(signature = (start=None, end=None, ensure_contiguous_files=None))]
    pub fn consolidate_catalog(
        &self,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .consolidate_catalog(start_nanos, end_nanos, ensure_contiguous_files)
            .map_err(|e| PyIOError::new_err(format!("Failed to consolidate catalog: {e}")))
    }

    /// Consolidate data files for a specific data type within the specified time range.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type name to consolidate
    /// - `instrument_id`: Optional instrument ID filter
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to ensure files are contiguous
    #[pyo3(signature = (type_name, instrument_id=None, start=None, end=None, ensure_contiguous_files=None))]
    pub fn consolidate_data(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .consolidate_data(
                type_name,
                instrument_id,
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to consolidate data: {e}")))
    }

    /// Consolidate all data files in the catalog by splitting them into fixed time periods.
    ///
    /// This method identifies all leaf directories in the catalog that contain parquet files
    /// and consolidates them by period. A leaf directory is one that contains files but no subdirectories.
    /// This is a convenience method that effectively calls `consolidate_data_by_period` for all data types
    /// and instrument IDs in the catalog.
    ///
    /// # Parameters
    ///
    /// - `period_nanos`: Optional period duration for consolidation in nanoseconds. Default is 1 day (86400000000000).
    ///   Examples: 3600000000000 (1 hour), 604800000000000 (7 days), 1800000000000 (30 minutes)
    /// - `start`: Optional start timestamp for the consolidation range (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp for the consolidation range (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to control file naming strategy
    #[pyo3(signature = (period_nanos=None, start=None, end=None, ensure_contiguous_files=None))]
    pub fn consolidate_catalog_by_period(
        &mut self,
        period_nanos: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .consolidate_catalog_by_period(
                period_nanos,
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
            )
            .map_err(|e| {
                PyIOError::new_err(format!("Failed to consolidate catalog by period: {e}"))
            })
    }

    /// Consolidate data files by splitting them into fixed time periods.
    ///
    /// This method queries data by period and writes consolidated files immediately,
    /// using efficient period-based consolidation logic. When start/end boundaries intersect existing files,
    /// the function automatically splits those files to preserve all data.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars")
    /// - `identifier`: Optional instrument ID to consolidate. If None, consolidates all instruments
    /// - `period_nanos`: Optional period duration for consolidation in nanoseconds. Default is 1 day (86400000000000).
    ///   Examples: 3600000000000 (1 hour), 604800000000000 (7 days), 1800000000000 (30 minutes)
    /// - `start`: Optional start timestamp for consolidation range (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp for consolidation range (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to control file naming strategy
    #[pyo3(signature = (type_name, identifier=None, period_nanos=None, start=None, end=None, ensure_contiguous_files=None))]
    pub fn consolidate_data_by_period(
        &mut self,
        type_name: &str,
        identifier: Option<String>,
        period_nanos: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .consolidate_data_by_period(
                type_name,
                identifier,
                period_nanos,
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to consolidate data by period: {e}")))
    }

    /// Reset all catalog file names to their canonical form.
    pub fn reset_all_file_names(&self) -> PyResult<()> {
        self.inner
            .reset_all_file_names()
            .map_err(|e| PyIOError::new_err(format!("Failed to reset catalog file names: {e}")))
    }

    /// Reset data file names for a specific data class to their canonical form.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name
    /// - `instrument_id`: Optional instrument ID filter
    #[pyo3(signature = (data_cls, instrument_id=None))]
    pub fn reset_data_file_names(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<()> {
        self.inner
            .reset_data_file_names(data_cls, instrument_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to reset data file names: {e}")))
    }

    /// Delete data within a specified time range across the entire catalog.
    ///
    /// This method identifies all leaf directories in the catalog that contain parquet files
    /// and deletes data within the specified time range from each directory. A leaf directory
    /// is one that contains files but no subdirectories. This is a convenience method that
    /// effectively calls `delete_data_range` for all data types and instrument IDs in the catalog.
    ///
    /// # Parameters
    ///
    /// - `start`: Optional start timestamp for the deletion range (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp for the deletion range (nanoseconds since Unix epoch)
    ///
    /// # Notes
    ///
    /// - This operation permanently removes data and cannot be undone
    /// - The deletion process handles file intersections intelligently by splitting files
    ///   when they partially overlap with the deletion range
    /// - Files completely within the deletion range are removed entirely
    /// - Files partially overlapping the deletion range are split to preserve data outside the range
    /// - This method is useful for bulk data cleanup operations across the entire catalog
    /// - Empty directories are not automatically removed after deletion
    #[pyo3(signature = (start=None, end=None))]
    pub fn delete_catalog_range(&mut self, start: Option<u64>, end: Option<u64>) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .delete_catalog_range(start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to delete catalog range: {e}")))
    }

    /// Delete data within a specified time range for a specific data type and instrument.
    ///
    /// This method identifies all parquet files that intersect with the specified time range
    /// and handles them appropriately:
    /// - Files completely within the range are deleted
    /// - Files partially overlapping the range are split to preserve data outside the range
    /// - The original intersecting files are removed after processing
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars")
    /// - `instrument_id`: Optional instrument ID to delete data for. If None, deletes data across all instruments
    /// - `start`: Optional start timestamp for the deletion range (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp for the deletion range (nanoseconds since Unix epoch)
    ///
    /// # Notes
    ///
    /// - This operation permanently removes data and cannot be undone
    /// - Files that partially overlap the deletion range are split to preserve data outside the range
    /// - The method ensures data integrity by using atomic operations where possible
    /// - Empty directories are not automatically removed after deletion
    #[pyo3(signature = (type_name, instrument_id=None, start=None, end=None))]
    pub fn delete_data_range(
        &mut self,
        type_name: &str,
        instrument_id: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .delete_data_range(type_name, instrument_id, start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to delete data range: {e}")))
    }

    /// Query files in the catalog matching the specified criteria.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name to query
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns a list of file paths matching the criteria.
    #[pyo3(signature = (data_cls, instrument_ids=None, start=None, end=None))]
    pub fn query_files(
        &self,
        data_cls: &str,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Vec<String>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_files(data_cls, instrument_ids, start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to query files list: {e}")))
    }

    /// Get missing time intervals for a data request.
    ///
    /// # Parameters
    ///
    /// - `start`: Start timestamp (nanoseconds since Unix epoch)
    /// - `end`: End timestamp (nanoseconds since Unix epoch)
    /// - `data_cls`: The data class name
    /// - `instrument_id`: Optional instrument ID filter
    ///
    /// # Returns
    ///
    /// Returns a list of (start, end) timestamp tuples representing missing intervals.
    #[pyo3(signature = (start, end, data_cls, instrument_id=None))]
    pub fn get_missing_intervals_for_request(
        &self,
        start: u64,
        end: u64,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Vec<(u64, u64)>> {
        self.inner
            .get_missing_intervals_for_request(start, end, data_cls, instrument_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to get missing intervals: {e}")))
    }

    /// Query the last timestamp for a specific data class and instrument.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name
    /// - `instrument_id`: Optional instrument ID filter
    ///
    /// # Returns
    ///
    /// Returns the last timestamp as nanoseconds since Unix epoch, or None if no data exists.
    #[pyo3(signature = (data_cls, instrument_id=None))]
    pub fn query_last_timestamp(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Option<u64>> {
        self.inner
            .query_last_timestamp(data_cls, instrument_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to query last timestamp: {e}")))
    }

    /// Get time intervals covered by data for a specific data class and instrument.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name
    /// - `instrument_id`: Optional instrument ID filter
    ///
    /// # Returns
    ///
    /// Returns a list of (start, end) timestamp tuples representing covered intervals.
    #[pyo3(signature = (data_cls, instrument_id=None))]
    pub fn get_intervals(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Vec<(u64, u64)>> {
        self.inner
            .get_intervals(data_cls, instrument_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to get intervals: {e}")))
    }

    /// Query quote tick data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `QuoteTick` objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_quote_ticks(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<QuoteTick>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<QuoteTick>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query trade tick data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `TradeTick` objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_trade_ticks(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<TradeTick>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<TradeTick>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query order book delta data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `OrderBookDelta` objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_order_book_deltas(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<OrderBookDelta>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query bar data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of Bar objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_bars(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<Bar>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<Bar>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query order book depth data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `OrderBookDepth10` objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_order_book_depths(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<OrderBookDepth10>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<OrderBookDepth10>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query mark price update data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `MarkPriceUpdate` objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_mark_price_updates(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<MarkPriceUpdate>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<MarkPriceUpdate>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query index price update data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `IndexPriceUpdate` objects matching the query criteria.
    #[pyo3(signature = (instrument_ids=None, start=None, end=None, where_clause=None))]
    pub fn query_index_price_updates(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<String>,
    ) -> PyResult<Vec<IndexPriceUpdate>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        // Use the backend catalog's generic query_typed_data function
        self.inner
            .query_typed_data::<IndexPriceUpdate>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause.as_deref(),
                None,
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }
}
