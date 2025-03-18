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

use nautilus_model::data::{Bar, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick};
use nautilus_serialization::enums::ParquetWriteMode;
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
    #[pyo3(signature = (base_path, batch_size=None))]
    #[must_use]
    pub fn new(base_path: String, batch_size: Option<usize>) -> Self {
        Self {
            inner: ParquetDataCatalog::new(PathBuf::from(base_path), batch_size),
        }
    }

    // TODO: Cannot pass mixed data across pyo3 as a single type
    // pub fn write_data(mut slf: PyRefMut<'_, Self>, data_type: NautilusDataType, data: Vec<Data>) {}

    #[pyo3(signature = (data, write_mode=None))]
    pub fn write_quote_ticks(
        &self,
        data: Vec<QuoteTick>,
        write_mode: Option<ParquetWriteMode>,
    ) -> PyResult<String> {
        self.inner
            .write_to_parquet(data, None, None, None, write_mode)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write quote ticks: {e}")))
    }

    #[pyo3(signature = (data, write_mode=None))]
    pub fn write_trade_ticks(
        &self,
        data: Vec<TradeTick>,
        write_mode: Option<ParquetWriteMode>,
    ) -> PyResult<String> {
        self.inner
            .write_to_parquet(data, None, None, None, write_mode)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write trade ticks: {e}")))
    }

    #[pyo3(signature = (data, write_mode=None))]
    pub fn write_order_book_deltas(
        &self,
        data: Vec<OrderBookDelta>,
        write_mode: Option<ParquetWriteMode>,
    ) -> PyResult<String> {
        self.inner
            .write_to_parquet(data, None, None, None, write_mode)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write order book deltas: {e}")))
    }

    #[pyo3(signature = (data, write_mode=None))]
    pub fn write_bars(
        &self,
        data: Vec<Bar>,
        write_mode: Option<ParquetWriteMode>,
    ) -> PyResult<String> {
        self.inner
            .write_to_parquet(data, None, None, None, write_mode)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write bars: {e}")))
    }

    #[pyo3(signature = (data, write_mode=None))]
    pub fn write_order_book_depths(
        &self,
        data: Vec<OrderBookDepth10>,
        write_mode: Option<ParquetWriteMode>,
    ) -> PyResult<String> {
        self.inner
            .write_to_parquet(data, None, None, None, write_mode)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write order book depths: {e}")))
    }

    #[pyo3(signature = ())]
    pub fn consolidate_catalog(&self) -> PyResult<()> {
        self.inner
            .consolidate_catalog()
            .map_err(|e| PyIOError::new_err(format!("Failed to consolidate catalog: {e}")))
    }

    #[pyo3(signature = (type_name, instrument_id=None))]
    pub fn consolidate_data(&self, type_name: &str, instrument_id: Option<String>) -> PyResult<()> {
        self.inner
            .consolidate_data(type_name, instrument_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to consolidate data: {e}")))
    }

    #[pyo3(signature = (data_cls, instrument_id=None, is_last=true))]
    pub fn query_timestamp_bound(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
        is_last: Option<bool>,
    ) -> PyResult<Option<i64>> {
        self.inner
            .query_timestamp_bound(data_cls, instrument_id, is_last)
            .map_err(|e| PyIOError::new_err(format!("Failed to compute timestamp bound: {e}")))
    }

    #[pyo3(signature = (type_name, instrument_id=None))]
    pub fn query_parquet_files(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Vec<String>> {
        self.inner
            .query_parquet_files(type_name, instrument_id)
            .map(|paths| {
                paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect()
            })
            .map_err(|e| PyIOError::new_err(format!("Failed to query parquet files: {e}")))
    }
}
