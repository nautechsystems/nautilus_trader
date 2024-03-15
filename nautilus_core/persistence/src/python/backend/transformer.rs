// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use datafusion::arrow::{
    datatypes::Schema, error::ArrowError, ipc::writer::StreamWriter, record_batch::RecordBatch,
};
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::data::{
    bar::Bar, delta::OrderBookDelta, depth::OrderBookDepth10, is_monotonically_increasing_by_init,
    quote::QuoteTick, trade::TradeTick,
};
use pyo3::{
    exceptions::{PyRuntimeError, PyTypeError, PyValueError},
    prelude::*,
    types::{IntoPyDict, PyBytes, PyDict, PyType},
};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

const ERROR_EMPTY_DATA: &str = "`data` was empty";
const ERROR_MONOTONICITY: &str = "`data` was not monotonically increasing by the `ts_init` field";

#[pyclass]
pub struct DataTransformer {}

impl DataTransformer {
    /// Transforms the given `data` Python objects into a vector of [`OrderBookDelta`] objects.
    fn pyobjects_to_order_book_deltas(
        py: Python<'_>,
        data: Vec<PyObject>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        let deltas: Vec<OrderBookDelta> = data
            .into_iter()
            .map(|obj| OrderBookDelta::from_pyobject(obj.as_ref(py)))
            .collect::<PyResult<Vec<OrderBookDelta>>>()?;

        // Validate monotonically increasing
        if !is_monotonically_increasing_by_init(&deltas) {
            return Err(PyValueError::new_err(ERROR_MONOTONICITY));
        }

        Ok(deltas)
    }

    /// Transforms the given `data` Python objects into a vector of [`QuoteTick`] objects.
    fn pyobjects_to_quote_ticks(py: Python<'_>, data: Vec<PyObject>) -> PyResult<Vec<QuoteTick>> {
        let ticks: Vec<QuoteTick> = data
            .into_iter()
            .map(|obj| QuoteTick::from_pyobject(obj.as_ref(py)))
            .collect::<PyResult<Vec<QuoteTick>>>()?;

        // Validate monotonically increasing
        if !is_monotonically_increasing_by_init(&ticks) {
            return Err(PyValueError::new_err(ERROR_MONOTONICITY));
        }

        Ok(ticks)
    }

    /// Transforms the given `data` Python objects into a vector of [`TradeTick`] objects.
    fn pyobjects_to_trade_ticks(py: Python<'_>, data: Vec<PyObject>) -> PyResult<Vec<TradeTick>> {
        let ticks: Vec<TradeTick> = data
            .into_iter()
            .map(|obj| TradeTick::from_pyobject(obj.as_ref(py)))
            .collect::<PyResult<Vec<TradeTick>>>()?;

        // Validate monotonically increasing
        if !is_monotonically_increasing_by_init(&ticks) {
            return Err(PyValueError::new_err(ERROR_MONOTONICITY));
        }

        Ok(ticks)
    }

    /// Transforms the given `data` Python objects into a vector of [`Bar`] objects.
    fn pyobjects_to_bars(py: Python<'_>, data: Vec<PyObject>) -> PyResult<Vec<Bar>> {
        let bars: Vec<Bar> = data
            .into_iter()
            .map(|obj| Bar::from_pyobject(obj.as_ref(py)))
            .collect::<PyResult<Vec<Bar>>>()?;

        // Validate monotonically increasing
        if !is_monotonically_increasing_by_init(&bars) {
            return Err(PyValueError::new_err(ERROR_MONOTONICITY));
        }

        Ok(bars)
    }

    /// Transforms the given record `batches` into Python `bytes`.
    fn record_batch_to_pybytes(
        py: Python<'_>,
        batch: RecordBatch,
        schema: Schema,
    ) -> PyResult<Py<PyBytes>> {
        // Create a cursor to write to a byte array in memory
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = StreamWriter::try_new(&mut cursor, &schema)
                .map_err(|err| PyRuntimeError::new_err(format!("{err}")))?;

            writer
                .write(&batch)
                .map_err(|err| PyRuntimeError::new_err(format!("{err}")))?;

            writer
                .finish()
                .map_err(|err| PyRuntimeError::new_err(format!("{err}")))?;
        }

        let buffer = cursor.into_inner();
        let pybytes = PyBytes::new(py, &buffer);

        Ok(pybytes.into())
    }
}

