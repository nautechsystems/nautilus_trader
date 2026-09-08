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

use std::{io::Cursor, sync::Arc};

use arrow::{
    datatypes::Schema,
    ffi_stream::FFI_ArrowArrayStream,
    ipc::{reader::StreamReader, writer::StreamWriter},
    record_batch::{RecordBatch, RecordBatchIterator},
};
use nautilus_core::python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, OptionGreeks,
        OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
    },
    python::data::{
        pyobjects_to_bars, pyobjects_to_book_deltas, pyobjects_to_index_prices,
        pyobjects_to_instrument_closes, pyobjects_to_instrument_statuses, pyobjects_to_mark_prices,
        pyobjects_to_option_greeks, pyobjects_to_quotes, pyobjects_to_trades,
    },
};
use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyBytes, PyCapsule, PyType},
};

use crate::arrow::{
    ArrowSchemaProvider, DecodeFromRecordBatch, DecodeTypedFromRecordBatch,
    bars_to_arrow_record_batch_bytes, book_deltas_to_arrow_record_batch_bytes,
    book_depths_to_arrow_record_batch_bytes, index_prices_to_arrow_record_batch_bytes,
    instrument_closes_to_arrow_record_batch_bytes, instrument_status_to_arrow_record_batch_bytes,
    mark_prices_to_arrow_record_batch_bytes, option_greeks_to_arrow_record_batch_bytes,
    quotes_to_arrow_record_batch_bytes, trades_to_arrow_record_batch_bytes,
};

/// Transforms the given record `batch` into Python `bytes`.
///
/// # Errors
///
/// Returns a `PyErr` if writing the Arrow IPC stream fails.
pub fn arrow_record_batch_to_pybytes(py: Python, batch: &RecordBatch) -> PyResult<Py<PyBytes>> {
    arrow_record_batches_to_pybytes(py, &batch.schema(), std::slice::from_ref(batch))
}

/// Transforms the given record `batches` into Python `bytes` as a single Arrow IPC stream.
///
/// # Errors
///
/// Returns a `PyErr` if writing the Arrow IPC stream fails.
pub fn arrow_record_batches_to_pybytes(
    py: Python,
    schema: &Schema,
    batches: &[RecordBatch],
) -> PyResult<Py<PyBytes>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = StreamWriter::try_new(&mut cursor, schema).map_err(to_pyruntime_err)?;

        for batch in batches {
            writer.write(batch).map_err(to_pyruntime_err)?;
        }

        writer.finish().map_err(to_pyruntime_err)?;
    }

    let buffer = cursor.into_inner();
    let pybytes = PyBytes::new(py, &buffer);

    Ok(pybytes.into())
}

/// Exports the given record `batches` as an Arrow C stream PyCapsule.
///
/// # Errors
///
/// Returns a `PyErr` if creating the PyCapsule fails.
pub fn arrow_record_batches_to_pyarrow_stream(
    py: Python<'_>,
    schema: &Schema,
    batches: Vec<RecordBatch>,
) -> PyResult<Py<PyAny>> {
    let reader = RecordBatchIterator::new(batches.into_iter().map(Ok), Arc::new(schema.clone()));
    let stream = FFI_ArrowArrayStream::new(Box::new(reader));

    // The Arrow PyCapsule protocol requires this exact name for ArrowArrayStream values.
    let capsule = PyCapsule::new_with_value(py, stream, c"arrow_array_stream")?;
    Ok(capsule.into_any().unbind())
}

