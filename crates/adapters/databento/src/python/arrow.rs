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

use std::io::Cursor;

use arrow::ipc::reader::StreamReader;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_serialization::{
    arrow::ArrowSchemaProvider, python::arrow::arrow_record_batch_to_pybytes,
};
use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyBytes, PyType},
};

use crate::{
    arrow::{
        imbalance::{decode_imbalance_batch, imbalance_to_arrow_record_batch},
        statistics::{decode_statistics_batch, statistics_to_arrow_record_batch},
    },
    types::{DatabentoImbalance, DatabentoStatistics},
};

/// Returns a mapping from field names to Arrow data types for the given Databento data class.
///
/// # Errors
///
/// Returns a `PyErr` if the class name is not recognized.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.databento")]
pub fn get_databento_arrow_schema_map(
    py: Python<'_>,
    cls: &Bound<'_, PyType>,
) -> PyResult<Py<PyAny>> {
    let cls_str: String = cls.getattr("__name__")?.extract()?;
    let result_map = match cls_str.as_str() {
        stringify!(DatabentoImbalance) => DatabentoImbalance::get_schema_map(),
        stringify!(DatabentoStatistics) => DatabentoStatistics::get_schema_map(),
        _ => {
            return Err(to_pyvalue_err(format!(
                "Arrow schema for `{cls_str}` is not currently implemented"
            )));
        }
    };

    result_map.into_py_any(py)
}

/// Encodes a list of `DatabentoImbalance` into Arrow IPC bytes.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "databento_imbalance_to_arrow_record_batch_bytes")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_databento_imbalance_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<DatabentoImbalance>,
) -> PyResult<Py<PyBytes>> {
    match imbalance_to_arrow_record_batch(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Decodes Arrow IPC bytes into a list of `DatabentoImbalance`.
///
/// # Errors
///
/// Returns a `PyErr` if decoding fails.
#[pyfunction(name = "databento_imbalance_from_arrow_record_batch_bytes")]
pub fn py_databento_imbalance_from_arrow_record_batch_bytes(
    _py: Python,
    data: Vec<u8>,
) -> PyResult<Vec<DatabentoImbalance>> {
    let cursor = Cursor::new(data);
    let reader = StreamReader::try_new(cursor, None).map_err(to_pyruntime_err)?;

    let mut results = Vec::new();

    for batch_result in reader {
        let batch = batch_result.map_err(to_pyruntime_err)?;
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_imbalance_batch(&metadata, &batch).map_err(to_pyvalue_err)?;
        results.extend(decoded);
    }

    Ok(results)
}

/// Encodes a list of `DatabentoStatistics` into Arrow IPC bytes.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction(name = "databento_statistics_to_arrow_record_batch_bytes")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_databento_statistics_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<DatabentoStatistics>,
) -> PyResult<Py<PyBytes>> {
    match statistics_to_arrow_record_batch(&data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, &batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Decodes Arrow IPC bytes into a list of `DatabentoStatistics`.
///
/// # Errors
///
/// Returns a `PyErr` if decoding fails.
#[pyfunction(name = "databento_statistics_from_arrow_record_batch_bytes")]
pub fn py_databento_statistics_from_arrow_record_batch_bytes(
    _py: Python,
    data: Vec<u8>,
) -> PyResult<Vec<DatabentoStatistics>> {
    let cursor = Cursor::new(data);
    let reader = StreamReader::try_new(cursor, None).map_err(to_pyruntime_err)?;

    let mut results = Vec::new();

    for batch_result in reader {
        let batch = batch_result.map_err(to_pyruntime_err)?;
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_statistics_batch(&metadata, &batch).map_err(to_pyvalue_err)?;
        results.extend(decoded);
    }

    Ok(results)
}
