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
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick,
        TradeTick,
    },
    python::data::data_to_pyobject,
};
use nautilus_serialization::arrow::{ArrowSchemaProvider, custom::CustomDataDecoder};
use pyo3::{IntoPyObjectExt, prelude::*};

use super::conversion::catalog_data_type_from_py;
use crate::backend::session::{DataBackendSession, DataQueryResult, QueryError};

/// Wrapper to pass a raw pointer across the GIL release boundary.
struct SendPtr<T>(*mut T);

// SAFETY: Access is serialized by the calling `PyRefMut`
unsafe impl<T> Send for SendPtr<T> {}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataBackendSession {
    /// Provides a DataFusion session for registering and querying catalog table sources.
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
        data_type: &Bound<'_, PyAny>,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> PyResult<()> {
        let _guard = slf.runtime.enter();
        let data_type = catalog_data_type_from_py(data_type)?;

        slf.add_file_for_data_type(&data_type, table_name, file_path, sql_query)
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

        let data_type = NautilusDataType::Custom {
            type_name: type_name.to_string(),
        };

        slf.add_file_for_data_type(&data_type, table_name, file_path, sql_query)
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

impl DataBackendSession {
    fn add_file_for_data_type(
        &mut self,
        data_type: &NautilusDataType,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> PyResult<()> {
        match data_type {
            NautilusDataType::OrderBookDelta => self
                .add_file::<OrderBookDelta>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::OrderBookDepth => self
                .add_file::<OrderBookDepth>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::QuoteTick => self
                .add_file::<QuoteTick>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::TradeTick => self
                .add_file::<TradeTick>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::Bar => self
                .add_file::<Bar>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::MarkPriceUpdate => self
                .add_file::<MarkPriceUpdate>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::IndexPriceUpdate => self
                .add_file::<IndexPriceUpdate>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::FundingRateUpdate => self
                .add_file::<FundingRateUpdate>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::OptionGreeks => self
                .add_file::<OptionGreeks>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::InstrumentStatus => self
                .add_file::<InstrumentStatus>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::InstrumentClose => self
                .add_file::<InstrumentClose>(table_name, file_path, sql_query, None)
                .map_err(to_pyruntime_err),
            NautilusDataType::Custom { type_name } => {
                let mut metadata = HashMap::new();
                metadata.insert("type_name".to_string(), type_name.clone());
                let base_schema = CustomDataDecoder::get_schema(Some(metadata));
                base_schema.field_with_name("ts_init").map_err(|_| {
                    to_pyruntime_err(format!(
                        "custom data type '{type_name}' is not registered with an Arrow schema containing ts_init"
                    ))
                })?;

                // Use schemaless registration so DataFusion preserves the parquet file's
                // schema metadata (e.g. `bar_type`) on output batches, since the
                // explicit-schema variant strips per-batch metadata that decoders rely on.
                self.add_file::<CustomDataDecoder>(
                    table_name,
                    file_path,
                    sql_query,
                    Some(type_name.as_str()),
                )
                .map_err(to_pyruntime_err)
            }
            NautilusDataType::Instrument => Err(to_pyvalue_err(format!(
                "DataBackendSession does not support data type {data_type}"
            ))),
            #[cfg(feature = "defi")]
            NautilusDataType::Defi => Err(to_pyvalue_err(format!(
                "DataBackendSession does not support data type {data_type}"
            ))),
        }
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataQueryResult {
    /// Collects the remaining query records as native Python objects.
    ///
    /// # Errors
    ///
    /// Returns an error if a query stream fails or a record batch cannot be decoded.
    #[pyo3(name = "to_list")]
    fn py_to_list(mut slf: PyRefMut<'_, Self>) -> PyResult<Vec<Py<PyAny>>> {
        let py = slf.py();
        let ptr = SendPtr(&raw mut *slf);

        // SAFETY: `PyRefMut` guarantees exclusive access to the underlying query result for the
        // duration of this method call. As with `__next__`, release the GIL while waiting for
        // decoder workers that may need to acquire it for custom data.
        let data = unsafe {
            py.detach(move || -> Result<Vec<_>, QueryError> {
                let p = ptr;
                let result = &mut *p.0;
                let mut data = Vec::new();

                for chunk in result.by_ref() {
                    let chunk = chunk?;

                    if chunk.is_empty() {
                        break;
                    }
                    data.extend(chunk);
                }

                Ok(data)
            })
        }
        .map_err(to_pyruntime_err)?;

        data.into_iter()
            .map(|item| data_to_pyobject(py, item))
            .collect()
    }

    /// The reader implements an iterator.
    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    ///
    /// # Errors
    ///
    /// Returns an error if a query stream fails or a record batch cannot be decoded.
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
            Some(Ok(acc)) if !acc.is_empty() => {
                let objects: Vec<Py<PyAny>> = acc
                    .into_iter()
                    .map(|item| data_to_pyobject(py, item))
                    .collect::<PyResult<_>>()?;
                Ok(Some(objects.into_py_any(py)?))
            }
            Some(Err(e)) => Err(to_pyruntime_err(e)),
            _ => Ok(None),
        }
    }
}
