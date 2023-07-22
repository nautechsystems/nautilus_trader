// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    datatypes::SchemaRef, ipc::writer::StreamWriter, record_batch::RecordBatch,
};
use nautilus_model::data::{bar::Bar, delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick};
use pyo3::{
    exceptions::{PyKeyError, PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict},
};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

const ERROR_EMPTY_DATA: &str = "`data` was empty";

#[pyclass]
pub struct DataTransformer {}

impl DataTransformer {
    /// Transforms the given `data_dicts` into a vector of [`OrderBookDelta`] objects.
    fn pydicts_to_order_book_deltas(
        py: Python<'_>,
        data_dicts: Vec<Py<PyDict>>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        let deltas: Vec<OrderBookDelta> = data_dicts
            .into_iter()
            .map(|dict| OrderBookDelta::from_dict(py, dict))
            .collect::<PyResult<Vec<OrderBookDelta>>>()?;

        Ok(deltas)
    }

    /// Transforms the given `data_dicts` into a vector of [`QuoteTick`] objects.
    fn pydicts_to_quote_ticks(
        py: Python<'_>,
        data_dicts: Vec<Py<PyDict>>,
    ) -> PyResult<Vec<QuoteTick>> {
        let ticks: Vec<QuoteTick> = data_dicts
            .into_iter()
            .map(|dict| QuoteTick::from_dict(py, dict))
            .collect::<PyResult<Vec<QuoteTick>>>()?;

        Ok(ticks)
    }

    /// Transforms the given `data_dicts` into a vector of [`TradeTick`] objects.
    fn pydicts_to_trade_ticks(
        py: Python<'_>,
        data_dicts: Vec<Py<PyDict>>,
    ) -> PyResult<Vec<TradeTick>> {
        let ticks: Vec<TradeTick> = data_dicts
            .into_iter()
            .map(|dict| TradeTick::from_dict(py, dict))
            .collect::<PyResult<Vec<TradeTick>>>()?;

        Ok(ticks)
    }

    /// Transforms the given `data_dicts` into a vector of [`Bar`] objects.
    fn pydicts_to_bars(py: Python<'_>, data_dicts: Vec<Py<PyDict>>) -> PyResult<Vec<Bar>> {
        let bars: Vec<Bar> = data_dicts
            .into_iter()
            .map(|dict| Bar::from_dict(py, dict))
            .collect::<PyResult<Vec<Bar>>>()?;

        Ok(bars)
    }

    /// Transforms the given `batches` into Python `bytes`.
    fn record_batches_to_pybytes(
        py: Python<'_>,
        batches: Vec<RecordBatch>,
        schema: SchemaRef,
    ) -> PyResult<Py<PyBytes>> {
        // Create a cursor to write to a byte array in memory
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = StreamWriter::try_new(&mut cursor, &schema)
                .map_err(|err| PyRuntimeError::new_err(format!("{err}")))?;
            for batch in batches {
                writer
                    .write(&batch)
                    .map_err(|err| PyRuntimeError::new_err(format!("{err}")))?;
            }

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
    /// Return Python `bytes` from the given list of 'legacy' data objects, which can be passed
    /// to `pa.ipc.open_stream` to create a `RecordBatchReader`.
    #[staticmethod]
    pub fn pyobjects_to_batches_bytes(
        py: Python<'_>,
        data: Vec<PyObject>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Iterate over all objects calling the legacy 'to_dict' method
        let mut data_dicts: Vec<Py<PyDict>> = vec![];
        for obj in data {
            let dict: Py<PyDict> = obj
                .call_method1(py, "to_dict", (obj.clone(),))?
                .extract(py)?;
            data_dicts.push(dict);
        }

        let data_type: String = data_dicts
            .first()
            .unwrap() // Safety: already checked that `data` not empty above
            .as_ref(py)
            .get_item("type")
            .ok_or_else(|| PyKeyError::new_err("'type' key not found in dict."))?
            .extract()?;

        match data_type.as_str() {
            stringify!(OrderBookDelta) => {
                let deltas = Self::pydicts_to_order_book_deltas(py, data_dicts)?;
                Self::pyo3_order_book_deltas_to_batches_bytes(py, deltas)
            }
            stringify!(QuoteTick) => {
                let ticks = Self::pydicts_to_quote_ticks(py, data_dicts)?;
                Self::pyo3_quote_ticks_to_batches_bytes(py, ticks)
            }
            stringify!(TradeTick) => {
                let ticks = Self::pydicts_to_trade_ticks(py, data_dicts)?;
                Self::pyo3_trade_ticks_to_batches_bytes(py, ticks)
            }
            stringify!(Bar) => {
                let bars = Self::pydicts_to_bars(py, data_dicts)?;
                Self::pyo3_bars_to_batches_bytes(py, bars)
            }
            _ => Err(PyValueError::new_err(format!(
                "unsupported data type: {data_type}"
            ))),
        }
    }

    #[staticmethod]
    pub fn pyo3_order_book_deltas_to_batches_bytes(
        py: Python<'_>,
        data: Vec<OrderBookDelta>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = OrderBookDelta::get_metadata(
            &first.instrument_id,
            first.order.price.precision,
            first.order.size.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|delta| OrderBookDelta::encode_batch(&metadata, &[delta]))
            .collect();

        let schema = OrderBookDelta::get_schema(metadata);
        Self::record_batches_to_pybytes(py, batches, schema)
    }

    #[staticmethod]
    pub fn pyo3_quote_ticks_to_batches_bytes(
        py: Python<'_>,
        data: Vec<QuoteTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = QuoteTick::get_metadata(
            &first.instrument_id,
            first.bid.precision,
            first.bid_size.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|quote| QuoteTick::encode_batch(&metadata, &[quote]))
            .collect();

        let schema = QuoteTick::get_schema(metadata);
        Self::record_batches_to_pybytes(py, batches, schema)
    }

    #[staticmethod]
    pub fn pyo3_trade_ticks_to_batches_bytes(
        py: Python<'_>,
        data: Vec<TradeTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = TradeTick::get_metadata(
            &first.instrument_id,
            first.price.precision,
            first.size.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|trade| TradeTick::encode_batch(&metadata, &[trade]))
            .collect();

        let schema = TradeTick::get_schema(metadata);
        Self::record_batches_to_pybytes(py, batches, schema)
    }

    #[staticmethod]
    pub fn pyo3_bars_to_batches_bytes(py: Python<'_>, data: Vec<Bar>) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyValueError::new_err(ERROR_EMPTY_DATA));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = Bar::get_metadata(
            &first.bar_type,
            first.open.precision,
            first.volume.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|bar| Bar::encode_batch(&metadata, &[bar]))
            .collect();

        let schema = TradeTick::get_schema(metadata);
        Self::record_batches_to_pybytes(py, batches, schema)
    }
}
