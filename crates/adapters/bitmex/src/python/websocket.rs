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

//! Python bindings for the BitMEX WebSocket client.
//!
//! [`PyBitmexWebSocketClient`] wraps the Rust [`BitmexWebSocketClient`] and adds an
//! instrument cache at the Python boundary. The inner client is a pure network component
//! that emits venue-specific types; this wrapper parses them into Nautilus domain objects
//! before passing them to Python callbacks.
//!
//! The instrument cache is shared via `Arc<RwLock>` so that:
//! - Python can inject instruments at any time via `cache_instrument`.
//! - The spawned stream task reads from the same cache for parsing.
//! - Instrument table messages from the venue update the cache automatically.

use std::{fmt::Debug, sync::Arc};

use ahash::AHashMap;
use futures_util::StreamExt;
use nautilus_common::{cache::quote::QuoteCache, live::get_runtime};
use nautilus_core::{
    UnixNanos,
    python::{call_python_threadsafe, to_pyruntime_err, to_pyvalue_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Data, InstrumentStatus, bar::BarType},
    enums::{MarketStatusAction, OrderType},
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
    types::Price,
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*};
use ustr::Ustr;

use crate::{
    common::{
        enums::{BitmexInstrumentState, BitmexOrderType, BitmexPegPriceType},
        parse::{
            parse_contracts_quantity, parse_instrument_id, parse_optional_datetime_to_unix_nanos,
        },
    },
    http::parse::{InstrumentParseResult, parse_instrument_any},
    websocket::{
        BitmexWebSocketClient,
        enums::{BitmexAction, BitmexWsTopic},
        messages::{BitmexTableMessage, BitmexWsMessage, OrderData},
        parse::{
            parse_book_msg_vec, parse_book10_msg_vec, parse_execution_msg, parse_funding_msg,
            parse_instrument_msg, parse_order_msg, parse_order_update_msg, parse_position_msg,
            parse_trade_bin_msg_vec, parse_trade_msg_vec, parse_wallet_msg,
        },
    },
};

/// Python wrapper around [`BitmexWebSocketClient`] that holds an instrument cache
/// at the Python boundary for parsing venue messages into Nautilus domain types.
#[pyclass(
    name = "BitmexWebSocketClient",
    module = "nautilus_trader.core.nautilus_pyo3.bitmex"
)]
pub struct PyBitmexWebSocketClient {
    inner: BitmexWebSocketClient,
    instruments_cache: Arc<tokio::sync::RwLock<AHashMap<Ustr, InstrumentAny>>>,
}

