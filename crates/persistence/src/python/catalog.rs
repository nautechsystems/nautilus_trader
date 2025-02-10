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
use pyo3::prelude::*;

use crate::backend::catalog::ParquetDataCatalog;

/// A catalog for writing data to Parquet files.
#[pyclass(name = "ParquetCatalogV2")]
pub struct PyParquetDataCatalogV2 {
    inner: ParquetDataCatalog,
}

#[pymethods]
impl PyParquetDataCatalogV2 {
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

    pub fn write_quote_ticks(&self, data: Vec<QuoteTick>) {
        let _ = self.inner.write_to_parquet(data, None, None, None);
    }

    pub fn write_trade_ticks(&self, data: Vec<TradeTick>) {
        let _ = self.inner.write_to_parquet(data, None, None, None);
    }

    pub fn write_order_book_deltas(&self, data: Vec<OrderBookDelta>) {
        let _ = self.inner.write_to_parquet(data, None, None, None);
    }

    pub fn write_bars(&self, data: Vec<Bar>) {
        let _ = self.inner.write_to_parquet(data, None, None, None);
    }

    pub fn write_order_book_depths(&self, data: Vec<OrderBookDepth10>) {
        let _ = self.inner.write_to_parquet(data, None, None, None);
    }
}
