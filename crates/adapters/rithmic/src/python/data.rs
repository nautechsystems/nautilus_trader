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

//! Python bindings for data client.

#![allow(
    clippy::needless_pass_by_value,
    reason = "PyO3 data-client APIs accept owned Python values at the FFI boundary"
)]

#[cfg(feature = "python")]
use pyo3::prelude::*;

#[cfg(feature = "python")]
use pyo3_async_runtimes::tokio::future_into_py;

use nautilus_common::live::get_runtime;
use nautilus_core::python::{to_pynotimplemented_err, to_pyruntime_err, to_pyvalue_err};
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::TimeBarType;
use crate::data::MarketDataEvent;
use crate::gateway::RithmicGateway;
use rithmic_rs::rti::{messages::RithmicMessage, request_tick_bar_update};

use super::events::{PyMarketDataEvent, PyQuoteTick, PyTimeBar, PyTradeTick};
use super::gateway::PyRithmicGateway;

/// Python wrapper for RithmicDataClient.
///
/// The data client manages market data subscriptions and receives
/// quotes/trades from the ticker plant plus live time bars from the history plant.
///
/// Example
/// -------
/// ```python
/// gateway = RithmicGateway.from_env()
/// await gateway.connect()
///
/// client = RithmicDataClient(gateway)
/// client.set_data_callback(on_market_data)
/// await client.subscribe_quotes("ESH5", "CME")
/// ```
#[cfg(feature = "python")]
#[pyclass(name = "RithmicDataClient")]
pub struct PyRithmicDataClient {
    /// Reference to the gateway for async operations.
    gateway: Arc<tokio::sync::RwLock<RithmicGateway>>,
    /// Local subscription tracking (mirrors the Rust client).
    /// Uses Arc so it can be shared with async futures.
    subscriptions: Arc<parking_lot::Mutex<std::collections::HashSet<String>>>,
    /// Local live bar subscription tracking.
    bar_subscriptions: Arc<parking_lot::Mutex<std::collections::HashSet<String>>>,
    /// Python callback for market data events.
    data_callback: Arc<parking_lot::Mutex<Option<Py<PyAny>>>>,
    event_task: Arc<parking_lot::Mutex<Option<JoinHandle<()>>>>,
    shutdown_tx: Arc<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

#[derive(Clone, Copy)]
enum ParsedBarType {
    Time(TimeBarType),
    Tick,
}

impl ParsedBarType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Time(TimeBarType::SecondBar) => "SecondBar",
            Self::Time(TimeBarType::MinuteBar) => "MinuteBar",
            Self::Time(TimeBarType::DailyBar) => "DailyBar",
            Self::Time(TimeBarType::WeeklyBar) => "WeeklyBar",
            Self::Tick => "TickBar",
        }
    }
}

