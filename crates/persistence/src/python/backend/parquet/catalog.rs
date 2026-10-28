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

#![expect(
    clippy::too_many_arguments,
    reason = "PyO3 catalog wrapper mirrors Python API dispatch surface"
)]

use std::{collections::HashMap, sync::Mutex};

use nautilus_common::{
    config::ConfigError, enums::Environment, python::config_error_to_pyvalue_err,
};
use nautilus_core::{
    UnixNanos,
    python::{to_pytype_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{
        Bar, Data, FundingRateUpdate, HasTsInit, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick,
        TradeTick, close::InstrumentClose,
    },
    python::{
        data::data_to_pyobject,
        instruments::{
            PyNautilusInstrumentType, instrument_any_to_pyobject, pyobject_to_instrument_any,
        },
    },
};
use nautilus_serialization::{
    arrow::{
        DecodeTypedFromRecordBatch, EncodeToRecordBatch, display::instrument::encode_instruments,
    },
    python::arrow::{arrow_record_batches_to_pyarrow_stream, arrow_record_batches_to_pybytes},
};
use pyo3::{
    exceptions::PyIOError,
    prelude::*,
    types::{PyBytes, PyDict, PyList},
};

use crate::{
    backend::{migration::build_catalog_migration_plan, parquet::catalog::ParquetDataCatalog},
    catalog::{
        traits::{CatalogQuery, CatalogReader, CatalogRecordQuery, CatalogWriter},
        types::{CatalogDataType, HasCatalogDataType, parquet_catalog_data_type_path_prefixes},
    },
    config::CatalogCompression,
    python::backend::{
        PyCatalogDataType, arrow_ipc_batches, arrow_ipc_data_schema, arrow_ipc_record_schema,
        arrow_record_batches_from_pybytes, catalog_metadata_to_pydict, catalog_record_type_from_py,
        nautilus_data_type_from_py, to_pyio_err, write_record_params_from_py,
    },
};

fn write_parquet_data<T>(
    py: Python<'_>,
    catalog: &PyParquetDataCatalog,
    data: Vec<T>,
    start: Option<u64>,
    end: Option<u64>,
    skip_disjoint_check: bool,
    label: &str,
) -> PyResult<String>
where
    T: HasTsInit + EncodeToRecordBatch + HasCatalogDataType + Send,
{
    catalog
        .mutate(py, move || {
            catalog.inner.write_to_parquet(
                &data,
                start.map(UnixNanos::from),
                end.map(UnixNanos::from),
                Some(skip_disjoint_check),
            )
        })
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|e| PyIOError::new_err(format!("Failed to write {label}: {e}")))
}

fn query_parquet_data<T>(
    py: Python<'_>,
    catalog: &mut ParquetDataCatalog,
    identifiers: Option<Vec<String>>,
    start: Option<u64>,
    end: Option<u64>,
    where_clause: Option<&str>,
    files: Option<Vec<String>>,
    optimize_file_loading: bool,
    error_context: &str,
) -> PyResult<Vec<T>>
where
    T: DecodeTypedFromRecordBatch + HasCatalogDataType + HasTsInit + Send,
{
    py.detach(move || {
        catalog.query::<T>(
            identifiers,
            start.map(UnixNanos::from),
            end.map(UnixNanos::from),
            where_clause,
            files,
            optimize_file_loading,
        )
    })
    .map_err(|e| PyIOError::new_err(format!("{error_context}: {e}")))
}

fn reject_parquet_as_of(as_of: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
    if as_of.is_some() {
        return Err(to_pyvalue_err("ParquetDataCatalog does not support as_of"));
    }

    Ok(())
}

