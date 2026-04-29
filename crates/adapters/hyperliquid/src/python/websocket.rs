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

//! Python bindings for the Hyperliquid WebSocket client.

use nautilus_common::live::get_runtime;
use nautilus_core::python::{call_python_threadsafe, to_pyruntime_err};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use nautilus_network::websocket::TransportBackend;
use pyo3::{conversion::IntoPyObjectExt, prelude::*};

use crate::{
    common::enums::HyperliquidEnvironment,
    websocket::{
        HyperliquidWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage},
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidWebSocketClient {
    /// Hyperliquid WebSocket client following the BitMEX pattern.
    ///
    /// Orchestrates WebSocket connection and subscriptions using a command-based architecture,
    /// where the inner FeedHandler owns the WebSocketClient and handles all I/O.
    #[new]
    #[pyo3(signature = (url=None, environment=HyperliquidEnvironment::Mainnet, account_id=None, proxy_url=None))]
    fn py_new(
        url: Option<String>,
        environment: HyperliquidEnvironment,
        account_id: Option<String>,
        proxy_url: Option<String>,
    ) -> Self {
        let account_id = account_id.map(|s| AccountId::from(s.as_str()));
        Self::new(
            url,
            environment,
            account_id,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Returns the URL of this WebSocket client.
    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> String {
        self.url().to_string()
    }

    /// Returns true if the WebSocket is actively connected.
    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        !self.is_active()
    }

    /// Caches spot fill coin mappings for instrument lookup.
    ///
    /// Hyperliquid WebSocket fills for spot use `@{pair_index}` format (e.g., `@107`),
    /// while instruments are identified by full symbols (e.g., `HYPE-USDC-SPOT`).
    /// This mapping allows the handler to look up instruments from spot fills.
    #[pyo3(name = "cache_spot_fill_coins")]
    fn py_cache_spot_fill_coins(&self, mapping: std::collections::HashMap<String, String>) {
        let ahash_mapping: ahash::AHashMap<ustr::Ustr, ustr::Ustr> = mapping
            .into_iter()
            .map(|(k, v)| (ustr::Ustr::from(&k), ustr::Ustr::from(&v)))
            .collect();
        self.cache_spot_fill_coins(ahash_mapping);
    }

    /// Caches a cloid (hex hash) to client_order_id mapping for order/fill resolution.
    ///
    /// The cloid is a keccak256 hash of the client_order_id that Hyperliquid uses internally.
    /// This mapping allows WebSocket order status and fill reports to be resolved back to
    /// the original client_order_id.
    ///
    /// This writes directly to a shared cache that the handler reads from, avoiding any
    /// race conditions between caching and WebSocket message processing.
    #[pyo3(name = "cache_cloid_mapping")]
    fn py_cache_cloid_mapping(&self, cloid: &str, client_order_id: ClientOrderId) {
        self.cache_cloid_mapping(ustr::Ustr::from(cloid), client_order_id);
    }

    /// Removes a cloid mapping from the cache.
    ///
    /// Should be called when an order reaches a terminal state (filled, canceled, expired)
    /// to prevent unbounded memory growth in long-running sessions.
    #[pyo3(name = "remove_cloid_mapping")]
    fn py_remove_cloid_mapping(&self, cloid: &str) {
        self.remove_cloid_mapping(&ustr::Ustr::from(cloid));
    }

    /// Clears all cloid mappings from the cache.
    ///
    /// Useful for cleanup during reconnection or shutdown.
    #[pyo3(name = "clear_cloid_cache")]
    fn py_clear_cloid_cache(&self) {
        self.clear_cloid_cache();
    }

    /// Returns the number of cloid mappings in the cache.
    #[pyo3(name = "cloid_cache_len")]
    fn py_cloid_cache_len(&self) -> usize {
        self.cloid_cache_len()
    }

    /// Looks up a client_order_id by its cloid hash.
    ///
    /// Returns `Some(ClientOrderId)` if the mapping exists, `None` otherwise.
    #[pyo3(name = "get_cloid_mapping")]
    fn py_get_cloid_mapping(&self, cloid: &str) -> Option<ClientOrderId> {
        self.get_cloid_mapping(&ustr::Ustr::from(cloid))
    }

    /// Establishes WebSocket connection and spawns the message handler.
    #[pyo3(name = "connect")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_connect<'py>(
        &self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            self.cache_instrument(inst_any);
        }

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            get_runtime().spawn(async move {
                loop {
                    let event = client.next_event().await;

                    match event {
                        Some(msg) => {
                            log::trace!("Received WebSocket message: {msg:?}");

                            match msg {
                                NautilusWsMessage::Trades(trade_ticks) => {
                                    Python::attach(|py| {
                                        for tick in trade_ticks {
                                            let py_obj = data_to_pycapsule(py, Data::Trade(tick));
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        }
                                    });
                                }
                                NautilusWsMessage::Quote(quote_tick) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(py, Data::Quote(quote_tick));
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                                NautilusWsMessage::Deltas(deltas) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(
                                            py,
                                            Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                        );
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                                NautilusWsMessage::Depth10(depth) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(py, Data::Depth10(depth));
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                                NautilusWsMessage::Candle(bar) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(py, Data::Bar(bar));
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                                NautilusWsMessage::MarkPrice(mark_price) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(
                                            py,
                                            Data::MarkPriceUpdate(mark_price),
                                        );
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                                NautilusWsMessage::IndexPrice(index_price) => {
                                    Python::attach(|py| {
                                        let py_obj = data_to_pycapsule(
                                            py,
                                            Data::IndexPriceUpdate(index_price),
                                        );
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                                NautilusWsMessage::FundingRate(funding_rate) => {
                                    Python::attach(|py| {
                                        if let Ok(py_obj) = funding_rate.into_py_any(py) {
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        }
                                    });
                                }
                                NautilusWsMessage::ExecutionReports(reports) => {
                                    Python::attach(|py| {
                                        for report in reports {
                                            match report {
                                                ExecutionReport::Order(order_report) => {
                                                    log::debug!(
                                                        "Forwarding order status report: order_id={}, status={:?}",
                                                        order_report.venue_order_id,
                                                        order_report.order_status
                                                    );

                                                    match Py::new(py, order_report) {
                                                        Ok(py_obj) => {
                                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                                        }
                                                        Err(e) => {
                                                            log::error!("Error converting OrderStatusReport to Python: {e}");
                                                        }
                                                    }
                                                }
                                                ExecutionReport::Fill(fill_report) => {
                                                    log::debug!(
                                                        "Forwarding fill report: trade_id={}, side={:?}, qty={}, price={}",
                                                        fill_report.trade_id,
                                                        fill_report.order_side,
                                                        fill_report.last_qty,
                                                        fill_report.last_px
                                                    );

                                                    match Py::new(py, fill_report) {
                                                        Ok(py_obj) => {
                                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                                        }
                                                        Err(e) => {
                                                            log::error!("Error converting FillReport to Python: {e}");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                                _ => {
                                    log::debug!("Unhandled message type: {msg:?}");
                                }
                            }
                        }
                        None => {
                            log::debug!("WebSocket connection closed");
                            break;
                        }
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
            let start = std::time::Instant::now();

            loop {
                if client.is_active() {
                    return Ok(());
                }

                if start.elapsed().as_secs_f64() >= timeout_secs {
                    return Err(to_pyruntime_err(format!(
                        "WebSocket connection did not become active within {timeout_secs} seconds"
                    )));
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.disconnect().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    /// Subscribe to trades for an instrument.
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

    /// Unsubscribe from trades for an instrument.
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

    /// Subscribe to L2 order book for an instrument.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from L2 order book for an instrument.
    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_deltas")]
    fn py_subscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    fn py_unsubscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_snapshots")]
    fn py_subscribe_book_snapshots<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to best bid/offer (BBO) quotes for an instrument.
    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_quotes(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from quote ticks for an instrument.
    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_quotes(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to candle/bar data for a specific coin and interval.
    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_bars(bar_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from candle/bar data.
    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_bars(bar_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to order updates for a specific user address.
    #[pyo3(name = "subscribe_order_updates")]
    fn py_subscribe_order_updates<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_order_updates(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to user events (fills, funding, liquidations) for a specific user address.
    #[pyo3(name = "subscribe_user_events")]
    fn py_subscribe_user_events<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_user_events(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to user fills for a specific user address.
    ///
    /// Note: This channel is redundant with `userEvents` which already includes fills.
    /// Prefer using `subscribe_user_events` or `subscribe_all_user_channels` instead.
    #[pyo3(name = "subscribe_user_fills")]
    fn py_subscribe_user_fills<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_user_fills(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to mark price updates for an instrument.
    #[pyo3(name = "subscribe_mark_prices")]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_mark_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from mark price updates for an instrument.
    #[pyo3(name = "unsubscribe_mark_prices")]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_mark_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to index/oracle price updates for an instrument.
    #[pyo3(name = "subscribe_index_prices")]
    fn py_subscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_index_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from index/oracle price updates for an instrument.
    #[pyo3(name = "unsubscribe_index_prices")]
    fn py_unsubscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_index_prices(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribe to funding rate updates for an instrument.
    #[pyo3(name = "subscribe_funding_rates")]
    fn py_subscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_funding_rates(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from funding rate updates for an instrument.
    #[pyo3(name = "unsubscribe_funding_rates")]
    fn py_unsubscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_funding_rates(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}
