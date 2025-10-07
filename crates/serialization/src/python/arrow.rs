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

use std::io::Cursor;

use arrow::{ipc::writer::StreamWriter, record_batch::RecordBatch};
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{
        Bar, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick,
        TradeTick, close::InstrumentClose,
    },
    python::data::{
        pyobjects_to_bars, pyobjects_to_book_deltas, pyobjects_to_index_prices,
        pyobjects_to_instrument_closes, pyobjects_to_mark_prices, pyobjects_to_quotes,
        pyobjects_to_trades,
    },
};
use pyo3::{
    conversion::IntoPyObjectExt,
    exceptions::{PyRuntimeError, PyTypeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyType},
};

use crate::arrow::{
    ArrowSchemaProvider, bars_to_arrow_record_batch_bytes, book_deltas_to_arrow_record_batch_bytes,
    book_depth10_to_arrow_record_batch_bytes, index_prices_to_arrow_record_batch_bytes,
    instrument_closes_to_arrow_record_batch_bytes, mark_prices_to_arrow_record_batch_bytes,
    quotes_to_arrow_record_batch_bytes, trades_to_arrow_record_batch_bytes,
};

/// Transforms the given record `batches` into Python `bytes`.
fn arrow_record_batch_to_pybytes(py: Python, batch: RecordBatch) -> PyResult<Py<PyBytes>> {
    // Create a cursor to write to a byte array in memory
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = StreamWriter::try_new(&mut cursor, &batch.schema())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;

        writer
            .write(&batch)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;

        writer
            .finish()
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;
    }

    let buffer = cursor.into_inner();
    let pybytes = PyBytes::new(py, &buffer);

    Ok(pybytes.into())
}

/// Returns a mapping from field names to Arrow data types for the given Rust data class.
///
/// # Errors
///
/// Returns a `PyErr` if the class name is not recognized or schema extraction fails.
#[pyfunction]
pub fn get_arrow_schema_map(py: Python<'_>, cls: &Bound<'_, PyType>) -> PyResult<Py<PyAny>> {
    let cls_str: String = cls.getattr("__name__")?.extract()?;
    let result_map = match cls_str.as_str() {
        stringify!(OrderBookDelta) => OrderBookDelta::get_schema_map(),
        stringify!(OrderBookDepth10) => OrderBookDepth10::get_schema_map(),
        stringify!(QuoteTick) => QuoteTick::get_schema_map(),
        stringify!(TradeTick) => TradeTick::get_schema_map(),
        stringify!(Bar) => Bar::get_schema_map(),
        stringify!(MarkPriceUpdate) => MarkPriceUpdate::get_schema_map(),
        stringify!(IndexPriceUpdate) => IndexPriceUpdate::get_schema_map(),
        stringify!(InstrumentClose) => InstrumentClose::get_schema_map(),
        _ => {
            return Err(PyTypeError::new_err(format!(
                "Arrow schema for `{cls_str}` is not currently implemented in Rust."
            )));
        }
    };

    result_map.into_py_any(py)
}

/// Returns Python `bytes` from the given list of legacy data objects, which can be passed
/// to `pa.ipc.open_stream` to create a `RecordBatchReader`.
///
/// # Errors
///
/// Returns an error if:
/// - The input list is empty: `PyErr`.
/// - An unsupported data type is encountered or conversion fails: `PyErr`.
///
/// # Panics
///
/// Panics if `data.first()` returns `None` (should not occur due to emptiness check).
#[pyfunction]
pub fn pyobjects_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<Bound<'_, PyAny>>,
) -> PyResult<Py<PyBytes>> {
    if data.is_empty() {
        return Err(to_pyvalue_err("Empty data"));
    }

    let data_type: String = data
        .first()
        .unwrap() // SAFETY: Unwrap safe as already checked that `data` not empty
        .getattr("__class__")?
        .getattr("__name__")?
        .extract()?;

    match data_type.as_str() {
        stringify!(OrderBookDelta) => {
            let deltas = pyobjects_to_book_deltas(data)?;
            py_book_deltas_to_arrow_record_batch_bytes(py, deltas)
        }
        stringify!(OrderBookDepth10) => {
            let depth_snapshots: Vec<OrderBookDepth10> = data
                .into_iter()
                .map(|obj| obj.extract::<OrderBookDepth10>())
                .collect::<PyResult<Vec<OrderBookDepth10>>>()?;
            py_book_depth10_to_arrow_record_batch_bytes(py, depth_snapshots)
        }
        stringify!(QuoteTick) => {
            let quotes = pyobjects_to_quotes(data)?;
            py_quotes_to_arrow_record_batch_bytes(py, quotes)
        }
        stringify!(TradeTick) => {
            let trades = pyobjects_to_trades(data)?;
            py_trades_to_arrow_record_batch_bytes(py, trades)
        }
        stringify!(Bar) => {
            let bars = pyobjects_to_bars(data)?;
            py_bars_to_arrow_record_batch_bytes(py, bars)
        }
        stringify!(MarkPriceUpdate) => {
            let updates = pyobjects_to_mark_prices(data)?;
            py_mark_prices_to_arrow_record_batch_bytes(py, updates)
        }
        stringify!(IndexPriceUpdate) => {
            let index_prices = pyobjects_to_index_prices(data)?;
            py_index_prices_to_arrow_record_batch_bytes(py, index_prices)
        }
        stringify!(InstrumentClose) => {
            let closes = pyobjects_to_instrument_closes(data)?;
            py_instrument_closes_to_arrow_record_batch_bytes(py, closes)
        }
        _ => Err(PyValueError::new_err(format!(
            "unsupported data type: {data_type}"
        ))),
    }
}

/// Converts a list of `OrderBookDelta` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "book_deltas_to_arrow_record_batch_bytes")]
pub fn py_book_deltas_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OrderBookDelta>,
) -> PyResult<Py<PyBytes>> {
    match book_deltas_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `OrderBookDepth10` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "book_depth10_to_arrow_record_batch_bytes")]
pub fn py_book_depth10_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OrderBookDepth10>,
) -> PyResult<Py<PyBytes>> {
    match book_depth10_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `QuoteTick` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "quotes_to_arrow_record_batch_bytes")]
pub fn py_quotes_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<QuoteTick>,
) -> PyResult<Py<PyBytes>> {
    match quotes_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `TradeTick` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "trades_to_arrow_record_batch_bytes")]
pub fn py_trades_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<TradeTick>,
) -> PyResult<Py<PyBytes>> {
    match trades_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `Bar` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "bars_to_arrow_record_batch_bytes")]
pub fn py_bars_to_arrow_record_batch_bytes(py: Python, data: Vec<Bar>) -> PyResult<Py<PyBytes>> {
    match bars_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `MarkPriceUpdate` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "mark_prices_to_arrow_record_batch_bytes")]
pub fn py_mark_prices_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<MarkPriceUpdate>,
) -> PyResult<Py<PyBytes>> {
    match mark_prices_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `IndexPriceUpdate` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "index_prices_to_arrow_record_batch_bytes")]
pub fn py_index_prices_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<IndexPriceUpdate>,
) -> PyResult<Py<PyBytes>> {
    match index_prices_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `InstrumentClose` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "instrument_closes_to_arrow_record_batch_bytes")]
pub fn py_instrument_closes_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<InstrumentClose>,
) -> PyResult<Py<PyBytes>> {
    match instrument_closes_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}
