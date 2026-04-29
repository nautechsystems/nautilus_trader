// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashMap;

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::Instrument,
    orders::OrderAny,
    python::{
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::pyobject_to_order_any,
    },
    types::{Price, Quantity},
};
use pyo3::{prelude::*, types::PyList};
use serde_json::to_string;

use crate::{
    common::enums::HyperliquidEnvironment,
    http::{client::HyperliquidHttpClient, parse::HyperliquidMarketType},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidHttpClient {
    /// Provides a high-level HTTP client for the [Hyperliquid](https://hyperliquid.xyz/) REST API.
    ///
    /// This domain client wraps `HyperliquidRawHttpClient` and provides methods that work
    /// with Nautilus domain types. It maintains an instrument cache and handles conversions
    /// between Hyperliquid API responses and Nautilus domain models.
    #[new]
    #[pyo3(signature = (private_key=None, vault_address=None, account_address=None, environment=HyperliquidEnvironment::Mainnet, timeout_secs=60, proxy_url=None, normalize_prices=true))]
    fn py_new(
        private_key: Option<String>,
        vault_address: Option<String>,
        account_address: Option<String>,
        environment: HyperliquidEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        normalize_prices: bool,
    ) -> PyResult<Self> {
        let mut client = Self::with_credentials(
            private_key,
            vault_address,
            account_address,
            environment,
            timeout_secs,
            proxy_url,
        )
        .map_err(to_pyvalue_err)?;
        client.set_normalize_prices(normalize_prices);
        Ok(client)
    }

    /// Creates an authenticated client from environment variables for the specified network.
    ///
    /// # Errors
    ///
    /// Returns `Error.Auth` if required environment variables are not set.
    #[staticmethod]
    #[pyo3(name = "from_env", signature = (environment=HyperliquidEnvironment::Mainnet))]
    fn py_from_env(environment: HyperliquidEnvironment) -> PyResult<Self> {
        Self::from_env(environment).map_err(to_pyvalue_err)
    }

    /// Creates a new `HyperliquidHttpClient` configured with explicit credentials.
    #[staticmethod]
    #[pyo3(name = "from_credentials", signature = (private_key, vault_address=None, environment=HyperliquidEnvironment::Mainnet, timeout_secs=60, proxy_url=None))]
    fn py_from_credentials(
        private_key: &str,
        vault_address: Option<&str>,
        environment: HyperliquidEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::from_credentials(
            private_key,
            vault_address,
            environment,
            timeout_secs,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Caches a single instrument.
    ///
    /// This is required for parsing orders, fills, and positions into reports.
    /// Any existing instrument with the same symbol will be replaced.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(&pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Set the account ID for this client.
    ///
    /// This is required for generating reports with the correct account ID.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: &str) {
        let account_id = AccountId::from(account_id);
        self.set_account_id(account_id);
    }

    /// Gets the user address derived from the private key (if client has credentials).
    ///
    /// # Errors
    ///
    /// Returns `Error.Auth` if the client has no signer configured.
    #[pyo3(name = "get_user_address")]
    fn py_get_user_address(&self) -> PyResult<String> {
        self.get_user_address().map_err(to_pyvalue_err)
    }

    /// Get mapping from spot fill coin identifiers to instrument symbols.
    ///
    /// Hyperliquid WebSocket fills for spot use `@{pair_index}` format (e.g., `@107`),
    /// while instruments are identified by full symbols (e.g., `HYPE-USDC-SPOT`).
    /// This mapping allows looking up the instrument from a spot fill.
    ///
    /// This method also caches the mapping internally for use by fill parsing methods.
    #[pyo3(name = "get_spot_fill_coin_mapping")]
    fn py_get_spot_fill_coin_mapping(&self) -> HashMap<String, String> {
        self.get_spot_fill_coin_mapping()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Get spot metadata (internal helper).
    #[pyo3(name = "get_spot_meta")]
    fn py_get_spot_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.get_spot_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "get_perp_meta")]
    fn py_get_perp_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.load_perp_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "load_instrument_definitions", signature = (include_spot=true, include_perps=true, include_perps_hip3=false))]
    fn py_load_instrument_definitions<'py>(
        &self,
        py: Python<'py>,
        include_spot: bool,
        include_perps: bool,
        include_perps_hip3: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut defs = client
                .request_instrument_defs()
                .await
                .map_err(to_pyvalue_err)?;

            defs.retain(|def| match def.market_type {
                HyperliquidMarketType::Perp => {
                    if def.is_hip3 {
                        include_perps_hip3
                    } else {
                        include_perps
                    }
                }
                HyperliquidMarketType::Spot => include_spot,
            });

            let mut instruments = client.convert_defs(defs);
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

    /// Request historical bars for an instrument.
    ///
    /// Fetches candle data from the Hyperliquid API and converts it to Nautilus bars.
    /// Incomplete bars (where end_timestamp >= current time) are filtered out.
    ///
    /// # References
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#candles-snapshot>
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

    /// Submits an order to the exchange.
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
    #[expect(clippy::too_many_arguments)]
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

    /// Cancel an order on the Hyperliquid exchange.
    ///
    /// Can cancel either by venue order ID or client order ID.
    /// At least one ID must be provided.
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

    /// Modify an order on the Hyperliquid exchange.
    ///
    /// The HL modify API requires a full replacement order spec plus the
    /// venue order ID. The caller must provide all order fields.
    #[pyo3(name = "modify_order")]
    #[expect(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        price: Price,
        quantity: Quantity,
        trigger_price: Option<Price>,
        reduce_only: bool,
        post_only: bool,
        time_in_force: TimeInForce,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    instrument_id,
                    venue_order_id,
                    order_side,
                    order_type,
                    price,
                    quantity,
                    trigger_price,
                    reduce_only,
                    post_only,
                    time_in_force,
                    client_order_id,
                )
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Submit multiple orders to the Hyperliquid exchange in a single request.
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

    /// Request order status reports for a user.
    ///
    /// Fetches open orders via `info_frontend_open_orders` and parses them into OrderStatusReports.
    /// This method requires instruments to be added to the client cache via `cache_instrument()`.
    ///
    /// For vault tokens (starting with "vntls:") that are not in the cache, synthetic instruments
    /// will be created automatically.
    #[pyo3(name = "request_order_status_reports")]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_order_status_reports(&account_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Request a single order status report by venue order ID.
    ///
    /// Queries `info_frontend_open_orders` and filters for the given oid so the
    /// result includes trigger metadata (trigger_px, tpsl, trailing_stop, etc.).
    /// Falls back to `info_order_status` when the order is no longer open.
    #[pyo3(name = "request_order_status_report")]
    #[pyo3(signature = (venue_order_id=None, client_order_id=None))]
    fn py_request_order_status_report<'py>(
        &self,
        py: Python<'py>,
        venue_order_id: Option<&str>,
        client_order_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let venue_order_id = venue_order_id.map(VenueOrderId::from);
        let client_order_id = client_order_id.map(ClientOrderId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if venue_order_id.is_none() && client_order_id.is_none() {
                return Err(to_pyvalue_err(
                    "at least one of venue_order_id or client_order_id is required",
                ));
            }

            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;

            if let Some(coid) = client_order_id.as_ref()
                && let Some(report) = client
                    .request_order_status_report_by_client_order_id(&account_address, coid)
                    .await
                    .map_err(to_pyvalue_err)?
            {
                return Python::attach(|py| Ok(report.into_py_any_unwrap(py)));
            }

            let report = if let Some(vid) = venue_order_id.as_ref() {
                let oid: u64 = vid
                    .as_str()
                    .parse()
                    .map_err(|e| to_pyvalue_err(format!("invalid venue_order_id: {e}")))?;

                client
                    .request_order_status_report(&account_address, oid)
                    .await
                    .map_err(to_pyvalue_err)?
            } else {
                None
            };

            Python::attach(|py| match report {
                Some(r) => Ok(r.into_py_any_unwrap(py)),
                None => Ok(py.None()),
            })
        })
    }

    /// Request fill reports for a user.
    ///
    /// Fetches user fills via `info_user_fills` and parses them into FillReports.
    /// This method requires instruments to be added to the client cache via `cache_instrument()`.
    ///
    /// For vault tokens (starting with "vntls:") that are not in the cache, synthetic instruments
    /// will be created automatically.
    #[pyo3(name = "request_fill_reports")]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_fill_reports(&account_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Request position status reports for a user.
    ///
    /// Fetches perp clearinghouse state and spot clearinghouse state, then returns
    /// the union of perp asset positions (short/long with PnL) and spot holdings
    /// (long only). This method requires instruments to be added to the client
    /// cache via `cache_instrument()`.
    ///
    /// When `instrument_id` resolves to a specific product type, the opposite
    /// product's endpoint is skipped to avoid wasted round trips and make
    /// filtered queries independent of the unused endpoint's availability.
    ///
    /// For vault tokens (starting with "vntls:") that are not in the cache,
    /// synthetic instruments will be created automatically. Spot balances whose
    /// base token has no cached instrument are skipped with a debug log.
    #[pyo3(name = "request_position_status_reports")]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_position_status_reports(&account_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Request account state (balances and margins) for a user.
    ///
    /// Fetches perp and spot clearinghouse state from Hyperliquid and merges them
    /// into a single `AccountState`. USDC is taken from the perp margin summary
    /// when present (to avoid double-counting combined `withdrawable`); non-USDC
    /// tokens are appended from the spot balances.
    ///
    /// # Errors
    ///
    /// Returns an error if `account_id` is not set, or if either the perp or
    /// spot clearinghouse request fails. Spot failures are propagated so the
    /// caller sees real API errors instead of a silently truncated snapshot.
    #[pyo3(name = "request_account_state")]
    fn py_request_account_state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let account_state = client
                .request_account_state(&account_address)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(account_state.into_py_any_unwrap(py)))
        })
    }

    /// Request spot token balances for a user.
    ///
    /// Fetches `spotClearinghouseState` and returns one `AccountBalance` per
    /// non-zero token. USDC is included as a separate balance entry when present;
    /// callers that also report perp margin state must dedupe currencies before
    /// emitting an `AccountState`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the response cannot be parsed.
    #[pyo3(name = "request_spot_balances")]
    fn py_request_spot_balances<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let balances = client
                .request_spot_balances(&account_address)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, balances.into_iter().map(|b| b.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Request spot position status reports for a user.
    ///
    /// Each non-zero spot balance is reported as a Long position against its
    /// `{BASE}-{QUOTE}-SPOT` instrument. Balances whose base token has no
    /// matching instrument in the cache are skipped with a debug log (callers
    /// should ensure `request_instruments` has run
    /// first).
    #[pyo3(name = "request_spot_position_status_reports")]
    fn py_request_spot_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_spot_position_status_reports(&account_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Get spot clearinghouse state (per-token spot balances) for a user.
    #[pyo3(name = "info_spot_clearinghouse_state")]
    fn py_info_spot_clearinghouse_state<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let json = client
                .info_spot_clearinghouse_state(&account_address)
                .await
                .map_err(to_pyvalue_err)?;
            to_string(&json).map_err(to_pyvalue_err)
        })
    }

    /// Get user fee schedule and effective rates.
    #[pyo3(name = "info_user_fees")]
    fn py_info_user_fees<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_address = client.get_account_address().map_err(to_pyvalue_err)?;
            let json = client
                .info_user_fees(&account_address)
                .await
                .map_err(to_pyvalue_err)?;
            to_string(&json).map_err(to_pyvalue_err)
        })
    }
}
