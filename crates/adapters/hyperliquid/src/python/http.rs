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
use nautilus_model::{
    instruments::{Instrument, InstrumentAny},
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{prelude::*, types::PyList};
use serde_json::to_string;

use crate::http::{client::HyperliquidHttpClient, query::ExchangeAction};

#[pymethods]
impl HyperliquidHttpClient {
    #[new]
    #[pyo3(signature = (is_testnet=false, timeout_secs=None))]
    fn py_new(is_testnet: bool, timeout_secs: Option<u64>) -> Self {
        Self::new(is_testnet, timeout_secs)
    }

    /// Get perpetuals metadata as a JSON string.
    #[pyo3(name = "get_perp_meta")]
    fn py_get_perp_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.load_perp_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    /// Get spot metadata as a JSON string.
    #[pyo3(name = "get_spot_meta")]
    fn py_get_spot_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.get_spot_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    /// Load all available instruments (perps and/or spot) as Nautilus instrument objects.
    #[pyo3(name = "load_instrument_definitions", signature = (include_perp=true, include_spot=true))]
    fn py_load_instrument_definitions<'py>(
        &self,
        py: Python<'py>,
        include_perp: bool,
        include_spot: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut instruments = client.request_instruments().await.map_err(to_pyvalue_err)?;

            if !include_perp || !include_spot {
                instruments.retain(|instrument| match instrument {
                    InstrumentAny::CryptoPerpetual(_) => include_perp,
                    InstrumentAny::CurrencyPair(_) => include_spot,
                    _ => true,
                });
            }

            instruments.sort_by_key(|instrument| instrument.id());

            Python::attach(|py| {
                let mut py_instruments = Vec::with_capacity(instruments.len());
                for instrument in instruments {
                    py_instruments.push(instrument_any_to_pyobject(py, instrument)?);
                }

                let py_list = PyList::new(py, &py_instruments)?;
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Submit a single order to the Hyperliquid exchange.
    ///
    /// Takes order parameters as JSON string and submits via the authenticated POST /exchange endpoint.
    ///
    /// Returns the response from the exchange as a JSON string.
    #[pyo3(name = "submit_order")]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        order_request_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Parse the order request from JSON
            let order_request: serde_json::Value =
                serde_json::from_str(&order_request_json).map_err(to_pyvalue_err)?;

            // Create orders array with single order
            let orders_value = serde_json::to_value(vec![order_request]).map_err(to_pyvalue_err)?;

            // Create exchange action
            let action = ExchangeAction::order(orders_value);

            // Submit to exchange
            let response = client.post_action(&action).await.map_err(to_pyvalue_err)?;

            // Return response as JSON string
            to_string(&response).map_err(to_pyvalue_err)
        })
    }

    /// Submit multiple orders to the Hyperliquid exchange in a single request.
    ///
    /// Takes a JSON string of order requests and submits them via the authenticated POST /exchange endpoint.
    ///
    /// Returns the response from the exchange as a JSON string.
    #[pyo3(name = "submit_orders")]
    fn py_submit_orders<'py>(
        &self,
        py: Python<'py>,
        orders_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Parse the orders from JSON
            let orders_value: serde_json::Value =
                serde_json::from_str(&orders_json).map_err(to_pyvalue_err)?;

            // Create exchange action
            let action = ExchangeAction::order(orders_value);

            // Submit to exchange
            let response = client.post_action(&action).await.map_err(to_pyvalue_err)?;

            // Return response as JSON string
            to_string(&response).map_err(to_pyvalue_err)
        })
    }

    /// Get open orders for the authenticated user.
    ///
    /// Returns the response from the exchange as a JSON string.
    #[pyo3(name = "get_open_orders")]
    fn py_get_open_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let response = client
                .info_open_orders(&user_address)
                .await
                .map_err(to_pyvalue_err)?;
            to_string(&response).map_err(to_pyvalue_err)
        })
    }

    /// Get clearinghouse state (balances, positions, margin) for the authenticated user.
    ///
    /// Returns the response from the exchange as a JSON string.
    #[pyo3(name = "get_clearinghouse_state")]
    fn py_get_clearinghouse_state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let response = client
                .info_clearinghouse_state(&user_address)
                .await
                .map_err(to_pyvalue_err)?;
            to_string(&response).map_err(to_pyvalue_err)
        })
    }
}
