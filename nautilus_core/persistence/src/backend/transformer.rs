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

use datafusion::{
    arrow::{datatypes::SchemaRef, ipc::writer::StreamWriter, record_batch::RecordBatch},
    error::Result,
};
use nautilus_model::data::{bar::Bar, delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict},
};

use crate::parquet::{ArrowSchemaProvider, EncodeToRecordBatch};

#[pyclass]
pub struct DataTransformer {}

impl DataTransformer {
    fn record_batches_to_pybytes(
        py: Python<'_>,
        batches: Vec<RecordBatch>,
        schema: SchemaRef,
    ) -> PyResult<Py<PyBytes>> {
        // Create a cursor to write to a byte array in memory
        let mut cursor = Cursor::new(Vec::new());

        {
            let mut writer = StreamWriter::try_new(&mut cursor, &schema)
                .map_err(|err| PyErr::new::<PyRuntimeError, _>(format!("{}", err)))?;
            for batch in batches {
                writer
                    .write(&batch)
                    .map_err(|err| PyErr::new::<PyRuntimeError, _>(format!("{}", err)))?;
            }

            writer
                .finish()
                .map_err(|err| PyErr::new::<PyRuntimeError, _>(format!("{}", err)))?;
        }

        let buffer = cursor.into_inner();
        let pybytes = PyBytes::new(py, &buffer);

        Ok(pybytes.into())
    }
}

#[pymethods]
impl DataTransformer {
    #[staticmethod]
    pub fn pyobjects_to_batches_bytes(
        py: Python<'_>,
        data: Vec<PyObject>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
        }

        let mut data_dicts: Vec<Py<PyDict>> = vec![];
        for obj in data.into_iter() {
            let dict: Py<PyDict> = obj
                .call_method1(py, "to_dict", (obj.clone(),))?
                .extract(py)?;
            data_dicts.push(dict);
        }

        let data_type: String = data_dicts
            .first()
            .ok_or_else(|| PyErr::new::<PyValueError, _>("Data vector was empty."))?
            .as_ref(py)
            .get_item("type")
            .ok_or_else(|| PyErr::new::<PyValueError, _>("'type' key not found in dict."))?
            .extract()?;

        match data_type.as_str() {
            stringify!(QuoteTick) => {
                let ticks: Result<Vec<QuoteTick>, _> = data_dicts
                    .into_iter()
                    .map(|dict| QuoteTick::from_dict(dict.as_ref(py)))
                    .collect();

                let ticks = ticks.map_err(|_| {
                    PyErr::new::<PyValueError, _>("Error converting dicts to QuoteTick objects.")
                })?;

                DataTransformer::pyo3_quote_ticks_to_batches_bytes(py, ticks)
            }
            stringify!(TradeTick) => {
                let ticks: Result<Vec<TradeTick>, _> = data_dicts
                    .into_iter()
                    .map(|dict| TradeTick::from_dict(dict.as_ref(py)))
                    .collect();

                let ticks = ticks.map_err(|_| {
                    PyErr::new::<PyValueError, _>("Error converting dicts to TradeTick objects.")
                })?;

                DataTransformer::pyo3_trade_ticks_to_batches_bytes(py, ticks)
            }
            _ => Err(PyErr::new::<PyValueError, _>(format!(
                "Unsupported data type: {}",
                data_type
            ))),
        }
    }

    #[staticmethod]
    pub fn pyo3_order_book_deltas_to_batches_bytes(
        py: Python<'_>,
        data: Vec<OrderBookDelta>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
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

        DataTransformer::record_batches_to_pybytes(py, batches, schema)
    }

    #[staticmethod]
    pub fn pyo3_quote_ticks_to_batches_bytes(
        py: Python<'_>,
        data: Vec<QuoteTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
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

        DataTransformer::record_batches_to_pybytes(py, batches, schema)
    }

    #[staticmethod]
    pub fn pyo3_trade_ticks_to_batches_bytes(
        py: Python<'_>,
        data: Vec<TradeTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
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

        DataTransformer::record_batches_to_pybytes(py, batches, schema)
    }

    #[staticmethod]
    pub fn pyo3_bars_to_batches_bytes(py: Python<'_>, data: Vec<Bar>) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
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

        DataTransformer::record_batches_to_pybytes(py, batches, schema)
    }
}
