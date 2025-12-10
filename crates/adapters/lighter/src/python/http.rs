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

use nautilus_core::{python::to_pyvalue_err, time::get_atomic_clock_realtime};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::Instrument,
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use pyo3::{
    prelude::*,
    types::{PyList, PyModule},
};
use pyo3_async_runtimes::tokio::future_into_py;
use serde::Serialize;
use serde_json;

use crate::data::order_book::depth_to_deltas_and_quote;
use crate::{common::LighterNetwork, http::client::LighterHttpClient};

/// PyO3 wrapper for the Lighter HTTP client.
#[pyclass(name = "LighterHttpClient", module = "nautilus_pyo3.lighter")]
#[derive(Clone)]
pub struct PyLighterHttpClient {
    pub(crate) inner: LighterHttpClient,
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

            Python::attach(|py| {
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

    #[pyo3(name = "get_order_book_snapshot")]
    fn py_get_order_book_snapshot<'py>(
        &self,
        py: Python<'py>,
        instrument: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let client = self.inner.clone();

        future_into_py(py, async move {
            let market_index = client.get_market_index(&instrument.id()).ok_or_else(|| {
                to_pyvalue_err(anyhow::anyhow!("missing market index for instrument"))
            })?;

            let depth = client
                .get_order_book_snapshot(market_index)
                .await
                .map_err(to_pyvalue_err)?;

            let ts_init = get_atomic_clock_realtime().get_time_ns();
            let (deltas, _) = depth_to_deltas_and_quote(&depth, &instrument, ts_init, ts_init)
                .map_err(to_pyvalue_err)?;

            // Return the OrderBookDeltas directly as a Python object
            Ok(deltas)
        })
    }

    #[pyo3(name = "next_nonce")]
    fn py_next_nonce<'py>(
        &self,
        py: Python<'py>,
        account_index: i64,
        api_key_index: i32,
        auth_token: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let resp = client
                .next_nonce(account_index, api_key_index, auth_token.as_deref())
                .await
                .map_err(to_pyvalue_err)?;
            Python::with_gil(|py| to_py_json(py, &resp).map(|obj| obj.into_any().unbind()))
        })
    }

    #[pyo3(name = "send_tx")]
    fn py_send_tx<'py>(
        &self,
        py: Python<'py>,
        tx_type: u8,
        tx_info: String,
        price_protection: Option<bool>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let resp = client
                .send_tx(tx_type, &tx_info, price_protection)
                .await
                .map_err(to_pyvalue_err)?;
            Python::with_gil(|py| to_py_json(py, &resp).map(|obj| obj.into_any().unbind()))
        })
    }

    #[pyo3(name = "account_active_orders")]
    fn py_account_active_orders<'py>(
        &self,
        py: Python<'py>,
        account_index: i64,
        market_id: u32,
        auth_token: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let resp = client
                .account_active_orders(account_index, market_id, &auth_token)
                .await
                .map_err(to_pyvalue_err)?;
            Python::with_gil(|py| to_py_json(py, &resp).map(|obj| obj.into_any().unbind()))
        })
    }

    #[pyo3(name = "account_by_index")]
    fn py_account_by_index<'py>(
        &self,
        py: Python<'py>,
        account_index: i64,
        auth_token: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let resp = client
                .account_by_index(account_index, auth_token.as_deref())
                .await
                .map_err(to_pyvalue_err)?;
            Python::with_gil(|py| to_py_json(py, &resp).map(|obj| obj.into_any().unbind()))
        })
    }
}

fn to_py_json<'py, T>(py: Python<'py>, value: &T) -> PyResult<PyObject>
where
    T: Serialize,
{
    let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;
    let json_mod = PyModule::import(py, "json")?;
    let loads = json_mod.getattr("loads")?;
    loads.call1((json_str,)).map(|obj| obj.into())
}
