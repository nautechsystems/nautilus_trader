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

//! Python bindings for Delta Exchange WebSocket client.
//!
//! # Design Pattern: Clone and Share State
//!
//! The WebSocket client must be cloned for async operations because PyO3's `future_into_py`
//! requires `'static` futures (cannot borrow from `self`). To ensure clones share the same
//! connection state, key fields use `Arc<RwLock<T>>`:
//!
//! - Connection state and subscriptions are shared across clones.
//!
//! Without shared state, clones would be independent, causing:
//! - Lost WebSocket messages.
//! - Missing subscription data.
//! - Connection state desynchronization.

use futures_util::StreamExt;
use nautilus_core::python::to_pyvalue_err;
use pyo3::{prelude::*, types::PyList};
use ustr::Ustr;

use super::{
    config::PyDeltaExchangeWsConfig,
    error::ws_error_to_py_err,
};
use crate::websocket::{
    client::DeltaExchangeWebSocketClient,
    enums::{ConnectionState, DeltaExchangeWsChannel, SubscriptionState},
    messages::NautilusWsMessage,
};

/// Python wrapper for Delta Exchange WebSocket client.
#[pyclass(name = "DeltaExchangeWebSocketClient")]
#[derive(Debug, Clone)]
pub struct PyDeltaExchangeWebSocketClient {
    pub inner: DeltaExchangeWebSocketClient,
}

#[pymethods]
impl PyDeltaExchangeWebSocketClient {
    #[new]
    #[pyo3(signature = (config=None, api_key=None, api_secret=None))]
    fn py_new(
        config: Option<PyDeltaExchangeWsConfig>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> PyResult<Self> {
        let ws_config = config.map(|c| c.inner);
        
        let client = DeltaExchangeWebSocketClient::new(
            ws_config,
            api_key,
            api_secret,
            None, // heartbeat_interval
        )
        .map_err(ws_error_to_py_err)?;

        Ok(Self { inner: client })
    }

    /// Create client for testnet environment.
    #[staticmethod]
    #[pyo3(name = "testnet")]
    fn py_testnet(api_key: Option<String>, api_secret: Option<String>) -> PyResult<Self> {
        let config = Some(crate::websocket::client::DeltaExchangeWsConfig::testnet());
        
        let client = DeltaExchangeWebSocketClient::new(
            config,
            api_key,
            api_secret,
            None,
        )
        .map_err(ws_error_to_py_err)?;

        Ok(Self { inner: client })
    }

    // Connection management

    /// Connect to the WebSocket.
    #[pyo3(name = "connect")]
    fn py_connect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(ws_error_to_py_err)?;
            Ok(())
        })
    }

    /// Disconnect from the WebSocket.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(ws_error_to_py_err)?;
            Ok(())
        })
    }

    /// Check if the WebSocket is connected.
    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.inner.connection_state() == ConnectionState::Connected
    }

    /// Get the current connection state.
    #[pyo3(name = "connection_state")]
    fn py_connection_state(&self) -> String {
        match self.inner.connection_state() {
            ConnectionState::Disconnected => "disconnected".to_string(),
            ConnectionState::Connecting => "connecting".to_string(),
            ConnectionState::Connected => "connected".to_string(),
            ConnectionState::Disconnecting => "disconnecting".to_string(),
            ConnectionState::Reconnecting => "reconnecting".to_string(),
            ConnectionState::Failed => "failed".to_string(),
        }
    }

    /// Get the number of reconnection attempts.
    #[pyo3(name = "reconnection_attempts")]
    fn py_reconnection_attempts(&self) -> u32 {
        self.inner.reconnection_attempts()
    }

    /// Reset the reconnection attempts counter.
    #[pyo3(name = "reset_reconnection_attempts")]
    fn py_reset_reconnection_attempts(&self) {
        self.inner.reset_reconnection_attempts();
    }

    // Subscription management

    /// Subscribe to a channel.
    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &mut self,
        py: Python<'py>,
        channel: String,
        symbols: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let ws_channel = parse_channel(&channel)?;
            let ustr_symbols = symbols.map(|s| s.into_iter().map(|sym| Ustr::from(&sym)).collect());

            client
                .subscribe(ws_channel, ustr_symbols)
                .await
                .map_err(ws_error_to_py_err)?;
            Ok(())
        })
    }

    /// Unsubscribe from a channel.
    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &mut self,
        py: Python<'py>,
        channel: String,
        symbols: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let ws_channel = parse_channel(&channel)?;
            let ustr_symbols = symbols.map(|s| s.into_iter().map(|sym| Ustr::from(&sym)).collect());

            client
                .unsubscribe(ws_channel, ustr_symbols)
                .await
                .map_err(ws_error_to_py_err)?;
            Ok(())
        })
    }

    /// Check if subscribed to a channel.
    #[pyo3(name = "is_subscribed")]
    fn py_is_subscribed(&self, channel: String, symbols: Option<Vec<String>>) -> PyResult<bool> {
        let ws_channel = parse_channel(&channel)?;
        let ustr_symbols = symbols.map(|s| s.into_iter().map(|sym| Ustr::from(&sym)).collect());

        Ok(self.inner.is_subscribed(ws_channel, ustr_symbols))
    }

    /// Get all current subscriptions.
    #[pyo3(name = "get_subscriptions")]
    fn py_get_subscriptions(&self) -> Vec<(String, Option<Vec<String>>, String)> {
        self.inner
            .get_subscriptions()
            .into_iter()
            .map(|(channel, symbols, state)| {
                let channel_str = channel_to_string(channel);
                let symbols_str = symbols.map(|s| s.into_iter().map(|sym| sym.to_string()).collect());
                let state_str = match state {
                    SubscriptionState::Pending => "pending".to_string(),
                    SubscriptionState::Active => "active".to_string(),
                    SubscriptionState::Unsubscribing => "unsubscribing".to_string(),
                    SubscriptionState::Failed => "failed".to_string(),
                    SubscriptionState::Inactive => "inactive".to_string(),
                };
                (channel_str, symbols_str, state_str)
            })
            .collect()
    }

    // Message handling

    /// Get the next message from the WebSocket.
    #[pyo3(name = "next_message")]
    fn py_next_message<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(mut rx) = client.message_receiver().await {
                if let Some(message) = rx.next().await {
                    return Python::with_gil(|py| {
                        match message {
                            NautilusWsMessage::Raw(text) => Ok(text.into_py(py).unbind()),
                            // Add more message type handling as needed
                        }
                    });
                }
            }
            Ok(Python::with_gil(|py| py.None()))
        })
    }

    /// Start message streaming with a callback.
    #[pyo3(name = "start_message_stream")]
    fn py_start_message_stream<'py>(
        &mut self,
        py: Python<'py>,
        callback: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(mut rx) = client.message_receiver().await {
                tokio::spawn(async move {
                    while let Some(message) = rx.next().await {
                        Python::with_gil(|py| {
                            let py_message = match message {
                                NautilusWsMessage::Raw(text) => text.into_py(py),
                                // Add more message type handling as needed
                            };

                            if let Err(e) = callback.call1(py, (py_message,)) {
                                eprintln!("Error calling Python callback: {}", e);
                            }
                        });
                    }
                });
            }
            Ok(())
        })
    }

    fn __str__(&self) -> String {
        format!(
            "DeltaExchangeWebSocketClient(state={})",
            self.py_connection_state()
        )
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

/// Parse channel string to DeltaExchangeWsChannel enum.
fn parse_channel(channel: &str) -> PyResult<DeltaExchangeWsChannel> {
    match channel {
        "v2_ticker" => Ok(DeltaExchangeWsChannel::V2Ticker),
        "l2_orderbook" => Ok(DeltaExchangeWsChannel::L2Orderbook),
        "l2_updates" => Ok(DeltaExchangeWsChannel::L2Updates),
        "all_trades" => Ok(DeltaExchangeWsChannel::AllTrades),
        "mark_price" => Ok(DeltaExchangeWsChannel::MarkPrice),
        "candlesticks" => Ok(DeltaExchangeWsChannel::Candlesticks),
        "spot_price" => Ok(DeltaExchangeWsChannel::SpotPrice),
        "funding_rate" => Ok(DeltaExchangeWsChannel::FundingRate),
        "product_updates" => Ok(DeltaExchangeWsChannel::ProductUpdates),
        "announcements" => Ok(DeltaExchangeWsChannel::Announcements),
        "margins" => Ok(DeltaExchangeWsChannel::Margins),
        "positions" => Ok(DeltaExchangeWsChannel::Positions),
        "orders" => Ok(DeltaExchangeWsChannel::Orders),
        "user_trades" => Ok(DeltaExchangeWsChannel::UserTrades),
        "v2/user_trades" => Ok(DeltaExchangeWsChannel::V2UserTrades),
        "portfolio_margins" => Ok(DeltaExchangeWsChannel::PortfolioMargins),
        "mmp_trigger" => Ok(DeltaExchangeWsChannel::MmpTrigger),
        _ => Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
            format!("Invalid channel: {}", channel),
        )),
    }
}