/// A catalog for writing data to Parquet files.
#[pyclass(name = "ParquetDataCatalog", module = "nautilus_trader.persistence")]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyParquetDataCatalog {
    inner: ParquetDataCatalog,
    mutation_lock: Mutex<()>,
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
    /// - `batch_size`: Optional batch size for processing (default: 10,000)
    /// - `compression`: Optional compression code (0=UNCOMPRESSED, 1=SNAPPY, 2=GZIP, 4=BROTLI, 5=`LZ4_RAW`, 6=ZSTD)
    /// - `max_row_group_size`: Optional maximum row group size (default: 131,072)
    ///
    /// # Errors
    ///
    /// Returns an error if `compression` is not one of the listed codes, `batch_size` or
    /// `max_row_group_size` is zero, or the underlying [`ParquetDataCatalog`] cannot be created.
    #[new]
    #[pyo3(signature = (base_path, storage_options=None, batch_size=None, compression=None, max_row_group_size=None))]
    pub fn py_new(
        base_path: &str,
        storage_options: Option<HashMap<String, String>>,
        batch_size: Option<usize>,
        compression: Option<u8>,
        max_row_group_size: Option<usize>,
    ) -> PyResult<Self> {
        let compression = compression
            .map(catalog_compression_from_code)
            .transpose()?
            .map(parquet::basic::Compression::from);

        // Convert HashMap to AHashMap for internal use
        let storage_options = storage_options.map(|m| m.into_iter().collect());

        let inner = ParquetDataCatalog::from_uri(
            base_path,
            storage_options,
            batch_size,
            compression,
            max_row_group_size,
        )
        .map_err(|e| match e.downcast::<ConfigError>() {
            Ok(e) => config_error_to_pyvalue_err(e),
            Err(e) => PyIOError::new_err(format!("Failed to create ParquetDataCatalog: {e}")),
        })?;

        Ok(Self {
            inner,
            mutation_lock: Mutex::new(()),
        })
    }

    /// Rewrites a legacy Parquet catalog into this current Parquet catalog.
    #[pyo3(signature = (parquet_path, storage_options=None, dry_run=false))]
    pub fn migrate_from_legacy_parquet_path(
        mut slf: PyRefMut<'_, Self>,
        parquet_path: &str,
        storage_options: Option<HashMap<String, String>>,
        dry_run: bool,
    ) -> PyResult<usize> {
        let storage_options = storage_options.map(|m| m.into_iter().collect());
        let source = ParquetDataCatalog::from_uri(parquet_path, storage_options, None, None, None)
            .map_err(to_pyio_err)?;
        let py = slf.py();
        if dry_run {
            return py
                .detach(|| {
                    let plan = build_catalog_migration_plan(&source)?;
                    plan.ensure_ready()?;
                    Ok::<usize, anyhow::Error>(0)
                })
                .map_err(to_pyio_err);
        }

        let inner = &mut slf.inner;
        py.detach(|| {
            inner
                .migrate_from_legacy_parquet_catalog(&source)
                .map(|report| report.migrated_rows)
        })
        .map_err(to_pyio_err)
    }

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
        py: Python<'_>,
        data: Vec<QuoteTick>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "quote ticks",
        )
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
        py: Python<'_>,
        data: Vec<TradeTick>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "trade ticks",
        )
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
        py: Python<'_>,
        data: Vec<OrderBookDelta>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "order book deltas",
        )
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
        py: Python<'_>,
        data: Vec<Bar>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(py, self, data, start, end, skip_disjoint_check, "bars")
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
        py: Python<'_>,
        data: Vec<OrderBookDepth>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "order book depths",
        )
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
        py: Python<'_>,
        data: Vec<MarkPriceUpdate>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "mark price updates",
        )
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
        py: Python<'_>,
        data: Vec<IndexPriceUpdate>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "index price updates",
        )
    }

    /// Write option greeks data to Parquet files.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of option greeks to write
    /// - `start`: Optional start timestamp override (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp override (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns the path of the created file as a string.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_option_greeks(
        &self,
        py: Python<'_>,
        data: Vec<OptionGreeks>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "option greeks",
        )
    }

    /// Write instrument status data to Parquet files.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_instrument_statuses(
        &self,
        py: Python<'_>,
        data: Vec<InstrumentStatus>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "instrument statuses",
        )
    }

    /// Write instrument close data to Parquet files.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_instrument_closes(
        &self,
        py: Python<'_>,
        data: Vec<InstrumentClose>,
        start: Option<u64>,
        end: Option<u64>,
        skip_disjoint_check: bool,
    ) -> PyResult<String> {
        write_parquet_data(
            py,
            self,
            data,
            start,
            end,
            skip_disjoint_check,
            "instrument closes",
        )
    }

    /// Write instruments to Parquet files in the catalog.
    ///
    /// Instruments are stored under `data/instruments/{instrument_id}/` using timestamp-ranged
    /// parquet file names, allowing multiple historical versions of the same instrument to be
    /// written across separate calls.
    ///
    /// # Parameters
    ///
    /// - `instruments`: A Python list of instrument objects (e.g. `CurrencyPair`, Equity).
    ///
    /// # Returns
    ///
    /// Returns a list of written file paths.
    pub fn write_instruments(&self, instruments: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
        let data = instruments;
        let py = data.py();
        let list = data.cast::<PyList>()?;
        let mut instruments = Vec::with_capacity(list.len());
        for item in list.iter() {
            let py_item: Py<PyAny> = item.unbind();
            let instrument = pyobject_to_instrument_any(py, py_item)?;
            instruments.push(instrument);
        }

        self.mutate(py, || self.inner.write_instruments(instruments))
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
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering.
    ///
    /// # Returns
    ///
    /// Returns a list of instrument objects (e.g. `CurrencyPair`, Equity).
    #[pyo3(signature = (
        instrument_ids=None,
        start=None,
        end=None,
        where_clause=None,
        instrument_type=None,
    ))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn instruments(
        &mut self,
        py: Python<'_>,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        instrument_type: Option<PyRef<'_, PyNautilusInstrumentType>>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let instrument_type = instrument_type.map(|instrument_type| instrument_type.inner());

        let rust_instruments = py
            .detach(|| {
                self.inner.query_instruments_filtered_with_where_and_type(
                    instrument_ids.as_deref(),
                    start.map(UnixNanos::from),
                    end.map(UnixNanos::from),
                    where_clause,
                    instrument_type.as_ref(),
                )
            })
            .map_err(|e| PyIOError::new_err(format!("Failed to query instruments: {e}")))?;

        rust_instruments
            .into_iter()
            .map(|inst| instrument_any_to_pyobject(py, inst))
            .collect()
    }

    /// Query instruments as display-friendly Arrow IPC stream bytes.
    #[pyo3(signature = (
        instrument_ids=None,
        start=None,
        end=None,
        where_clause=None,
        instrument_type=None,
    ))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_instrument_arrow_bytes(
        &mut self,
        py: Python<'_>,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        instrument_type: Option<PyRef<'_, PyNautilusInstrumentType>>,
    ) -> PyResult<Py<PyBytes>> {
        let instrument_type = instrument_type.map(|instrument_type| instrument_type.inner());

        let instruments = py
            .detach(|| {
                self.inner.query_instruments_filtered_with_where_and_type(
                    instrument_ids.as_deref(),
                    start.map(UnixNanos::from),
                    end.map(UnixNanos::from),
                    where_clause,
                    instrument_type.as_ref(),
                )
            })
            .map_err(|e| PyIOError::new_err(format!("Failed query instruments: {e}")))?;

        let batch = encode_instruments(&instruments)
            .map_err(|e| PyIOError::new_err(format!("Failed encode instruments: {e}")))?;
        let schema = batch.schema().as_ref().clone();
        arrow_record_batches_to_pybytes(py, &schema, &[batch])
    }

    /// Query instruments as an Arrow C stream `PyCapsule`.
    #[pyo3(signature = (
        instrument_ids=None,
        start=None,
        end=None,
        where_clause=None,
        instrument_type=None,
    ))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_instrument_arrow_stream(
        &mut self,
        py: Python<'_>,
        instrument_ids: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        instrument_type: Option<PyRef<'_, PyNautilusInstrumentType>>,
    ) -> PyResult<Py<PyAny>> {
        let instrument_type = instrument_type.map(|instrument_type| instrument_type.inner());

        let instruments = py
            .detach(|| {
                self.inner.query_instruments_filtered_with_where_and_type(
                    instrument_ids.as_deref(),
                    start.map(UnixNanos::from),
                    end.map(UnixNanos::from),
                    where_clause,
                    instrument_type.as_ref(),
                )
            })
            .map_err(|e| PyIOError::new_err(format!("Failed query instruments: {e}")))?;

        let batch = encode_instruments(&instruments)
            .map_err(|e| PyIOError::new_err(format!("Failed encode instruments: {e}")))?;
        let schema = batch.schema().as_ref().clone();
        arrow_record_batches_to_pyarrow_stream(py, &schema, vec![batch])
    }

    /// Extend file names in the catalog with additional timestamp information.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    /// - `start`: Start timestamp (nanoseconds since Unix epoch)
    /// - `end`: End timestamp (nanoseconds since Unix epoch)
    #[pyo3(signature = (data_type, identifier=None, *, start, end))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn extend_file_name(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
        start: u64,
        end: u64,
    ) -> PyResult<()> {
        let data_type = data_type.into_inner();
        let start_nanos = UnixNanos::from(start);
        let end_nanos = UnixNanos::from(end);

        self.mutate(py, || {
            self.inner
                .extend_file_name(&data_type, identifier.as_deref(), start_nanos, end_nanos)
        })
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
        py: Python<'_>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> PyResult<()> {
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        self.mutate(py, || {
            self.inner.consolidate_catalog(
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
                deduplicate,
            )
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to consolidate catalog: {e}")))
    }

    /// Consolidate data files for a specific data type within the specified time range.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to ensure files are contiguous
    /// - `deduplicate`: Optional flag to deduplicate rows when combining files
    #[pyo3(signature = (data_type, identifier=None, start=None, end=None, ensure_contiguous_files=None, deduplicate=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn consolidate_data(
        &mut self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
        deduplicate: Option<bool>,
    ) -> PyResult<()> {
        let data_type = data_type.into_inner();
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        py.detach(|| {
            self.inner.consolidate_data(
                &data_type,
                identifier.as_deref(),
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
                deduplicate,
            )
        })
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
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID to consolidate. If None, consolidates all instruments
    /// - `period_nanos`: Optional period duration for consolidation in nanoseconds. Default is 1 day (86400000000000).
    ///   Examples: 3600000000000 (1 hour), 604800000000000 (7 days), 1800000000000 (30 minutes)
    /// - `start`: Optional start timestamp for consolidation range (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp for consolidation range (nanoseconds since Unix epoch)
    /// - `ensure_contiguous_files`: Optional flag to control file naming strategy
    #[pyo3(signature = (data_type, identifier=None, period_nanos=None, start=None, end=None, ensure_contiguous_files=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn consolidate_data_by_period(
        &mut self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
        period_nanos: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        ensure_contiguous_files: Option<bool>,
    ) -> PyResult<()> {
        let data_type = data_type.into_inner();
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let mut consolidate = || {
            self.inner.consolidate_data_by_period(
                &data_type,
                identifier.as_deref(),
                period_nanos,
                start_nanos,
                end_nanos,
                ensure_contiguous_files,
            )
        };

        let result = if matches!(
            &data_type,
            CatalogDataType::Data(NautilusDataType::Custom { .. })
        ) {
            consolidate()
        } else {
            py.detach(consolidate)
        };

        result.map_err(|e| PyIOError::new_err(format!("Failed to consolidate data by period: {e}")))
    }

    /// Reset all catalog file names to their canonical form.
    pub fn reset_all_file_names(&self, py: Python<'_>) -> PyResult<()> {
        self.mutate(py, || self.inner.reset_all_file_names())
            .map_err(|e| PyIOError::new_err(format!("Failed to reset catalog file names: {e}")))
    }

    /// Reset data file names for a specific data class to their canonical form.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    #[pyo3(signature = (data_type, identifier=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn reset_data_file_names(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
    ) -> PyResult<()> {
        let data_type = data_type.into_inner();
        self.mutate(py, || {
            self.inner
                .reset_data_file_names(&data_type, identifier.as_deref())
        })
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
    /// - `data_type`: The data type to delete from.
    /// - `identifier`: Optional identifier to delete data for. If None, deletes data across all identifiers
    /// - `start`: Optional start timestamp for the deletion range (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp for the deletion range (nanoseconds since Unix epoch)
    ///
    /// # Notes
    ///
    /// - This operation permanently removes data and cannot be undone
    /// - Files that partially overlap the deletion range are split to preserve data outside the range
    /// - The method ensures data integrity by using atomic operations where possible
    /// - Empty directories are not automatically removed after deletion
    #[pyo3(signature = (data_type, identifier=None, start=None, end=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn delete_data_range(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<
            '_,
            PyAny,
        >,
        identifier: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<()> {
        let data_type = nautilus_data_type_from_py(data_type)?;
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let mut delete = || {
            self.inner
                .delete_data_range(&data_type, identifier.as_deref(), start_nanos, end_nanos)
        };

        let result = if matches!(&data_type, NautilusDataType::Custom { .. }) {
            delete()
        } else {
            py.detach(delete)
        };

        result.map_err(|e| PyIOError::new_err(format!("Failed to delete data range: {e}")))
    }

    /// Writes custom data to Parquet files.
    ///
    /// Requires `CustomData` wrappers. Callers must wrap raw custom objects in
    /// `CustomData(data_type=DataType(cls, metadata=...), data=...)` before writing.
    ///
    /// The registered Arrow schema must contain `ts_init`. Any `ts_event` or `ts_init` fields
    /// must use `timestamp("ns", tz="UTC")`; incompatible schemas fail before writing.
    #[pyo3(signature = (data, start=None, end=None, skip_disjoint_check=false))]
    pub fn write_custom_data(
        &self,
        py: Python<'_>,
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

        if custom_items.is_empty() {
            return Ok(String::new());
        }

        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);
        let write_error =
            |e: anyhow::Error| PyIOError::new_err(format!("Failed to write custom data: {e}"));
        let custom_refs = custom_items.iter().collect::<Vec<_>>();
        let write = self
            .inner
            .prepare_custom_data_write(&custom_refs, start_nanos, end_nanos)
            .map_err(write_error)?;

        self.mutate(py, || {
            self.inner
                .write_custom_data_prepared(&write, Some(skip_disjoint_check))
        })
        .map(|path| path.to_string_lossy().to_string())
        .map_err(write_error)
    }

    /// List all instrument IDs available in the catalog for a given catalog type.
    pub fn list_instruments(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
    ) -> PyResult<Vec<String>> {
        let data_type = data_type.into_inner();
        py.detach(|| self.inner.list_instruments(&data_type))
            .map_err(|e| PyIOError::new_err(format!("Failed to list instruments: {e}")))
    }

    /// List all Parquet files in the catalog for a given data type and identifier.
    pub fn list_parquet_files(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: &str,
    ) -> PyResult<Vec<String>> {
        let data_type = data_type.into_inner();
        let list_files = |prefix: &str| -> PyResult<Vec<String>> {
            let directory = self
                .inner
                .make_path(prefix, Some(identifier))
                .map_err(|e| PyIOError::new_err(format!("Failed to list parquet files: {e}")))?;
            py.detach(|| self.inner.list_parquet_files(&directory))
                .map_err(|e| PyIOError::new_err(format!("Failed to list parquet files: {e}")))
        };
        let mut files = Vec::new();

        for prefix in parquet_catalog_data_type_path_prefixes(&data_type) {
            files.extend(list_files(prefix.as_ref())?);
        }

        Ok(files)
    }

    /// Query files in the catalog matching the specified criteria.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings
    ///   (e.g., "EUR/USD.SIM") or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///   For bars, partial matching is supported.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    ///
    /// # Returns
    ///
    /// Returns a list of file paths matching the criteria.
    #[pyo3(signature = (data_type, identifiers=None, start=None, end=None))]
    pub fn query_files(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Vec<String>> {
        let data_type = data_type.into_inner();
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        py.detach(|| {
            self.inner
                .query_files(&data_type, identifiers, start_nanos, end_nanos)
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to query files list: {e}")))
    }

    /// Query catalog metadata keyed by the first timestamp where each metadata is used.
    #[pyo3(signature = (data_type, identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_metadata(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<
            '_,
            PyAny,
        >,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Py<PyDict>> {
        let data_type = nautilus_data_type_from_py(data_type)?;

        let metadata = py
            .detach(|| {
                CatalogReader::query_metadata(
                    &mut self.inner,
                    &CatalogQuery::new(data_type)
                        .with_identifiers(identifiers)
                        .with_range(start.map(UnixNanos::from), end.map(UnixNanos::from))
                        .with_where_clause(where_clause.map(str::to_string)),
                )
            })
            .map_err(|e| PyIOError::new_err(format!("Metadata query failed: {e}")))?;

        catalog_metadata_to_pydict(py, metadata)
    }

    /// Query catalog data as display-friendly Arrow IPC stream bytes.
    #[pyo3(signature = (
        data_type,
        identifiers=None,
        start=None,
        end=None,
        where_clause=None,
        display=true,
        as_of=None,
    ))]
    pub fn query_data_arrow_bytes(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<
            '_,
            PyAny,
        >,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        display: bool,
        as_of: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyBytes>> {
        let data_type = nautilus_data_type_from_py(data_type)?;
        reject_parquet_as_of(as_of)?;
        let query = CatalogQuery::new(data_type.clone())
            .with_identifiers(identifiers)
            .with_range(start.map(UnixNanos::from), end.map(UnixNanos::from))
            .with_where_clause(where_clause.map(str::to_string));

        let batches = py
            .detach(|| {
                if display {
                    CatalogReader::query_display_record_batches(&mut self.inner, &query)
                } else {
                    let data = CatalogReader::query_batch(&mut self.inner, &query)?
                        .to_data_vec_for_compat();
                    crate::common::arrow::data_to_arrow_batches(&data_type, data)
                }
            })
            .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;

        let schema = arrow_ipc_data_schema(&data_type, &batches, display)?;
        let batches = arrow_ipc_batches(&schema, batches)?;
        arrow_record_batches_to_pybytes(py, &schema, &batches)
    }

    /// Query catalog data as an Arrow C stream `PyCapsule`.
    #[pyo3(signature = (
        data_type,
        identifiers=None,
        start=None,
        end=None,
        where_clause=None,
        display=true,
        as_of=None,
    ))]
    pub fn query_data_arrow_stream(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<
            '_,
            PyAny,
        >,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        display: bool,
        as_of: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let data_type = nautilus_data_type_from_py(data_type)?;
        reject_parquet_as_of(as_of)?;
        let query = CatalogQuery::new(data_type.clone())
            .with_identifiers(identifiers)
            .with_range(start.map(UnixNanos::from), end.map(UnixNanos::from))
            .with_where_clause(where_clause.map(str::to_string));

        let batches = py
            .detach(|| {
                if display {
                    CatalogReader::query_display_record_batches(&mut self.inner, &query)
                } else {
                    let data = CatalogReader::query_batch(&mut self.inner, &query)?
                        .to_data_vec_for_compat();
                    crate::common::arrow::data_to_arrow_batches(&data_type, data)
                }
            })
            .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;

        let schema = arrow_ipc_data_schema(&data_type, &batches, display)?;
        let batches = arrow_ipc_batches(&schema, batches)?;
        arrow_record_batches_to_pyarrow_stream(py, &schema, batches)
    }

    /// Write catalog records from Arrow IPC stream bytes.
    #[pyo3(signature = (record_type, data, identifier=None, params=None))]
    pub fn write_record_arrow_bytes(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusRecordType"))] record_type: &Bound<
            '_,
            PyAny,
        >,
        data: Vec<u8>,
        identifier: Option<String>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let record_type = catalog_record_type_from_py(record_type)?;
        let batches = arrow_record_batches_from_pybytes(data)?;
        let params = write_record_params_from_py(py, identifier, params)?;

        py.detach(|| CatalogWriter::write_records(&mut self.inner, record_type, &batches, params))
            .map_err(|e| PyIOError::new_err(format!("Failed write records: {e}")))
    }

    /// Query catalog records and return Arrow IPC stream bytes.
    #[pyo3(signature = (
        record_type,
        identifier=None,
        start=None,
        end=None,
        where_clause=None,
        display=true,
        as_of=None,
    ))]
    pub fn query_record_arrow_bytes(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusRecordType"))] record_type: &Bound<
            '_,
            PyAny,
        >,
        identifier: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        display: bool,
        as_of: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyBytes>> {
        let record_type = catalog_record_type_from_py(record_type)?;
        reject_parquet_as_of(as_of)?;
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let batches = py
            .detach(|| {
                if display {
                    CatalogReader::query_record_display_batches(
                        &mut self.inner,
                        &CatalogRecordQuery::new(record_type)
                            .with_identifier(identifier)
                            .with_range(start_nanos, end_nanos)
                            .with_where_clause(where_clause.map(str::to_string)),
                    )
                } else {
                    CatalogReader::query_record_batches(
                        &mut self.inner,
                        &CatalogRecordQuery::new(record_type)
                            .with_identifier(identifier)
                            .with_range(start_nanos, end_nanos)
                            .with_where_clause(where_clause.map(str::to_string)),
                    )
                }
            })
            .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;

        let schema = arrow_ipc_record_schema(record_type, &batches)?;
        let batches = arrow_ipc_batches(&schema, batches)?;

        arrow_record_batches_to_pybytes(py, &schema, &batches)
    }

    /// Query catalog records as an Arrow C stream `PyCapsule`.
    #[pyo3(signature = (
        record_type,
        identifier=None,
        start=None,
        end=None,
        where_clause=None,
        display=true,
        as_of=None,
    ))]
    pub fn query_record_arrow_stream(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusRecordType"))] record_type: &Bound<
            '_,
            PyAny,
        >,
        identifier: Option<String>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        display: bool,
        as_of: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let record_type = catalog_record_type_from_py(record_type)?;
        reject_parquet_as_of(as_of)?;
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let batches = py
            .detach(|| {
                if display {
                    CatalogReader::query_record_display_batches(
                        &mut self.inner,
                        &CatalogRecordQuery::new(record_type)
                            .with_identifier(identifier)
                            .with_range(start_nanos, end_nanos)
                            .with_where_clause(where_clause.map(str::to_string)),
                    )
                } else {
                    CatalogReader::query_record_batches(
                        &mut self.inner,
                        &CatalogRecordQuery::new(record_type)
                            .with_identifier(identifier)
                            .with_range(start_nanos, end_nanos)
                            .with_where_clause(where_clause.map(str::to_string)),
                    )
                }
            })
            .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;

        let schema = arrow_ipc_record_schema(record_type, &batches)?;
        let batches = arrow_ipc_batches(&schema, batches)?;

        arrow_record_batches_to_pyarrow_stream(py, &schema, batches)
    }

    /// Get missing time intervals for a data request.
    ///
    /// # Parameters
    ///
    /// - `start`: Start timestamp (nanoseconds since Unix epoch)
    /// - `end`: End timestamp (nanoseconds since Unix epoch)
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    ///
    /// # Returns
    ///
    /// Returns a list of (start, end) timestamp tuples representing missing intervals.
    #[pyo3(signature = (start, end, data_type, identifier=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn get_missing_intervals_for_request(
        &self,
        py: Python<'_>,
        start: u64,
        end: u64,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
    ) -> PyResult<Vec<(u64, u64)>> {
        let data_type = data_type.into_inner();
        py.detach(|| {
            self.inner.get_missing_intervals_for_request(
                start,
                end,
                &data_type,
                identifier.as_deref(),
            )
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to get missing intervals: {e}")))
    }

    /// Query the first timestamp for a specific data class and instrument.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    ///
    /// # Returns
    ///
    /// Returns the first timestamp as nanoseconds since Unix epoch, or None if no data exists.
    #[pyo3(signature = (data_type, identifier=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_first_timestamp(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
    ) -> PyResult<Option<u64>> {
        let data_type = data_type.into_inner();
        py.detach(|| {
            self.inner
                .query_first_timestamp(&data_type, identifier.as_deref())
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to query first timestamp: {e}")))
    }

    /// Query the last timestamp for a specific data class and instrument.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    ///
    /// # Returns
    ///
    /// Returns the last timestamp as nanoseconds since Unix epoch, or None if no data exists.
    #[pyo3(signature = (data_type, identifier=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_last_timestamp(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
    ) -> PyResult<Option<u64>> {
        let data_type = data_type.into_inner();
        py.detach(|| {
            self.inner
                .query_last_timestamp(&data_type, identifier.as_deref())
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to query last timestamp: {e}")))
    }

    /// Get time intervals covered by data for a specific data class and instrument.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target (data type, record type, or instrument type).
    /// - `identifier`: Optional instrument ID or bar type filter
    ///
    /// # Returns
    ///
    /// Returns a list of (start, end) timestamp tuples representing covered intervals.
    #[pyo3(signature = (data_type, identifier=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn get_intervals(
        &self,
        py: Python<'_>,
        data_type: PyCatalogDataType,
        identifier: Option<String>,
    ) -> PyResult<Vec<(u64, u64)>> {
        let data_type = data_type.into_inner();
        py.detach(|| self.inner.get_intervals(&data_type, identifier.as_deref()))
            .map_err(|e| PyIOError::new_err(format!("Failed to get intervals: {e}")))
    }

    /// Queries one data family and returns the decoded Python objects.
    #[pyo3(signature = (data_type, identifiers=None, start=None, end=None, where_clause=None, files=None, optimize_file_loading=true))]
    #[expect(
        clippy::too_many_arguments,
        reason = "PyO3 signature mirrors the catalog query filters"
    )]
    pub fn query(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<
            '_,
            PyAny,
        >,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
        files: Option<Vec<String>>,
        optimize_file_loading: bool,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let data_type = nautilus_data_type_from_py(data_type)?;
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        macro_rules! typed {
            ($type:ty) => {
                query_parquet_data::<$type>(
                    py,
                    &mut self.inner,
                    identifiers,
                    start,
                    end,
                    where_clause,
                    files,
                    optimize_file_loading,
                    "Query failed",
                )?
                .into_iter()
                .map(Data::from)
                .collect::<Vec<Data>>()
            };
        }

        let data = match data_type {
            NautilusDataType::QuoteTick => typed!(QuoteTick),
            NautilusDataType::TradeTick => typed!(TradeTick),
            NautilusDataType::Bar => typed!(Bar),
            NautilusDataType::OrderBookDelta => typed!(OrderBookDelta),
            NautilusDataType::OrderBookDepth => typed!(OrderBookDepth),
            NautilusDataType::IndexPriceUpdate => typed!(IndexPriceUpdate),
            NautilusDataType::MarkPriceUpdate => typed!(MarkPriceUpdate),
            NautilusDataType::FundingRateUpdate => typed!(FundingRateUpdate),
            NautilusDataType::OptionGreeks => typed!(OptionGreeks),
            NautilusDataType::InstrumentStatus => typed!(InstrumentStatus),
            NautilusDataType::InstrumentClose => typed!(InstrumentClose),
            NautilusDataType::Custom { type_name } => py
                .detach(|| {
                    self.inner.query_custom_data_dynamic(
                        &type_name,
                        identifiers.as_deref(),
                        start_nanos,
                        end_nanos,
                        where_clause,
                        files.clone(),
                        optimize_file_loading,
                    )
                })
                .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?,
            NautilusDataType::Instrument => {
                let instruments = py
                    .detach(|| {
                        self.inner.query_instruments_filtered_with_where(
                            identifiers.as_deref(),
                            start_nanos,
                            end_nanos,
                            where_clause,
                        )
                    })
                    .map_err(|e| PyIOError::new_err(format!("Query failed: {e}")))?;

                return instruments
                    .into_iter()
                    .map(|instrument| instrument_any_to_pyobject(py, instrument))
                    .collect();
            }
            #[cfg(feature = "defi")]
            NautilusDataType::Defi => {
                return Err(to_pytype_err("Defi data is not supported by query"));
            }
            #[cfg(not(feature = "defi"))]
            #[allow(
                unreachable_patterns,
                reason = "DeFi variants can exist without this crate's defi feature"
            )]
            _ => return Err(to_pytype_err("Defi data is not supported by query")),
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
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings
    ///   (e.g., "EUR/USD.SIM") or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<QuoteTick>> {
        query_parquet_data::<QuoteTick>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query trade tick data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings
    ///   (e.g., "EUR/USD.SIM") or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<TradeTick>> {
        query_parquet_data::<TradeTick>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query order book delta data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings
    ///   (e.g., "EUR/USD.SIM") or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        query_parquet_data::<OrderBookDelta>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query bar data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by. Can be `instrument_id` strings
    ///   (e.g., "EUR/USD.SIM") or `bar_type` strings (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
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
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<Bar>> {
        query_parquet_data::<Bar>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query order book depth data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `OrderBookDepth` objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_order_book_depths(
        &mut self,
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<OrderBookDepth>> {
        query_parquet_data::<OrderBookDepth>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query mark price update data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `MarkPriceUpdate` objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_mark_price_updates(
        &mut self,
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<MarkPriceUpdate>> {
        query_parquet_data::<MarkPriceUpdate>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query index price update data from Parquet files.
    ///
    /// # Parameters
    ///
    /// - `identifiers`: Optional list of identifiers to filter by.
    /// - `start`: Optional start timestamp (nanoseconds since Unix epoch)
    /// - `end`: Optional end timestamp (nanoseconds since Unix epoch)
    /// - `where_clause`: Optional SQL WHERE clause for additional filtering
    ///
    /// # Returns
    ///
    /// Returns a vector of `IndexPriceUpdate` objects matching the query criteria.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_index_price_updates(
        &mut self,
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<IndexPriceUpdate>> {
        query_parquet_data::<IndexPriceUpdate>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// Query option greeks data from Parquet files.
    #[pyo3(signature = (identifiers=None, start=None, end=None, where_clause=None))]
    pub fn query_option_greeks(
        &mut self,
        py: Python<'_>,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<OptionGreeks>> {
        query_parquet_data::<OptionGreeks>(
            py,
            &mut self.inner,
            identifiers,
            start,
            end,
            where_clause,
            None,
            true,
            "Failed to query data",
        )
    }

    /// List all data types available in the catalog.
    ///
    /// # Returns
    ///
    /// Returns the data, record, and instrument types stored in the catalog.
    pub fn list_data_types(&self, py: Python<'_>) -> PyResult<Vec<PyCatalogDataType>> {
        py.detach(|| self.inner.list_data_types())
            .map(|data_types| data_types.into_iter().map(PyCatalogDataType::new).collect())
            .map_err(|e| PyIOError::new_err(format!("Failed to list data types: {e}")))
    }

    /// List all live run IDs available in the catalog.
    ///
    /// # Returns
    ///
    /// Returns a list of live run IDs (as directory stems) in the catalog.
    pub fn list_live_runs(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        py.detach(|| self.inner.list_live_runs())
            .map_err(|e| PyIOError::new_err(format!("Failed to list live runs: {e}")))
    }

    /// List all backtest run IDs available in the catalog.
    ///
    /// # Returns
    ///
    /// Returns a list of backtest run IDs (as directory stems) in the catalog.
    pub fn list_backtest_runs(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        py.detach(|| self.inner.list_backtest_runs())
            .map_err(|e| PyIOError::new_err(format!("Failed to list backtest runs: {e}")))
    }

    /// List all backtest run instances available in the catalog.
    pub fn list_backtests(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        py.detach(|| self.inner.list_backtest_runs())
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
    /// - `data_type`: The stored family to convert (data type or record type).
    /// - `environment`: The environment of the run, which names the folder holding its feather files (default: `Environment.BACKTEST`)
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
    ///     NautilusDataType.QuoteTick,
    ///     environment=Environment.BACKTEST
    /// )
    ///
    /// # Convert live run data with identifier filtering
    /// catalog.convert_stream_to_data(
    ///     "instance-456",
    ///     NautilusDataType.TradeTick,
    ///     environment=Environment.LIVE,
    ///     identifiers=["EUR/USD.SIM"]
    /// )
    /// ```
    #[pyo3(signature = (instance_id, data_type, environment=Environment::Backtest, identifiers=None, use_ts_event_for_ts_init=false))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn convert_stream_to_data(
        &mut self,
        py: Python<'_>,
        instance_id: &str,
        data_type: PyCatalogDataType,
        environment: Environment,
        identifiers: Option<Vec<String>>,
        use_ts_event_for_ts_init: bool,
    ) -> PyResult<()> {
        let data_type = data_type.into_inner();

        py.detach(|| {
            self.inner.convert_stream_to_data(
                instance_id,
                &data_type,
                environment,
                identifiers.as_deref(),
                use_ts_event_for_ts_init,
            )
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to convert stream to data: {e}")))
    }

    /// Query custom data from Parquet files.
    #[pyo3(signature = (data_type, identifiers=None, start=None, end=None, where_clause=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn query_custom_data(
        &mut self,
        py: Python<'_>,
        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<
            '_,
            PyAny,
        >,
        identifiers: Option<Vec<String>>,
        start: Option<u64>,
        end: Option<u64>,
        where_clause: Option<&str>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let NautilusDataType::Custom { type_name } = nautilus_data_type_from_py(data_type)? else {
            return Err(to_pytype_err("data_type must be a custom NautilusDataType"));
        };

        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);

        let data = py
            .detach(|| {
                self.inner.query_custom_data_dynamic(
                    &type_name,
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
                Data::Custom(custom) => Py::new(py, custom)?.into_any(),
                _ => return Err(PyIOError::new_err("Expected custom data")),
            };

            python_objects.push(py_obj);
        }

        Ok(python_objects)
    }
}

impl PyParquetDataCatalog {
    fn mutate<T: Send>(
        &self,
        py: Python<'_>,
        operation: impl FnOnce() -> anyhow::Result<T> + Send,
    ) -> anyhow::Result<T> {
        py.detach(|| {
            let _guard = self
                .mutation_lock
                .lock()
                .map_err(|_| anyhow::anyhow!("catalog mutation lock is poisoned"))?;
            operation()
        })
    }
}

// Codes follow the Parquet format codec numbering, except that 5 writes LZ4_RAW rather than the
// deprecated Hadoop-framed LZ4; LZO (3) is excluded because the writer cannot produce it
fn catalog_compression_from_code(code: u8) -> PyResult<CatalogCompression> {
    match code {
        0 => Ok(CatalogCompression::Uncompressed),
        1 => Ok(CatalogCompression::Snappy),
        2 => Ok(CatalogCompression::Gzip),
        4 => Ok(CatalogCompression::Brotli),
        5 => Ok(CatalogCompression::Lz4),
        6 => Ok(CatalogCompression::Zstd),
        _ => Err(to_pyvalue_err(format!(
            "Invalid compression code {code}, expected one of 0 (UNCOMPRESSED), 1 (SNAPPY), \
             2 (GZIP), 4 (BROTLI), 5 (LZ4_RAW), or 6 (ZSTD)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Borrow,
        cell::Cell,
        sync::{Arc, Barrier, Once, TryLockError, mpsc},
        thread,
        time::Duration,
    };

    use arrow::{datatypes::Schema, error::ArrowError, record_batch::RecordBatch};
    use nautilus_model::{
        custom_data,
        data::{
            BookOrder, CustomData, CustomDataTrait, DataType, ensure_custom_data_json_registered,
            register_arrow,
        },
        enums::{BookAction, OrderSide},
        identifiers::InstrumentId,
        python::data::PyNautilusDataType,
        types::{Price, Quantity},
    };
    use nautilus_serialization::{
        arrow::{ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodingError},
        arrow_custom_data,
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case::uncompressed(0, CatalogCompression::Uncompressed)]
    #[case::snappy(1, CatalogCompression::Snappy)]
    #[case::gzip(2, CatalogCompression::Gzip)]
    #[case::brotli(4, CatalogCompression::Brotli)]
    #[case::lz4(5, CatalogCompression::Lz4)]
    #[case::zstd(6, CatalogCompression::Zstd)]
    fn catalog_compression_from_code_maps_parquet_codes(
        #[case] code: u8,
        #[case] expected: CatalogCompression,
    ) {
        assert_eq!(catalog_compression_from_code(code).unwrap(), expected);
    }

    #[rstest]
    fn test_write_parquet_data_releases_gil() {
        Python::initialize();
        let directory = TempDir::new().unwrap();
        let mut catalog = PyParquetDataCatalog::py_new(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let delta = OrderBookDelta::clear(
            InstrumentId::from("ETH/USDT.BINANCE"),
            17,
            23.into(),
            29.into(),
        );

        let path = Python::attach(|py| {
            write_parquet_data(
                py,
                &catalog,
                vec![GilCheckedDelta(delta, Some(&catalog.mutation_lock))],
                None,
                None,
                false,
                "deltas",
            )
            .unwrap()
        });

        let loaded = catalog
            .inner
            .query::<OrderBookDelta>(None, None, None, None, None, true)
            .unwrap();

        assert!(catalog.inner.file_exists(&path).unwrap());
        assert_eq!(loaded, vec![delta]);
    }

    #[rstest]
    fn test_query_parquet_data_releases_gil() {
        Python::initialize();
        let directory = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(directory.path(), None, None, None, None);
        let delta = OrderBookDelta::clear(
            InstrumentId::from("ETH/USDT.BINANCE"),
            17,
            23.into(),
            29.into(),
        );
        catalog
            .write_to_parquet(&[delta], None, None, None)
            .unwrap();

        let loaded = Python::attach(|py| {
            query_parquet_data::<GilCheckedDelta>(
                py,
                &mut catalog,
                None,
                None,
                None,
                None,
                None,
                true,
                "Query failed",
            )
            .unwrap()
        });

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, delta);
        assert!(loaded[0].1.is_none());
        assert_eq!(loaded[0].0.action, BookAction::Clear);
    }

    #[rstest]
    fn test_catalog_python_delta_round_trip_and_maintenance() {
        Python::initialize();
        let directory = TempDir::new().unwrap();
        let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
        let deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("100.23"),
                    Quantity::from("1.234"),
                    41,
                ),
                7,
                17,
                23.into(),
                29.into(),
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Update,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.45"),
                    Quantity::from("2.345"),
                    47,
                ),
                8,
                31,
                37.into(),
                43.into(),
            ),
        ];

        Python::attach(|py| {
            let catalog = PyParquetDataCatalog::py_new(
                directory.path().to_str().unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let catalog = Bound::new(py, catalog).unwrap();
            let data_type = PyNautilusDataType::new(NautilusDataType::OrderBookDelta);
            let data_type = Bound::new(py, data_type).unwrap();
            let path: String = catalog
                .call_method1("write_order_book_deltas", (deltas.clone(), 11, 53))
                .unwrap()
                .extract()
                .unwrap();
            let loaded: Vec<OrderBookDelta> = catalog
                .call_method0("query_order_book_deltas")
                .unwrap()
                .extract()
                .unwrap();
            let queried: Vec<OrderBookDelta> = catalog
                .call_method1("query", (&data_type,))
                .unwrap()
                .extract()
                .unwrap();
            let intervals: Vec<(u64, u64)> = catalog
                .call_method1("get_intervals", (&data_type, instrument_id.to_string()))
                .unwrap()
                .extract()
                .unwrap();

            assert!(catalog.borrow().inner.file_exists(&path).unwrap());
            assert_eq!(loaded, deltas);
            assert_eq!(queried, deltas);
            assert_eq!(intervals, vec![(11, 53)]);

            catalog
                .call_method1("consolidate_data", (&data_type, instrument_id.to_string()))
                .unwrap();
            catalog
                .call_method1(
                    "delete_data_range",
                    (&data_type, instrument_id.to_string(), 29, 29),
                )
                .unwrap();
            let remaining: Vec<OrderBookDelta> = catalog
                .call_method0("query_order_book_deltas")
                .unwrap()
                .extract()
                .unwrap();

            assert_eq!(remaining, vec![deltas[1]]);
        });
    }

    #[rstest]
    fn test_catalog_python_delta_write_errors_and_empty_input() {
        Python::initialize();
        let directory = TempDir::new().unwrap();

        Python::attach(|py| {
            let catalog = PyParquetDataCatalog::py_new(
                directory.path().to_str().unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let catalog = Bound::new(py, catalog).unwrap();
            let path: String = catalog
                .call_method1("write_order_book_deltas", (Vec::<OrderBookDelta>::new(),))
                .unwrap()
                .extract()
                .unwrap();
            let delta = OrderBookDelta::clear(
                InstrumentId::from("ETH/USDT.BINANCE"),
                17,
                23.into(),
                29.into(),
            );

            let earlier = OrderBookDelta {
                ts_init: 19.into(),
                ..delta
            };

            let error = catalog
                .call_method1("write_order_book_deltas", (vec![delta, earlier],))
                .unwrap_err();
            let loaded: Vec<OrderBookDelta> = catalog
                .call_method0("query_order_book_deltas")
                .unwrap()
                .extract()
                .unwrap();

            assert_eq!(path, "");
            assert!(error.is_instance_of::<PyIOError>(py));
            assert_eq!(
                error.value(py).str().unwrap().to_str().unwrap(),
                "Failed to write order book deltas: nautilus_model_data_delta_order_book_delta timestamps must be in ascending order"
            );
            assert_eq!(loaded, Vec::<OrderBookDelta>::new());
        });
    }

    #[rstest]
    fn test_concurrent_delta_writes_reject_overlapping_intervals() {
        Python::initialize();
        let directory = TempDir::new().unwrap();
        let mut catalog = PyParquetDataCatalog::py_new(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
        let first = vec![
            OrderBookDelta::clear(instrument_id, 11, 17.into(), 23.into()),
            OrderBookDelta::clear(instrument_id, 29, 31.into(), 37.into()),
        ];
        let second = vec![
            OrderBookDelta::clear(instrument_id, 41, 43.into(), 37.into()),
            OrderBookDelta::clear(instrument_id, 47, 53.into(), 59.into()),
        ];
        let start = Barrier::new(2);

        let (first_result, second_result) = thread::scope(|scope| {
            let first_thread = scope.spawn(|| {
                start.wait();
                Python::attach(|py| {
                    catalog.write_order_book_deltas(py, first.clone(), None, None, false)
                })
            });

            let second_thread = scope.spawn(|| {
                start.wait();
                Python::attach(|py| {
                    catalog.write_order_book_deltas(py, second.clone(), None, None, false)
                })
            });

            (first_thread.join().unwrap(), second_thread.join().unwrap())
        });

        let (expected, error) = match (first_result, second_result) {
            (Ok(_), Err(e)) => (first, e),
            (Err(e), Ok(_)) => (second, e),
            results => {
                panic!("Expected one successful write and one overlap error, received {results:?}")
            }
        };

        let loaded = catalog
            .inner
            .query::<OrderBookDelta>(None, None, None, None, None, true)
            .unwrap();

        Python::attach(|py| {
            assert!(error.is_instance_of::<PyIOError>(py));
            assert!(
                error
                    .value(py)
                    .str()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .contains("would create non-disjoint intervals")
            );
        });

        assert_eq!(loaded, expected);
    }

    #[rstest]
    fn test_poisoned_mutation_lock_returns_python_io_error() {
        Python::initialize();
        let directory = TempDir::new().unwrap();
        let catalog = PyParquetDataCatalog::py_new(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let poisoned = thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = catalog.mutation_lock.lock().unwrap();
                    panic!("native mutation failed");
                })
                .join()
                .is_err()
        });

        let error = Python::attach(|py| {
            catalog
                .write_order_book_deltas(py, Vec::new(), None, None, false)
                .unwrap_err()
        });

        assert!(poisoned);
        Python::attach(|py| {
            assert!(error.is_instance_of::<PyIOError>(py));
            assert_eq!(
                error.value(py).str().unwrap().to_str().unwrap(),
                "Failed to write order book deltas: catalog mutation lock is poisoned"
            );
        });
    }

    #[rstest]
    fn test_mutation_wait_releases_gil() {
        Python::initialize();
        let directory = TempDir::new().unwrap();
        let catalog = PyParquetDataCatalog::py_new(
            directory.path().to_str().unwrap(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let delta = OrderBookDelta::clear(
            InstrumentId::from("ETH/USDT.BINANCE"),
            17,
            23.into(),
            29.into(),
        );
        let (started_tx, started_rx) = mpsc::channel();
        let (available_tx, available_rx) = mpsc::channel();
        let guard = catalog.mutation_lock.lock().unwrap();

        let (gil_available, path) = thread::scope(|scope| {
            let writer = scope.spawn(|| {
                Python::attach(|py| {
                    started_tx.send(()).unwrap();
                    catalog.write_order_book_deltas(py, vec![delta], None, None, false)
                })
            });

            started_rx.recv().unwrap();

            let probe = scope.spawn(|| Python::attach(|_| available_tx.send(()).unwrap()));
            let gil_available = available_rx.recv_timeout(Duration::from_secs(5)).is_ok();
            drop(guard);
            let path = writer.join().unwrap().unwrap();
            probe.join().unwrap();
            (gil_available, path)
        });

        assert!(
            gil_available,
            "GIL remained held while waiting for the mutation lock"
        );
        assert!(catalog.inner.file_exists(&path).unwrap());
    }

    #[rstest]
    #[case::delete_data("delete_data_range")]
    #[case::delete_catalog("delete_catalog_range")]
    #[case::consolidate_data("consolidate_data_by_period")]
    #[case::consolidate_catalog("consolidate_catalog_by_period")]
    fn test_custom_maintenance_keeps_callbacks_attached(#[case] method: &str) {
        Python::initialize();
        register_gil_checked_custom_data();
        let directory = TempDir::new().unwrap();

        let expected = [11u64, 29, 47]
            .into_iter()
            .map(|ts| {
                let value = GilCheckedCustomData {
                    value: ts + 100,
                    ts_event: (ts - 3).into(),
                    ts_init: ts.into(),
                };

                let data_type = DataType::new(GilCheckedCustomData::type_name_static(), None, None);
                CustomData::new(Arc::new(value), data_type)
            })
            .collect::<Vec<_>>();

        Python::attach(|py| {
            let catalog = PyParquetDataCatalog::py_new(
                directory.path().to_str().unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            catalog
                .inner
                .write_custom_data_batch(&expected, None, None, None)
                .unwrap();
            let catalog = Bound::new(py, catalog).unwrap();

            let data_type = PyNautilusDataType::new(NautilusDataType::Custom {
                type_name: GilCheckedCustomData::type_name_static().to_string(),
            });

            let data_type = Bound::new(py, data_type).unwrap();
            CALLBACKS.with(|calls| calls.set((false, false)));

            match method {
                "delete_data_range" => {
                    catalog.call_method1(method, (&data_type, None::<String>, 29, 29))
                }
                "delete_catalog_range" => catalog.call_method1(method, (29, 29)),
                "consolidate_data_by_period" => {
                    catalog.call_method1(method, (&data_type, None::<String>, 20))
                }
                "consolidate_catalog_by_period" => catalog.call_method1(method, (20,)),
                _ => unreachable!(),
            }
            .unwrap();

            let callbacks = CALLBACKS.with(Cell::get);
            let loaded = catalog
                .borrow_mut()
                .inner
                .query_custom_data_dynamic(
                    GilCheckedCustomData::type_name_static(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                )
                .unwrap();
            let expected = expected
                .iter()
                .filter(|value| !method.starts_with("delete") || value.ts_init().as_u64() != 29)
                .cloned()
                .map(Data::Custom)
                .collect::<Vec<_>>();

            assert_eq!(callbacks, (true, true));
            assert_eq!(loaded, expected);
        });
    }

    #[rstest]
    #[case::empty(true)]
    #[case::nonempty(false)]
    fn test_custom_write_prepares_attached_before_native_mutation(#[case] empty: bool) {
        Python::initialize();
        register_gil_checked_custom_data();
        let directory = TempDir::new().unwrap();

        Python::attach(|py| {
            let catalog = PyParquetDataCatalog::py_new(
                directory.path().to_str().unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let catalog = Bound::new(py, catalog).unwrap();

            let rows = if empty {
                Vec::new()
            } else {
                let value = GilCheckedCustomData {
                    value: 113,
                    ts_event: 17.into(),
                    ts_init: 23.into(),
                };

                let data_type = DataType::new(GilCheckedCustomData::type_name_static(), None, None);
                vec![CustomData::new(Arc::new(value), data_type)]
            };

            let objects = rows
                .iter()
                .cloned()
                .map(|row| Py::new(py, row).unwrap())
                .collect::<Vec<_>>();
            CALLBACKS.with(|calls| calls.set((false, false)));
            let path: String = catalog
                .call_method1("write_custom_data", (objects,))
                .unwrap()
                .extract()
                .unwrap();
            let callbacks = CALLBACKS.with(Cell::get);
            let loaded = catalog
                .borrow_mut()
                .inner
                .query_custom_data_dynamic(
                    GilCheckedCustomData::type_name_static(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                )
                .unwrap();

            assert_eq!(callbacks, (!empty, false));
            assert_eq!(
                loaded,
                rows.into_iter().map(Data::Custom).collect::<Vec<_>>()
            );

            if empty {
                assert_eq!(path, "");
            } else {
                assert!(catalog.borrow().inner.file_exists(&path).unwrap());
            }
        });
    }

    #[arrow_custom_data]
    #[custom_data]
    struct GilCheckedCustomData {
        value: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    }

    thread_local! {
        static CALLBACKS: Cell<(bool, bool)> = const { Cell::new((false, false)) };
    }

    fn register_gil_checked_custom_data() {
        static REGISTER: Once = Once::new();
        REGISTER.call_once(|| {
            ensure_custom_data_json_registered::<GilCheckedCustomData>().unwrap();
            register_arrow(
                GilCheckedCustomData::type_name_static(),
                Arc::new(GilCheckedCustomData::get_schema(None)),
                Box::new(|items| {
                    assert_gil_held();
                    CALLBACKS.with(|calls| calls.set((true, calls.get().1)));
                    Python::attach(|_| {
                        let rows = items
                            .iter()
                            .map(|item| {
                                item.as_any()
                                    .downcast_ref::<GilCheckedCustomData>()
                                    .unwrap()
                                    .clone()
                            })
                            .collect::<Vec<_>>();

                        let metadata = rows[0].metadata();
                        GilCheckedCustomData::encode_batch(&metadata, &rows)
                            .map_err(anyhow::Error::from)
                    })
                }),
                Box::new(|metadata, batch| {
                    assert_gil_held();
                    CALLBACKS.with(|calls| calls.set((calls.get().0, true)));
                    Python::attach(|_| GilCheckedCustomData::decode_data_batch(metadata, batch))
                        .map_err(anyhow::Error::from)
                }),
            )
            .unwrap();
        });
    }

    #[derive(Clone, Copy, Debug)]
    struct GilCheckedDelta<'a>(OrderBookDelta, Option<&'a Mutex<()>>);

    impl HasTsInit for GilCheckedDelta<'_> {
        fn ts_init(&self) -> UnixNanos {
            self.0.ts_init
        }
    }

    impl HasCatalogDataType for GilCheckedDelta<'_> {
        fn catalog_data_type() -> NautilusDataType {
            NautilusDataType::OrderBookDelta
        }
    }

    impl ArrowSchemaProvider for GilCheckedDelta<'_> {
        fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
            OrderBookDelta::get_schema(metadata)
        }
    }

    impl EncodeToRecordBatch for GilCheckedDelta<'_> {
        fn encode_batch<T>(
            metadata: &HashMap<String, String>,
            data: &[T],
        ) -> Result<RecordBatch, ArrowError>
        where
            T: Borrow<Self>,
        {
            assert_gil_released();
            let deltas = data.iter().map(|item| item.borrow().0).collect::<Vec<_>>();
            OrderBookDelta::encode_batch(metadata, &deltas)
        }

        fn metadata(&self) -> HashMap<String, String> {
            if let Some(lock) = self.1 {
                assert!(matches!(lock.try_lock(), Err(TryLockError::WouldBlock)));
            }

            self.0.metadata()
        }
    }

    impl DecodeTypedFromRecordBatch for GilCheckedDelta<'_> {
        fn decode_typed_batch(
            metadata: &HashMap<String, String>,
            record_batch: RecordBatch,
        ) -> Result<Vec<Self>, EncodingError> {
            assert_gil_released();
            OrderBookDelta::decode_typed_batch(metadata, record_batch)
                .map(|deltas| deltas.into_iter().map(|delta| Self(delta, None)).collect())
        }
    }

    fn assert_gil_released() {
        // SAFETY: Python is initialized before this check, which does not require the GIL.
        assert_eq!(unsafe { pyo3::ffi::PyGILState_Check() }, 0);
    }

    fn assert_gil_held() {
        // SAFETY: Python is initialized before this check, which does not require the GIL.
        assert_eq!(unsafe { pyo3::ffi::PyGILState_Check() }, 1);
    }
}
