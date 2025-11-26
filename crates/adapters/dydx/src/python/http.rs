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

//! Python bindings for dYdX HTTP client.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::python::instruments::instrument_any_to_pyobject;
use pyo3::prelude::*;
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::http::client::DydxHttpClient;

#[pymethods]
impl DydxHttpClient {
    /// Creates a new [`DydxHttpClient`] instance.
    #[new]
    #[pyo3(signature = (base_url=None, is_testnet=false))]
    fn py_new(base_url: Option<String>, is_testnet: bool) -> PyResult<Self> {
        // Mirror the Rust client's constructor signature with sensible defaults
        Self::new(
            base_url, None, // timeout_secs
            None, // proxy_url
            is_testnet, None, // retry_config
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns `true` if the client is configured for testnet.
    #[pyo3(name = "is_testnet")]
    fn py_is_testnet(&self) -> bool {
        self.is_testnet()
    }

    /// Returns the base URL for the HTTP client.
    #[pyo3(name = "base_url")]
    fn py_base_url(&self) -> String {
        self.base_url().to_string()
    }

    /// Requests all available instruments from the dYdX Indexer API.
    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        maker_fee: Option<String>,
        taker_fee: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let maker = maker_fee
            .as_ref()
            .map(|s| Decimal::from_str(s))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let taker = taker_fee
            .as_ref()
            .map(|s| Decimal::from_str(s))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(None, maker, taker)
                .await
                .map_err(to_pyvalue_err)?;

            #[allow(deprecated)]
            Python::with_gil(|py| {
                let py_instruments: PyResult<Vec<Py<PyAny>>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                py_instruments
            })
        })
    }

    /// Gets a cached instrument by symbol.
    #[pyo3(name = "get_instrument")]
    fn py_get_instrument(&self, py: Python<'_>, symbol: &str) -> PyResult<Option<Py<PyAny>>> {
        let symbol_ustr = Ustr::from(symbol);
        let instrument = self.get_instrument(&symbol_ustr);
        match instrument {
            Some(inst) => Ok(Some(instrument_any_to_pyobject(py, inst)?)),
            None => Ok(None),
        }
    }

    /// Returns the number of cached instruments.
    #[pyo3(name = "instrument_count")]
    fn py_instrument_count(&self) -> usize {
        self.instruments().len()
    }

    /// Returns all cached instrument symbols.
    #[pyo3(name = "instrument_symbols")]
    fn py_instrument_symbols(&self) -> Vec<String> {
        self.instruments()
            .iter()
            .map(|entry| entry.key().to_string())
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "DydxHttpClient(base_url='{}', is_testnet={}, cached_instruments={})",
            self.base_url(),
            self.is_testnet(),
            self.instruments().len()
        )
    }
}