const TICK_BAR_HISTORY_NOT_EXPOSED: &str = "Historical TickBar requests with period > 1 are not exposed by the current \
     rithmic-rs history API. The adapter intentionally avoids local re-aggregation \
     outside Nautilus aggregators.";

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicDataClient {
    /// Creates a new data client from a connected gateway.
    ///
    /// Parameters
    /// ----------
    /// gateway : RithmicGateway
    ///     The connected gateway instance.
    #[new]
    fn new(gateway: &PyRithmicGateway) -> Self {
        Self {
            gateway: Arc::clone(&gateway.inner),
            subscriptions: Arc::new(parking_lot::Mutex::new(std::collections::HashSet::new())),
            bar_subscriptions: Arc::new(parking_lot::Mutex::new(std::collections::HashSet::new())),
            data_callback: Arc::new(parking_lot::Mutex::new(None)),
            event_task: Arc::new(parking_lot::Mutex::new(None)),
            shutdown_tx: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// Returns true if the gateway is connected.
    #[getter]
    fn is_connected(&self) -> bool {
        self.gateway
            .try_read()
            .map(|g| g.is_connected())
            .unwrap_or(false)
    }

    /// Returns the number of active subscriptions.
    #[getter]
    fn subscription_count(&self) -> usize {
        self.subscriptions.lock().len()
    }

    /// Returns all active subscription keys in "EXCHANGE:SYMBOL" format.
    fn subscriptions(&self) -> Vec<String> {
        self.subscriptions.lock().iter().cloned().collect()
    }

    /// Returns the number of active live bar subscriptions.
    #[getter]
    fn bar_subscription_count(&self) -> usize {
        self.bar_subscriptions.lock().len()
    }

    /// Returns all active live bar subscription keys.
    fn bar_subscriptions(&self) -> Vec<String> {
        self.bar_subscriptions.lock().iter().cloned().collect()
    }

    /// Returns true if subscribed to quotes for the given instrument.
    fn is_subscribed(&self, symbol: &str, exchange: &str) -> bool {
        let key = format!("{exchange}:{symbol}");
        self.subscriptions.lock().contains(&key)
    }

    /// Returns true if subscribed to the given live time-bar stream.
    fn is_subscribed_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: String,
        bar_period: i32,
    ) -> bool {
        let key = Self::bar_subscription_key(symbol, exchange, &bar_type, bar_period);
        self.bar_subscriptions.lock().contains(&key)
    }

    /// Sets the callback for market data events.
    ///
    /// The callback will be called with each market data event (quotes, trades,
    /// live bars, connection state changes, etc.).
    ///
    /// Parameters
    /// ----------
    /// callback : callable
    ///     A Python callable that accepts a single argument (the event).
    ///     The event can be a QuoteTick, TradeTick, or MarketDataEvent.
    ///
    /// Example
    /// -------
    /// ```python
    /// def on_data(event):
    ///     if event.is_quote():
    ///         quote = event.as_quote()
    ///         print(f"Quote: {quote.symbol} bid={quote.bid_price}")
    ///
    /// client.set_data_callback(on_data)
    /// ```
    fn set_data_callback(&self, callback: Py<PyAny>) {
        *self.data_callback.lock() = Some(callback);
    }

    /// Clears the data callback.
    fn clear_data_callback(&self) {
        *self.data_callback.lock() = None;
    }

    /// Starts the background event loop for market data.
    ///
    /// This takes ownership of the gateway's market data receiver and dispatches
    /// events to the Python callback set via `set_data_callback`.
    ///
    /// This is an async method - use `await client.start_event_loop()`.
    fn start_event_loop<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let gateway = Arc::clone(&self.gateway);
        let callback = Arc::clone(&self.data_callback);
        let event_task = Arc::clone(&self.event_task);
        let shutdown_tx = Arc::clone(&self.shutdown_tx);

        future_into_py(py, async move {
            // Check if already running
            if event_task.lock().is_some() {
                return Err(to_pyruntime_err("Market data event loop already running"));
            }

            // Take the receiver from gateway
            let mut gw = gateway.write().await;
            let rx = gw.take_market_data_receiver().ok_or_else(|| {
                to_pyruntime_err("Market data receiver already taken or not available")
            })?;

            // Create shutdown channel
            let (tx, rx_shutdown) = tokio::sync::oneshot::channel();
            *shutdown_tx.lock() = Some(tx);

            // Spawn event processing task
            let handle = get_runtime().spawn(Self::event_loop(rx, rx_shutdown, callback));

            // Store task handle
            *event_task.lock() = Some(handle);

            Ok(())
        })
    }

    /// Stops the background event loop for market data.
    fn stop_event_loop(&self) {
        // Send shutdown signal first, then abort
        // Using take() ensures idempotent cleanup
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.event_task.lock().take() {
            handle.abort();
        }
    }

    /// Subscribes to quotes (best bid/offer) for an instrument.
    ///
    /// This is an async method - use `await client.subscribe_quotes(symbol, exchange)`.
    ///
    /// Parameters
    /// ----------
    /// symbol : str
    ///     The instrument symbol (e.g., "ESH5").
    /// exchange : str
    ///     The exchange code (e.g., "CME").
    ///
    /// Returns
    /// -------
    /// None
    ///     On successful subscription.
    ///
    /// Raises
    /// ------
    /// RuntimeError
    ///     If subscription fails.
    /// ValueError
    ///     If symbol or exchange is empty.
    fn subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Validate inputs
        Self::validate_symbol_exchange(&symbol, &exchange)?;

        let gateway = Arc::clone(&self.gateway);
        let subscriptions = Arc::clone(&self.subscriptions);
        let key = format!("{exchange}:{symbol}");

        future_into_py(py, async move {
            let gw = gateway.read().await;
            gw.subscribe_market_data(&symbol, &exchange)
                .await
                .map(|()| {
                    // Only add to tracking on success
                    subscriptions.lock().insert(key);
                })
                .map_err(|e| to_pyruntime_err(format!("Subscription failed: {e}")))
        })
    }

    /// Subscribes to trades (last trade) for an instrument.
    ///
    /// This is an async method.
    ///
    /// Note: Rithmic's subscription returns both BBO and trades,
    /// so this is equivalent to subscribe_quotes().
    fn subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Same as subscribe_quotes - Rithmic sends both
        self.subscribe_quotes(py, symbol, exchange)
    }

    /// Subscribes to both quotes and trades for an instrument.
    ///
    /// This is an async method.
    fn subscribe<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.subscribe_quotes(py, symbol, exchange)
    }

    /// Subscribes to live bars on the history plant.
    ///
    /// This is an async method.
    fn subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
        bar_type: String,
        bar_period: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        Self::validate_symbol_exchange(&symbol, &exchange)?;
        let bar_type = Self::parse_bar_type(&bar_type)?;

        let gateway = Arc::clone(&self.gateway);
        let bar_subscriptions = Arc::clone(&self.bar_subscriptions);
        let key = Self::bar_subscription_key(&symbol, &exchange, bar_type.as_str(), bar_period);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            match bar_type {
                ParsedBarType::Time(bar_type) => gw
                    .subscribe_time_bars(&symbol, &exchange, bar_type, bar_period)
                    .await
                    .map_err(|e| to_pyruntime_err(format!("Live bar subscription failed: {e}")))?,
                ParsedBarType::Tick => {
                    let handle = gw.history_handle().ok_or_else(|| {
                        to_pyruntime_err("History plant not connected".to_string())
                    })?;

                    let response = handle
                        .subscribe_tick_bar_updates(
                            &symbol,
                            &exchange,
                            request_tick_bar_update::BarType::TickBar,
                            request_tick_bar_update::BarSubType::Regular,
                            &bar_period.to_string(),
                            request_tick_bar_update::Request::Subscribe,
                        )
                        .await
                        .map_err(|e| {
                            to_pyruntime_err(format!("Live bar subscription failed: {e}"))
                        })?;

                    if let Some(error) = response.error {
                        return Err(to_pyruntime_err(format!(
                            "Live bar subscription failed: {error}"
                        )));
                    }
                }
            }

            bar_subscriptions.lock().insert(key);
            Ok(())
        })
    }

    /// Unsubscribes from market data for an instrument.
    ///
    /// This is an async method.
    fn unsubscribe<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Validate inputs
        Self::validate_symbol_exchange(&symbol, &exchange)?;

        let gateway = Arc::clone(&self.gateway);
        let subscriptions = Arc::clone(&self.subscriptions);
        let key = format!("{exchange}:{symbol}");

        future_into_py(py, async move {
            let gw = gateway.read().await;
            gw.unsubscribe_market_data(&symbol, &exchange)
                .await
                .map(|()| {
                    // Only remove from tracking on success
                    subscriptions.lock().remove(&key);
                })
                .map_err(|e| to_pyruntime_err(format!("Unsubscribe failed: {e}")))
        })
    }

    /// Unsubscribes from live bars on the history plant.
    fn unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
        bar_type: String,
        bar_period: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        Self::validate_symbol_exchange(&symbol, &exchange)?;
        let bar_type = Self::parse_bar_type(&bar_type)?;

        let gateway = Arc::clone(&self.gateway);
        let bar_subscriptions = Arc::clone(&self.bar_subscriptions);
        let key = Self::bar_subscription_key(&symbol, &exchange, bar_type.as_str(), bar_period);

        future_into_py(py, async move {
            let gw = gateway.read().await;
            match bar_type {
                ParsedBarType::Time(bar_type) => gw
                    .unsubscribe_time_bars(&symbol, &exchange, bar_type, bar_period)
                    .await
                    .map_err(|e| to_pyruntime_err(format!("Live bar unsubscribe failed: {e}")))?,
                ParsedBarType::Tick => {
                    let handle = gw.history_handle().ok_or_else(|| {
                        to_pyruntime_err("History plant not connected".to_string())
                    })?;

                    let response = handle
                        .subscribe_tick_bar_updates(
                            &symbol,
                            &exchange,
                            request_tick_bar_update::BarType::TickBar,
                            request_tick_bar_update::BarSubType::Regular,
                            &bar_period.to_string(),
                            request_tick_bar_update::Request::Unsubscribe,
                        )
                        .await
                        .map_err(|e| {
                            to_pyruntime_err(format!("Live bar unsubscribe failed: {e}"))
                        })?;

                    if let Some(error) = response.error {
                        return Err(to_pyruntime_err(format!(
                            "Live bar unsubscribe failed: {error}"
                        )));
                    }
                }
            }

            bar_subscriptions.lock().remove(&key);
            Ok(())
        })
    }

    /// Unsubscribes from all market data (local tracking only).
    fn unsubscribe_all(&self) {
        self.subscriptions.lock().clear();
        self.bar_subscriptions.lock().clear();
    }

    /// Requests historical bars via the history plant.
    ///
    /// This is an async method - use `await client.request_bars(...)`.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (symbol, exchange, bar_type, bar_period, start_time_sec, end_time_sec))]
    fn request_bars<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: String,
        bar_type: String,
        bar_period: i32,
        start_time_sec: i32,
        end_time_sec: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Validate inputs
        Self::validate_symbol_exchange(&symbol, &exchange)?;

        let gateway = Arc::clone(&self.gateway);

        future_into_py(py, async move {
            let bar_type = Self::parse_bar_type(&bar_type)?;
            let gw = gateway.read().await;
            let responses = match bar_type {
                ParsedBarType::Time(bar_type) => gw
                    .request_bars(
                        &symbol,
                        &exchange,
                        bar_type,
                        bar_period,
                        start_time_sec,
                        end_time_sec,
                    )
                    .await
                    .map_err(|e| to_pyruntime_err(e.to_string()))?,
                ParsedBarType::Tick => {
                    if bar_period != 1 {
                        return Err(to_pynotimplemented_err(TICK_BAR_HISTORY_NOT_EXPOSED));
                    }

                    let handle = gw.history_handle().ok_or_else(|| {
                        to_pyruntime_err("History plant not connected".to_string())
                    })?;

                    handle
                        .load_ticks(
                            symbol.clone(),
                            exchange.clone(),
                            start_time_sec,
                            end_time_sec,
                        )
                        .await
                        .map_err(|e| to_pyruntime_err(e.to_string()))?
                }
            };

            let mut bars = Vec::new();
            for response in responses {
                if let Some(error) = &response.error {
                    return Err(to_pyruntime_err(error.clone()));
                }

                match response.message {
                    RithmicMessage::ResponseTimeBarReplay(bar) => {
                        bars.push(PyTimeBar::from_time_response(&bar));
                    }
                    RithmicMessage::ResponseTickBarReplay(bar) => {
                        bars.push(PyTimeBar::from_tick_response(&bar));
                    }
                    _ => {}
                }
            }

            Ok(bars)
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "RithmicDataClient(connected={}, subscriptions={}, bar_subscriptions={})",
            self.is_connected(),
            self.subscription_count(),
            self.bar_subscription_count()
        )
    }
}

