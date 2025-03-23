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

//! Provides PyO3 bindings for the Coinbase International FIX client.

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::prelude::*;

use crate::fix::client::CoinbaseIntxFixClient;

#[pymethods]
impl CoinbaseIntxFixClient {
    #[new]
    #[pyo3(signature = (endpoint=None, api_key=None, api_secret=None, api_passphrase=None, portfolio_id=None))]
    fn py_new(
        endpoint: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        portfolio_id: Option<String>,
    ) -> PyResult<Self> {
        Self::new(endpoint, api_key, api_secret, api_passphrase, portfolio_id)
            .map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "endpoint")]
    pub fn py_endpoint(&self) -> &str {
        self.endpoint()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    pub fn py_api_key(&self) -> &str {
        self.api_key()
    }

    #[getter]
    #[pyo3(name = "portfolio_id")]
    pub fn py_portfolio_id(&self) -> &str {
        self.portfolio_id()
    }

    #[getter]
    #[pyo3(name = "sender_comp_id")]
    pub fn py_sender_comp_id(&self) -> &str {
        self.sender_comp_id()
    }

    #[getter]
    #[pyo3(name = "target_comp_id")]
    pub fn py_target_comp_id(&self) -> &str {
        self.target_comp_id()
    }

    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    #[pyo3(name = "is_logged_on")]
    fn py_is_logged_on(&self) -> bool {
        self.is_logged_on()
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        handler: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect(handler).await.map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await.map_err(to_pyruntime_err)
        })
    }
}
