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

use std::collections::HashMap;

use nautilus_core::{UnixNanos, python::to_pytype_err};
use nautilus_model::{
    data::{
        Bar, Data, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, OrderBookDelta,
        OrderBookDepth10, QuoteTick, TradeTick, close::InstrumentClose,
    },
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use pyo3::{exceptions::PyIOError, prelude::*, types::PyList};

use crate::backend::catalog::ParquetDataCatalog;

/// Converts a single `Data` variant into a Python object for returning from catalog methods.
fn data_to_pyobject(py: Python<'_>, item: Data) -> PyResult<Py<PyAny>> {
    match item {
        Data::Quote(quote) => Py::new(py, quote).map(|x| x.into_any()),
        Data::Trade(trade) => Py::new(py, trade).map(|x| x.into_any()),
        Data::Bar(bar) => Py::new(py, bar).map(|x| x.into_any()),
        Data::Delta(delta) => Py::new(py, delta).map(|x| x.into_any()),
        Data::Deltas(deltas) => Py::new(py, (*deltas).clone()).map(|x| x.into_any()),
        Data::Depth10(depth) => Py::new(py, *depth).map(|x| x.into_any()),
        Data::IndexPriceUpdate(price) => Py::new(py, price).map(|x| x.into_any()),
        Data::MarkPriceUpdate(price) => Py::new(py, price).map(|x| x.into_any()),
        Data::InstrumentStatus(status) => Py::new(py, status).map(|x| x.into_any()),
        Data::InstrumentClose(close) => Py::new(py, close).map(|x| x.into_any()),
        Data::Custom(custom) => Py::new(py, custom).map(|x| x.into_any()),
    }
}

/// A catalog for writing data to Parquet files.
#[pyclass(
    name = "ParquetDataCatalog",
    module = "nautilus_trader.core.nautilus_pyo3.persistence"
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyParquetDataCatalog {
    inner: ParquetDataCatalog,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyParquetDataCatalog {
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
        base_path: &str,
        storage_options: Option<HashMap<String, String>>,
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
                let level = parquet::basic::GzipLevel::default();
                parquet::basic::Compression::GZIP(level)
            }
            3 => parquet::basic::Compression::LZO,
            4 => {
                let level = parquet::basic::BrotliLevel::default();
                parquet::basic::Compression::BROTLI(level)
            }
            5 => parquet::basic::Compression::LZ4,
            6 => {
                let level = parquet::basic::ZstdLevel::default();
                parquet::basic::Compression::ZSTD(level)
            }
            _ => parquet::basic::Compression::SNAPPY,
        });

        // Convert HashMap to AHashMap for internal use
        let storage_options = storage_options.map(|m| m.into_iter().collect());

        Self {
            inner: ParquetDataCatalog::from_uri(
                base_path,
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

    /// Write instruments to Parquet files in the catalog.
    ///
    /// Instruments are stored under `data/instruments/{instrument_id}/` using timestamp-ranged
    /// parquet file names, allowing multiple historical versions of the same instrument to be
    /// written across separate calls.
    ///
    /// # Parameters
    ///
    /// - `data`: A Python list of instrument objects (e.g. CurrencyPair, Equity).
    ///
    /// # Returns
    ///
    /// Returns a list of written file paths.
    #[pyo3(signature = (data))]
    pub fn write_instruments(&self, data: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
        let py = data.py();
        let list = data.cast::<PyList>()?;
        let mut instruments = Vec::with_capacity(list.len());
        for item in list.iter() {
            let py_item: Py<PyAny> = item.unbind();
            let instrument = pyobject_to_instrument_any(py, py_item)?;
            instruments.push(instrument);
        }
        self.inner
            .write_instruments(instruments)
            .map(|paths| {
                paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect()
            })
            .map_err(|e| PyIOError::new_err(format!("Failed to write instruments: {e}")))
    }

    /// Query instruments from the catalog.
    ///
    /// # Parameters
    ///
    /// - `instrument_ids`: Optional list of instrument IDs to filter by. If `None`, returns all instruments.
    /// - `start`: Optional inclusive lower bound for `ts_init` filtering.
    /// - `end`: Optional inclusive upper bound for `ts_init` filtering.
    ///
    /// # Returns
    ///
    /// Returns a list of instrument objects (e.g. CurrencyPair, Equity).
    #[pyo3(signature = (instrument_ids=None, start=None, end=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn instruments(
        &self,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let rust_instruments = self
            .inner
            .query_instruments_filtered(
                instrument_ids.as_deref(),
                start.map(UnixNanos::from),
                end.map(UnixNanos::from),
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query instruments: {e}")))?;
        Python::attach(|py| {
            rust_instruments
                .into_iter()
                .map(|inst| instrument_any_to_pyobject(py, inst))
                .collect()
        })
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
    #[expect(clippy::needless_pass_by_value)]
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
            .extend_file_name(data_cls, instrument_id.as_deref(), start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to extend file name: {e}")))
    }

    /// Consolidate all data files in the catalog within the specified time range.
    ///
    /// # Parameters
    ///
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to ensure files are contiguous
    /// - `deduplicate`: Optional flag to deduplicate rows when combining files
    #[pyo3(signature = (start=None, end=None, ensure_contiguous_files=None, deduplicate=None))]
    pub fn consolidate_catalog(
        &self,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .consolidate_catalog(start_nanos, end_nanos, ensure_contiguous_files, deduplicate)
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
    /// - `deduplicate`: Optional flag to deduplicate rows when combining files
    #[pyo3(signature = (type_name, instrument_id=None, start=None, end=None, ensure_contiguous_files=None, deduplicate=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn consolidate_data(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> PyResult<()> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .consolidate_data(
                type_name,
                instrument_id.as_deref(),
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
                deduplicate,
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
    #[expect(clippy::needless_pass_by_value)]
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
                identifier.as_deref(),
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
    #[expect(clippy::needless_pass_by_value)]
    pub fn reset_data_file_names(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<()> {
        self.inner
            .reset_data_file_names(data_cls, instrument_id.as_deref())
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
    #[expect(clippy::needless_pass_by_value)]
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
            .delete_data_range(type_name, instrument_id.as_deref(), start_nanos, end_nanos)
            .map_err(|e| PyIOError::new_err(format!("Failed to delete data range: {e}")))
    }

    /// Write custom data to Parquet files.
    ///
    /// Requires `CustomData` wrappers. Callers must wrap raw custom objects in
    /// `CustomData(data_type=DataType(cls, metadata=...), data=...)` before writing.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_custom_data(
        &self,
        _py: Python<'_>,
        data: Vec<Bound<'_, PyAny>>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        use nautilus_model::data::CustomData;

        let mut custom_items: Vec<CustomData> = Vec::with_capacity(data.len());
        for obj in data {
            let custom = obj.extract::<CustomData>().map_err(|_| {
                to_pytype_err(
                    "write_custom_data requires CustomData wrappers; wrap with CustomData(data_type=DataType(cls, metadata=...), data=...)",
                )
            })?;
            custom_items.push(custom);
        }

        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .write_custom_data_batch(
                custom_items,
                start_nanos,
                end_nanos,
                Some(skip_disjoint_check),
            )
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyIOError::new_err(format!("Failed to write custom data: {e}")))
    }

    /// List all instrument IDs available in the catalog for a given data type.
    pub fn list_instruments(&self, data_type: &str) -> PyResult<Vec<String>> {
        self.inner
            .list_instruments(data_type)
            .map_err(|e| PyIOError::new_err(format!("Failed to list instruments: {e}")))
    }

    /// List all Parquet files in the catalog for a given data type and instrument.
    pub fn list_parquet_files(
        &self,
        data_type: &str,
        instrument_id: &str,
    ) -> PyResult<Vec<String>> {
        let directory = format!("data/{data_type}/{instrument_id}");
        self.inner
            .list_parquet_files(&directory)
            .map_err(|e| PyIOError::new_err(format!("Failed to list parquet files: {e}")))
    }

    /// Query files in the catalog matching the specified criteria.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name to query
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings
    ///   (e.g., "EUR/USD.SIM") or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns a list of file paths matching the criteria.
    #[pyo3(signature = (data_cls, identifiers=None, start=None, end=None))]
    pub fn query_files(
        &self,
        data_cls: &str,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Vec<String>> {
        // Convert u64 timestamps to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_files(data_cls, identifiers, start_nanos, end_nanos)
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
    #[expect(clippy::needless_pass_by_value)]
    pub fn get_missing_intervals_for_request(
        &self,
        start: u64,
        end: u64,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Vec<(u64, u64)>> {
        self.inner
            .get_missing_intervals_for_request(start, end, data_cls, instrument_id.as_deref())
            .map_err(|e| PyIOError::new_err(format!("Failed to get missing intervals: {e}")))
    }

    /// Query the first timestamp for a specific data class and instrument.
    ///
    /// # Parameters
    ///
    /// - `data_cls`: The data class name
    /// - `instrument_id`: Optional instrument ID filter
    ///
    /// # Returns
    ///
    /// Returns the first timestamp as nanoseconds since Unix epoch, or None if no data exists.
    #[pyo3(signature = (data_cls, instrument_id=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_first_timestamp(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Option<u64>> {
        self.inner
            .query_first_timestamp(data_cls, instrument_id.as_deref())
            .map_err(|e| PyIOError::new_err(format!("Failed to query first timestamp: {e}")))
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
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_last_timestamp(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Option<u64>> {
        self.inner
            .query_last_timestamp(data_cls, instrument_id.as_deref())
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
    #[expect(clippy::needless_pass_by_value)]
    pub fn get_intervals(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> PyResult<Vec<(u64, u64)>> {
        self.inner
            .get_intervals(data_cls, instrument_id.as_deref())
            .map_err(|e| PyIOError::new_err(format!("Failed to get intervals: {e}")))
    }

    /// Query Parquet files for data matching the given criteria.
    #[pyo3(signature = (data_type, identifiers=None, start=None, end=None, where_clause=None, files=None, optimize_file_loading=true))]
    #[expect(clippy::too_many_arguments)]
    pub fn query(
        &mut self,
        py: Python<'_>,
        data_type: &str,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let data = match data_type {
            "quotes" => {
                let ticks = self
                    .inner
                    .query_typed_data::<QuoteTick>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                ticks.into_iter().map(Data::from).collect()
            }
            "trades" => {
                let ticks = self
                    .inner
                    .query_typed_data::<TradeTick>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                ticks.into_iter().map(Data::from).collect()
            }
            "bars" => {
                let bars = self
                    .inner
                    .query_typed_data::<Bar>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                bars.into_iter().map(Data::from).collect()
            }
            "order_book_deltas" => {
                let deltas = self
                    .inner
                    .query_typed_data::<OrderBookDelta>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                deltas.into_iter().map(Data::from).collect()
            }
            "order_book_depths" => {
                let depths = self
                    .inner
                    .query_typed_data::<OrderBookDepth10>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                depths.into_iter().map(Data::from).collect()
            }
            "index_prices" => {
                let prices = self
                    .inner
                    .query_typed_data::<IndexPriceUpdate>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                prices.into_iter().map(Data::from).collect()
            }
            "mark_prices" => {
                let prices = self
                    .inner
                    .query_typed_data::<MarkPriceUpdate>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                prices.into_iter().map(Data::from).collect()
            }
            "instrument_status" => {
                let statuses = self
                    .inner
                    .query_typed_data::<InstrumentStatus>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                statuses.into_iter().map(Data::from).collect()
            }
            "instrument_closes" => {
                let closes = self
                    .inner
                    .query_typed_data::<InstrumentClose>(
                        identifiers,
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files,
                        optimize_file_loading,
                    )
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;
                closes.into_iter().map(Data::from).collect()
            }
            _ => py
                .detach(|| {
                    self.inner.query_custom_data_dynamic(
                        data_type,
                        identifiers.as_deref(),
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files.clone(),
                        optimize_file_loading,
                    )
                })
                .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?,
        };

        let mut python_objects = Vec::new();
        for item in data {
            python_objects.push(data_to_pyobject(py, item)?);
        }
        Ok(python_objects)
    }

    /// Query quote tick data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings
    ///   (e.g., "EUR/USD.SIM") or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `QuoteTick` objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_quote_ticks(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<QuoteTick>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<QuoteTick>(
                identifiers,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query trade tick data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings
    ///   (e.g., "EUR/USD.SIM") or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `TradeTick` objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_trade_ticks(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<TradeTick>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<TradeTick>(
                identifiers,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query order book delta data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings
    ///   (e.g., "EUR/USD.SIM") or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `OrderBookDelta` objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_order_book_deltas(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<OrderBookDelta>(
                identifiers,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// Query bar data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be instrument_id strings
    ///   (e.g., "EUR/USD.SIM") or bar_type strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported (e.g., "EUR/USD.SIM" will match all bar types for that instrument).
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of Bar objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_bars(
        &mut self,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<Bar>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<Bar>(
                identifiers,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
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
        where_clause: Option<&str>,
    ) -> PyResult<Vec<OrderBookDepth10>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<OrderBookDepth10>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
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
        where_clause: Option<&str>,
    ) -> PyResult<Vec<MarkPriceUpdate>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<MarkPriceUpdate>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
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
        where_clause: Option<&str>,
    ) -> PyResult<Vec<IndexPriceUpdate>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.inner
            .query_typed_data::<IndexPriceUpdate>(
                instrument_ids,
                start_nanos,
                end_nanos,
                where_clause,
                None,
                true, // optimize_file_loading=true for directory-based registration (default)
            )
            .map_err(|e| PyIOError::new_err(format!("Failed to query data: {e}")))
    }

    /// List all data types available in the catalog.
    ///
    /// # Returns
    ///
    /// Returns a list of data type names (as directory stems) in the catalog.
    pub fn list_data_types(&self) -> PyResult<Vec<String>> {
        self.inner
            .list_data_types()
            .map_err(|e| PyIOError::new_err(format!("Failed to list data types: {e}")))
    }

    /// List all live run IDs available in the catalog.
    ///
    /// # Returns
    ///
    /// Returns a list of live run IDs (as directory stems) in the catalog.
    pub fn list_live_runs(&self) -> PyResult<Vec<String>> {
        self.inner
            .list_live_runs()
            .map_err(|e| PyIOError::new_err(format!("Failed to list live runs: {e}")))
    }

    /// List all backtest run IDs available in the catalog.
    ///
    /// # Returns
    ///
    /// Returns a list of backtest run IDs (as directory stems) in the catalog.
    pub fn list_backtest_runs(&self) -> PyResult<Vec<String>> {
        self.inner
            .list_backtest_runs()
            .map_err(|e| PyIOError::new_err(format!("Failed to list backtest runs: {e}")))
    }

    /// List all backtest run instances available in the catalog.
    pub fn list_backtests(&self) -> PyResult<Vec<String>> {
        self.inner
            .list_backtest_runs()
            .map_err(|e| PyIOError::new_err(format!("Failed to list backtests: {e}")))
    }

    /// Read data from a live run instance.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the live run instance
    ///
    /// # Returns
    ///
    /// Returns a list of data objects from the live run, sorted by timestamp.
    #[pyo3(signature = (instance_id))]
    pub fn read_live_run(&self, py: Python<'_>, instance_id: &str) -> PyResult<Vec<Py<PyAny>>> {
        let data = self
            .inner
            .read_live_run(instance_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to read live run: {e}")))?;

        let mut python_objects = Vec::new();
        for item in data {
            python_objects.push(data_to_pyobject(py, item)?);
        }
        Ok(python_objects)
    }

    /// Read data from a backtest run instance.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the backtest run instance
    ///
    /// # Returns
    ///
    /// Returns a list of data objects from the backtest run, sorted by timestamp.
    #[pyo3(signature = (instance_id))]
    pub fn read_backtest(&self, py: Python<'_>, instance_id: &str) -> PyResult<Vec<Py<PyAny>>> {
        let data = self
            .inner
            .read_backtest(instance_id)
            .map_err(|e| PyIOError::new_err(format!("Failed to read backtest: {e}")))?;

        let mut python_objects = Vec::new();
        for item in data {
            python_objects.push(data_to_pyobject(py, item)?);
        }
        Ok(python_objects)
    }

    /// Convert stream data from feather files to parquet files.
    ///
    /// This method reads data from feather files generated during a backtest or live run
    /// and writes it to the catalog in parquet format. It's useful for converting temporary
    /// stream data into a more permanent and queryable format.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the backtest or live run instance
    /// - `data_cls`: The data class name (e.g., "quotes", "trades", "bars")
    /// - `subdirectory`: Optional subdirectory containing the feather files. Either "backtest" or "live" (default: "backtest")
    /// - `identifiers`: Optional list of identifiers to filter by (instrument IDs or bar types)
    /// - `use_ts_event_for_ts_init`: If true, replaces the `ts_init` column with `ts_event` column values before deserializing
    ///
    /// # Returns
    ///
    /// Returns nothing on success.
    ///
    /// # Examples
    ///
    /// ```python
    /// # Convert backtest stream data to parquet
    /// catalog.convert_stream_to_data(
    ///     "instance-123",
    ///     "quotes",
    ///     subdirectory="backtest"
    /// )
    ///
    /// # Convert live run data with identifier filtering
    /// catalog.convert_stream_to_data(
    ///     "instance-456",
    ///     "trades",
    ///     subdirectory="live",
    ///     identifiers=["EUR/USD.SIM"]
    /// )
    /// ```
    #[pyo3(signature = (instance_id, data_cls, subdirectory=None, identifiers=None, use_ts_event_for_ts_init=false))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn convert_stream_to_data(
        &mut self,
        instance_id: &str,
        data_cls: &str,
        subdirectory: Option<&str>,
        identifiers: Option<Vec<String>>,
        use_ts_event_for_ts_init: bool,
    ) -> PyResult<()> {
        let subdir = subdirectory.unwrap_or("backtest");

        match self.inner.convert_stream_to_data(
            instance_id,
            data_cls,
            Some(subdir),
            identifiers.as_deref(),
            use_ts_event_for_ts_init,
        ) {
            Ok(()) => Ok(()),
            Err(e) => Err(PyIOError::new_err(format!(
                "Failed to convert stream to data: {e}"
            ))),
        }
    }

    /// Query custom data from Parquet files.
    #[pyo3(signature = (type_name, identifiers=None, start=None, end=None, where_clause=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_custom_data(
        &mut self,
        py: Python<'_>,
        type_name: &str,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let data = py
            .detach(|| {
                self.inner.query_custom_data_dynamic(
                    type_name,
                    identifiers.as_deref(),
                    start_nanos,
                    end_nanos,
                    where_clause,
                    None,
                    true,
                )
            })
            .map_err(|e| PyIOError::new_err(format!("Failed to query custom data: {e}")))?;

        let mut python_objects = Vec::new();

        for item in data {
            let py_obj: Py<PyAny> = match item {
                Data::Custom(custom) => Py::new(py, custom.clone())?.into_any(),
                _ => return Err(PyIOError::new_err("Expected custom data")),
            };
            python_objects.push(py_obj);
        }
        Ok(python_objects)
    }
}
