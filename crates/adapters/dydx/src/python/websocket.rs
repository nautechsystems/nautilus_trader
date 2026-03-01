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

//! Python bindings for the dYdX WebSocket client.

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    UUID4,
    python::{call_python_threadsafe, to_pyvalue_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    enums::AccountType,
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    types::{AccountBalance, Currency, Money},
};
use nautilus_network::mode::ConnectionMode;
use pyo3::{IntoPyObjectExt, prelude::*, types::PyDict};

use crate::{
    common::{credential::DydxCredential, enums::DydxCandleResolution, parse::extract_raw_symbol},
    execution::types::OrderContext,
    http::{client::DydxHttpClient, parse::parse_account_state},
    python::encoder::PyDydxClientOrderIdEncoder,
    websocket::{
        client::DydxWebSocketClient,
        enums::NautilusWsMessage,
        handler::HandlerCommand,
        parse::{parse_ws_fill_report, parse_ws_order_report, parse_ws_position_report},
    },
};

#[pymethods]
impl DydxWebSocketClient {
    #[staticmethod]
    #[pyo3(name = "new_public")]
    fn py_new_public(url: String, heartbeat: Option<u64>) -> Self {
        Self::new_public(url, heartbeat)
    }

    #[staticmethod]
    #[pyo3(name = "new_private")]
    fn py_new_private(
        url: String,
        private_key: String,
        authenticator_ids: Vec<u64>,
        account_id: AccountId,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        let credential = DydxCredential::from_private_key(&private_key, authenticator_ids)
            .map_err(to_pyvalue_err)?;
        Ok(Self::new_private(url, credential, account_id, heartbeat))
    }

    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    #[pyo3(name = "set_bars_timestamp_on_close")]
    fn py_set_bars_timestamp_on_close(&mut self, value: bool) {
        self.set_bars_timestamp_on_close(value);
    }

    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Share the HTTP client's instrument cache with this WebSocket client.
    ///
    /// The HTTP client's cache includes CLOB pair ID and market ticker indices
    /// needed for parsing SubaccountsChannelData into typed execution events.
    /// Must be called before `connect()`.
    #[pyo3(name = "share_instrument_cache")]
    fn py_share_instrument_cache(&mut self, http_client: &DydxHttpClient) {
        self.set_instrument_cache(http_client.instrument_cache().clone());
    }

    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> Option<AccountId> {
        self.account_id()
    }

    /// Returns the shared client order ID encoder.
    #[pyo3(name = "encoder")]
    fn py_encoder(&self) -> PyDydxClientOrderIdEncoder {
        PyDydxClientOrderIdEncoder::from_arc(self.encoder().clone())
    }