#[pymethods]
impl DataTransformer {
    #[staticmethod]
    pub fn get_schema_map(py: Python<'_>, cls: &PyType) -> PyResult<Py<PyDict>> {
        let cls_str: &str = cls.getattr("__name__")?.extract()?;
        let result_map = match cls_str {
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

        Ok(result_map.into_py_dict(py).into())
    }

    /// Return Python `bytes` from the given list of 'legacy' data objects, which can be passed
    /// to `pa.ipc.open_stream` to create a `RecordBatchReader`.
    #[staticmethod]
    pub fn pyobjects_to_record_batch_bytes(
        py: Python<'_>,
        data: Vec<PyObject>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(to_pyvalue_err(ERROR_EMPTY_DATA));
        }

        let data_type: String = data
            .first()
            .unwrap() // SAFETY: Unwrap safe as already checked that `data` not empty
            .as_ref(py)
            .getattr("__class__")?
            .getattr("__name__")?
            .extract()?;

        match data_type.as_str() {
            stringify!(OrderBookDelta) => {
                let deltas = Self::pyobjects_to_order_book_deltas(py, data)?;
                Self::pyo3_order_book_deltas_to_record_batch_bytes(py, deltas)
            }
            stringify!(QuoteTick) => {
                let quotes = Self::pyobjects_to_quote_ticks(py, data)?;
                Self::pyo3_quote_ticks_to_record_batch_bytes(py, quotes)
            }
            stringify!(TradeTick) => {
                let trades = Self::pyobjects_to_trade_ticks(py, data)?;
                Self::pyo3_trade_ticks_to_record_batch_bytes(py, trades)
            }
            stringify!(Bar) => {
                let bars = Self::pyobjects_to_bars(py, data)?;
                Self::pyo3_bars_to_record_batch_bytes(py, bars)
            }
            _ => Err(PyValueError::new_err(format!(
                "unsupported data type: {data_type}"
            ))),
        }
    }

    #[staticmethod]
    pub fn pyo3_order_book_deltas_to_record_batch_bytes(
        py: Python<'_>,
        data: Vec<OrderBookDelta>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        // SAFETY: Unwrap safe as already checked that `data` not empty
        let first = data.first().unwrap();
        let mut price_precision = first.order.price.precision;
        let mut size_precision = first.order.size.precision;

        // Check if price and size precision are both zero
        if price_precision == 0 && size_precision == 0 {
            // If both are zero, try the second delta if available
            if data.len() > 1 {
                let second = &data[1];
                price_precision = second.order.price.precision;
                size_precision = second.order.size.precision;
            } else {
                // If there is no second delta, use zero precision
                price_precision = 0;
                size_precision = 0;
            }
        }

        let metadata =
            OrderBookDelta::get_metadata(&first.instrument_id, price_precision, size_precision);

        let result: Result<RecordBatch, ArrowError> =
            OrderBookDelta::encode_batch(&metadata, &data);

        match result {
            Ok(batch) => {
                let schema = OrderBookDelta::get_schema(Some(metadata));
                Self::record_batch_to_pybytes(py, batch, schema)
            }
            Err(e) => Err(to_pyvalue_err(e)),
        }
    }

    #[staticmethod]
    pub fn pyo3_order_book_depth10_to_record_batch_bytes(
        py: Python<'_>,
        data: Vec<OrderBookDepth10>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        // SAFETY: Unwrap safe as already checked that `data` not empty
        let first = data.first().unwrap();
        let metadata = OrderBookDepth10::get_metadata(
            &first.instrument_id,
            first.bids[0].price.precision,
            first.bids[0].size.precision,
        );

        let result: Result<RecordBatch, ArrowError> =
            OrderBookDepth10::encode_batch(&metadata, &data);

        match result {
            Ok(batch) => {
                let schema = OrderBookDepth10::get_schema(Some(metadata));
                Self::record_batch_to_pybytes(py, batch, schema)
            }
            Err(e) => Err(to_pyvalue_err(e)),
        }
    }

    #[staticmethod]
    pub fn pyo3_quote_ticks_to_record_batch_bytes(
        py: Python<'_>,
        data: Vec<QuoteTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(to_pyvalue_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        // SAFETY: Unwrap safe as already checked that `data` not empty
        let first = data.first().unwrap();
        let metadata = QuoteTick::get_metadata(
            &first.instrument_id,
            first.bid_price.precision,
            first.bid_size.precision,
        );

        let result: Result<RecordBatch, ArrowError> = QuoteTick::encode_batch(&metadata, &data);

        match result {
            Ok(batch) => {
                let schema = QuoteTick::get_schema(Some(metadata));
                Self::record_batch_to_pybytes(py, batch, schema)
            }
            Err(e) => Err(to_pyvalue_err(e)),
        }
    }

    #[staticmethod]
    pub fn pyo3_trade_ticks_to_record_batch_bytes(
        py: Python<'_>,
        data: Vec<TradeTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(to_pyvalue_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        // SAFETY: Unwrap safe as already checked that `data` not empty
        let first = data.first().unwrap();
        let metadata = TradeTick::get_metadata(
            &first.instrument_id,
            first.price.precision,
            first.size.precision,
        );

        let result: Result<RecordBatch, ArrowError> = TradeTick::encode_batch(&metadata, &data);

        match result {
            Ok(batch) => {
                let schema = TradeTick::get_schema(Some(metadata));
                Self::record_batch_to_pybytes(py, batch, schema)
            }
            Err(e) => Err(to_pyvalue_err(e)),
        }
    }

    #[staticmethod]
    pub fn pyo3_bars_to_record_batch_bytes(
        py: Python<'_>,
        data: Vec<Bar>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(to_pyvalue_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        // SAFETY: Unwrap safe as already checked that `data` not empty
        let first = data.first().unwrap();
        let metadata = Bar::get_metadata(
            &first.bar_type,
            first.open.precision,
            first.volume.precision,
        );

        let result: Result<RecordBatch, ArrowError> = Bar::encode_batch(&metadata, &data);

        match result {
            Ok(batch) => {
                let schema = Bar::get_schema(Some(metadata));
                Self::record_batch_to_pybytes(py, batch, schema)
            }
            Err(e) => Err(to_pyvalue_err(e)),
        }
    }
}
