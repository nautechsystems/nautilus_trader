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

//! Python bindings for Lighter HTTP client.

use nautilus_model::instruments::InstrumentAny;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::{
    common::credential::LighterCredentials,
    http::LighterHttpClient,
};

/// Python wrapper for `LighterHttpClient`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterHttpClient")]
pub struct PyLighterHttpClient {
    client: LighterHttpClient,
}

#[pymethods]
impl PyLighterHttpClient {
    #[new]
    #[pyo3(signature = (base_http_url=None, base_ws_url=None, is_testnet=false, api_key_private_key=None, eth_private_key=None, api_key_index=None, account_index=None))]
    fn new(
        base_http_url: Option<String>,
        base_ws_url: Option<String>,
        is_testnet: bool,
        api_key_private_key: Option<String>,
        eth_private_key: Option<String>,
        api_key_index: Option<u8>,
        account_index: Option<u64>,
    ) -> PyResult<Self> {
        let credentials = match (api_key_private_key, eth_private_key, api_key_index, account_index) {
            (Some(api_key), Some(eth_key), Some(key_idx), Some(acc_idx)) => {
                Some(LighterCredentials::new(api_key, eth_key, key_idx, acc_idx)
                    .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?)
            }
            _ => None,
        };

        Ok(Self {
            client: LighterHttpClient::new(base_http_url, base_ws_url, is_testnet, credentials),
        })
    }

    /// Loads instruments from the API.
    #[pyo3(name = "load_instruments")]
    fn py_load_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let instruments = client
                .load_instruments()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(instruments)
        })
    }

    /// Gets account information.
    #[pyo3(name = "get_account")]
    fn py_get_account<'py>(&self, py: Python<'py>, account_id: Option<u64>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let account = client
                .request_account(account_id)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Python::with_gil(|py| {
                let dict = pyo3::types::PyDict::new(py);
                dict.set_item("id", account.id)?;
                dict.set_item("address", account.address)?;
                // Add other fields as needed
                Ok(dict.into())
            })
        })
    }

    /// Gets the next nonce for transaction signing.
    #[pyo3(name = "get_next_nonce")]
    fn py_get_next_nonce<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let nonce = client
                .get_next_nonce()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(nonce)
        })
    }

    fn __repr__(&self) -> String {
        "LighterHttpClient()".to_string()
    }
}
