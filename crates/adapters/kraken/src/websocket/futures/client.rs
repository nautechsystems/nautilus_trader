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

//! WebSocket client for the Kraken Futures v1 streaming API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use arc_swap::ArcSwap;
use dashmap::DashSet;
use nautilus_common::live::runtime::get_runtime;
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{WebSocketClient, WebSocketConfig, channel_message_handler},
};
use tokio_util::sync::CancellationToken;

// Re-export for backward compatibility
pub use super::messages::FuturesWsMessage as KrakenFuturesWsMessage;
use super::{
    handler::{FuturesFeedHandler, HandlerCommand},
    messages::FuturesWsMessage,
};
use crate::websocket::error::KrakenWsError;

#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct KrakenFuturesWebSocketClient {
    url: String,
    heartbeat_secs: Option<u64>,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<FuturesWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: Arc<DashSet<String>>,
    cancellation_token: CancellationToken,
}

impl Clone for KrakenFuturesWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            heartbeat_secs: self.heartbeat_secs,
            signal: Arc::clone(&self.signal),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: self.out_rx.clone(),
            task_handle: self.task_handle.clone(),
            subscriptions: self.subscriptions.clone(),
            cancellation_token: self.cancellation_token.clone(),
        }
    }
}

impl KrakenFuturesWebSocketClient {
    #[must_use]
    pub fn new(url: String, heartbeat_secs: Option<u64>) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat_secs,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: Arc::new(DashSet::new()),
            cancellation_token: CancellationToken::new(),
        }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        ConnectionMode::from_u8(self.connection_mode.load().load(Ordering::Relaxed))
            == ConnectionMode::Closed
    }

    /// Cache instruments for price precision lookup (bulk replace).
    ///
    /// Must be called after `connect()` when the handler is ready to receive commands.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        if let Ok(tx) = self.cmd_tx.try_read()
            && let Err(e) = tx.send(HandlerCommand::InitializeInstruments(instruments))
        {
            tracing::debug!("Failed to send instruments to handler: {e}");
        }
    }

    /// Cache a single instrument for price precision lookup (upsert).
    ///
    /// Must be called after `connect()` when the handler is ready to receive commands.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        if let Ok(tx) = self.cmd_tx.try_read()
            && let Err(e) = tx.send(HandlerCommand::UpdateInstrument(instrument))
        {
            tracing::debug!("Failed to send instrument update to handler: {e}");
        }
    }

    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Connecting to Futures WebSocket: {}", self.url);

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        let ws_config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            message_handler: Some(raw_handler),
            ping_handler: None,
            heartbeat: self.heartbeat_secs,
            heartbeat_msg: None, // Futures uses heartbeat feed, not ping
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };

        let ws_client = WebSocketClient::connect(ws_config, None, vec![], None)
            .await
            .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        self.connection_mode
            .store(ws_client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        let signal = self.signal.clone();
        let subscriptions = self.subscriptions.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FuturesFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions);

            while let Some(msg) = handler.next().await {
                if let Err(e) = out_tx.send(msg) {
                    tracing::debug!("Output channel closed: {e}");
                    break;
                }
            }

            tracing::debug!("Futures handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        tracing::debug!("Futures WebSocket connected successfully");
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Disconnecting Futures WebSocket");

        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            tracing::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => tracing::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => tracing::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            tracing::warn!("Timeout waiting for task handle");
                        }
                    }
                }
                Err(arc_handle) => {
                    tracing::debug!("Cannot take ownership of task handle, aborting");
                    arc_handle.abort();
                }
            }
        }

        self.subscriptions.clear();
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), KrakenWsError> {
        self.disconnect().await
    }

    /// Subscribe to mark price updates for the given instrument.
    pub async fn subscribe_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("mark:{product_id}");
        if self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.insert(key);
        self.ensure_ticker_subscribed(product_id).await
    }

    /// Unsubscribe from mark price updates for the given instrument.
    pub async fn unsubscribe_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        self.subscriptions.remove(&format!("mark:{product_id}"));
        self.maybe_unsubscribe_ticker(product_id).await
    }

    /// Subscribe to index price updates for the given instrument.
    pub async fn subscribe_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("index:{product_id}");
        if self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.insert(key);
        self.ensure_ticker_subscribed(product_id).await
    }

    /// Unsubscribe from index price updates for the given instrument.
    pub async fn unsubscribe_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        self.subscriptions.remove(&format!("index:{product_id}"));
        self.maybe_unsubscribe_ticker(product_id).await
    }

    /// Subscribe to quote updates for the given instrument.
    ///
    /// Uses the order book channel for low-latency top-of-book quotes.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("quotes:{product_id}");
        if self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.insert(key);

        // Use book feed for low-latency quotes (not throttled ticker)
        self.ensure_book_subscribed(product_id).await
    }

    /// Unsubscribe from quote updates for the given instrument.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        self.subscriptions.remove(&format!("quotes:{product_id}"));
        self.maybe_unsubscribe_book(product_id).await
    }

    /// Subscribe to trade updates for the given instrument.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("trades:{product_id}");
        if self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.insert(key.clone());

        // Subscribe to trade feed
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeTrade(product_id.to_string()))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        Ok(())
    }

    /// Unsubscribe from trade updates for the given instrument.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("trades:{product_id}");
        if !self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.remove(&key);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeTrade(product_id.to_string()))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        Ok(())
    }

    /// Subscribe to order book updates for the given instrument.
    ///
    /// Note: The `depth` parameter is accepted for API compatibility with spot client but is
    /// not used by Kraken Futures (full book is always returned).
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        _depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("book:{product_id}");
        if self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.insert(key.clone());

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeBook(product_id.to_string()))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        Ok(())
    }

    /// Unsubscribe from order book updates for the given instrument.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let product_id = instrument_id.symbol.as_str();
        let key = format!("book:{product_id}");
        if !self.subscriptions.contains(&key) {
            return Ok(());
        }

        self.subscriptions.remove(&key);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeBook(product_id.to_string()))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        Ok(())
    }

    /// Ensure ticker feed is subscribed for the given product.
    async fn ensure_ticker_subscribed(&self, product_id: &str) -> Result<(), KrakenWsError> {
        let ticker_key = format!("ticker:{product_id}");
        if !self.subscriptions.contains(&ticker_key) {
            self.subscriptions.insert(ticker_key);
            self.cmd_tx
                .read()
                .await
                .send(HandlerCommand::SubscribeTicker(product_id.to_string()))
                .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        }
        Ok(())
    }

    /// Unsubscribe from ticker if no more dependent subscriptions.
    async fn maybe_unsubscribe_ticker(&self, product_id: &str) -> Result<(), KrakenWsError> {
        let has_mark = self.subscriptions.contains(&format!("mark:{product_id}"));
        let has_index = self.subscriptions.contains(&format!("index:{product_id}"));

        if !has_mark && !has_index {
            self.subscriptions.remove(&format!("ticker:{product_id}"));
            self.cmd_tx
                .read()
                .await
                .send(HandlerCommand::UnsubscribeTicker(product_id.to_string()))
                .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        }
        Ok(())
    }

    /// Ensure book feed is subscribed for the given product (for quotes).
    async fn ensure_book_subscribed(&self, product_id: &str) -> Result<(), KrakenWsError> {
        let book_key = format!("book:{product_id}");
        if !self.subscriptions.contains(&book_key) {
            self.subscriptions.insert(book_key);
            self.cmd_tx
                .read()
                .await
                .send(HandlerCommand::SubscribeBook(product_id.to_string()))
                .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        }
        Ok(())
    }

    /// Unsubscribe from book if no more dependent subscriptions.
    async fn maybe_unsubscribe_book(&self, product_id: &str) -> Result<(), KrakenWsError> {
        let has_quotes = self.subscriptions.contains(&format!("quotes:{product_id}"));
        let has_book = self.subscriptions.contains(&format!("book:{product_id}"));

        // Only unsubscribe if no quotes subscription and no explicit book subscription
        if !has_quotes && !has_book {
            self.subscriptions.remove(&format!("book:{product_id}"));
            self.cmd_tx
                .read()
                .await
                .send(HandlerCommand::UnsubscribeBook(product_id.to_string()))
                .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        }
        Ok(())
    }

    /// Get the output receiver for processed messages.
    pub fn take_output_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<FuturesWsMessage>> {
        self.out_rx.take().and_then(|arc| Arc::try_unwrap(arc).ok())
    }
}