/// Returns a mapping from field names to Arrow data types for the given Rust data class.
///
/// # Errors
///
/// Returns a `PyErr` if the class name is not recognized or schema extraction fails.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
pub fn get_arrow_schema_map(py: Python<'_>, cls: &Bound<'_, PyType>) -> PyResult<Py<PyAny>> {
    let cls_str: String = cls.getattr("__name__")?.extract()?;
    let result_map = match cls_str.as_str() {
        stringify!(OrderBookDelta) => OrderBookDelta::get_schema_map(),
        stringify!(OrderBookDepth) | "OrderBookDepth10" => OrderBookDepth::get_schema_map(),
        stringify!(QuoteTick) => QuoteTick::get_schema_map(),
        stringify!(TradeTick) => TradeTick::get_schema_map(),
        stringify!(Bar) => Bar::get_schema_map(),
        stringify!(MarkPriceUpdate) => MarkPriceUpdate::get_schema_map(),
        stringify!(IndexPriceUpdate) => IndexPriceUpdate::get_schema_map(),
        stringify!(FundingRateUpdate) => FundingRateUpdate::get_schema_map(),
        stringify!(InstrumentStatus) => InstrumentStatus::get_schema_map(),
        stringify!(OptionGreeks) => OptionGreeks::get_schema_map(),
        stringify!(InstrumentClose) => InstrumentClose::get_schema_map(),
        _ => {
            return Err(to_pytype_err(format!(
                "Arrow schema for `{cls_str}` is not currently implemented in Rust."
            )));
        }
    };

    result_map.into_py_any(py)
}

/// Returns an Arrow IPC stream containing the Rust schema for the given data class.
///
/// # Errors
///
/// Returns a `PyErr` if the class name is not recognized or schema serialization fails.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
pub fn get_arrow_schema_bytes(py: Python<'_>, cls: &Bound<'_, PyType>) -> PyResult<Py<PyBytes>> {
    let cls_str: String = cls.getattr("__name__")?.extract()?;
    let schema = match cls_str.as_str() {
        stringify!(OrderBookDelta) => OrderBookDelta::get_schema(None),
        stringify!(OrderBookDepth) | "OrderBookDepth10" => OrderBookDepth::get_schema(None),
        stringify!(QuoteTick) => QuoteTick::get_schema(None),
        stringify!(TradeTick) => TradeTick::get_schema(None),
        stringify!(Bar) => Bar::get_schema(None),
        stringify!(MarkPriceUpdate) => MarkPriceUpdate::get_schema(None),
        stringify!(IndexPriceUpdate) => IndexPriceUpdate::get_schema(None),
        stringify!(FundingRateUpdate) => FundingRateUpdate::get_schema(None),
        stringify!(InstrumentStatus) => InstrumentStatus::get_schema(None),
        stringify!(OptionGreeks) => OptionGreeks::get_schema(None),
        stringify!(InstrumentClose) => InstrumentClose::get_schema(None),
        _ => {
            return Err(to_pytype_err(format!(
                "Arrow schema for `{cls_str}` is not currently implemented in Rust."
            )));
        }
    };

    arrow_record_batches_to_pybytes(py, &schema, &[])
}

/// Converts a vector of `OrderBookDelta` into an Arrow `RecordBatch`.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::missing_panics_doc)] // Guarded by empty check
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
        stringify!(OrderBookDepth) => {
            let depth_snapshots: Vec<OrderBookDepth> = data
                .into_iter()
                .map(|obj| obj.extract::<OrderBookDepth>().map_err(Into::into))
                .collect::<PyResult<Vec<OrderBookDepth>>>()?;
            py_book_depths_to_arrow_record_batch_bytes(py, depth_snapshots)
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
        stringify!(InstrumentStatus) => {
            let statuses = pyobjects_to_instrument_statuses(data)?;
            py_instrument_status_to_arrow_record_batch_bytes(py, statuses)
        }
        stringify!(OptionGreeks) => {
            let greeks = pyobjects_to_option_greeks(data)?;
            py_option_greeks_to_arrow_record_batch_bytes(py, greeks)
        }
        stringify!(InstrumentClose) => {
            let closes = pyobjects_to_instrument_closes(data)?;
            py_instrument_closes_to_arrow_record_batch_bytes(py, closes)
        }
        _ => Err(to_pyvalue_err(format!(
            "unsupported data type: {data_type}"
        ))),
    }
}

/// Converts a vector of `OrderBookDelta` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Instrument IDs differ, or non-clear precision metadata differs:
///   `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "book_deltas_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_book_deltas_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OrderBookDelta>,
) -> PyResult<Py<PyBytes>> {
    match book_deltas_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `OrderBookDepth` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "book_depths_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_book_depths_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OrderBookDepth>,
) -> PyResult<Py<PyBytes>> {
    match book_depths_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

#[allow(
    deprecated,
    reason = "generated code references the one-release Python compatibility alias"
)]
mod depth10_compat {
    use super::{
        OrderBookDepth, Py, PyBytes, PyResult, Python, py_book_depths_to_arrow_record_batch_bytes,
        pyfunction,
    };