impl Debug for PyBitmexWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyBitmexWebSocketClient))
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl PyBitmexWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, account_id=None, heartbeat=None, testnet=false))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        testnet: bool,
    ) -> PyResult<Self> {
        let inner = BitmexWebSocketClient::new_with_env(
            url, api_key, api_secret, account_id, heartbeat, testnet,
        )
        .map_err(to_pyvalue_err)?;
        Ok(Self {
            inner,
            instruments_cache: Arc::new(tokio::sync::RwLock::new(AHashMap::new())),
        })
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        let inner = BitmexWebSocketClient::from_env().map_err(to_pyvalue_err)?;
        Ok(Self {
            inner,
            instruments_cache: Arc::new(tokio::sync::RwLock::new(AHashMap::new())),
        })
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    fn py_url(&self) -> &str {
        self.inner.url()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    fn py_api_key(&self) -> Option<&str> {
        self.inner.api_key()
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    fn py_api_key_masked(&self) -> Option<String> {
        self.inner.api_key_masked()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&mut self) -> bool {
        self.inner.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&mut self) -> bool {
        self.inner.is_closed()
    }

    #[pyo3(name = "get_subscriptions")]
    fn py_get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<String> {
        self.inner.get_subscriptions(instrument_id)
    }

    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.inner.set_account_id(account_id);
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        let symbol = inst.symbol().inner();
        let cache = Arc::clone(&self.instruments_cache);
        // Spawn as background task to avoid deadlock: this method is called from the
        // Python main thread (holding the GIL) via call_soon_threadsafe callbacks.
        // The stream task may hold a read lock on the same cache while waiting for the
        // GIL via Python::attach, so blocking here would create an ABBA deadlock.
        get_runtime().spawn(async move {
            cache.write().await.insert(symbol, inst);
        });
        Ok(())
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let cache = Arc::clone(&self.instruments_cache);
        {
            let mut guard = cache.blocking_write();
            for inst_py in instruments {
                let inst = pyobject_to_instrument_any(py, inst_py)?;
                guard.insert(inst.symbol().inner(), inst);
            }
        }

        let clock = get_atomic_clock_realtime();
        let mut client = self.inner.clone();
        let account_id = self.inner.account_id();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();

            get_runtime().spawn(async move {
                let _client = client; // Keep client alive for the entire duration
                tokio::pin!(stream);

                let mut quote_cache = QuoteCache::new();
                let mut order_type_cache: AHashMap<ClientOrderId, OrderType> = AHashMap::new();
                let mut order_symbol_cache: AHashMap<ClientOrderId, Ustr> = AHashMap::new();

                while let Some(msg) = stream.next().await {
                    let ts_init = clock.get_time_ns();

                    match msg {
                        BitmexWsMessage::Table(table_msg) => {
                            handle_table_message(
                                table_msg,
                                &cache,
                                &mut quote_cache,
                                &mut order_type_cache,
                                &mut order_symbol_cache,
                                account_id,
                                ts_init,
                                &call_soon,
                                &callback,
                            )
                            .await;
                        }
                        BitmexWsMessage::Reconnected => {
                            quote_cache.clear();
                            order_type_cache.clear();
                            order_symbol_cache.clear();
                        }
                        BitmexWsMessage::Authenticated => {}
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
        let client = self.inner.clone();

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
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instruments")]
    fn py_subscribe_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instruments().await {
                log::error!("Failed to subscribe to instruments: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instrument(instrument_id).await {
                log::error!("Failed to subscribe to instrument: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book(instrument_id).await {
                log::error!("Failed to subscribe to order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_25")]
    fn py_subscribe_book_25<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_25(instrument_id).await {
                log::error!("Failed to subscribe to order book 25: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_depth10")]
    fn py_subscribe_book_depth10<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_depth10(instrument_id).await {
                log::error!("Failed to subscribe to order book depth 10: {e}");
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
        let client = self.inner.clone();

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
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_id).await {
                log::error!("Failed to subscribe to trades: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_mark_prices(instrument_id).await {
                log::error!("Failed to subscribe to mark prices: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_index_prices(instrument_id).await {
                log::error!("Failed to subscribe to index prices: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_funding_rates(instrument_id).await {
                log::error!("Failed to subscribe to funding: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_bars(bar_type).await {
                log::error!("Failed to subscribe to bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_instruments")]
    fn py_unsubscribe_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_instruments().await {
                log::error!("Failed to unsubscribe from instruments: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_instrument")]
    fn py_unsubscribe_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_instrument(instrument_id).await {
                log::error!("Failed to unsubscribe from instrument: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book(instrument_id).await {
                log::error!("Failed to unsubscribe from order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_25")]
    fn py_unsubscribe_book_25<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_25(instrument_id).await {
                log::error!("Failed to unsubscribe from order book 25: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_depth10")]
    fn py_unsubscribe_book_depth10<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_depth10(instrument_id).await {
                log::error!("Failed to unsubscribe from order book depth 10: {e}");
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
        let client = self.inner.clone();

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
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_id).await {
                log::error!("Failed to unsubscribe from trades: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_mark_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from mark prices: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_index_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from index prices: {e}");
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
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_funding_rates(instrument_id).await {
                log::error!("Failed to unsubscribe from funding rates: {e}");
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
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_bars(bar_type).await {
                log::error!("Failed to unsubscribe from bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders().await {
                log::error!("Failed to subscribe to orders: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_executions")]
    fn py_subscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_executions().await {
                log::error!("Failed to subscribe to executions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_positions")]
    fn py_subscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_positions().await {
                log::error!("Failed to subscribe to positions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_margin")]
    fn py_subscribe_margin<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_margin().await {
                log::error!("Failed to subscribe to margin: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_wallet")]
    fn py_subscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_wallet().await {
                log::error!("Failed to subscribe to wallet: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders().await {
                log::error!("Failed to unsubscribe from orders: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_executions")]
    fn py_unsubscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_executions().await {
                log::error!("Failed to unsubscribe from executions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_positions")]
    fn py_unsubscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_positions().await {
                log::error!("Failed to unsubscribe from positions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_margin")]
    fn py_unsubscribe_margin<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_margin().await {
                log::error!("Failed to unsubscribe from margin: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_wallet")]
    fn py_unsubscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_wallet().await {
                log::error!("Failed to unsubscribe from wallet: {e}");
            }
            Ok(())
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_table_message(
    table_msg: BitmexTableMessage,
    instruments_cache: &Arc<tokio::sync::RwLock<AHashMap<Ustr, InstrumentAny>>>,
    quote_cache: &mut QuoteCache,
    order_type_cache: &mut AHashMap<ClientOrderId, OrderType>,
    order_symbol_cache: &mut AHashMap<ClientOrderId, Ustr>,
    account_id: AccountId,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let instruments = instruments_cache.read().await;

    match table_msg {
        BitmexTableMessage::OrderBookL2 { action, data }
        | BitmexTableMessage::OrderBookL2_25 { action, data } => {
            if !data.is_empty() {
                for d in parse_book_msg_vec(data, action, &instruments, ts_init) {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::OrderBook10 { data, .. } => {
            if !data.is_empty() {
                for d in parse_book10_msg_vec(data, &instruments, ts_init) {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::Quote { data, .. } => {
            for msg in data {
                let Some(instrument) = instruments.get(&msg.symbol) else {
                    log::error!(
                        "Instrument cache miss: quote dropped for symbol={}",
                        msg.symbol,
                    );
                    continue;
                };

                let instrument_id = instrument.id();
                let price_precision = instrument.price_precision();

                let bid_price = msg.bid_price.map(|p| Price::new(p, price_precision));
                let ask_price = msg.ask_price.map(|p| Price::new(p, price_precision));
                let bid_size = msg
                    .bid_size
                    .map(|s| parse_contracts_quantity(s, instrument));
                let ask_size = msg
                    .ask_size
                    .map(|s| parse_contracts_quantity(s, instrument));
                let ts_event = UnixNanos::from(msg.timestamp);

                match quote_cache.process(
                    instrument_id,
                    bid_price,
                    ask_price,
                    bid_size,
                    ask_size,
                    ts_event,
                    ts_init,
                ) {
                    Ok(quote) => send_data_to_python(Data::Quote(quote), call_soon, callback),
                    Err(e) => {
                        log::warn!("Failed to process quote for {}: {e}", msg.symbol);
                    }
                }
            }
        }
        BitmexTableMessage::Trade { data, .. } => {
            if !data.is_empty() {
                for d in parse_trade_msg_vec(data, &instruments, ts_init) {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::TradeBin1m { action, data } => {
            if action != BitmexAction::Partial && !data.is_empty() {
                for d in
                    parse_trade_bin_msg_vec(data, BitmexWsTopic::TradeBin1m, &instruments, ts_init)
                {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::TradeBin5m { action, data } => {
            if action != BitmexAction::Partial && !data.is_empty() {
                for d in
                    parse_trade_bin_msg_vec(data, BitmexWsTopic::TradeBin5m, &instruments, ts_init)
                {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::TradeBin1h { action, data } => {
            if action != BitmexAction::Partial && !data.is_empty() {
                for d in
                    parse_trade_bin_msg_vec(data, BitmexWsTopic::TradeBin1h, &instruments, ts_init)
                {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::TradeBin1d { action, data } => {
            if action != BitmexAction::Partial && !data.is_empty() {
                for d in
                    parse_trade_bin_msg_vec(data, BitmexWsTopic::TradeBin1d, &instruments, ts_init)
                {
                    send_data_to_python(d, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::Instrument { action, data } => {
            // Drop the read lock before acquiring write lock
            drop(instruments);

            let mut cache = instruments_cache.write().await;

            if action == BitmexAction::Partial || action == BitmexAction::Insert {
                let data_for_prices = data.clone();

                for msg in data {
                    match msg.try_into() {
                        Ok(http_inst) => match parse_instrument_any(&http_inst, ts_init) {
                            InstrumentParseResult::Ok(boxed) => {
                                let inst = *boxed;
                                let symbol = inst.symbol().inner();
                                cache.insert(symbol, inst.clone());

                                Python::attach(|py| {
                                    if let Ok(py_obj) = instrument_any_to_pyobject(py, inst) {
                                        call_python_threadsafe(py, call_soon, callback, py_obj);
                                    }
                                });
                            }
                            InstrumentParseResult::Unsupported { .. }
                            | InstrumentParseResult::Inactive { .. } => {}
                            InstrumentParseResult::Failed { symbol, error, .. } => {
                                log::warn!("Failed to parse instrument {symbol}: {error}");
                            }
                        },
                        Err(e) => {
                            log::debug!("Skipping instrument (missing required fields): {e}");
                        }
                    }
                }

                for msg in data_for_prices {
                    for d in parse_instrument_msg(msg, &cache, ts_init) {
                        send_data_to_python(d, call_soon, callback);
                    }
                }
            } else {
                for msg in &data {
                    if let Some(state_str) = &msg.state
                        && let Ok(state) = serde_json::from_str::<BitmexInstrumentState>(&format!(
                            "\"{state_str}\""
                        ))
                    {
                        let instrument_id = parse_instrument_id(msg.symbol);
                        let action = MarketStatusAction::from(&state);
                        let is_trading = Some(state == BitmexInstrumentState::Open);
                        let ts_event = parse_optional_datetime_to_unix_nanos(
                            &Some(msg.timestamp),
                            "timestamp",
                        );
                        let status = InstrumentStatus::new(
                            instrument_id,
                            action,
                            ts_event,
                            ts_init,
                            None,
                            None,
                            is_trading,
                            None,
                            None,
                        );
                        send_to_python(status, call_soon, callback);
                    }
                }

                for msg in data {
                    for d in parse_instrument_msg(msg, &cache, ts_init) {
                        send_data_to_python(d, call_soon, callback);
                    }
                }
            }
        }
        BitmexTableMessage::Funding { data, .. } => {
            for msg in data {
                send_to_python(parse_funding_msg(msg, ts_init), call_soon, callback);
            }
        }
        BitmexTableMessage::Order { data, .. } => {
            for order_msg in data {
                match &order_msg {
                    OrderData::Full(msg) => {
                        let client_order_id = msg.cl_ord_id.map(ClientOrderId::new);
                        if let Some(cid) = &client_order_id {
                            order_symbol_cache.insert(*cid, msg.symbol);
                        }

                        let Some(instrument) = instruments.get(&msg.symbol) else {
                            log::warn!("Instrument cache miss for order symbol={}", msg.symbol);
                            continue;
                        };

                        match parse_order_msg(msg, instrument, order_type_cache, ts_init) {
                            Ok(report) => {
                                if let Some(cid) = &client_order_id
                                    && let Some(ord_type) = &msg.ord_type
                                {
                                    let order_type: OrderType = if *ord_type
                                        == BitmexOrderType::Pegged
                                        && msg.peg_price_type
                                            == Some(BitmexPegPriceType::TrailingStopPeg)
                                    {
                                        if msg.price.is_some() {
                                            OrderType::TrailingStopLimit
                                        } else {
                                            OrderType::TrailingStopMarket
                                        }
                                    } else {
                                        (*ord_type).into()
                                    };
                                    order_type_cache.insert(*cid, order_type);
                                }

                                if report.order_status.is_closed()
                                    && let Some(cid) = report.client_order_id
                                {
                                    order_type_cache.remove(&cid);
                                    order_symbol_cache.remove(&cid);
                                }

                                send_to_python(report, call_soon, callback);
                            }
                            Err(e) => log::error!("Failed to parse order message: {e}"),
                        }
                    }
                    OrderData::Update(msg) => {
                        // Populate cache for execution message routing
                        if let Some(cl_ord_id) = &msg.cl_ord_id {
                            let cid = ClientOrderId::new(cl_ord_id);
                            order_symbol_cache.insert(cid, msg.symbol);
                        }

                        let Some(instrument) = instruments.get(&msg.symbol) else {
                            log::warn!(
                                "Instrument cache miss for order update symbol={}",
                                msg.symbol,
                            );
                            continue;
                        };

                        if let Some(event) =
                            parse_order_update_msg(msg, instrument, account_id, ts_init)
                        {
                            send_to_python(event, call_soon, callback);
                        }
                    }
                }
            }
        }
        BitmexTableMessage::Execution { data, .. } => {
            for exec_msg in data {
                let symbol = exec_msg.symbol.or_else(|| {
                    exec_msg
                        .cl_ord_id
                        .map(ClientOrderId::new)
                        .and_then(|cid| order_symbol_cache.get(&cid).copied())
                });

                let Some(symbol) = symbol else {
                    log::debug!("Execution without symbol, skipping");
                    continue;
                };

                let Some(instrument) = instruments.get(&symbol) else {
                    log::warn!("Instrument cache miss for execution symbol={symbol}");
                    continue;
                };

                if let Some(fill) = parse_execution_msg(exec_msg, instrument, ts_init) {
                    send_to_python(fill, call_soon, callback);
                }
            }
        }
        BitmexTableMessage::Position { data, .. } => {
            for msg in data {
                let Some(instrument) = instruments.get(&msg.symbol) else {
                    log::warn!("Instrument cache miss for position symbol={}", msg.symbol);
                    continue;
                };

                send_to_python(
                    parse_position_msg(msg, instrument, ts_init),
                    call_soon,
                    callback,
                );
            }
        }
        BitmexTableMessage::Wallet { data, .. } => {
            for msg in data {
                send_to_python(parse_wallet_msg(msg, ts_init), call_soon, callback);
            }
        }
        BitmexTableMessage::Margin { .. } => {}
        _ => {
            log::debug!("Unhandled table message type in Python WebSocket client");
        }
    }
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
