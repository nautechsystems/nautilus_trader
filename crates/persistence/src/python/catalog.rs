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

use std::path::PathBuf;

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
    /// Create a new ParquetCatalog with the given base path and optional batch size.
    #[new]
    #[pyo3(signature = (base_path, batch_size=None, compression=None, max_row_group_size=None))]
    #[must_use]
    pub fn new(
        base_path: String,
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
            inner: ParquetDataCatalog::new(
                PathBuf::from(base_path),
                batch_size,
                compression,
                max_row_group_size,
            ),
        }
    }

    // TODO: Cannot pass mixed data across pyo3 as a single type
    // pub fn write_data(mut slf: PyRefMut<'_, Self>, data_type: NautilusDataType, data: Vec<Data>) {}

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

    #[pyo3(signature = (data_cls, instrument_id=None, start=None, end=None))]
    pub fn extend_file_name(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from).unwrap_or_default();
        let end_nanos = end.map(UnixNanos::from).unwrap_or_default();

        self.inner
            .extend_file_name(data_cls, instrument_id, start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to extend file name: {e}")))
    }

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

    #[pyo3(signature = ())]
    pub fn reset_catalog_file_names(&self) -> PyResult<()> {
        self.inner
            .reset_catalog_file_names()
            .map_err(|e| PyIOError::new_err(format!("Failed to reset catalog file names: {e}")))
    }

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
            .map(|paths| {
                paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect()
            })
            .map_err(|e| PyIOError::new_err(format!("Failed to query files list: {e}")))
    }

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
