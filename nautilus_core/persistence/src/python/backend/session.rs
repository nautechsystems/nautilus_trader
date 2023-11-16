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

use nautilus_core::{ffi::cvec::CVec, python::to_pyruntime_err};
use nautilus_model::data::{
    bar::Bar, delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick, Data,
};
use pyo3::{prelude::*, types::PyCapsule};

use crate::backend::session::{DataBackendSession, DataQueryResult};

#[repr(C)]
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum NautilusDataType {
    // Custom = 0,  # First slot reserved for custom data
    OrderBookDelta = 1,
    QuoteTick = 2,
    TradeTick = 3,
    Bar = 4,
}

#[pymethods]
impl DataBackendSession {
    #[new]
    #[pyo3(signature=(chunk_size=5_000))]
    fn new_session(chunk_size: usize) -> Self {
        Self::new(chunk_size)
    }

    /// Query a file for its records. the caller must specify `T` to indicate
    /// the kind of data expected from this query.
    ///
    /// table_name: Logical table_name assigned to this file. Queries to this file should address the
    /// file by its table name.
    /// file_path: Path to file
    /// sql_query: A custom sql query to retrieve records from file. If no query is provided a default
    /// query "SELECT * FROM <table_name>" is run.
    ///
    /// # Safety
    /// The file data must be ordered by the ts_init in ascending order for this
    /// to work correctly.
    #[pyo3(name = "add_file")]
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
            NautilusDataType::QuoteTick => slf
                .add_file::<QuoteTick>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::TradeTick => slf
                .add_file::<TradeTick>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
            NautilusDataType::Bar => slf
                .add_file::<Bar>(table_name, file_path, sql_query)
                .map_err(to_pyruntime_err),
        }
    }

    fn to_query_result(mut slf: PyRefMut<'_, Self>) -> DataQueryResult {
        let query_result = slf.get_query_result();
        DataQueryResult::new(query_result, slf.chunk_size)
    }
}

#[pymethods]
impl DataQueryResult {
    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<PyObject>> {
        slf.drop_chunk();

        for _ in 0..slf.size {
            match slf.result.next() {
                Some(item) => slf.acc.push(item),
                None => break,
            }
        }

        let mut acc: Vec<Data> = Vec::new();
        std::mem::swap(&mut acc, &mut slf.acc);

        if !acc.is_empty() {
            let cvec = acc.into();
            Python::with_gil(|py| match PyCapsule::new::<CVec>(py, cvec, None) {
                Ok(capsule) => Ok(Some(capsule.into_py(py))),
                Err(err) => Err(to_pyruntime_err(err)),
            })
        } else {
            Ok(None)
        }
    }
}