impl PyRithmicDataClient {
    /// Validates symbol and exchange are non-empty.
    fn validate_symbol_exchange(symbol: &str, exchange: &str) -> PyResult<()> {
        if symbol.trim().is_empty() {
            return Err(to_pyvalue_err("symbol cannot be empty"));
        }

        if exchange.trim().is_empty() {
            return Err(to_pyvalue_err("exchange cannot be empty"));
        }
        Ok(())
    }

    fn parse_bar_type(bar_type: &str) -> PyResult<ParsedBarType> {
        match bar_type {
            "SecondBar" => Ok(ParsedBarType::Time(TimeBarType::SecondBar)),
            "MinuteBar" => Ok(ParsedBarType::Time(TimeBarType::MinuteBar)),
            "DailyBar" => Ok(ParsedBarType::Time(TimeBarType::DailyBar)),
            "WeeklyBar" => Ok(ParsedBarType::Time(TimeBarType::WeeklyBar)),
            "TickBar" => Ok(ParsedBarType::Tick),
            _ => Err(to_pyvalue_err(
                "Unsupported bar type. Valid values: SecondBar, MinuteBar, DailyBar, WeeklyBar, TickBar",
            )),
        }
    }

    fn bar_subscription_key(
        symbol: &str,
        exchange: &str,
        bar_type: &str,
        bar_period: i32,
    ) -> String {
        format!("{exchange}:{symbol}:{bar_type}:{bar_period}")
    }

