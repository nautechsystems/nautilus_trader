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

//! Python bindings for the OKX WebSocket client.
//!
//! # Design Pattern: Clone and Share State
//!
//! The WebSocket client must be cloned for async operations because PyO3's `future_into_py`
//! requires `'static` futures (cannot borrow from `self`). To ensure clones share the same
//! connection state, key fields use `Arc<RwLock<T>>`:
//!
//! - `inner: Arc<RwLock<Option<WebSocketClient>>>` - The WebSocket connection.
//!
//! Without shared state, clones would be independent, causing:
//! - Lost WebSocket messages.
//! - Missing instrument data.
//! - Connection state desynchronization.
//!
//! ## Connection Flow
//!
//! 1. Clone the client for async operation.
//! 2. Connect and populate shared state on the clone.
//! 3. Spawn stream handler as background task.
//! 4. Return immediately (non-blocking).
//!
//! ## Important Notes
//!
//! - Never use `block_on()` - it blocks the runtime.
//! - Always clone before async blocks for lifetime requirements.
//! - RwLock is preferred over Mutex (many reads, few writes).

use std::str::FromStr;

use ahash::{AHashMap, AHashSet};
use futures_util::StreamExt;
use nautilus_common::{cache::quote::QuoteCache, live::get_runtime};
use nautilus_core::{
    UUID4, UnixNanos,
    python::{call_python_threadsafe, to_pyruntime_err, to_pyvalue_err},
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{BarType, Data, InstrumentStatus, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, PositionSide, TimeInForce},
    events::{OrderAccepted, OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
    types::{Money, Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use pyo3::{IntoPyObjectExt, prelude::*, types::PyDict};
use ustr::Ustr;

use super::{extract_optional_string, extract_optional_trigger_type};
use crate::{
    common::{
        consts::{OKX_FIELD_CLORDID, OKX_FIELD_SCODE, OKX_FIELD_SMSG, OKX_SUCCESS_CODE},
        enums::{
            OKXBookAction, OKXGreeksType, OKXInstrumentStatus, OKXInstrumentType, OKXTradeMode,
            OKXVipLevel,
        },
        models::OKXInstrument,
        parse::{
            okx_status_to_market_action, parse_account_state, parse_instrument_any,
            parse_millisecond_timestamp, parse_position_status_report, parse_price, parse_quantity,
        },
    },
    http::models::{OKXAccount, OKXPosition},
    websocket::{
        OKXWebSocketClient,
        enums::{OKXWsChannel, OKXWsOperation},
        messages::{
            ExecutionReport, NautilusWsMessage, OKXAlgoOrderMsg, OKXBookMsg, OKXOptionSummaryMsg,
            OKXOrderMsg, OKXWebSocketError, OKXWsMessage, WsAttachAlgoOrdParams,
            WsAttachAlgoOrdParamsBuilder,
        },
        parse::{
            extract_fees_from_cached_instrument, parse_algo_order_msg, parse_book_msg_vec,
            parse_index_price_msg_vec, parse_option_summary_greeks, parse_order_msg_vec,
            parse_ws_message_data,
        },
    },
};

fn parse_attach_algo_ords(
    py: Python<'_>,
    attach_algo_ords: Option<Vec<Py<PyDict>>>,
) -> PyResult<Option<Vec<WsAttachAlgoOrdParams>>> {
    attach_algo_ords
        .map(|items| {
            items
                .into_iter()
                .map(|item| {
                    let dict = item.bind(py);
                    let mut builder = WsAttachAlgoOrdParamsBuilder::default();

                    if let Some(value) = extract_optional_string(dict, "attach_algo_cl_ord_id")? {
                        builder.attach_algo_cl_ord_id(value);
                    }

                    if let Some(value) = extract_optional_string(dict, "sl_trigger_px")? {
                        builder.sl_trigger_px(value);
                    }

                    if let Some(value) = extract_optional_string(dict, "sl_ord_px")? {
                        builder.sl_ord_px(value);
                    }

                    if let Some(value) = extract_optional_trigger_type(dict, "sl_trigger_px_type")?
                    {
                        builder.sl_trigger_px_type(value);
                    }

                    if let Some(value) = extract_optional_string(dict, "tp_trigger_px")? {
                        builder.tp_trigger_px(value);
                    }

                    if let Some(value) = extract_optional_string(dict, "tp_ord_px")? {
                        builder.tp_ord_px(value);
                    }

                    if let Some(value) = extract_optional_trigger_type(dict, "tp_trigger_px_type")?
                    {
                        builder.tp_trigger_px_type(value);
                    }

                    builder.build().map_err(to_pyvalue_err)
                })
                .collect::<PyResult<Vec<_>>>()
        })
        .transpose()
}

#[pyo3::pymethods]
impl OKXWebSocketError {
    #[getter]
    pub fn code(&self) -> &str {
        &self.code
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
    pub fn ts_event(&self) -> u64 {
        self.timestamp
    }

    fn __repr__(&self) -> String {
        format!(
            "OKXWebSocketError(code='{}', message='{}', conn_id={:?}, ts_event={})",
            self.code, self.message, self.conn_id, self.timestamp
        )
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OKXWebSocketClient {
    /// Provides a WebSocket client for connecting to [OKX](https://okx.com).
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, account_id=None, heartbeat=None, auth_timeout_secs=None, proxy_url=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        auth_timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            url,
            api_key,
            api_secret,
            api_passphrase,
            account_id,
            heartbeat,
            auth_timeout_secs,
            TransportBackend::default(),
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, account_id=None, heartbeat=None, auth_timeout_secs=None, proxy_url=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_with_credentials(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        auth_timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::with_credentials(
            url,
            api_key,
            api_secret,
            api_passphrase,
            account_id,
            heartbeat,
            auth_timeout_secs,
            TransportBackend::default(),
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    pub fn py_api_key(&self) -> Option<&str> {
        self.api_key()
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> Option<String> {
        self.api_key_masked()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&mut self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&mut self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    #[pyo3(name = "get_subscriptions")]
    fn py_get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<String> {
        let channels = self.get_subscriptions(instrument_id);

        // Convert to OKX channel names
        channels
            .iter()
            .map(|c| {
                serde_json::to_value(c)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| c.to_string())
            })
            .collect()
    }

    /// Sets the VIP level for this client.
    ///
    /// The VIP level determines which WebSocket channels are available.
    #[pyo3(name = "set_vip_level")]
    fn py_set_vip_level(&self, vip_level: OKXVipLevel) {
        self.set_vip_level(vip_level);
    }

    /// Gets the current VIP level.
    #[pyo3(name = "vip_level")]
    #[getter]
    fn py_vip_level(&self) -> OKXVipLevel {
        self.vip_level()
    }

    #[pyo3(name = "connect")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let mut instruments_any = Vec::new();

        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        self.cache_instruments(&instruments_any);

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();
            let clock = get_atomic_clock_realtime();

            get_runtime().spawn(async move {
                let account_id = client.account_id;
                let mut instruments_by_symbol = client.instruments_snapshot();
                let mut quote_cache = QuoteCache::new();
                let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();
                let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
                let mut filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
                let option_greeks_subs_arc = client.option_greeks_subs().clone();
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        OKXWsMessage::BookData { arg, action, data } => {
                            handle_book_data(
                                arg.inst_id,
                                action,
                                data,
                                &instruments_by_symbol,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::ChannelData {
                            channel,
                            inst_id,
                            data,
                        } => {
                            let greeks_guard = option_greeks_subs_arc.load();
                            handle_channel_data(
                                &channel,
                                inst_id,
                                data,
                                &mut instruments_by_symbol,
                                &mut quote_cache,
                                &mut funding_cache,
                                &greeks_guard,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::Instruments(okx_instruments) => {
                            handle_instruments(
                                okx_instruments,
                                &mut instruments_by_symbol,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::Orders(order_msgs) => {
                            handle_orders(
                                &order_msgs,
                                account_id,
                                &instruments_by_symbol,
                                &mut fee_cache,
                                &mut filled_qty_cache,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::AlgoOrders(algo_msgs) => {
                            handle_algo_orders(
                                algo_msgs,
                                account_id,
                                &instruments_by_symbol,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::Account(data) => {
                            handle_account(data, account_id, clock, &call_soon, &callback);
                        }
                        OKXWsMessage::Positions(data) => {
                            handle_positions(
                                data,
                                account_id,
                                &instruments_by_symbol,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::OrderResponse {
                            id,
                            op,
                            code,
                            msg,
                            data,
                        } => {
                            handle_order_response(
                                id.as_deref(),
                                &op,
                                &code,
                                &msg,
                                &data,
                                &client,
                                account_id,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::SendFailed {
                            request_id,
                            client_order_id,
                            op,
                            error,
                        } => {
                            handle_send_failed(
                                &request_id,
                                client_order_id,
                                op.as_ref(),
                                &error,
                                &client,
                                account_id,
                                clock,
                                &call_soon,
                                &callback,
                            );
                        }
                        OKXWsMessage::Error(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        OKXWsMessage::Reconnected => {
                            quote_cache.clear();
                        }
                        OKXWsMessage::Authenticated => {}
                    }
                }
            });

            Ok(())
        })
    }

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

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instruments")]
    fn py_subscribe_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instruments(instrument_type).await {
                log::error!("Failed to subscribe to instruments '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instrument")]
    fn py_subscribe_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instrument(instrument_id).await {
                log::error!("Failed to subscribe to instrument {instrument_id}: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book")]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_book50_l2_tbt")]
    fn py_subscribe_book50_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book50_l2_tbt(instrument_id).await {
                log::error!("Failed to subscribe to book50_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_l2_tbt")]
    fn py_subscribe_book_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_l2_tbt(instrument_id).await {
                log::error!("Failed to subscribe to books_l2_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_with_depth")]
    fn py_subscribe_book_with_depth<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: u16,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book_with_depth(instrument_id, depth)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_book_depth5")]
    fn py_subscribe_book_depth5<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_depth5(instrument_id).await {
                log::error!("Failed to subscribe to books5: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        aggregated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_id, aggregated).await {
                log::error!("Failed to subscribe to trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_bars(bar_type).await {
                log::error!("Failed to subscribe to bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book(instrument_id).await {
                log::error!("Failed to unsubscribe from order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_depth5")]
    fn py_unsubscribe_book_depth5<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_depth5(instrument_id).await {
                log::error!("Failed to unsubscribe from books5: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book50_l2_tbt")]
    fn py_unsubscribe_book50_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book50_l2_tbt(instrument_id).await {
                log::error!("Failed to unsubscribe from books50_l2_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_l2_tbt")]
    fn py_unsubscribe_book_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_l2_tbt(instrument_id).await {
                log::error!("Failed to unsubscribe from books_l2_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_quotes(instrument_id).await {
                log::error!("Failed to unsubscribe from quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        aggregated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_id, aggregated).await {
                log::error!("Failed to unsubscribe from trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_bars(bar_type).await {
                log::error!("Failed to unsubscribe from bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_ticker")]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_ticker(instrument_id).await {
                log::error!("Failed to subscribe to ticker: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_ticker")]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_ticker(instrument_id).await {
                log::error!("Failed to unsubscribe from ticker: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_mark_prices")]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_mark_prices(instrument_id).await {
                log::error!("Failed to subscribe to mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_mark_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_index_prices")]
    fn py_subscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_index_prices(instrument_id).await {
                log::error!("Failed to subscribe to index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    fn py_unsubscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_index_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "add_option_greeks_sub")]
    fn py_add_option_greeks_sub(&self, instrument_id: InstrumentId) {
        self.add_option_greeks_sub(instrument_id);
    }

    #[pyo3(name = "add_option_greeks_sub_with_conventions")]
    fn py_add_option_greeks_sub_with_conventions(
        &self,
        instrument_id: InstrumentId,
        conventions: Vec<OKXGreeksType>,
    ) {
        self.add_option_greeks_sub_with_conventions(
            instrument_id,
            conventions.into_iter().collect(),
        );
    }

    #[pyo3(name = "remove_option_greeks_sub")]
    fn py_remove_option_greeks_sub(&self, instrument_id: InstrumentId) {
        self.remove_option_greeks_sub(&instrument_id);
    }

    #[pyo3(name = "subscribe_option_summary")]
    fn py_subscribe_option_summary<'py>(
        &self,
        py: Python<'py>,
        inst_family: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let family = Ustr::from(inst_family);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_option_summary(family).await {
                log::error!("Failed to subscribe to option summary: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_option_summary")]
    fn py_unsubscribe_option_summary<'py>(
        &self,
        py: Python<'py>,
        inst_family: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let family = Ustr::from(inst_family);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_option_summary(family).await {
                log::error!("Failed to unsubscribe from option summary: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_funding_rates")]
    fn py_subscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_funding_rates(instrument_id).await {
                log::error!("Failed to subscribe to funding rates: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_funding_rates")]
    fn py_unsubscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_funding_rates(instrument_id).await {
                log::error!("Failed to unsubscribe from funding rates: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders(instrument_type).await {
                log::error!("Failed to subscribe to orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders(instrument_type).await {
                log::error!("Failed to unsubscribe from orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orders_algo")]
    fn py_subscribe_orders_algo<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders_algo(instrument_type).await {
                log::error!("Failed to subscribe to algo orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders_algo")]
    fn py_unsubscribe_orders_algo<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders_algo(instrument_type).await {
                log::error!("Failed to unsubscribe from algo orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_algo_advance")]
    fn py_subscribe_algo_advance<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_algo_advance(instrument_type).await {
                log::error!("Failed to subscribe to algo-advance '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_algo_advance")]
    fn py_unsubscribe_algo_advance<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_algo_advance(instrument_type).await {
                log::error!("Failed to unsubscribe from algo-advance '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_fills")]
    fn py_subscribe_fills<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_fills(instrument_type).await {
                log::error!("Failed to subscribe to fills '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_fills")]
    fn py_unsubscribe_fills<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_fills(instrument_type).await {
                log::error!("Failed to unsubscribe from fills '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_account")]
    fn py_subscribe_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_account().await {
                log::error!("Failed to subscribe to account: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_account")]
    fn py_unsubscribe_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_account().await {
                log::error!("Failed to unsubscribe from account: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        td_mode,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force=None,
        price=None,
        trigger_price=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        position_side=None,
        attach_algo_ords=None,
        px_usd=None,
        px_vol=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        position_side: Option<PositionSide>,
        attach_algo_ords: Option<Vec<Py<PyDict>>>,
        px_usd: Option<String>,
        px_vol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let attach_algo_ords = parse_attach_algo_ords(py, attach_algo_ords)?;
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    td_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    post_only,
                    reduce_only,
                    quote_quantity,
                    position_side,
                    attach_algo_ords,
                    px_usd,
                    px_vol,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "cancel_order", signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
    ))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
        price=None,
        quantity=None,
        new_px_usd=None,
        new_px_vol=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        price: Option<Price>,
        quantity: Option<Quantity>,
        new_px_usd: Option<String>,
        new_px_vol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    price,
                    quantity,
                    venue_order_id,
                    new_px_usd,
                    new_px_vol,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[expect(clippy::type_complexity)]
    #[pyo3(name = "batch_submit_orders")]
    fn py_batch_submit_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (
                instrument_type,
                instrument_id,
                td_mode,
                client_order_id,
                order_side,
                order_type,
                quantity,
                position_side,
                price,
                trigger_price,
                post_only,
                reduce_only,
            ): (
                OKXInstrumentType,
                InstrumentId,
                OKXTradeMode,
                ClientOrderId,
                OrderSide,
                OrderType,
                Quantity,
                Option<PositionSide>,
                Option<Price>,
                Option<Price>,
                Option<bool>,
                Option<bool>,
            ) = obj.extract(py).map_err(to_pyruntime_err)?;

            domain_orders.push((
                instrument_type,
                instrument_id,
                td_mode,
                client_order_id,
                order_side,
                position_side,
                order_type,
                quantity,
                price,
                trigger_price,
                post_only,
                reduce_only,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_submit_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Cancels multiple orders via WebSocket.
    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        cancels: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut batched_cancels = Vec::with_capacity(cancels.len());

        for obj in cancels {
            let (instrument_id, client_order_id, order_id): (
                InstrumentId,
                Option<ClientOrderId>,
                Option<VenueOrderId>,
            ) = obj.extract(py).map_err(to_pyruntime_err)?;
            batched_cancels.push((instrument_id, client_order_id, order_id));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_cancel_orders(batched_cancels)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "batch_modify_orders")]
    fn py_batch_modify_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (
                instrument_type,
                instrument_id,
                client_order_id,
                new_client_order_id,
                price,
                quantity,
            ): (
                String,
                InstrumentId,
                ClientOrderId,
                ClientOrderId,
                Option<Price>,
                Option<Quantity>,
            ) = obj.extract(py).map_err(to_pyruntime_err)?;
            let inst_type =
                OKXInstrumentType::from_str(&instrument_type).map_err(to_pyvalue_err)?;
            domain_orders.push((
                inst_type,
                instrument_id,
                client_order_id,
                new_client_order_id,
                price,
                quantity,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_modify_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "mass_cancel_orders")]
    fn py_mass_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .mass_cancel_orders(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(&self, py: Python<'_>, instruments: Vec<Py<PyAny>>) -> PyResult<()> {
        let instruments: Result<Vec<_>, _> = instruments
            .into_iter()
            .map(|inst| pyobject_to_instrument_any(py, inst))
            .collect();
        self.cache_instruments(&instruments?);
        Ok(())
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "cache_inst_id_codes")]
    fn py_cache_inst_id_codes(&self, mappings: Vec<(String, u64)>) {
        let ustr_mappings = mappings
            .into_iter()
            .map(|(inst_id, code)| (Ustr::from(&inst_id), code));
        self.cache_inst_id_codes(ustr_mappings);
    }
}

fn handle_book_data(
    inst_id: Option<Ustr>,
    action: OKXBookAction,
    data: Vec<OKXBookMsg>,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(inst_id) = inst_id else { return };
    let Some(instrument) = instruments_by_symbol.get(&inst_id) else {
        log::warn!("No cached instrument for book data: {inst_id}");
        return;
    };
    let ts_init = clock.get_time_ns();

    match parse_book_msg_vec(
        data,
        &instrument.id(),
        instrument.price_precision(),
        instrument.size_precision(),
        action,
        ts_init,
    ) {
        Ok(data_vec) => Python::attach(|py| {
            for d in data_vec {
                let py_obj = data_to_pycapsule(py, d);
                call_python_threadsafe(py, call_soon, callback, py_obj);
            }
        }),
        Err(e) => log::error!("Failed to parse book data: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_channel_data(
    channel: &OKXWsChannel,
    inst_id: Option<Ustr>,
    data: serde_json::Value,
    instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
    quote_cache: &mut QuoteCache,
    funding_cache: &mut AHashMap<Ustr, (Ustr, u64)>,
    option_greeks_subs: &AHashMap<InstrumentId, AHashSet<OKXGreeksType>>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    if matches!(channel, OKXWsChannel::OptionSummary) {
        let ts_init = clock.get_time_ns();

        match serde_json::from_value::<Vec<OKXOptionSummaryMsg>>(data) {
            Ok(msgs) => {
                for msg in &msgs {
                    let Some(instrument) = instruments_by_symbol.get(&msg.inst_id) else {
                        continue;
                    };
                    let instrument_id = instrument.id();
                    let Some(conventions) = option_greeks_subs.get(&instrument_id) else {
                        continue;
                    };

                    for greeks_type in conventions {
                        match parse_option_summary_greeks(
                            msg,
                            &instrument_id,
                            *greeks_type,
                            ts_init,
                        ) {
                            Ok(greeks) => {
                                Python::attach(|py| match greeks.into_py_any(py) {
                                    Ok(py_obj) => {
                                        call_python_threadsafe(py, call_soon, callback, py_obj);
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to convert OptionGreeks to Python: {e}"
                                        );
                                    }
                                });
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to parse option summary for {} ({greeks_type:?}): {e}",
                                    msg.inst_id
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => log::error!("Failed to deserialize option summary data: {e}"),
        }
        return;
    }

    let Some(inst_id) = inst_id else { return };

    if matches!(channel, OKXWsChannel::IndexTickers) {
        let ts_init = clock.get_time_ns();
        let prefix = format!("{inst_id}-");
        let matching: Vec<_> = instruments_by_symbol
            .values()
            .filter(|i| {
                let s = i.symbol().inner();
                s == inst_id || s.as_str().starts_with(&prefix)
            })
            .collect();

        for instrument in matching {
            if let Ok(data_vec) = parse_index_price_msg_vec(
                data.clone(),
                &instrument.id(),
                instrument.price_precision(),
                ts_init,
            ) {
                Python::attach(|py| {
                    for d in data_vec {
                        let py_obj = data_to_pycapsule(py, d);
                        call_python_threadsafe(py, call_soon, callback, py_obj);
                    }
                });
            }
        }
        return;
    }

    let Some(instrument) = instruments_by_symbol.get(&inst_id) else {
        log::warn!("No cached instrument for {channel:?}: {inst_id}");
        return;
    };
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_init = clock.get_time_ns();

    if matches!(channel, OKXWsChannel::BboTbt) {
        handle_bbo_tbt(
            data,
            instrument_id,
            price_precision,
            size_precision,
            ts_init,
            quote_cache,
            call_soon,
            callback,
        );
        return;
    }

    match parse_ws_message_data(
        channel,
        data,
        &instrument_id,
        price_precision,
        size_precision,
        ts_init,
        funding_cache,
        instruments_by_symbol,
    ) {
        Ok(Some(ws_msg)) => {
            dispatch_nautilus_ws_msg_to_python(ws_msg, call_soon, callback, instruments_by_symbol);
        }
        Ok(None) => {}
        Err(e) => {
            log::error!("Failed to parse {channel:?} data: {e}");
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_bbo_tbt(
    data: serde_json::Value,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
    quote_cache: &mut QuoteCache,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let msgs: Vec<OKXBookMsg> = match serde_json::from_value(data) {
        Ok(msgs) => msgs,
        Err(e) => {
            log::error!("Failed to deserialize BboTbt data: {e}");
            return;
        }
    };

    for msg in &msgs {
        let bid = msg.bids.first();
        let ask = msg.asks.first();

        let bid_price = bid.and_then(|e| parse_price(&e.price, price_precision).ok());
        let bid_size = bid.and_then(|e| parse_quantity(&e.size, size_precision).ok());
        let ask_price = ask.and_then(|e| parse_price(&e.price, price_precision).ok());
        let ask_size = ask.and_then(|e| parse_quantity(&e.size, size_precision).ok());
        let ts_event = parse_millisecond_timestamp(msg.ts);

        match quote_cache.process(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        ) {
            Ok(quote) => {
                Python::attach(|py| {
                    let py_obj = data_to_pycapsule(py, Data::Quote(quote));
                    call_python_threadsafe(py, call_soon, callback, py_obj);
                });
            }
            Err(e) => {
                log::debug!("Skipping partial BboTbt for {instrument_id}: {e}");
            }
        }
    }
}

fn handle_instruments(
    okx_instruments: Vec<OKXInstrument>,
    instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();

    for okx_inst in okx_instruments {
        let inst_key = Ustr::from(&okx_inst.inst_id);
        let (margin_init, margin_maint, maker_fee, taker_fee) =
            instruments_by_symbol.get(&inst_key).map_or(
                (None, None, None, None),
                extract_fees_from_cached_instrument,
            );
        let status_action = okx_status_to_market_action(okx_inst.state);
        let is_live = matches!(okx_inst.state, OKXInstrumentStatus::Live);

        if let Ok(Some(inst_any)) = parse_instrument_any(
            &okx_inst,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
        ) {
            let instrument_id = inst_any.id();
            instruments_by_symbol.insert(inst_any.symbol().inner(), inst_any.clone());
            call_python_with_data(call_soon, callback, |py| {
                instrument_any_to_pyobject(py, inst_any)
            });
            let status = InstrumentStatus::new(
                instrument_id,
                status_action,
                ts_init,
                ts_init,
                None,
                None,
                Some(is_live),
                None,
                None,
            );
            call_python_with_data(call_soon, callback, |py| status.into_py_any(py));
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_orders(
    order_msgs: &[OKXOrderMsg],
    account_id: AccountId,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();

    match parse_order_msg_vec(
        order_msgs,
        account_id,
        instruments_by_symbol,
        fee_cache,
        filled_qty_cache,
        ts_init,
    ) {
        Ok(reports) => {
            dispatch_execution_reports_to_python(reports, call_soon, callback);
        }
        Err(e) => {
            log::error!("Failed to parse order messages: {e}");
        }
    }
}

fn handle_algo_orders(
    algo_msgs: Vec<OKXAlgoOrderMsg>,
    account_id: AccountId,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let ts_init = clock.get_time_ns();
    for algo_msg in algo_msgs {
        match parse_algo_order_msg(&algo_msg, account_id, instruments_by_symbol, ts_init) {
            Ok(Some(report)) => {
                dispatch_execution_reports_to_python(vec![report], call_soon, callback);
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("Failed to parse algo order: {e}");
            }
        }
    }
}

fn handle_account(
    data: serde_json::Value,
    account_id: AccountId,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    if let Ok(accounts) = serde_json::from_value::<Vec<OKXAccount>>(data) {
        let ts_init = clock.get_time_ns();
        for account in &accounts {
            if let Ok(account_state) = parse_account_state(account, account_id, ts_init) {
                call_python_with_data(call_soon, callback, |py| account_state.into_py_any(py));
            }
        }
    }
}

fn handle_positions(
    data: serde_json::Value,
    account_id: AccountId,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    if let Ok(positions) = serde_json::from_value::<Vec<OKXPosition>>(data) {
        let ts_init = clock.get_time_ns();

        for position in positions {
            let inst_key = Ustr::from(&position.inst_id);
            if let Some(instrument) = instruments_by_symbol.get(&inst_key) {
                match parse_position_status_report(
                    &position,
                    account_id,
                    instrument.id(),
                    instrument.size_precision(),
                    ts_init,
                ) {
                    Ok(report) => {
                        call_python_with_data(call_soon, callback, |py| report.into_py_any(py));
                    }
                    Err(e) => {
                        log::error!("Failed to parse position: {e}");
                    }
                }
            }
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_order_response(
    id: Option<&str>,
    op: &OKXWsOperation,
    code: &str,
    msg: &str,
    data: &[serde_json::Value],
    client: &OKXWebSocketClient,
    account_id: AccountId,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    for item in data {
        let s_code = item
            .get(OKX_FIELD_SCODE)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let s_msg = item
            .get(OKX_FIELD_SMSG)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let cl_ord_id = item
            .get(OKX_FIELD_CLORDID)
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if s_code == OKX_SUCCESS_CODE {
            log::debug!("Order response ok: op={op:?} cl_ord_id={cl_ord_id}");
            match op {
                OKXWsOperation::Order | OKXWsOperation::BatchOrders => {
                    if let Some((_, info)) = client.pending_orders.remove(cl_ord_id) {
                        let venue_order_id = item
                            .get("ordId")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty());

                        if let Some(ord_id) = venue_order_id {
                            let ts_init = clock.get_time_ns();
                            let accepted = OrderAccepted::new(
                                info.trader_id,
                                info.strategy_id,
                                info.instrument_id,
                                ClientOrderId::from(cl_ord_id),
                                VenueOrderId::new(ord_id),
                                account_id,
                                UUID4::new(),
                                ts_init,
                                ts_init,
                                false,
                            );
                            call_python_with_data(call_soon, callback, |py| {
                                accepted.into_py_any(py)
                            });
                        } else {
                            log::error!(
                                "No venue_order_id for accepted order: cl_ord_id={cl_ord_id}"
                            );
                        }
                    }
                }
                OKXWsOperation::OrderAlgo => {
                    client.pending_orders.remove(cl_ord_id);
                    log::debug!("Algo order placement confirmed: cl_ord_id={cl_ord_id}");
                }
                OKXWsOperation::CancelOrder
                | OKXWsOperation::BatchCancelOrders
                | OKXWsOperation::MassCancel
                | OKXWsOperation::CancelAlgos => {
                    client.pending_cancels.remove(cl_ord_id);
                }
                OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders => {
                    client.pending_amends.remove(cl_ord_id);
                }
                _ => {}
            }
        } else if !cl_ord_id.is_empty() {
            log::warn!(
                "Order response rejected: op={op:?} cl_ord_id={cl_ord_id} \
                 s_code={s_code} s_msg={s_msg}"
            );
            let ts_init = clock.get_time_ns();
            let client_order_id = ClientOrderId::from(cl_ord_id);
            let venue_order_id = item
                .get("ordId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(VenueOrderId::new);

            match op {
                OKXWsOperation::Order | OKXWsOperation::BatchOrders | OKXWsOperation::OrderAlgo => {
                    if let Some((_, info)) = client.pending_orders.remove(cl_ord_id) {
                        let rejected = OrderRejected::new(
                            info.trader_id,
                            info.strategy_id,
                            info.instrument_id,
                            client_order_id,
                            account_id,
                            Ustr::from(s_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            false,
                        );
                        call_python_with_data(call_soon, callback, |py| rejected.into_py_any(py));
                    }
                }
                OKXWsOperation::CancelOrder
                | OKXWsOperation::BatchCancelOrders
                | OKXWsOperation::MassCancel
                | OKXWsOperation::CancelAlgos => {
                    if let Some((_, info)) = client.pending_cancels.remove(cl_ord_id) {
                        let rejected = OrderCancelRejected::new(
                            info.trader_id,
                            info.strategy_id,
                            info.instrument_id,
                            client_order_id,
                            Ustr::from(s_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        call_python_with_data(call_soon, callback, |py| rejected.into_py_any(py));
                    }
                }
                OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders => {
                    if let Some((_, info)) = client.pending_amends.remove(cl_ord_id) {
                        let rejected = OrderModifyRejected::new(
                            info.trader_id,
                            info.strategy_id,
                            info.instrument_id,
                            client_order_id,
                            Ustr::from(s_msg),
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        call_python_with_data(call_soon, callback, |py| rejected.into_py_any(py));
                    }
                }
                _ => {}
            }
        }
    }

    if code != "0" && data.is_empty() {
        log::warn!("Order response error (no data): id={id:?} op={op:?} code={code} msg={msg}");
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_send_failed(
    request_id: &str,
    client_order_id: Option<ClientOrderId>,
    op: Option<&OKXWsOperation>,
    error: &str,
    client: &OKXWebSocketClient,
    account_id: AccountId,
    clock: &AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    log::error!("WebSocket send failed: request_id={request_id} error={error}");

    let Some(client_order_id) = client_order_id else {
        return;
    };
    let cl_ord_str = client_order_id.to_string();
    let ts_init = clock.get_time_ns();

    match op {
        Some(OKXWsOperation::Order | OKXWsOperation::BatchOrders | OKXWsOperation::OrderAlgo) => {
            if let Some((_, info)) = client.pending_orders.remove(&cl_ord_str) {
                let rejected = OrderRejected::new(
                    info.trader_id,
                    info.strategy_id,
                    info.instrument_id,
                    client_order_id,
                    account_id,
                    Ustr::from(error),
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    false,
                );
                call_python_with_data(call_soon, callback, |py| rejected.into_py_any(py));
            }
        }
        Some(
            OKXWsOperation::CancelOrder
            | OKXWsOperation::BatchCancelOrders
            | OKXWsOperation::MassCancel
            | OKXWsOperation::CancelAlgos,
        ) => {
            if let Some((_, info)) = client.pending_cancels.remove(&cl_ord_str) {
                let rejected = OrderCancelRejected::new(
                    info.trader_id,
                    info.strategy_id,
                    info.instrument_id,
                    client_order_id,
                    Ustr::from(error),
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    None,
                    Some(account_id),
                );
                call_python_with_data(call_soon, callback, |py| rejected.into_py_any(py));
            }
        }
        Some(OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders) => {
            if let Some((_, info)) = client.pending_amends.remove(&cl_ord_str) {
                let rejected = OrderModifyRejected::new(
                    info.trader_id,
                    info.strategy_id,
                    info.instrument_id,
                    client_order_id,
                    Ustr::from(error),
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    None,
                    Some(account_id),
                );
                call_python_with_data(call_soon, callback, |py| rejected.into_py_any(py));
            }
        }
        _ => {
            log::warn!("SendFailed for {client_order_id} with unknown op, cannot emit rejection");
        }
    }
}

fn call_python_with_data<F>(call_soon: &Py<PyAny>, callback: &Py<PyAny>, data_converter: F)
where
    F: FnOnce(Python) -> PyResult<Py<PyAny>>,
{
    Python::attach(|py| match data_converter(py) {
        Ok(py_obj) => call_python_threadsafe(py, call_soon, callback, py_obj),
        Err(e) => log::error!("Failed to convert data to Python object: {e}"),
    });
}

fn dispatch_nautilus_ws_msg_to_python(
    msg: NautilusWsMessage,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
    instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
) {
    match msg {
        NautilusWsMessage::Data(payloads) => Python::attach(|py| {
            for data in payloads {
                let py_obj = data_to_pycapsule(py, data);
                call_python_threadsafe(py, call_soon, callback, py_obj);
            }
        }),
        NautilusWsMessage::Deltas(deltas) => Python::attach(|py| {
            let py_obj = data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(deltas)));
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }),
        NautilusWsMessage::FundingRates(updates) => {
            for data in updates {
                call_python_with_data(call_soon, callback, |py| data.into_py_any(py));
            }
        }
        NautilusWsMessage::Instrument(instrument, status) => {
            instruments_by_symbol.insert(instrument.symbol().inner(), (*instrument).clone());
            call_python_with_data(call_soon, callback, |py| {
                instrument_any_to_pyobject(py, *instrument)
            });

            if let Some(status) = status {
                call_python_with_data(call_soon, callback, |py| status.into_py_any(py));
            }
        }
        _ => {}
    }
}

fn dispatch_execution_reports_to_python(
    reports: Vec<ExecutionReport>,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    for report in reports {
        match report {
            ExecutionReport::Order(report) => {
                call_python_with_data(call_soon, callback, |py| report.into_py_any(py));
            }
            ExecutionReport::Fill(report) => {
                call_python_with_data(call_soon, callback, |py| report.into_py_any(py));
            }
        }
    }
}
