// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use databento::{self};
use nautilus_core::python::to_pyvalue_err;
use pyo3::{exceptions::PyException, prelude::*, types::PyDict};

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoHistoricalClient {
    key: String,
}

#[pymethods]
impl DatabentoHistoricalClient {
    #[new]
    pub fn py_new(key: String) -> PyResult<Self> {
        Ok(Self { key })
    }

    #[pyo3(name = "get_dataset_range")]
    fn py_get_dataset_range<'py>(&self, py: Python<'py>, dataset: &str) -> PyResult<&'py PyAny> {
        let dataset_clone = dataset.to_string();

        // TODO: Cheaper way of accessing client as mutable `Send` (Arc<Mutex alone doesn't work)
        let mut client = databento::HistoricalClient::builder()
            .key(self.key.clone())
            .map_err(to_pyvalue_err)?
            .build()
            .map_err(to_pyvalue_err)?;

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let response = client.metadata().get_dataset_range(&dataset_clone).await;

            match response {
                Ok(res) => Python::with_gil(|py| {
                    let dict = PyDict::new(py);
                    dict.set_item("start_date", res.start_date.to_string())?;
                    dict.set_item("end_date", res.end_date.to_string())?;
                    Ok(dict.to_object(py))
                }),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling response: {e}"
                ))),
            }
        })
    }
}
