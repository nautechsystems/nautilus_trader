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

use nautilus_core::{
    ffi::cvec::CVec,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err},
};
use nautilus_model::data::{
    Bar, Data, DataFFI, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDepth10,
    QuoteTick, TradeTick,
};
use nautilus_serialization::arrow::custom::CustomDataDecoder;
use pyo3::{prelude::*, types::PyCapsule};

use crate::backend::session::{DataBackendSession, DataQueryResult};

/// Wrapper to pass a raw pointer across the GIL release boundary.
struct SendPtr<T>(*mut T);

// SAFETY: Access is serialized by the calling `PyRefMut`
unsafe impl<T> Send for SendPtr<T> {}

/// Converts a `Data` variant into a Python object via PyO3.
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

#[repr(C)]
#[pyclass(frozen, eq, eq_int, from_py_object)]
#[pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.persistence")]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum NautilusDataType {
    // Custom = 0,  # First slot reserved for custom data
    OrderBookDelta = 1,
    OrderBookDepth10 = 2,
    QuoteTick = 3,
    TradeTick = 4,
    Bar = 5,
    MarkPriceUpdate = 6,
    InstrumentStatus = 7,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl NautilusDataType {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataBackendSession {
    #[new]
    #[pyo3(signature=(chunk_size=10_000))]
    fn new_session(chunk_size: usize) -> Self {
        Self::new(chunk_size)
    }

    /// Registers a Parquet file and adds a batch stream for decoding.
    ///
    /// The caller must specify `T` to indicate the kind of data expected. `table_name` is
    /// the logical name for queries; `file_path` is the Parquet path; `sql_query` defaults
    /// to `SELECT * FROM {table_name} ORDER BY ts_init` if `None`.
    ///
    /// When `custom_type_name` is `Some`, it is merged into each batch's schema metadata
    /// before decoding (as `type_name`). Use this for custom data when Parquet/DataFusion
    /// does not preserve schema metadata so the decoder can look up the type in the registry.
    ///
    /// The file data must be ordered by the `ts_init` in ascending order for this
    /// to work correctly.
    #[pyo3(name = "add_file")]
    #[pyo3(signature = (data_type, table_name, file_path, sql_query=None))]
    fn py_add_file(
        mut slf: PyRefMut<'_, Self>,
        data_type: NautilusDataType,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> PyResult<()> {
        let _guard = slf.runtime.enter();

        match data_type {
            NautilusDataType::OrderBookDelta => slf
                .add_file::<OrderBookDelta>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::OrderBookDepth10 => slf
                .add_file::<OrderBookDepth10>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::QuoteTick => slf
                .add_file::<QuoteTick>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::TradeTick => slf
                .add_file::<TradeTick>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::Bar => slf
                .add_file::<Bar>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::MarkPriceUpdate => slf
                .add_file::<MarkPriceUpdate>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::InstrumentStatus => slf
                .add_file::<InstrumentStatus>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
        }
    }

    /// Registers a Parquet file for a custom data type identified by `type_name`.
    ///
    /// The custom data type must have been registered via
    /// `ensure_custom_data_registered::<T>()` before calling this method.
    #[pyo3(name = "add_custom_file")]
    #[pyo3(signature = (type_name, table_name, file_path, sql_query=None))]
    fn py_add_custom_file(
        mut slf: PyRefMut<'_, Self>,
        type_name: &str,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> PyResult<()> {
        let _guard = slf.runtime.enter();
        slf.add_file::<CustomDataDecoder>(table_name, file_path, sql_query, Some(type_name))
            .map_err(to_pyruntime_err)
    }

    fn to_query_result(mut slf: PyRefMut<'_, Self>) -> DataQueryResult {
        let py = slf.py();
        let chunk_size = slf.chunk_size;
        let ptr = SendPtr(&raw mut *slf);

        // SAFETY: see comment on `__next__` for the safety argument.
        // The GIL release is needed here because `get_query_result` eagerly
        // pulls the first element from each stream (via `KMerge::push_iter`),
        // which blocks on the tokio channel while workers may need the GIL.
        let query_result = unsafe {
            py.detach(move || {
                let p = ptr;
                (*p.0).get_query_result()
            })
        };

        DataQueryResult::new(query_result, chunk_size)
    }

    /// Register an object store with the session context from a URI with optional storage options
    #[pyo3(name = "register_object_store_from_uri")]
    #[pyo3(signature = (uri, storage_options=None))]
    fn py_register_object_store_from_uri(
        mut slf: PyRefMut<'_, Self>,
        uri: &str,
        storage_options: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        // Convert HashMap to AHashMap for internal use
        let storage_options = storage_options.map(|m| m.into_iter().collect());
        slf.register_object_store_from_uri(uri, storage_options)
            .map_err(to_pyruntime_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataQueryResult {
    /// The reader implements an iterator.
    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    ///
    /// For built-in types, returns a PyCapsule containing a CVec of DataFFI (C layout)
    /// consumed by Cython `capsule_to_list`. For custom data types (which are not
    /// FFI-safe), returns a Python list of PyO3 objects directly.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<Py<PyAny>>> {
        let py = slf.py();
        let ptr = SendPtr(&raw mut *slf);

        // SAFETY: `PyRefMut` guarantees exclusive access to the underlying
        // object for the duration of this method call. The runtime borrow
        // flag prevents any other Python thread from accessing it.
        //
        // The GIL must be released here so that tokio worker threads can
        // acquire it when decoding custom data types via `Python::attach`.
        // Without this, custom-type streaming deadlocks: the main thread
        // holds the GIL while blocking on `recv`, and workers block on
        // `Python::attach` waiting for the GIL.
        let acc = unsafe {
            py.detach(move || {
                let p = ptr;
                (*p.0).next()
            })
        };

        match acc {
            Some(acc) if !acc.is_empty() => {
                let has_non_ffi = acc
                    .iter()
                    .any(|d| matches!(d, Data::Custom(_) | Data::InstrumentStatus(_)));

                if has_non_ffi {
                    // Custom and instrument-status data: convert directly to Python objects (bypasses FFI)
                    let objects: Vec<Py<PyAny>> = acc
                        .into_iter()
                        .map(|item| data_to_pyobject(py, item))
                        .collect::<PyResult<_>>()?;
                    Ok(Some(objects.into_py_any_unwrap(py)))
                } else {
                    // Built-in types: FFI capsule path
                    let ffi_data: Vec<DataFFI> = acc
                        .into_iter()
                        .map(DataFFI::try_from)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(to_pyruntime_err)?;
                    let cvec: CVec = ffi_data.into();
                    match PyCapsule::new_with_destructor::<CVec, _>(py, cvec, None, |_, _| {}) {
                        Ok(capsule) => Ok(Some(capsule.into_py_any_unwrap(py))),
                        Err(e) => Err(to_pyruntime_err(e)),
                    }
                }
            }
            _ => Ok(None),
        }
    }
}
