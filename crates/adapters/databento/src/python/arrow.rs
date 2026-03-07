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

//! Python bindings for Databento Arrow serialization.

use std::io::Cursor;

use arrow::{ipc::writer::StreamWriter, record_batch::RecordBatch};
use nautilus_core::python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err};
use nautilus_serialization::arrow::ArrowSchemaProvider;
use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyBytes, PyType},
};

use crate::{
    arrow::{imbalance_to_arrow_record_batch_bytes, statistics_to_arrow_record_batch_bytes},
    types::{DatabentoImbalance, DatabentoStatistics},
};

/// Transforms the given record `batch` into Python `bytes`.
fn arrow_record_batch_to_pybytes(py: Python, batch: RecordBatch) -> PyResult<Py<PyBytes>> {
    // Create a cursor to write to a byte array in memory
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer =
            StreamWriter::try_new(&mut cursor, &batch.schema()).map_err(to_pyruntime_err)?;

        writer.write(&batch).map_err(to_pyruntime_err)?;

        writer.finish().map_err(to_pyruntime_err)?;
    }

    let buffer = cursor.into_inner();
    let pybytes = PyBytes::new(py, &buffer);

    Ok(pybytes.into())
}

/// Returns a mapping from field names to Arrow data types for Databento types.
///
/// # Errors
///
/// Returns a `PyErr` if the class name is not recognized or schema extraction fails.
#[pyfunction]
pub fn get_databento_arrow_schema_map(
    py: Python<'_>,
    cls: &Bound<'_, PyType>,
) -> PyResult<Py<PyAny>> {
    let cls_str: String = cls.getattr("__name__")?.extract()?;
    let result_map = match cls_str.as_str() {
        stringify!(DatabentoStatistics) => DatabentoStatistics::get_schema_map(),
        stringify!(DatabentoImbalance) => DatabentoImbalance::get_schema_map(),
        _ => {
            return Err(to_pytype_err(format!(
                "Arrow schema for `{cls_str}` is not currently implemented for Databento types."
            )));
        }
    };

    result_map.into_py_any(py)
}

/// Converts a list of `DatabentoStatistics` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction]
pub fn py_databento_statistics_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<DatabentoStatistics>,
) -> PyResult<Py<PyBytes>> {
    match statistics_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}

/// Converts a list of `DatabentoImbalance` into Arrow IPC bytes for Python.
///
/// # Errors
///
/// Returns a `PyErr` if encoding fails.
#[pyfunction]
pub fn py_databento_imbalance_to_arrow_record_batch_bytes(
    py: Python,
    data: Vec<DatabentoImbalance>,
) -> PyResult<Py<PyBytes>> {
    match imbalance_to_arrow_record_batch_bytes(data) {
        Ok(batch) => arrow_record_batch_to_pybytes(py, batch),
        Err(e) => Err(to_pyvalue_err(e)),
    }
}