    #[getter]
    fn py_url(&self) -> String {
        self.url().to_string()
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon = loop_.getattr(py, "call_soon_threadsafe")?;

        // Convert Python instruments to Rust InstrumentAny
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        // Cache instruments first
        self.cache_instruments(instruments_any);

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Connect the WebSocket client
            client.connect().await.map_err(to_pyvalue_err)?;

            // Take the receiver for messages
            if let Some(mut rx) = client.take_receiver() {
                // Spawn task to process messages and call Python callback
                get_runtime().spawn(async move {
                    let _client = client; // Keep client alive in spawned task
                    let clock = get_atomic_clock_realtime();
                    let order_contexts: DashMap<u32, OrderContext> = DashMap::new();
                    let order_id_map: DashMap<String, (u32, u32)> = DashMap::new();

                    while let Some(msg) = rx.recv().await {
                        match msg {
                            NautilusWsMessage::Data(items) => {
                                Python::attach(|py| {
                                    for data in items {
                                        let py_obj = data_to_pycapsule(py, data);
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    }
                                });
                            }
                            NautilusWsMessage::Deltas(deltas) => {
                                Python::attach(|py| {
                                    let data = Data::Deltas(OrderBookDeltas_API::new(*deltas));
                                    let py_obj = data_to_pycapsule(py, data);
                                    call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                });
                            }
                            NautilusWsMessage::BlockHeight { height, time } => {
                                Python::attach(|py| {
                                    let dict = PyDict::new(py);
                                    let _ = dict.set_item("type", "block_height");
                                    let _ = dict.set_item("height", height);
                                    let _ = dict.set_item("time", time.to_rfc3339());
                                    if let Ok(py_obj) = dict.into_py_any(py) {
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    }
                                });
                            }
                            NautilusWsMessage::SubaccountSubscribed(data) => {
                                // Get account_id from the client
                                let Some(account_id) = _client.account_id() else {
                                    log::warn!("Cannot parse subaccount subscription: account_id not set");
                                    continue;
                                };

                                let instrument_cache = _client.instrument_cache();
                                let ts_init = clock.get_time_ns();

                                // Build maps from instrument cache
                                let inst_map = instrument_cache.to_instrument_id_map();
                                let oracle_map = instrument_cache.to_oracle_prices_map();

                                // Parse and emit AccountState + PositionStatusReports
                                if let Some(ref subaccount) = data.contents.subaccount {
                                    match parse_account_state(
                                        subaccount,
                                        account_id,
                                        &inst_map,
                                        &oracle_map,
                                        ts_init,
                                        ts_init,
                                    ) {
                                        Ok(account_state) => {
                                            Python::attach(|py| {
                                                match account_state.into_py_any(py) {
                                                    Ok(py_obj) => {
                                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                    }
                                                    Err(e) => log::error!("Failed to convert AccountState to Python: {e}"),
                                                }
                                            });
                                        }
                                    Err(e) => log::error!("Failed to parse account state: {e}"),
                                }

                                // Parse and emit PositionStatusReports
                                if let Some(ref positions) = subaccount.open_perpetual_positions {
                                    for (market, ws_position) in positions {
                                        match parse_ws_position_report(
                                            ws_position,
                                            instrument_cache,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                Python::attach(|py| {
                                                    match pyo3::Py::new(py, report) {
                                                        Ok(py_obj) => {
                                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                                        }
                                                        Err(e) => log::error!("Failed to convert PositionStatusReport to Python: {e}"),
                                                    }
                                                });
                                            }
                                            Err(e) => log::error!("Failed to parse position for {market}: {e}"),
                                        }
                                    }
                                }
                                } else {
                                    log::warn!("Subaccount subscription without initial state (new/empty subaccount)");

                                    // Emit zero-balance account state so account gets registered
                                    let currency = Currency::get_or_create_crypto_with_context("USDC", None);
                                    let zero = Money::zero(currency);
                                    let balance = AccountBalance::new_checked(zero, zero, zero)
                                        .expect("zero balance should always be valid");
                                    let account_state = AccountState::new(
                                        account_id,
                                        AccountType::Margin,
                                        vec![balance],
                                        vec![],
                                        true,
                                        UUID4::new(),
                                        ts_init,
                                        ts_init,
                                        None,
                                    );
                                    Python::attach(|py| {
                                        match account_state.into_py_any(py) {
                                            Ok(py_obj) => {
                                                call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                            }
                                            Err(e) => log::error!("Failed to convert AccountState to Python: {e}"),
                                        }
                                    });
                                }
                            }
                            NautilusWsMessage::SubaccountsChannelData(data) => {
                                let Some(account_id) = _client.account_id() else {
                                    log::warn!("Cannot parse SubaccountsChannelData: account_id not set");
                                    continue;
                                };

                                let instrument_cache = _client.instrument_cache();
                                let encoder = _client.encoder();
                                let ts_init = clock.get_time_ns();

                                let mut terminal_orders: Vec<(u32, u32, String)> = Vec::new();

                                // Phase 1: Parse orders and build order_id_map (needed for fill correlation)
                                // but DON'T send order reports yet — fills must be sent first
                                // to prevent reconciliation from inferring fills at the limit price.
                                let mut pending_order_reports = Vec::new();

                                if let Some(ref orders) = data.contents.orders {
                                    for ws_order in orders {
                                        // Build order_id → (client_id, client_metadata) for fill correlation
                                        if let Ok(client_id_u32) = ws_order.client_id.parse::<u32>() {
                                            let client_meta = ws_order.client_metadata
                                                .as_ref()
                                                .and_then(|s| s.parse::<u32>().ok())
                                                .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);
                                            order_id_map.insert(ws_order.id.clone(), (client_id_u32, client_meta));
                                        }

                                        match parse_ws_order_report(
                                            ws_order,
                                            instrument_cache,
                                            &order_contexts,
                                            encoder,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                if !report.order_status.is_open()
                                                    && let Ok(cid) = ws_order.client_id.parse::<u32>()
                                                {
                                                    let meta = ws_order.client_metadata
                                                        .as_ref()
                                                        .and_then(|s| s.parse::<u32>().ok())
                                                        .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);
                                                    terminal_orders.push((cid, meta, ws_order.id.clone()));
                                                }
                                                pending_order_reports.push(report);
                                            }
                                            Err(e) => log::error!("Failed to parse WS order: {e}"),
                                        }
                                    }
                                }

                                // Phase 2: Send fills FIRST so reconciliation sees them before
                                // the terminal order status (prevents inferred fills at limit price)
                                if let Some(ref fills) = data.contents.fills {
                                    for ws_fill in fills {
                                        match parse_ws_fill_report(
                                            ws_fill,
                                            instrument_cache,
                                            &order_id_map,
                                            &order_contexts,
                                            encoder,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                Python::attach(|py| {
                                                    match pyo3::Py::new(py, report) {
                                                        Ok(py_obj) => {
                                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                                        }
                                                        Err(e) => log::error!("Failed to convert FillReport: {e}"),
                                                    }
                                                });
                                            }
                                            Err(e) => log::error!("Failed to parse WS fill: {e}"),
                                        }
                                    }
                                }

                                // Phase 3: Now send order status reports
                                for report in pending_order_reports {
                                    Python::attach(|py| {
                                        match pyo3::Py::new(py, report) {
                                            Ok(py_obj) => {
                                                call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                            }
                                            Err(e) => log::error!("Failed to convert OrderStatusReport: {e}"),
                                        }
                                    });
                                }

                                // Deferred cleanup after fills are correlated
                                for (client_id, client_metadata, order_id) in terminal_orders {
                                    order_contexts.remove(&client_id);
                                    encoder.remove(client_id, client_metadata);
                                    order_id_map.remove(&order_id);
                                }
                            }
                            NautilusWsMessage::MarkPrice(mark_price) => {
                                Python::attach(|py| {
                                    match mark_price.into_py_any(py) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        }
                                        Err(e) => log::error!("Failed to convert MarkPriceUpdate to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::IndexPrice(index_price) => {
                                Python::attach(|py| {
                                    match index_price.into_py_any(py) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        }
                                        Err(e) => log::error!("Failed to convert IndexPriceUpdate to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::FundingRate(funding_rate) => {
                                Python::attach(|py| {
                                    match funding_rate.into_py_any(py) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        }
                                        Err(e) => log::error!("Failed to convert FundingRateUpdate to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::Error(err) => {
                                log::error!("dYdX WebSocket error: {err}");
                            }
                            NautilusWsMessage::Reconnected => {
                                log::info!("dYdX WebSocket reconnected");
                            }
                            NautilusWsMessage::AccountState(state) => {
                                Python::attach(|py| {
                                    match state.into_py_any(py) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        }
                                        Err(e) => log::error!("Failed to convert AccountState to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::Position(report) => {
                                Python::attach(|py| {
                                    match pyo3::Py::new(py, *report) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                        }
                                        Err(e) => log::error!("Failed to convert PositionStatusReport to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::Order(report) => {
                                Python::attach(|py| {
                                    match pyo3::Py::new(py, *report) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                        }
                                        Err(e) => log::error!("Failed to convert OrderStatusReport to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::Fill(report) => {
                                Python::attach(|py| {
                                    match pyo3::Py::new(py, *report) {
                                        Ok(py_obj) => {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                        }
                                        Err(e) => log::error!("Failed to convert FillReport to Python: {e}"),
                                    }
                                });
                            }
                            NautilusWsMessage::NewInstrumentDiscovered { ticker } => {
                                log::info!("New instrument discovered via WebSocket: {ticker}");
                                Python::attach(|py| {
                                    let dict = PyDict::new(py);
                                    let _ = dict.set_item("type", "new_instrument_discovered");
                                    let _ = dict.set_item("ticker", &ticker);
                                    if let Ok(py_obj) = dict.into_py_any(py) {
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    }
                                });
                            }
                        }
                    }
                });
            }

            Ok(())
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let connection_mode = self.connection_mode_atomic();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timeout = Duration::from_secs_f64(timeout_secs);
            let start = Instant::now();

            loop {
                let mode = connection_mode.load();
                let mode_u8 = mode.load(Ordering::Relaxed);
                let is_connected = matches!(
                    mode_u8,
                    x if x == ConnectionMode::Active as u8 || x == ConnectionMode::Reconnect as u8
                );

                if is_connected {
                    break;
                }

                if start.elapsed() > timeout {
                    return Err(to_pyvalue_err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("Client did not become active within {timeout_secs}s"),
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }

            Ok(())
        })
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, instrument: Py<PyAny>, py: Python<'_>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }

    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(&self, instruments: Vec<Py<PyAny>>, py: Python<'_>) -> PyResult<()> {
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }
        self.cache_instruments(instruments_any);
        Ok(())
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        !self.is_connected()
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orderbook")]
    fn py_subscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_orderbook(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orderbook")]
    fn py_unsubscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orderbook(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let spec = bar_type.spec();
        let resolution = DydxCandleResolution::from_bar_spec(&spec).map_err(to_pyvalue_err)?;
        let resolution = resolution.to_string();

        let client = self.clone();
        let instrument_id = bar_type.instrument_id();

        // Build topic for bar type registration (e.g., "ETH-USD/1MIN")
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Register bar type in handler before subscribing
            client
                .send_command(HandlerCommand::RegisterBarType { topic, bar_type })
                .map_err(to_pyvalue_err)?;

            // Brief delay to ensure handler processes registration
            tokio::time::sleep(Duration::from_millis(50)).await;

            client
                .subscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let spec = bar_type.spec();
        let resolution = DydxCandleResolution::from_bar_spec(&spec).map_err(to_pyvalue_err)?;
        let resolution = resolution.to_string();

        let client = self.clone();
        let instrument_id = bar_type.instrument_id();

        // Build topic for unregistration
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)?;

            // Unregister bar type after unsubscribing
            client
                .send_command(HandlerCommand::UnregisterBarType { topic })
                .map_err(to_pyvalue_err)?;

            Ok(())
        })
    }

    #[pyo3(name = "subscribe_markets")]
    fn py_subscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_markets().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_markets")]
    fn py_unsubscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe_markets().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_subaccount")]
    fn py_subscribe_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_subaccount")]
    fn py_unsubscribe_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_block_height")]
    fn py_subscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_block_height()
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_block_height")]
    fn py_unsubscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_block_height()
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }
}