    #[pyfunction(name = "book_depth10_to_arrow_record_batch_bytes")]
    #[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
    #[deprecated(note = "use book_depths_to_arrow_record_batch_bytes")]
    pub fn py_book_depth10_to_arrow_record_batch_bytes(
        py: Python,
        data: Vec<OrderBookDepth>,
    ) -> PyResult<Py<PyBytes>> {
        py_book_depths_to_arrow_record_batch_bytes(py, data)
    }
}

#[allow(
    deprecated,
    reason = "re-exports the one-release Python compatibility alias"
)]
pub use depth10_compat::py_book_depth10_to_arrow_record_batch_bytes;

/// Converts a vector of `QuoteTick` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "quotes_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_quotes_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<QuoteTick>,
) -> PyResult<Py<PyBytes>> {
    match quotes_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `TradeTick` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "trades_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_trades_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<TradeTick>,
) -> PyResult<Py<PyBytes>> {
    match trades_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `Bar` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "bars_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_bars_to_arrow_record_batch_bytes(py: Python, data: Vec<Bar>) -> PyResult<Py<PyBytes>> {
    match bars_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `MarkPriceUpdate` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "mark_prices_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_mark_prices_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<MarkPriceUpdate>,
) -> PyResult<Py<PyBytes>> {
    match mark_prices_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `IndexPriceUpdate` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "index_prices_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_index_prices_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<IndexPriceUpdate>,
) -> PyResult<Py<PyBytes>> {
    match index_prices_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `InstrumentStatus` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "instrument_status_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_instrument_status_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<InstrumentStatus>,
) -> PyResult<Py<PyBytes>> {
    match instrument_status_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a vector of `OptionGreeks` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "option_greeks_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_option_greeks_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<OptionGreeks>,
) -> PyResult<Py<PyBytes>> {
    match option_greeks_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Decodes Arrow IPC bytes into a list of `OptionGreeks`.
///
/// # Errors
///
/// Returns a `PyErr` if decoding fails.
#[pyfunction(name = "option_greeks_from_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
pub fn py_option_greeks_from_arrow_record_batch_bytes(
    _py: Python,
    data: Vec<u8>,
) -> PyResult<Vec<OptionGreeks>> {
    let cursor = Cursor::new(data);
    let reader = StreamReader::try_new(cursor, None).map_err(to_pyruntime_err)?;

    let mut results = Vec::new();

    for batch_result in reader {
        let batch = batch_result.map_err(to_pyruntime_err)?;
        let metadata = batch.schema().metadata().clone();
        let decoded = OptionGreeks::decode_batch(&metadata, batch).map_err(to_pyvalue_err)?;
        results.extend(decoded);
    }

    Ok(results)
}

/// Decodes Arrow IPC bytes into a list of `InstrumentStatus`.
///
/// # Errors
///
/// Returns a `PyErr` if decoding fails.
#[pyfunction(name = "instrument_status_from_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
pub fn py_instrument_status_from_arrow_record_batch_bytes(
    _py: Python,
    data: Vec<u8>,
) -> PyResult<Vec<InstrumentStatus>> {
    let cursor = Cursor::new(data);
    let reader = StreamReader::try_new(cursor, None).map_err(to_pyruntime_err)?;

    let mut results = Vec::new();

    for batch_result in reader {
        let batch = batch_result.map_err(to_pyruntime_err)?;
        let metadata = batch.schema().metadata().clone();
        let decoded =
            InstrumentStatus::decode_typed_batch(&metadata, batch).map_err(to_pyvalue_err)?;
        results.extend(decoded);
    }

    Ok(results)
}

/// Converts a vector of `InstrumentClose` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[pyfunction(name = "instrument_closes_to_arrow_record_batch_bytes")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.serialization")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_instrument_closes_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<InstrumentClose>,
) -> PyResult<Py<PyBytes>> {
    match instrument_closes_to_arrow_record_batch_bytes(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}
