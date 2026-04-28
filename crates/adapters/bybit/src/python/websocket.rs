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

//! Python bindings for the Bybit WebSocket client.

use std::sync::Arc;

use ahash::AHashMap;
use dashmap::DashMap;
use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    AtomicMap, AtomicSet, UUID4, UnixNanos,
    python::{call_python_threadsafe, to_pyruntime_err, to_pyvalue_err},
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API, QuoteTick},
    enums::{
        AggregationSource, BarAggregation, OrderSide, OrderType, PriceType, TimeInForce,
        TriggerType,
    },
    events::{OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use pyo3::{IntoPyObjectExt, prelude::*};
use ustr::Ustr;

use crate::{
    common::{
        consts::BYBIT_VENUE,
        enums::{BybitEnvironment, BybitPositionIdx, BybitProductType},
        parse::make_bybit_symbol,
    },
    python::params::{BybitWsAmendOrderParams, BybitWsCancelOrderParams, BybitWsPlaceOrderParams},
    websocket::{
        client::{BATCH_PROCESSING_LIMIT, BybitWebSocketClient, PendingPyRequest},
        dispatch::PendingOperation,
        messages::{BybitWebSocketError, BybitWsMessage},
        parse::{
            parse_kline_topic, parse_millis_i64, parse_orderbook_deltas, parse_orderbook_quote,
            parse_ticker_linear_funding, parse_ticker_linear_index_price,
            parse_ticker_linear_mark_price, parse_ticker_linear_quote, parse_ticker_option_greeks,
            parse_ticker_option_index_price, parse_ticker_option_mark_price,
            parse_ticker_option_quote, parse_ws_account_state, parse_ws_fill_report,
            parse_ws_kline_bar, parse_ws_order_status_report, parse_ws_position_status_report,
            parse_ws_trade_tick,
        },
    },
};

fn validate_bar_type(bar_type: &BarType) -> anyhow::Result<()> {
    let spec = bar_type.spec();

    if spec.price_type != PriceType::Last {
        anyhow::bail!(
            "Invalid bar type: Bybit bars only support LAST price type, received {}",
            spec.price_type
        );
    }

    if bar_type.aggregation_source() != AggregationSource::External {
        anyhow::bail!(
            "Invalid bar type: Bybit bars only support EXTERNAL aggregation source, received {}",
            bar_type.aggregation_source()
        );
    }

    let step = spec.step.get();
    if spec.aggregation == BarAggregation::Minute && step >= 60 {
        let hours = step / 60;
        anyhow::bail!("Invalid bar type: {step}-MINUTE not supported, use {hours}-HOUR instead");
    }

    Ok(())
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitWebSocketError {
    fn __repr__(&self) -> String {
        format!(
            "BybitWebSocketError(code={}, message='{}', conn_id={:?}, topic={:?})",
            self.code, self.message, self.conn_id, self.topic
        )
    }

    #[getter]
    pub fn code(&self) -> i64 {
        self.code
    }

    #[getter]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[getter]
    pub fn conn_id(&self) -> Option<&str> {
        self.conn_id.as_deref()
    }

    #[getter]
    pub fn topic(&self) -> Option<&str> {
        self.topic.as_deref()
    }

    #[getter]
    pub fn req_id(&self) -> Option<&str> {
        self.req_id.as_deref()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitWebSocketClient {
    /// Creates a new Bybit public WebSocket client.
    #[staticmethod]
    #[pyo3(name = "new_public")]
    #[pyo3(signature = (product_type, environment, url=None, heartbeat=20, proxy_url=None))]
    fn py_new_public(
        product_type: BybitProductType,
        environment: BybitEnvironment,
        url: Option<String>,
        heartbeat: u64,
        proxy_url: Option<String>,
    ) -> Self {
        Self::new_public_with(
            product_type,
            environment,
            url,
            heartbeat,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Creates a new Bybit private WebSocket client.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from
    /// environment variables based on the environment:
    /// - Demo: `BYBIT_DEMO_API_KEY`, `BYBIT_DEMO_API_SECRET`
    /// - Testnet: `BYBIT_TESTNET_API_KEY`, `BYBIT_TESTNET_API_SECRET`
    /// - Mainnet: `BYBIT_API_KEY`, `BYBIT_API_SECRET`
    #[staticmethod]
    #[pyo3(name = "new_private")]
    #[pyo3(signature = (environment, api_key=None, api_secret=None, url=None, heartbeat=20, proxy_url=None))]
    fn py_new_private(
        environment: BybitEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url: Option<String>,
        heartbeat: u64,
        proxy_url: Option<String>,
    ) -> Self {
        Self::new_private(
            environment,
            api_key,
            api_secret,
            url,
            heartbeat,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Creates a new Bybit trade WebSocket client for order operations.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from
    /// environment variables based on the environment:
    /// - Demo: `BYBIT_DEMO_API_KEY`, `BYBIT_DEMO_API_SECRET`
    /// - Testnet: `BYBIT_TESTNET_API_KEY`, `BYBIT_TESTNET_API_SECRET`
    /// - Mainnet: `BYBIT_API_KEY`, `BYBIT_API_SECRET`
    #[staticmethod]
    #[pyo3(name = "new_trade")]
    #[pyo3(signature = (environment, api_key=None, api_secret=None, url=None, heartbeat=20, proxy_url=None))]
    fn py_new_trade(
        environment: BybitEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url: Option<String>,
        heartbeat: u64,
        proxy_url: Option<String>,
    ) -> Self {
        Self::new_trade(
            environment,
            api_key,
            api_secret,
            url,
            heartbeat,
            TransportBackend::default(),
            proxy_url,
        )
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> Option<String> {
        self.credential().map(|c| c.api_key_masked())
    }

    /// Returns a value indicating whether the client is active.
    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    /// Returns a value indicating whether the client is closed.
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Returns the number of currently registered subscriptions.
    #[pyo3(name = "subscription_count")]
    fn py_subscription_count(&self) -> usize {
        self.subscription_count()
    }

    /// Adds an instrument to the shared instruments cache.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Sets the account ID for account message parsing.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Sets the account market maker level.
    #[pyo3(name = "set_mm_level")]
    fn py_set_mm_level(&self, mm_level: u8) {
        self.set_mm_level(mm_level);
    }

    /// Sets whether bar timestamps use the close time.
    #[pyo3(name = "set_bars_timestamp_on_close")]
    fn py_set_bars_timestamp_on_close(&self, value: bool) {
        self.set_bars_timestamp_on_close(value);
    }

    /// Adds an instrument ID to the option greeks subscription set.
    #[pyo3(name = "add_option_greeks_sub")]
    fn py_add_option_greeks_sub(&self, instrument_id: InstrumentId) {
        self.add_option_greeks_sub(instrument_id);
    }

    /// Removes an instrument ID from the option greeks subscription set.
    #[pyo3(name = "remove_option_greeks_sub")]
    fn py_remove_option_greeks_sub(&self, instrument_id: InstrumentId) {
        self.remove_option_greeks_sub(&instrument_id);
    }

    /// Disconnects the WebSocket client and stops the background task.
    #[pyo3(name = "connect")]
    #[expect(clippy::needless_pass_by_value)] // PyO3 extracted parameter
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();
            let clock = get_atomic_clock_realtime();
            let product_type = client.product_type();
            let account_id = client.account_id();
            let bar_types_cache = client.bar_types_cache().clone();
            let trade_subs = client.trade_subs().clone();
            let option_greeks_subs = client.option_greeks_subs().clone();
            let bars_timestamp_on_close = client.bars_timestamp_on_close();
            let instruments = Arc::clone(client.instruments_cache_ref());
            let pending_py_requests = Arc::clone(client.pending_py_requests());

            get_runtime().spawn(async move {
                let mut quote_cache = AHashMap::new();
                let mut funding_cache: AHashMap<Ustr, (Option<String>, Option<String>)> =
                    AHashMap::new();
                let _client = client;
                let _resolve = |raw_symbol: &Ustr| -> Option<InstrumentAny> {
                    let key =
                        product_type.map_or(*raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));
                    instruments.get_cloned(&key)
                };

                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        BybitWsMessage::Orderbook(ref msg) => {
                            handle_orderbook(
                                msg,
                                product_type,
                                &instruments,
                                &mut quote_cache,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::Trade(ref msg) => {
                            handle_trade(
                                msg,
                                product_type,
                                &instruments,
                                &trade_subs,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::Kline(ref msg) => {
                            handle_kline(
                                msg,
                                product_type,
                                &instruments,
                                &bar_types_cache,
                                bars_timestamp_on_close,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::TickerLinear(ref msg) => {
                            handle_ticker_linear(
                                msg,
                                product_type,
                                &instruments,
                                &mut quote_cache,
                                &mut funding_cache,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::TickerOption(ref msg) => {
                            handle_ticker_option(
                                msg,
                                product_type,
                                &instruments,
                                &mut quote_cache,
                                &option_greeks_subs,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::AccountOrder(ref msg) => {
                            handle_account_order(
                                msg,
                                &instruments,
                                account_id,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::AccountExecution(ref msg) => {
                            handle_account_execution(
                                msg,
                                &instruments,
                                account_id,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::AccountWallet(ref msg) => {
                            handle_account_wallet(msg, account_id, clock, &call_soon, &callback);
                        }
                        BybitWsMessage::AccountPosition(ref msg) => {
                            handle_account_position(
                                msg,
                                &instruments,
                                account_id,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::OrderResponse(ref resp) => {
                            handle_order_response(
                                resp,
                                &pending_py_requests,
                                account_id,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        BybitWsMessage::Error(err) => {
                            send_to_python(err, &call_soon, &callback);
                        }
                        BybitWsMessage::Reconnected => {
                            quote_cache.clear();
                            funding_cache.clear();
                            log::info!("WebSocket reconnected");
                        }
                        BybitWsMessage::Auth(_) => {
                            log::info!("WebSocket authenticated");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    /// Subscribe to the provided topic strings.
    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
        py: Python<'py>,
        topics: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe(topics).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from the provided topics.
    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &self,
        py: Python<'py>,
        topics: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe(topics).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to orderbook updates for a specific instrument.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/orderbook>
    #[pyo3(name = "subscribe_orderbook")]
    fn py_subscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_orderbook(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    #[pyo3(name = "unsubscribe_orderbook")]
    fn py_unsubscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orderbook(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to public trade updates for a specific instrument.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/trade>
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from public trade updates for a specific instrument.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to ticker updates for a specific instrument.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/ticker>
    #[pyo3(name = "subscribe_ticker")]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_option_greeks")]
    fn py_subscribe_option_greeks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.add_option_greeks_sub(instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_option_greeks")]
    fn py_unsubscribe_option_greeks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.remove_option_greeks_sub(&instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from ticker updates for a specific instrument.
    #[pyo3(name = "unsubscribe_ticker")]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to kline/candlestick updates for a specific instrument.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/kline>
    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_bar_type(&bar_type).map_err(to_pyvalue_err)?;

        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_bars(bar_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from kline/candlestick updates for a specific instrument.
    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_bar_type(&bar_type).map_err(to_pyvalue_err)?;

        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_bars(bar_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to order updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/order>
    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_orders().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from order updates.
    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orders()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to execution/fill updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/execution>
    #[pyo3(name = "subscribe_executions")]
    fn py_subscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_executions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from execution/fill updates.
    #[pyo3(name = "unsubscribe_executions")]
    fn py_unsubscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_executions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to position updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/position>
    #[pyo3(name = "subscribe_positions")]
    fn py_subscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_positions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from position updates.
    #[pyo3(name = "unsubscribe_positions")]
    fn py_unsubscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_positions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to wallet/balance updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/wallet>
    #[pyo3(name = "subscribe_wallet")]
    fn py_subscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_wallet().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from wallet/balance updates.
    #[pyo3(name = "unsubscribe_wallet")]
    fn py_unsubscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_wallet()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Waits until the WebSocket client becomes active or times out.
    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .wait_until_active(timeout_secs)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Submits an order using Nautilus domain objects.
    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        is_quote_quantity=false,
        time_in_force=None,
        price=None,
        trigger_price=None,
        trigger_type=None,
        post_only=None,
        reduce_only=None,
        is_leverage=false,
        position_idx=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        is_quote_quantity: bool,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
        position_idx: Option<BybitPositionIdx>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let pending_py_requests = Arc::clone(self.pending_py_requests());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let req_id = client
                .submit_order(
                    product_type,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    is_quote_quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    trigger_type,
                    post_only,
                    reduce_only,
                    is_leverage,
                    position_idx,
                )
                .await
                .map_err(to_pyruntime_err)?;
            pending_py_requests.insert(
                req_id,
                vec![PendingPyRequest {
                    client_order_id,
                    operation: PendingOperation::Place,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id: None,
                }],
            );
            Ok(())
        })
    }

    /// Modifies an existing order using Nautilus domain objects.
    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id=None,
        quantity=None,
        price=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let pending_py_requests = Arc::clone(self.pending_py_requests());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let req_id = client
                .modify_order(
                    product_type,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                )
                .await
                .map_err(to_pyruntime_err)?;
            pending_py_requests.insert(
                req_id,
                vec![PendingPyRequest {
                    client_order_id,
                    operation: PendingOperation::Amend,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                }],
            );
            Ok(())
        })
    }

    /// Cancels an order via WebSocket, returning the request ID for correlation.
    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (
        product_type,
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let pending_py_requests = Arc::clone(self.pending_py_requests());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let req_id = client
                .cancel_order_by_id(product_type, instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(to_pyruntime_err)?;
            pending_py_requests.insert(
                req_id,
                vec![PendingPyRequest {
                    client_order_id,
                    operation: PendingOperation::Cancel,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                }],
            );
            Ok(())
        })
    }

    /// Builds order params for placing an order.
    #[pyo3(name = "build_place_order_params")]
    #[pyo3(signature = (
        product_type,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        is_quote_quantity=false,
        time_in_force=None,
        price=None,
        trigger_price=None,
        trigger_type=None,
        post_only=None,
        reduce_only=None,
        is_leverage=false,
        take_profit=None,
        stop_loss=None,
        position_idx=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_build_place_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        is_quote_quantity: bool,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
        take_profit: Option<Price>,
        stop_loss: Option<Price>,
        position_idx: Option<BybitPositionIdx>,
    ) -> PyResult<BybitWsPlaceOrderParams> {
        let params = self
            .build_place_order_params(
                product_type,
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                is_quote_quantity,
                time_in_force,
                price,
                trigger_price,
                trigger_type,
                post_only,
                reduce_only,
                is_leverage,
                take_profit,
                stop_loss,
                position_idx,
            )
            .map_err(to_pyruntime_err)?;
        Ok(params.into())
    }

    /// Batch cancels multiple orders via WebSocket, returning the request ID for correlation.
    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsCancelOrderParams>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let pending_py_requests = Arc::clone(self.pending_py_requests());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_params: Vec<crate::websocket::messages::BybitWsCancelOrderParams> = orders
                .into_iter()
                .map(|p| p.try_into())
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_pyruntime_err)?;

            let per_order = build_pending_entries(
                &order_params,
                PendingOperation::Cancel,
                trader_id,
                strategy_id,
            );

            let req_ids = client
                .batch_cancel_orders(order_params)
                .await
                .map_err(to_pyruntime_err)?;

            register_batch_pending(req_ids, &per_order, &pending_py_requests);
            Ok(())
        })
    }

    /// Builds order params for amending an order.
    #[pyo3(name = "build_amend_order_params")]
    fn py_build_amend_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> PyResult<crate::python::params::BybitWsAmendOrderParams> {
        let params = self
            .build_amend_order_params(
                product_type,
                instrument_id,
                venue_order_id,
                client_order_id,
                quantity,
                price,
            )
            .map_err(to_pyruntime_err)?;
        Ok(params.into())
    }

    /// Builds order params for canceling an order via WebSocket.
    #[pyo3(name = "build_cancel_order_params")]
    fn py_build_cancel_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<crate::python::params::BybitWsCancelOrderParams> {
        let params = self
            .build_cancel_order_params(product_type, instrument_id, venue_order_id, client_order_id)
            .map_err(to_pyruntime_err)?;
        Ok(params.into())
    }

    #[pyo3(name = "batch_modify_orders")]
    fn py_batch_modify_orders<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let pending_py_requests = Arc::clone(self.pending_py_requests());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_params: Vec<crate::websocket::messages::BybitWsAmendOrderParams> = orders
                .into_iter()
                .map(|p| p.try_into())
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_pyruntime_err)?;

            let per_order = build_pending_entries(
                &order_params,
                PendingOperation::Amend,
                trader_id,
                strategy_id,
            );

            let req_ids = client
                .batch_amend_orders(order_params)
                .await
                .map_err(to_pyruntime_err)?;

            register_batch_pending(req_ids, &per_order, &pending_py_requests);
            Ok(())
        })
    }

    /// Batch creates multiple orders via WebSocket, returning the request ID for correlation.
    #[pyo3(name = "batch_place_orders")]
    fn py_batch_place_orders<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let pending_py_requests = Arc::clone(self.pending_py_requests());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_params: Vec<crate::websocket::messages::BybitWsPlaceOrderParams> = orders
                .into_iter()
                .map(|p| p.try_into())
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_pyruntime_err)?;

            let per_order = build_pending_entries(
                &order_params,
                PendingOperation::Place,
                trader_id,
                strategy_id,
            );

            let req_ids = client
                .batch_place_orders(order_params)
                .await
                .map_err(to_pyruntime_err)?;

            register_batch_pending(req_ids, &per_order, &pending_py_requests);
            Ok(())
        })
    }
}

trait BatchOrderParams {
    fn order_link_id(&self) -> Option<&str>;
    fn symbol(&self) -> Ustr;
    fn category(&self) -> BybitProductType;
    fn venue_order_id(&self) -> Option<VenueOrderId>;
}

impl BatchOrderParams for crate::websocket::messages::BybitWsCancelOrderParams {
    fn order_link_id(&self) -> Option<&str> {
        self.order_link_id.as_deref()
    }
    fn symbol(&self) -> Ustr {
        self.symbol
    }
    fn category(&self) -> BybitProductType {
        self.category
    }
    fn venue_order_id(&self) -> Option<VenueOrderId> {
        self.order_id.as_ref().map(VenueOrderId::new)
    }
}

impl BatchOrderParams for crate::websocket::messages::BybitWsAmendOrderParams {
    fn order_link_id(&self) -> Option<&str> {
        self.order_link_id.as_deref()
    }
    fn symbol(&self) -> Ustr {
        self.symbol
    }
    fn category(&self) -> BybitProductType {
        self.category
    }
    fn venue_order_id(&self) -> Option<VenueOrderId> {
        self.order_id.as_ref().map(VenueOrderId::new)
    }
}

impl BatchOrderParams for crate::websocket::messages::BybitWsPlaceOrderParams {
    fn order_link_id(&self) -> Option<&str> {
        self.order_link_id.as_deref()
    }
    fn symbol(&self) -> Ustr {
        self.symbol
    }
    fn category(&self) -> BybitProductType {
        self.category
    }
    fn venue_order_id(&self) -> Option<VenueOrderId> {
        None
    }
}

fn build_pending_entries<P: BatchOrderParams>(
    params: &[P],
    operation: PendingOperation,
    trader_id: TraderId,
    strategy_id: StrategyId,
) -> Vec<PendingPyRequest> {
    params
        .iter()
        .map(|p| PendingPyRequest {
            client_order_id: p
                .order_link_id()
                .filter(|s| !s.is_empty())
                .map_or(ClientOrderId::from("UNKNOWN"), ClientOrderId::new),
            operation,
            trader_id,
            strategy_id,
            instrument_id: InstrumentId::new(
                Symbol::new(make_bybit_symbol(p.symbol().as_str(), p.category()).as_str()),
                *BYBIT_VENUE,
            ),
            venue_order_id: p.venue_order_id(),
        })
        .collect()
}

fn register_batch_pending(
    req_ids: Vec<String>,
    per_order: &[PendingPyRequest],
    pending_py_requests: &DashMap<String, Vec<PendingPyRequest>>,
) {
    for (req_id, chunk) in req_ids
        .into_iter()
        .zip(per_order.chunks(BATCH_PROCESSING_LIMIT))
    {
        pending_py_requests.insert(req_id, chunk.to_vec());
    }
}

fn resolve_instrument(
    raw_symbol: &Ustr,
    product_type: Option<BybitProductType>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
) -> Option<InstrumentAny> {
    let key = product_type.map_or(*raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));
    instruments.get_cloned(&key)
}

fn send_data_to_python(data: Data, call_soon: &Py<PyAny>, callback: &Py<PyAny>) {
    Python::attach(|py| {
        let py_obj = data_to_pycapsule(py, data);
        call_python_threadsafe(py, call_soon, callback, py_obj);
    });
}

fn send_to_python<T: for<'py> IntoPyObjectExt<'py>>(
    value: T,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    Python::attach(|py| {
        if let Ok(py_obj) = value.into_py_any(py) {
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }
    });
}

fn handle_orderbook(
    msg: &crate::websocket::messages::BybitWsOrderbookDepthMsg,
    product_type: Option<BybitProductType>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    quote_cache: &mut AHashMap<InstrumentId, QuoteTick>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = resolve_instrument(&msg.data.s, product_type, instruments) else {
        return;
    };
    let ts_init = clock.get_time_ns();

    match parse_orderbook_deltas(msg, &instrument, ts_init) {
        Ok(deltas) => {
            send_data_to_python(
                Data::Deltas(OrderBookDeltas_API::new(deltas)),
                call_soon,
                callback,
            );
        }
        Err(e) => log::error!("Failed to parse orderbook deltas: {e}"),
    }

    let instrument_id = instrument.id();
    let last_quote = quote_cache.get(&instrument_id);

    match parse_orderbook_quote(msg, &instrument, last_quote, ts_init) {
        Ok(quote) => {
            quote_cache.insert(instrument_id, quote);
            send_data_to_python(Data::Quote(quote), call_soon, callback);
        }
        Err(e) => log::error!("Failed to parse orderbook quote: {e}"),
    }
}

fn handle_trade(
    msg: &crate::websocket::messages::BybitWsTradeMsg,
    product_type: Option<BybitProductType>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    trade_subs: &AtomicSet<InstrumentId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();

    for trade in &msg.data {
        let Some(instrument) = resolve_instrument(&trade.s, product_type, instruments) else {
            continue;
        };

        if product_type == Some(BybitProductType::Option)
            && !trade_subs.is_empty()
            && !trade_subs.contains(&instrument.id())
        {
            continue;
        }

        match parse_ws_trade_tick(trade, &instrument, ts_init) {
            Ok(tick) => send_data_to_python(Data::Trade(tick), call_soon, callback),
            Err(e) => log::error!("Failed to parse trade tick: {e}"),
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_kline(
    msg: &crate::websocket::messages::BybitWsKlineMsg,
    product_type: Option<BybitProductType>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    bar_types_cache: &AtomicMap<String, BarType>,
    bars_timestamp_on_close: bool,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Ok((_, raw_symbol)) = parse_kline_topic(msg.topic.as_str()) else {
        return;
    };
    let ustr_symbol = Ustr::from(raw_symbol);
    let Some(instrument) = resolve_instrument(&ustr_symbol, product_type, instruments) else {
        return;
    };
    let Some(bar_type) = bar_types_cache.load().get(msg.topic.as_str()).copied() else {
        return;
    };

    let ts_init = clock.get_time_ns();

    for kline in &msg.data {
        if !kline.confirm {
            continue;
        }

        match parse_ws_kline_bar(
            kline,
            &instrument,
            bar_type,
            bars_timestamp_on_close,
            ts_init,
        ) {
            Ok(bar) => send_data_to_python(Data::Bar(bar), call_soon, callback),
            Err(e) => log::error!("Failed to parse kline bar: {e}"),
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_ticker_linear(
    msg: &crate::websocket::messages::BybitWsTickerLinearMsg,
    product_type: Option<BybitProductType>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    quote_cache: &mut AHashMap<InstrumentId, QuoteTick>,
    funding_cache: &mut AHashMap<Ustr, (Option<String>, Option<String>)>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = resolve_instrument(&msg.data.symbol, product_type, instruments) else {
        return;
    };
    let instrument_id = instrument.id();
    let ts_init = clock.get_time_ns();

    if msg.data.bid1_price.is_some() {
        match parse_ticker_linear_quote(msg, &instrument, ts_init) {
            Ok(quote) => {
                let last = quote_cache.get(&instrument_id);

                if last.is_none_or(|q| *q != quote) {
                    quote_cache.insert(instrument_id, quote);
                    send_data_to_python(Data::Quote(quote), call_soon, callback);
                }
            }
            Err(e) => log::debug!("Skipping partial ticker update: {e}"),
        }
    }

    let ts_event = match parse_millis_i64(msg.ts, "ticker.ts") {
        Ok(ts) => ts,
        Err(e) => {
            log::error!("Failed to parse ticker timestamp: {e}");
            return;
        }
    };

    let cache_entry = funding_cache.entry(msg.data.symbol).or_insert((None, None));
    let mut changed = false;

    if let Some(rate) = &msg.data.funding_rate
        && cache_entry.0.as_ref() != Some(rate)
    {
        cache_entry.0 = Some(rate.clone());
        changed = true;
    }

    if let Some(next_time) = &msg.data.next_funding_time
        && cache_entry.1.as_ref() != Some(next_time)
    {
        cache_entry.1 = Some(next_time.clone());
        changed = true;
    }

    if changed {
        match parse_ticker_linear_funding(&msg.data, instrument_id, ts_event, ts_init) {
            Ok(update) => send_to_python(update, call_soon, callback),
            Err(e) => log::debug!("Skipping funding rate update: {e}"),
        }
    }

    if msg.data.mark_price.is_some() {
        match parse_ticker_linear_mark_price(&msg.data, &instrument, ts_event, ts_init) {
            Ok(update) => send_to_python(update, call_soon, callback),
            Err(e) => log::debug!("Skipping mark price update: {e}"),
        }
    }

    if msg.data.index_price.is_some() {
        match parse_ticker_linear_index_price(&msg.data, &instrument, ts_event, ts_init) {
            Ok(update) => send_to_python(update, call_soon, callback),
            Err(e) => log::debug!("Skipping index price update: {e}"),
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_ticker_option(
    msg: &crate::websocket::messages::BybitWsTickerOptionMsg,
    product_type: Option<BybitProductType>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    quote_cache: &mut AHashMap<InstrumentId, QuoteTick>,
    option_greeks_subs: &AtomicSet<InstrumentId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = resolve_instrument(&msg.data.symbol, product_type, instruments) else {
        return;
    };
    let instrument_id = instrument.id();
    let ts_init = clock.get_time_ns();

    match parse_ticker_option_quote(msg, &instrument, ts_init) {
        Ok(quote) => {
            let last = quote_cache.get(&instrument_id);

            if last.is_none_or(|q| *q != quote) {
                quote_cache.insert(instrument_id, quote);
                send_data_to_python(Data::Quote(quote), call_soon, callback);
            }
        }
        Err(e) => log::error!("Failed to parse ticker option quote: {e}"),
    }

    match parse_ticker_option_mark_price(msg, &instrument, ts_init) {
        Ok(update) => send_to_python(update, call_soon, callback),
        Err(e) => log::error!("Failed to parse ticker option mark price: {e}"),
    }

    match parse_ticker_option_index_price(msg, &instrument, ts_init) {
        Ok(update) => send_to_python(update, call_soon, callback),
        Err(e) => log::error!("Failed to parse ticker option index price: {e}"),
    }

    if option_greeks_subs.contains(&instrument_id) {
        match parse_ticker_option_greeks(msg, &instrument, ts_init) {
            Ok(greeks) => send_to_python(greeks, call_soon, callback),
            Err(e) => log::error!("Failed to parse option greeks: {e}"),
        }
    }
}

fn handle_account_order(
    msg: &crate::websocket::messages::BybitWsAccountOrderMsg,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: Option<AccountId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();

    for order in &msg.data {
        let symbol = make_bybit_symbol(order.symbol, order.category);
        let Some(instrument) = instruments.get_cloned(&symbol) else {
            log::warn!("No instrument for order update: {symbol}");
            continue;
        };
        let Some(account_id) = account_id else {
            continue;
        };

        match parse_ws_order_status_report(order, &instrument, account_id, ts_init) {
            Ok(report) => send_to_python(report, call_soon, callback),
            Err(e) => log::error!("Failed to parse order status report: {e}"),
        }
    }
}

fn handle_account_execution(
    msg: &crate::websocket::messages::BybitWsAccountExecutionMsg,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: Option<AccountId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();

    for exec in &msg.data {
        let symbol = make_bybit_symbol(exec.symbol, exec.category);
        let Some(instrument) = instruments.get_cloned(&symbol) else {
            log::warn!("No instrument for execution update: {symbol}");
            continue;
        };
        let Some(account_id) = account_id else {
            continue;
        };

        match parse_ws_fill_report(exec, account_id, &instrument, ts_init) {
            Ok(report) => send_to_python(report, call_soon, callback),
            Err(e) => log::error!("Failed to parse fill report: {e}"),
        }
    }
}

fn handle_account_wallet(
    msg: &crate::websocket::messages::BybitWsAccountWalletMsg,
    account_id: Option<AccountId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();
    let ts_event = parse_millis_i64(msg.creation_time, "wallet.creation_time").unwrap_or(ts_init);
    let Some(account_id) = account_id else {
        return;
    };

    for wallet in &msg.data {
        match parse_ws_account_state(wallet, account_id, ts_event, ts_init) {
            Ok(state) => send_to_python(state, call_soon, callback),
            Err(e) => log::error!("Failed to parse account state: {e}"),
        }
    }
}

fn handle_account_position(
    msg: &crate::websocket::messages::BybitWsAccountPositionMsg,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: Option<AccountId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();

    for position in &msg.data {
        let symbol = make_bybit_symbol(position.symbol, position.category);
        let Some(instrument) = instruments.get_cloned(&symbol) else {
            log::warn!("No instrument for position update: {symbol}");
            continue;
        };
        let Some(account_id) = account_id else {
            continue;
        };

        match parse_ws_position_status_report(position, account_id, &instrument, ts_init) {
            Ok(report) => send_to_python(report, call_soon, callback),
            Err(e) => log::error!("Failed to parse position status report: {e}"),
        }
    }
}

fn handle_order_response(
    resp: &crate::websocket::messages::BybitWsOrderResponse,
    pending_py_requests: &DashMap<String, Vec<PendingPyRequest>>,
    account_id: Option<AccountId>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    if resp.ret_code == 0 {
        let entries = resp
            .req_id
            .as_ref()
            .and_then(|rid| pending_py_requests.remove(rid))
            .map(|(_, v)| v);

        // Check for per-order failures in batch retExtInfo
        if let Some(entries) = entries {
            let batch_errors = resp.extract_batch_errors();
            let data_array = resp.data.as_array();
            let ts_init = clock.get_time_ns();

            for (idx, error) in batch_errors.iter().enumerate() {
                if error.code == 0 {
                    continue;
                }

                let pending = data_array
                    .and_then(|arr| arr.get(idx))
                    .and_then(|item| item.get("orderLinkId"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|oli| {
                        let cid = ClientOrderId::new(oli);
                        entries.iter().find(|e| e.client_order_id == cid)
                    })
                    .or_else(|| entries.get(idx));

                if let Some(pending) = pending {
                    let reason = Ustr::from(&error.msg);
                    emit_rejection(pending, reason, account_id, ts_init, call_soon, callback);
                } else {
                    log::warn!(
                        "Batch error at index {idx} without correlation: code={}, msg={}",
                        error.code,
                        error.msg,
                    );
                }
            }
        }
        return;
    }

    // Try to find the pending entries by req_id, then by orderLinkId
    let entries = resp
        .req_id
        .as_ref()
        .and_then(|rid| pending_py_requests.remove(rid))
        .map(|(_, v)| v)
        .or_else(|| {
            // Bybit sometimes omits req_id, search by orderLinkId instead
            let order_link_id = resp
                .data
                .get("orderLinkId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())?;
            let cid = ClientOrderId::new(order_link_id);
            let key = pending_py_requests
                .iter()
                .find(|entry| entry.value().iter().any(|e| e.client_order_id == cid))
                .map(|entry| entry.key().clone())?;
            pending_py_requests.remove(&key).map(|(_, v)| v)
        });

    let Some(entries) = entries else {
        log::warn!(
            "Unmatched order response: ret_code={}, ret_msg={}",
            resp.ret_code,
            resp.ret_msg,
        );
        return;
    };

    let ts_init = clock.get_time_ns();
    let reason = Ustr::from(&resp.ret_msg);

    for pending in &entries {
        emit_rejection(pending, reason, account_id, ts_init, call_soon, callback);
    }
}

fn emit_rejection(
    pending: &PendingPyRequest,
    reason: Ustr,
    account_id: Option<AccountId>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    match pending.operation {
        PendingOperation::Place => {
            let event = OrderRejected::new(
                pending.trader_id,
                pending.strategy_id,
                pending.instrument_id,
                pending.client_order_id,
                account_id.unwrap_or(AccountId::from("BYBIT-000")),
                reason,
                UUID4::new(),
                ts_init,
                ts_init,
                false,
                false,
            );
            send_to_python(event, call_soon, callback);
        }
        PendingOperation::Cancel => {
            let event = OrderCancelRejected::new(
                pending.trader_id,
                pending.strategy_id,
                pending.instrument_id,
                pending.client_order_id,
                reason,
                UUID4::new(),
                ts_init,
                ts_init,
                false,
                pending.venue_order_id,
                account_id,
            );
            send_to_python(event, call_soon, callback);
        }
        PendingOperation::Amend => {
            let event = OrderModifyRejected::new(
                pending.trader_id,
                pending.strategy_id,
                pending.instrument_id,
                pending.client_order_id,
                reason,
                UUID4::new(),
                ts_init,
                ts_init,
                false,
                pending.venue_order_id,
                account_id,
            );
            send_to_python(event, call_soon, callback);
        }
    }
}
