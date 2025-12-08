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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{identifiers::InstrumentId, python::instruments::instrument_any_to_pyobject};
use pyo3::{prelude::*, types::PyList};
use pyo3_async_runtimes::tokio::future_into_py;

use crate::{common::LighterNetwork, http::client::LighterHttpClient};

/// PyO3 wrapper for the Lighter HTTP client.
#[pyclass(name = "LighterHttpClient", module = "nautilus_pyo3.lighter")]
#[derive(Clone)]
pub struct PyLighterHttpClient {
    inner: LighterHttpClient,
}

#[pymethods]
impl PyLighterHttpClient {
    #[new]
    #[pyo3(
        signature = (
            is_testnet = false,
            base_url_override = None,
            timeout_secs = None,
            proxy_url = None,
        )
    )]
    fn py_new(
        is_testnet: bool,
        base_url_override: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        let network = LighterNetwork::from(is_testnet);
        let client = LighterHttpClient::new(
            network,
            base_url_override.as_deref(),
            timeout_secs,
            proxy_url.as_deref(),
        )
        .map_err(to_pyvalue_err)?;

        Ok(Self { inner: client })
    }

    #[pyo3(name = "load_instrument_definitions")]
    fn py_load_instrument_definitions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let instruments = client
                .load_instrument_definitions()
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| {
                let py_instruments = instruments
                    .into_iter()
                    .map(|instrument| instrument_any_to_pyobject(py, instrument))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(PyList::new(py, &py_instruments)?.into_any().unbind())
            })
        })
    }

    #[pyo3(name = "get_market_index")]
    fn py_get_market_index(&self, instrument_id: InstrumentId) -> Option<u32> {
        self.inner.get_market_index(&instrument_id)
    }
}
