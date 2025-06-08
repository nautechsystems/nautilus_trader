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

use nautilus_core::{
    ffi::cvec::CVec,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err},
};
use nautilus_model::data::{
    Bar, MarkPriceUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
};
use pyo3::{prelude::*, types::PyCapsule};
use url::Url;

use crate::backend::session::{DataBackendSession, DataQueryResult};

#[repr(C)]
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NautilusDataType {
    // Custom = 0,  # First slot reserved for custom data
    OrderBookDelta = 1,
    OrderBookDepth10 = 2,
    QuoteTick = 3,
    TradeTick = 4,
    Bar = 5,
    MarkPriceUpdate = 6,
}

#[pymethods]
impl DataBackendSession {
    #[new]
    #[pyo3(signature=(chunk_size=10_000))]
    fn new_session(chunk_size: usize) -> Self {
        Self::new(chunk_size)
    }

    /// Query a file for its records. the caller must specify `T` to indicate
    /// the kind of data expected from this query.
    ///
    /// `table_name`: Logical `table_name` assigned to this file. Queries to this file should address the
    /// file by its table name.
    /// `file_path`: Path to file
    /// `sql_query`: A custom sql query to retrieve records from file. If no query is provided a default
    /// query "SELECT * FROM <`table_name`>" is run.
    ///
    /// # Safety
    ///
    /// The file data must be ordered by the `ts_init` in ascending order for this
    /// to work correctly.
    #[pyo3(name = "add_file")]
    #[pyo3(signature = (data_type, table_name, file_path, sql_query=None))]
    fn add_file_py(
        mut slf: PyRefMut<'_, Self>,
        data_type: NautilusDataType,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> PyResult<()> {
        let _guard = slf.runtime.enter();

        match data_type {
            NautilusDataType::OrderBookDelta => slf
                .add_file::<OrderBookDelta>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::OrderBookDepth10 => slf
                .add_file::<OrderBookDepth10>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::QuoteTick => slf
                .add_file::<QuoteTick>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::TradeTick => slf
                .add_file::<TradeTick>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::Bar => slf
                .add_file::<Bar>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::MarkPriceUpdate => slf
                .add_file::<MarkPriceUpdate>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
        }
    }

    fn to_query_result(mut slf: PyRefMut<'_, Self>) -> DataQueryResult {
        let query_result = slf.get_query_result();
        DataQueryResult::new(query_result, slf.chunk_size)
    }

    /// Register an object store with the session context from a URI
    #[pyo3(name = "register_object_store_from_uri")]
    fn register_object_store_from_uri_py(mut slf: PyRefMut<'_, Self>, uri: &str) -> PyResult<()> {
        // Create object store from URI using the Rust implementation
        let (object_store, _, _) =
            crate::parquet::create_object_store_from_path(uri).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "Failed to create object store: {e}"
                ))
            })?;

        // Parse the URI to get the base URL for registration
        let parsed_uri = Url::parse(uri)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid URI: {e}")))?;

        // Register the object store with the session
        if matches!(
            parsed_uri.scheme(),
            "s3" | "gs" | "gcs" | "azure" | "abfs" | "http" | "https"
        ) {
            // For cloud storage, register with the base URL (scheme + netloc)
            let base_url = format!(
                "{}://{}",
                parsed_uri.scheme(),
                parsed_uri.host_str().unwrap_or("")
            );
            let base_parsed_url = Url::parse(&base_url).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Invalid base URL: {e}"))
            })?;
            slf.register_object_store(&base_parsed_url, object_store);
        }

        Ok(())
    }
}

#[pymethods]
impl DataQueryResult {
    /// The reader implements an iterator.
    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<PyObject>> {
        match slf.next() {
            Some(acc) if !acc.is_empty() => {
                let cvec = slf.set_chunk(acc);
                Python::with_gil(|py| match PyCapsule::new::<CVec>(py, cvec, None) {
                    Ok(capsule) => Ok(Some(capsule.into_py_any_unwrap(py))),
                    Err(e) => Err(to_pyruntime_err(e)),
                })
            }
            _ => Ok(None),
        }
    }
}
