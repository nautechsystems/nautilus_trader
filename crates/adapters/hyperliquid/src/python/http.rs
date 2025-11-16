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

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::OrderAny,
    python::{
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::pyobject_to_order_any,
    },
    types::{Price, Quantity},
};
use pyo3::{prelude::*, types::PyList};
use serde_json::to_string;

use crate::http::client::HyperliquidHttpClient;

#[pymethods]
impl HyperliquidHttpClient {
    #[new]
    #[pyo3(signature = (private_key=None, vault_address=None, is_testnet=false, timeout_secs=None, proxy_url=None))]
    fn py_new(
        private_key: Option<String>,
        vault_address: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        // Try to get credentials from parameters or environment variables
        let pk = private_key.or_else(|| {
            if is_testnet {
                std::env::var("HYPERLIQUID_TESTNET_PK").ok()
            } else {
                std::env::var("HYPERLIQUID_PK").ok()
            }
        });

        let vault = vault_address.or_else(|| {
            if is_testnet {
                std::env::var("HYPERLIQUID_TESTNET_VAULT").ok()
            } else {
                std::env::var("HYPERLIQUID_VAULT").ok()
            }
        });

        if let Some(key) = pk {
            Self::from_credentials(&key, vault.as_deref(), is_testnet, timeout_secs, proxy_url)
                .map_err(to_pyvalue_err)
        } else {
            Self::new(is_testnet, timeout_secs, proxy_url).map_err(to_pyvalue_err)
        }
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_credentials", signature = (private_key, vault_address=None, is_testnet=false, timeout_secs=None, proxy_url=None))]
    fn py_from_credentials(
        private_key: &str,
        vault_address: Option<&str>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::from_credentials(
            private_key,
            vault_address,
            is_testnet,
            timeout_secs,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: &str) -> PyResult<()> {
        let account_id = AccountId::from(account_id);
        self.set_account_id(account_id);
        Ok(())
    }

    #[pyo3(name = "get_user_address")]
    fn py_get_user_address(&self) -> PyResult<String> {
        self.get_user_address().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "get_perp_meta")]
    fn py_get_perp_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.load_perp_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "get_spot_meta")]
    fn py_get_spot_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.get_spot_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "get_l2_book")]
    fn py_get_l2_book<'py>(&self, py: Python<'py>, coin: &str) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = coin.to_string();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let book = client.info_l2_book(&coin).await.map_err(to_pyvalue_err)?;
            to_string(&book).map_err(to_pyvalue_err)
        })
    }

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

    #[pyo3(name = "request_quote_ticks", signature = (instrument_id, start=None, end=None, limit=None))]
    fn py_request_quote_ticks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<chrono::DateTime<chrono::Utc>>,
        end: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = (instrument_id, start, end, limit);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Err::<Vec<u8>, _>(to_pyvalue_err(anyhow::anyhow!(
                "Hyperliquid does not provide historical quotes via HTTP API"
            )))
        })
    }

    #[pyo3(name = "request_trade_ticks", signature = (instrument_id, start=None, end=None, limit=None))]
    fn py_request_trade_ticks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<chrono::DateTime<chrono::Utc>>,
        end: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = (instrument_id, start, end, limit);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Err::<Vec<u8>, _>(to_pyvalue_err(anyhow::anyhow!(
                "Hyperliquid does not provide historical market trades via HTTP API"
            )))
        })
    }

    #[pyo3(name = "request_bars", signature = (bar_type, start=None, end=None, limit=None))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<chrono::DateTime<chrono::Utc>>,
        end: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, start, end, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(py, bars.into_iter().map(|b| b.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "submit_order", signature = (
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        price=None,
        trigger_price=None,
        post_only=false,
        reduce_only=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: bool,
        reduce_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .submit_order(
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    post_only,
                    reduce_only,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(report.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "cancel_order", signature = (
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
    ))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "submit_orders")]
    fn py_submit_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_anys: Vec<OrderAny> = Python::attach(|py| {
                orders
                    .into_iter()
                    .map(|order| pyobject_to_order_any(py, order))
                    .collect::<PyResult<Vec<_>>>()
                    .map_err(to_pyvalue_err)
            })?;

            let order_refs: Vec<&OrderAny> = order_anys.iter().collect();

            let reports = client
                .submit_orders(&order_refs)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

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

    #[pyo3(name = "request_order_status_reports")]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_order_status_reports(&user_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "request_fill_reports")]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_fill_reports(&user_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "request_position_status_reports")]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_position_status_reports(&user_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }
}
