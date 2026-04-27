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

//! Python bindings for the Deribit WebSocket client.
//!
//! # Design Pattern: Clone and Share State
//!
//! The WebSocket client must be cloned for async operations because PyO3's `future_into_py`
//! requires `'static` futures (cannot borrow from `self`). To ensure clones share the same
//! connection state, key fields use `Arc<RwLock<T>>`:
//!
//! - Connection mode and signal are shared via Arc.
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

use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::python::{call_python_threadsafe, to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
    types::{Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{
    common::{
        enums::{DeribitEnvironment, DeribitTimeInForce, resolve_trigger_type},
        parse::parse_instrument_kind_currency,
    },
    websocket::{
        client::DeribitWebSocketClient,
        enums::DeribitUpdateInterval,
        messages::{DeribitOrderParams, NautilusWsMessage},
    },
};

fn call_python_with_data<F>(call_soon: &Py<PyAny>, callback: &Py<PyAny>, data_converter: F)
where
    F: FnOnce(Python) -> PyResult<Py<PyAny>>,
{
    Python::attach(|py| match data_converter(py) {
        Ok(py_obj) => call_python_threadsafe(py, call_soon, callback, py_obj),
        Err(e) => log::error!("Failed to convert data to Python object: {e}"),
    });
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeribitWebSocketClient {
    /// WebSocket client for connecting to Deribit.
    #[new]
    #[pyo3(signature = (
        url=None,
        api_key=None,
        api_secret=None,
        heartbeat_interval=30,
        environment=DeribitEnvironment::Mainnet,
        proxy_url=None,
    ))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat_interval: u64,
        environment: DeribitEnvironment,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            url,
            api_key,
            api_secret,
            heartbeat_interval,
            environment,
            TransportBackend::default(),
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Creates a new public (unauthenticated) client.
    ///
    /// Does NOT fall back to environment variables for credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    #[staticmethod]
    #[pyo3(name = "new_public", signature = (environment, proxy_url = None))]
    fn py_new_public(environment: DeribitEnvironment, proxy_url: Option<String>) -> PyResult<Self> {
        Self::new_public(environment, proxy_url).map_err(to_pyvalue_err)
    }

    /// Creates an authenticated client with credentials.
    ///
    /// Uses environment variables to load credentials:
    /// - Testnet: `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET`
    /// - Mainnet: `DERIBIT_API_KEY` and `DERIBIT_API_SECRET`
    #[staticmethod]
    #[pyo3(name = "with_credentials", signature = (environment, account_id = None, proxy_url = None))]
    fn py_with_credentials(
        environment: DeribitEnvironment,
        account_id: Option<AccountId>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        let mut client = Self::with_credentials(environment, proxy_url).map_err(to_pyvalue_err)?;

        if let Some(id) = account_id {
            client.set_account_id(id);
        }
        Ok(client)
    }

    /// Returns the WebSocket URL.
    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> String {
        self.url().to_string()
    }

    #[getter]
    #[pyo3(name = "is_testnet")]
    #[must_use]
    pub fn py_is_testnet(&self) -> bool {
        self.environment() == DeribitEnvironment::Testnet
    }

    /// Returns whether the client is actively connected.
    #[pyo3(name = "is_active")]
    #[must_use]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    /// Returns whether the client is closed.
    #[pyo3(name = "is_closed")]
    #[must_use]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Returns whether the client has credentials configured.
    #[pyo3(name = "has_credentials")]
    #[must_use]
    fn py_has_credentials(&self) -> bool {
        self.has_credentials()
    }

    /// Returns whether the client is authenticated.
    #[pyo3(name = "is_authenticated")]
    #[must_use]
    fn py_is_authenticated(&self) -> bool {
        self.is_authenticated()
    }

    /// Cancel all pending WebSocket requests.
    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Caches instruments for use during message parsing.
    #[pyo3(name = "cache_instruments")]
    pub fn py_cache_instruments(
        &self,
        py: Python<'_>,
        instruments: Vec<Py<PyAny>>,
    ) -> PyResult<()> {
        let instruments: Result<Vec<_>, _> = instruments
            .into_iter()
            .map(|inst| pyobject_to_instrument_any(py, inst))
            .collect();
        self.cache_instruments(&instruments?);
        Ok(())
    }

    /// Caches a single instrument.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst);
        Ok(())
    }

    /// Sets the account ID for order/fill reports.
    #[pyo3(name = "set_account_id")]
    pub fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Sets whether bar timestamps should use the close time.
    ///
    /// When `true` (default), bar `ts_event` is set to the bar's close time.
    #[pyo3(name = "set_bars_timestamp_on_close")]
    pub fn py_set_bars_timestamp_on_close(&mut self, value: bool) {
        self.set_bars_timestamp_on_close(value);
    }

    /// Connects to the Deribit WebSocket API.
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

            let stream = client.stream().map_err(to_pyruntime_err)?;

            // Keep client alive in the spawned task to prevent handler from dropping
            get_runtime().spawn(async move {
                let _client = client;
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        NautilusWsMessage::Instrument(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| {
                                instrument_any_to_pyobject(py, *msg)
                            });
                        }
                        NautilusWsMessage::Data(msg) => Python::attach(|py| {
                            for data in msg {
                                let py_obj = data_to_pycapsule(py, data);
                                call_python_threadsafe(py, &call_soon, &callback, py_obj);
                            }
                        }),
                        NautilusWsMessage::Deltas(msg) => Python::attach(|py| {
                            let py_obj =
                                data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(msg)));
                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                        }),
                        NautilusWsMessage::Error(err) => {
                            log::error!("WebSocket error: {err}");
                        }
                        NautilusWsMessage::Reconnected => {
                            log::info!("WebSocket reconnected");
                        }
                        NautilusWsMessage::Authenticated(auth_result) => {
                            log::info!("WebSocket authenticated (scope: {})", auth_result.scope);
                        }
                        NautilusWsMessage::InstrumentStatus(status) => {
                            call_python_with_data(&call_soon, &callback, |py| {
                                status.into_py_any(py)
                            });
                        }
                        NautilusWsMessage::Raw(msg) => {
                            log::debug!("Received raw message, skipping: {msg}");
                        }
                        NautilusWsMessage::FundingRates(funding_rates) => Python::attach(|py| {
                            for funding_rate in funding_rates {
                                match Py::new(py, funding_rate) {
                                    Ok(py_obj) => call_python_threadsafe(
                                        py,
                                        &call_soon,
                                        &callback,
                                        py_obj.into_any(),
                                    ),
                                    Err(e) => {
                                        log::error!("Failed to create FundingRateUpdate: {e}");
                                    }
                                }
                            }
                        }),
                        NautilusWsMessage::OptionGreeks(greeks) => {
                            call_python_with_data(&call_soon, &callback, |py| {
                                Py::new(py, greeks).map(|obj| obj.into_any())
                            });
                        }
                        // Execution events - route to Python callback
                        NautilusWsMessage::OrderStatusReports(reports) => Python::attach(|py| {
                            for report in reports {
                                match Py::new(py, report) {
                                    Ok(py_obj) => call_python_threadsafe(
                                        py,
                                        &call_soon,
                                        &callback,
                                        py_obj.into_any(),
                                    ),
                                    Err(e) => {
                                        log::error!("Failed to create OrderStatusReport: {e}");
                                    }
                                }
                            }
                        }),
                        NautilusWsMessage::FillReports(reports) => Python::attach(|py| {
                            for report in reports {
                                match Py::new(py, report) {
                                    Ok(py_obj) => call_python_threadsafe(
                                        py,
                                        &call_soon,
                                        &callback,
                                        py_obj.into_any(),
                                    ),
                                    Err(e) => log::error!("Failed to create FillReport: {e}"),
                                }
                            }
                        }),
                        NautilusWsMessage::OrderRejected(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderAccepted(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderCanceled(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderExpired(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderUpdated(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderCancelRejected(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderModifyRejected(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::AccountState(msg) => {
                            call_python_with_data(&call_soon, &callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::AuthenticationFailed(reason) => {
                            log::error!("Authentication failed: {reason}");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    /// Waits until the client is active or timeout expires.
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

    /// Closes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the close operation fails.
    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    /// Authenticates the WebSocket session with Deribit.
    ///
    /// Uses the `client_signature` grant type with HMAC-SHA256 signature.
    /// This must be called before subscribing to raw data streams.
    ///
    /// # Arguments
    ///
    /// * `session_name` - Optional session name for session-scoped authentication.
    ///   When provided, uses `session:<name>` scope which allows skipping `access_token`
    ///   in subsequent private requests. When `None`, uses default `connection` scope.
    ///   Recommended to use session scope for order execution compatibility.
    #[pyo3(name = "authenticate")]
    #[pyo3(signature = (session_name=None))]
    fn py_authenticate<'py>(
        &self,
        py: Python<'py>,
        session_name: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate(session_name.as_deref())
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Authenticates with session scope using the provided session name.
    ///
    /// Use `DERIBIT_DATA_SESSION_NAME` for data clients and
    /// `DERIBIT_EXECUTION_SESSION_NAME` for execution clients.
    #[pyo3(name = "authenticate_session")]
    fn py_authenticate_session<'py>(
        &self,
        py: Python<'py>,
        session_name: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate_session(&session_name)
                .await
                .map_err(|e| {
                    to_pyruntime_err(format!(
                        "Failed to authenticate Deribit websocket session '{session_name}': {e}"
                    ))
                })?;
            Ok(())
        })
    }

    /// Subscribes to trade updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    #[pyo3(name = "subscribe_trades")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from trade updates for an instrument.
    #[pyo3(name = "unsubscribe_trades")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to order book updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    #[pyo3(name = "subscribe_book")]
    #[pyo3(signature = (instrument_id, interval=None, depth=None))]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(d) = depth {
                client
                    .subscribe_book_grouped(instrument_id, "none", d, interval)
                    .await
                    .map_err(to_pyvalue_err)
            } else {
                client
                    .subscribe_book(instrument_id, interval)
                    .await
                    .map_err(to_pyvalue_err)
            }
        })
    }

    /// Unsubscribes from order book updates for an instrument.
    #[pyo3(name = "unsubscribe_book")]
    #[pyo3(signature = (instrument_id, interval=None, depth=None))]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(d) = depth {
                client
                    .unsubscribe_book_grouped(instrument_id, "none", d, interval)
                    .await
                    .map_err(to_pyvalue_err)
            } else {
                client
                    .unsubscribe_book(instrument_id, interval)
                    .await
                    .map_err(to_pyvalue_err)
            }
        })
    }

    /// Subscribes to grouped (depth-limited) order book updates for an instrument.
    ///
    /// Uses the Deribit grouped book channel format: `book.{instrument}.{group}.{depth}.{interval}`
    ///
    /// Depth is normalized to Deribit supported values: 1, 10, or 20.
    #[pyo3(name = "subscribe_book_grouped")]
    #[pyo3(signature = (instrument_id, group, depth, interval=None))]
    fn py_subscribe_book_grouped<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        group: String,
        depth: u32,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book_grouped(instrument_id, &group, depth, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from grouped (depth-limited) order book updates for an instrument.
    ///
    /// Depth is normalized to Deribit supported values: 1, 10, or 20.
    #[pyo3(name = "unsubscribe_book_grouped")]
    #[pyo3(signature = (instrument_id, group, depth, interval=None))]
    fn py_unsubscribe_book_grouped<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        group: String,
        depth: u32,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book_grouped(instrument_id, &group, depth, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to ticker updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    #[pyo3(name = "subscribe_ticker")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from ticker updates for an instrument.
    #[pyo3(name = "unsubscribe_ticker")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to mark prices for the given instrument.
    ///
    /// Registers the instrument in the `mark_price_subs` set so the handler
    /// emits `MarkPriceUpdate` from ticker messages, then subscribes to the ticker channel.
    #[pyo3(name = "subscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.add_mark_price_sub(instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from mark prices for the given instrument.
    ///
    /// Removes the instrument from the `mark_price_subs` set and unsubscribes
    /// from the ticker channel.
    #[pyo3(name = "unsubscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.remove_mark_price_sub(&instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to index prices for the given instrument.
    ///
    /// Registers the instrument in the `index_price_subs` set so the handler
    /// emits `IndexPriceUpdate` from ticker messages, then subscribes to the ticker channel.
    #[pyo3(name = "subscribe_index_prices")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.add_index_price_sub(instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from index prices for the given instrument.
    ///
    /// Removes the instrument from the `index_price_subs` set and unsubscribes
    /// from the ticker channel.
    #[pyo3(name = "unsubscribe_index_prices")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.remove_index_price_sub(&instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to option greeks for the given instrument.
    ///
    /// Registers the instrument in the `option_greeks_subs` set so the handler
    /// emits `OptionGreeks` from ticker messages, then subscribes to the ticker channel.
    #[pyo3(name = "subscribe_option_greeks")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_option_greeks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.add_option_greeks_sub(instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from option greeks for the given instrument.
    ///
    /// Removes the instrument from the `option_greeks_subs` set and unsubscribes
    /// from the ticker channel.
    #[pyo3(name = "unsubscribe_option_greeks")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_option_greeks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.remove_option_greeks_sub(&instrument_id);
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to quote (best bid/ask) updates for an instrument.
    ///
    /// Note: Quote channel does not support interval parameter.
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
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from quote updates for an instrument.
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
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to user order updates for all instruments.
    ///
    /// Requires authentication. Subscribes to `user.orders.any.any.raw` channel.
    ///
    /// # Errors
    ///
    /// Returns an error if client is not authenticated or subscription fails.
    #[pyo3(name = "subscribe_user_orders")]
    fn py_subscribe_user_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_user_orders().await.map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from user order updates for all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    #[pyo3(name = "unsubscribe_user_orders")]
    fn py_unsubscribe_user_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_user_orders()
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to user trade/fill updates for all instruments.
    ///
    /// Requires authentication. Subscribes to `user.trades.any.any.raw` channel.
    ///
    /// # Errors
    ///
    /// Returns an error if client is not authenticated or subscription fails.
    #[pyo3(name = "subscribe_user_trades")]
    fn py_subscribe_user_trades<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_user_trades().await.map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from user trade/fill updates for all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    #[pyo3(name = "unsubscribe_user_trades")]
    fn py_unsubscribe_user_trades<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_user_trades()
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to user portfolio updates for all currencies.
    ///
    /// Requires authentication. Subscribes to `user.portfolio.any` channel which
    /// provides real-time account balance and margin updates for all currencies
    /// (BTC, ETH, USDC, USDT, etc.).
    ///
    /// # Errors
    ///
    /// Returns an error if client is not authenticated or subscription fails.
    #[pyo3(name = "subscribe_user_portfolio")]
    fn py_subscribe_user_portfolio<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_user_portfolio()
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from user portfolio updates for all currencies.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    #[pyo3(name = "unsubscribe_user_portfolio")]
    fn py_unsubscribe_user_portfolio<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_user_portfolio()
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to multiple channels at once.
    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
        py: Python<'py>,
        channels: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe(channels).await.map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from multiple channels at once.
    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &self,
        py: Python<'py>,
        channels: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe(channels).await.map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_perpetual_interest_rates")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_perpetual_interest_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_perpetual_interests_rates_updates(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "unsubscribe_perpetual_interest_rates")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_perpetual_interest_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_perpetual_interest_rates_updates(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to instrument status changes for lifecycle notifications.
    ///
    /// Channel format: `instrument.state.{kind}.{currency}`
    #[pyo3(name = "subscribe_instrument_status")]
    fn py_subscribe_instrument_status<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_instrument_status(&kind, &currency)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from instrument status changes.
    #[pyo3(name = "unsubscribe_instrument_status")]
    fn py_unsubscribe_instrument_status<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_instrument_status(&kind, &currency)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to chart/OHLC bar updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `resolution` - Bar resolution: "1", "3", "5", "10", "15", "30", "60", "120", "180",
    ///   "360", "720", "1D" (minutes or 1D for daily)
    #[pyo3(name = "subscribe_chart")]
    fn py_subscribe_chart<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        resolution: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_chart(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from chart/OHLC bar updates.
    #[pyo3(name = "unsubscribe_chart")]
    fn py_unsubscribe_chart<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        resolution: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_chart(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to bar updates for an instrument using a BarType specification.
    ///
    /// Converts the BarType to the nearest supported Deribit resolution and subscribes
    /// to the chart channel.
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
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from bar updates for an instrument using a BarType specification.
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
                .map_err(to_pyvalue_err)
        })
    }

    /// Submits an order to Deribit via WebSocket.
    ///
    /// Routes to `private/buy` or `private/sell` JSON-RPC method based on order side.
    /// Requires authentication (call `authenticate_session()` first).
    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        order_side,
        quantity,
        order_type,
        client_order_id,
        trader_id,
        strategy_id,
        instrument_id,
        price=None,
        time_in_force=None,
        post_only=false,
        reduce_only=false,
        trigger_price=None,
        trigger_type=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        order_side: OrderSide,
        quantity: Quantity,
        order_type: OrderType,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        price: Option<Price>,
        time_in_force: Option<TimeInForce>,
        post_only: bool,
        reduce_only: bool,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_name = instrument_id.symbol.to_string();

        // Convert Nautilus TimeInForce to Deribit format
        let deribit_tif = time_in_force
            .map(|tif| {
                DeribitTimeInForce::try_from(tif)
                    .map(|deribit_tif| deribit_tif.as_str().to_string())
            })
            .transpose()
            .map_err(to_pyvalue_err)?;

        let params = DeribitOrderParams {
            instrument_name,
            amount: quantity.as_decimal(),
            order_type: order_type.to_string().to_lowercase(),
            label: Some(client_order_id.to_string()),
            price: price.map(|p| p.as_decimal()),
            time_in_force: deribit_tif,
            post_only: if post_only { Some(true) } else { None },
            reject_post_only: if post_only { Some(true) } else { None },
            reduce_only: if reduce_only { Some(true) } else { None },
            trigger_price: trigger_price.map(|p| p.as_decimal()),
            trigger: resolve_trigger_type(trigger_type),
            max_show: None,
            valid_until: None,
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    order_side,
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Modifies an existing order on Deribit via WebSocket.
    ///
    /// The order parameters are sent using the `private/edit` JSON-RPC method.
    /// Requires authentication (call `authenticate_session()` first).
    #[pyo3(name = "modify_order")]
    #[expect(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        order_id: String,
        quantity: Quantity,
        price: Price,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    &order_id,
                    quantity,
                    price,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Cancels an existing order on Deribit via WebSocket.
    ///
    /// The order is cancelled using the `private/cancel` JSON-RPC method.
    /// Requires authentication (call `authenticate_session()` first).
    #[pyo3(name = "cancel_order")]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        order_id: String,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(
                    &order_id,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Cancels all orders for a specific instrument on Deribit via WebSocket.
    ///
    /// Uses the `private/cancel_all_by_instrument` JSON-RPC method.
    /// Requires authentication (call `authenticate_session()` first).
    #[pyo3(name = "cancel_all_orders")]
    #[pyo3(signature = (instrument_id, order_type=None))]
    fn py_cancel_all_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        order_type: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_all_orders(instrument_id, order_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Queries the state of an order on Deribit via WebSocket.
    ///
    /// Uses the `private/get_order_state` JSON-RPC method.
    /// Requires authentication (call `authenticate_session()` first).
    #[pyo3(name = "query_order")]
    fn py_query_order<'py>(
        &self,
        py: Python<'py>,
        order_id: String,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .query_order(
                    &order_id,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}