    /// Event processing loop that runs in a spawned task.
    ///
    /// This is separated out to make the async flow clearer and testable.
    async fn event_loop(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<MarketDataEvent>,
        mut rx_shutdown: tokio::sync::oneshot::Receiver<()>,
        callback: Arc<parking_lot::Mutex<Option<Py<PyAny>>>>,
    ) {
        loop {
            tokio::select! {
                _ = &mut rx_shutdown => {
                    tracing::debug!("Market data event loop received shutdown signal");
                    break;
                }
                event = rx.recv() => {
                    match event {
                        Some(event) => {
                            // Acquire GIL and dispatch event
                            // Note: Python::attach is blocking but safe here since
                            // we don't hold any Rust locks while waiting for GIL
                            pyo3::Python::attach(|py| {
                                // Access callback under GIL
                                let guard = callback.lock();
                                if let Some(ref cb) = *guard {
                                    let py_event = PyMarketDataEvent::from(event);
                                    if let Err(e) = cb.call1(py, (py_event,)) {
                                        tracing::error!("Error in Python data callback: {e}");
                                    }
                                }
                            });
                        }
                        None => {
                            tracing::debug!("Market data channel closed");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Dispatches a market data event to the Python callback.
    ///
    /// This is called from the Rust event processing loop.
    #[allow(dead_code)]
    pub(crate) fn dispatch_event(&self, event: MarketDataEvent) {
        Python::attach(|py| {
            let guard = self.data_callback.lock();
            if let Some(ref cb) = *guard {
                let py_event = PyMarketDataEvent::from(event);
                if let Err(e) = cb.call1(py, (py_event,)) {
                    tracing::error!("Error in Python data callback: {e}");
                }
            }
        });
    }

    /// Dispatches a quote tick directly to the Python callback.
    #[allow(dead_code)]
    pub(crate) fn dispatch_quote(&self, tick: crate::data::QuoteTick) {
        Python::attach(|py| {
            let guard = self.data_callback.lock();
            if let Some(ref cb) = *guard {
                let py_tick = PyQuoteTick::from(tick);
                if let Err(e) = cb.call1(py, (py_tick,)) {
                    tracing::error!("Error in Python quote callback: {e}");
                }
            }
        });
    }

    /// Dispatches a trade tick directly to the Python callback.
    #[allow(dead_code)]
    pub(crate) fn dispatch_trade(&self, tick: crate::data::TradeTick) {
        Python::attach(|py| {
            let guard = self.data_callback.lock();
            if let Some(ref cb) = *guard {
                let py_tick = PyTradeTick::from(tick);
                if let Err(e) = cb.call1(py, (py_tick,)) {
                    tracing::error!("Error in Python trade callback: {e}");
                }
            }
        });
    }
}

#[cfg(feature = "python")]
impl Drop for PyRithmicDataClient {
    fn drop(&mut self) {
        // Reuse stop_event_loop logic for consistent cleanup
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.event_task.lock().take() {
            handle.abort();
        }
    }
}

/// Registers data client types with the Python module.
#[cfg(feature = "python")]
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRithmicDataClient>()?;
    Ok(())
}
