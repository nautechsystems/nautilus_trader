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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_quote_ticks(
        &self,
        data: Vec<QuoteTick>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_trade_ticks(
        &self,
        data: Vec<TradeTick>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_order_book_deltas(
        &self,
        data: Vec<OrderBookDelta>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_bars(
        &self,
        data: Vec<Bar>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_order_book_depths(
        &self,
        data: Vec<OrderBookDepth10>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_mark_price_updates(
        &self,
        data: Vec<MarkPriceUpdate>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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
    #[pyo3(signature = (data, start=None, end=None))]
    pub fn write_index_price_updates(
        &self,
        data: Vec<IndexPriceUpdate>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<String> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_to_parquet(data, start_nanos, end_nanos)
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

    /// Reset all catalog file names to their canonical form.
    pub fn reset_catalog_file_names(&self) -> PyResult<()> {
        self.inner
            .reset_catalog_file_names()
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
}