/// Convert DeltaExchangeWsChannel enum to string.
fn channel_to_string(channel: DeltaExchangeWsChannel) -> String {
    match channel {
        DeltaExchangeWsChannel::V2Ticker => "v2_ticker".to_string(),
        DeltaExchangeWsChannel::L2Orderbook => "l2_orderbook".to_string(),
        DeltaExchangeWsChannel::L2Updates => "l2_updates".to_string(),
        DeltaExchangeWsChannel::AllTrades => "all_trades".to_string(),
        DeltaExchangeWsChannel::MarkPrice => "mark_price".to_string(),
        DeltaExchangeWsChannel::Candlesticks => "candlesticks".to_string(),
        DeltaExchangeWsChannel::SpotPrice => "spot_price".to_string(),
        DeltaExchangeWsChannel::FundingRate => "funding_rate".to_string(),
        DeltaExchangeWsChannel::ProductUpdates => "product_updates".to_string(),
        DeltaExchangeWsChannel::Announcements => "announcements".to_string(),
        DeltaExchangeWsChannel::Margins => "margins".to_string(),
        DeltaExchangeWsChannel::Positions => "positions".to_string(),
        DeltaExchangeWsChannel::Orders => "orders".to_string(),
        DeltaExchangeWsChannel::UserTrades => "user_trades".to_string(),
        DeltaExchangeWsChannel::V2UserTrades => "v2/user_trades".to_string(),
        DeltaExchangeWsChannel::PortfolioMargins => "portfolio_margins".to_string(),
        DeltaExchangeWsChannel::MmpTrigger => "mmp_trigger".to_string(),
    }
}
