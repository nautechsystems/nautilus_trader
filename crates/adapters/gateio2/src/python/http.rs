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

//! Python bindings for Gate.io HTTP client.

use nautilus_model::instruments::InstrumentAny;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::common::credential::GateioCredentials;
use crate::http::GateioHttpClient;

#[pyclass(name = "GateioHttpClient")]
#[derive(Clone)]
pub struct PyGateioHttpClient {
    client: GateioHttpClient,
}

#[pymethods]
impl PyGateioHttpClient {
    #[new]
    #[pyo3(signature = (base_http_url=None, base_ws_spot_url=None, base_ws_futures_url=None, base_ws_options_url=None, api_key=None, api_secret=None))]
    fn py_new(
        base_http_url: Option<String>,
        base_ws_spot_url: Option<String>,
        base_ws_futures_url: Option<String>,
        base_ws_options_url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> PyResult<Self> {
        let credentials = match (api_key, api_secret) {
            (Some(key), Some(secret)) => {
                Some(GateioCredentials::new(key, secret).map_err(to_pyerr)?)
            }
            _ => None,
        };

        let client = GateioHttpClient::new(
            base_http_url,
            base_ws_spot_url,
            base_ws_futures_url,
            base_ws_options_url,
            credentials,
        );

        Ok(Self { client })
    }

    /// Loads all instruments from Gate.io.
    #[pyo3(name = "load_instruments")]
    fn py_load_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client
                .load_instruments()
                .await
                .map(|instruments| {
                    instruments
                        .into_iter()
                        .map(|i| i.into_py(Python::with_gil(|py| py)))
                        .collect::<Vec<_>>()
                })
                .map_err(to_pyerr)
        })
    }

    /// Returns the loaded instruments.
    #[pyo3(name = "instruments")]
    fn py_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let instruments = client.instruments().await;
            let py_instruments: Vec<PyObject> = Python::with_gil(|py| {
                instruments
                    .into_iter()
                    .map(|(_, i)| i.into_py(py))
                    .collect()
            });
            Ok(py_instruments)
        })
    }
}

fn to_pyerr<E: std::fmt::Display>(err: E) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(format!("{}", err))
}
