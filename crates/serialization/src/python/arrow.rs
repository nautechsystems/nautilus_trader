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
    data::{Bar, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    python::data::{
        pyobjects_to_bars, pyobjects_to_order_book_deltas, pyobjects_to_quote_ticks,
        pyobjects_to_trade_ticks,
    },
};
use pyo3::{
    conversion::IntoPyObjectExt,
    exceptions::{PyRuntimeError, PyTypeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyType},
};

use crate::arrow::{
    ArrowSchemaProvider, bars_to_arrow_record_batch_bytes,
    order_book_deltas_to_arrow_record_batch_bytes, order_book_depth10_to_arrow_record_batch_bytes,
    quote_ticks_to_arrow_record_batch_bytes, trade_ticks_to_arrow_record_batch_bytes,
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

#[pyfunction]
pub fn get_arrow_schema_map(py: Python<'_>, cls: &Bound<'_, PyType>) -> PyResult<Py<PyAny>> {
    let cls_str: String = cls.getattr("__name__")?.extract()?;
    let result_map = match cls_str.as_str() {
        stringify!(OrderBookDelta) => OrderBookDelta::get_schema_map(),
        stringify!(OrderBookDepth10) => OrderBookDepth10::get_schema_map(),
        stringify!(QuoteTick) => QuoteTick::get_schema_map(),
        stringify!(TradeTick) => TradeTick::get_schema_map(),
        stringify!(Bar) => Bar::get_schema_map(),
        _ => {
            return Err(PyTypeError::new_err(format!(
                "Arrow schema for `{cls_str}` is not currently implemented in Rust."
            )));
        }
    };

    result_map.into_py_any(py)
}

/// Return Python `bytes` from the given list of 'legacy' data objects, which can be passed
/// to `pa.ipc.open_stream` to create a `RecordBatchReader`.
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
        .as_ref()
        .getattr("__class__")?
        .getattr("__name__")?
        .extract()?;

    match data_type.as_str() {
        stringify!(OrderBookDelta) => {
            let deltas = pyobjects_to_order_book_deltas(data)?;
            py_order_book_deltas_to_arrow_record_batch_bytes(py, deltas)
        }
        stringify!(QuoteTick) => {
            let quotes = pyobjects_to_quote_ticks(data)?;
            py_quote_ticks_to_arrow_record_batch_bytes(py, quotes)
        }
        stringify!(TradeTick) => {
            let trades = pyobjects_to_trade_ticks(data)?;
            py_trade_ticks_to_arrow_record_batch_bytes(py, trades)
        }
        stringify!(Bar) => {
            let bars = pyobjects_to_bars(data)?;
            py_bars_to_arrow_record_batch_bytes(py, bars)
        }
        _ => Err(PyValueError::new_err(format!(
            "unsupported data type: {data_type}"
        ))),
    }
}

#[pyfunction(name = "order_book_deltas_to_arrow_record_batch_bytes")]
pub fn py_order_book_deltas_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OrderBookDelta>,
) -> PyResult<Py<PyBytes>> {
    match order_book_deltas_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

#[pyfunction(name = "order_book_depth10_to_arrow_record_batch_bytes")]
pub fn py_order_book_depth10_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OrderBookDepth10>,
) -> PyResult<Py<PyBytes>> {
    match order_book_depth10_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

#[pyfunction(name = "quote_ticks_to_arrow_record_batch_bytes")]
pub fn py_quote_ticks_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<QuoteTick>,
) -> PyResult<Py<PyBytes>> {
    match quote_ticks_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

#[pyfunction(name = "trade_ticks_to_arrow_record_batch_bytes")]
pub fn py_trade_ticks_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<TradeTick>,
) -> PyResult<Py<PyBytes>> {
    match trade_ticks_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

#[pyfunction(name = "bars_to_arrow_record_batch_bytes")]
pub fn py_bars_to_arrow_record_batch_bytes(py: Python, data: Vec<Bar>) -> PyResult<Py<PyBytes>> {
    match bars_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}
