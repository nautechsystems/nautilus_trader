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

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{
        Bar, InstrumentStatus, MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDepth10,
        QuoteTick, TradeTick,
    },
    python::data::data_to_pyobject,
};
use nautilus_serialization::arrow::{ArrowSchemaProvider, custom::CustomDataDecoder};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::backend::session::{DataBackendSession, DataQueryResult};

/// Wrapper to pass a raw pointer across the GIL release boundary.
struct SendPtr<T>(*mut T);

// SAFETY: Access is serialized by the calling `PyRefMut`
unsafe impl<T> Send for SendPtr<T> {}

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
    OptionGreeks = 7,
    InstrumentStatus = 8,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl NautilusDataType {
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "PyO3 special methods use a borrowed receiver"
    )]
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataBackendSession {
    /// Provides a DataFusion session and registers DataFusion queries.
    ///
    /// The session is used to register data sources and make queries on them. A
    /// query returns a Chunk of Arrow records. It is decoded and converted into
    /// a Vec of data by types that implement `DecodeDataFromRecordBatch`.
    #[new]
    #[pyo3(signature=(chunk_size=10_000))]
    fn py_new(chunk_size: usize) -> PyResult<Self> {
        if chunk_size == 0 {
            return Err(to_pyvalue_err("chunk_size must be positive"));
        }

        Ok(Self::new(chunk_size))
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
    ///
    /// # Errors
    ///
    /// Returns an error if parquet registration, SQL planning, stream execution, or
    /// data decoding setup fails.
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
            NautilusDataType::OptionGreeks => slf
                .add_file::<OptionGreeks>(table_name, file_path, sql_query, None)
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
        let mut metadata = HashMap::new();
        metadata.insert("type_name".to_string(), type_name.to_string());
        let base_schema = CustomDataDecoder::get_schema(Some(metadata));
        base_schema.field_with_name("ts_init").map_err(|_| {
            to_pyruntime_err(format!(
                "custom data type '{type_name}' is not registered with an Arrow schema containing ts_init"
            ))
        })?;
        // Use schemaless registration so DataFusion preserves the parquet file's
        // schema metadata (e.g. `bar_type`) on output batches, since the
        // explicit-schema variant strips per-batch metadata that decoders rely on.
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

    /// Register an object store with the session context from a URI with optional storage options.
    ///
    /// # Errors
    ///
    /// Returns an error if the object store URI cannot be normalized or the backend
    /// cannot be created.
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
    /// Collects the remaining query records as native Python objects.
    #[pyo3(name = "to_list")]
    fn py_to_list(mut slf: PyRefMut<'_, Self>) -> PyResult<Vec<Py<PyAny>>> {
        let py = slf.py();
        let ptr = SendPtr(&raw mut *slf);

        // SAFETY: `PyRefMut` guarantees exclusive access to the underlying query result for the
        // duration of this method call. As with `__next__`, release the GIL while waiting for
        // decoder workers that may need to acquire it for custom data.
        let data = unsafe {
            py.detach(move || {
                let p = ptr;
                let result = &mut *p.0;
                let mut data = Vec::new();

                for chunk in result.by_ref() {
                    if chunk.is_empty() {
                        break;
                    }
                    data.extend(chunk);
                }

                data
            })
        };

        data.into_iter()
            .map(|item| data_to_pyobject(py, item))
            .collect()
    }

    /// The reader implements an iterator.
    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
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
                let objects: Vec<Py<PyAny>> = acc
                    .into_iter()
                    .map(|item| data_to_pyobject(py, item))
                    .collect::<PyResult<_>>()?;
                Ok(Some(objects.into_py_any(py)?))
            }
            _ => Ok(None),
        }
    }
}
