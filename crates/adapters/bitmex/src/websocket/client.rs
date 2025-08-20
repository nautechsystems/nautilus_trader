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

//! Provides the WebSocket client integration for the [BitMEX](https://bitmex.com) WebSocket API.
//!
//! This module defines and implements a strongly-typed [`BitmexWebSocketClient`] for
//! connecting to BitMEX WebSocket streams. It handles authentication (when credentials
//! are provided), manages subscriptions to market data and account update channels,
//! and parses incoming messages into structured Nautilus domain objects.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use ahash::AHashSet;
use dashmap::DashMap;
use futures_util::{Stream, StreamExt};
use nautilus_common::runtime::get_runtime;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, env::get_env_var, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Data, bar::BarType},
    identifiers::{InstrumentId, Symbol},
};
use nautilus_network::websocket::{Consumer, MessageReader, WebSocketClient, WebSocketConfig};
use reqwest::header::USER_AGENT;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    cache::QuoteCache,
    enums::{Action, WsTopic},
    error::BitmexWsError,
    messages::{NautilusWsMessage, TableMessage, WsMessage},
    parse::{
        self, is_index_symbol, parse_book_msg_vec, parse_book10_msg_vec, parse_trade_bin_msg_vec,
        parse_trade_msg_vec, topic_from_bar_spec,
    },
};
use crate::{consts::BITMEX_WS_URL, credential::Credential};

#[derive(Debug, Clone, Default)]
struct InstrumentSubscriptionFlags {
    mark_prices: bool,
    index_prices: bool,
}

/// Provides a WebSocket client for connecting to the [BitMEX](https://bitmex.com) real-time API.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BitmexWebSocketClient {
    url: String,
    credential: Option<Credential>,
    heartbeat: Option<u64>,
    inner: Option<Arc<WebSocketClient>>,
    rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: Arc<DashMap<String, AHashSet<Ustr>>>,
    instrument_subscriptions: Arc<DashMap<Symbol, InstrumentSubscriptionFlags>>,
    message_count: Arc<AtomicU64>,
}

impl BitmexWebSocketClient {
    /// Creates a new [`BitmexWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided (both or neither required).
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key.to_string(), secret.to_string())),
            (None, None) => None,
            _ => anyhow::bail!("Both `api_key` and `api_secret` must be provided together"),
        };

        Ok(Self {
            url: url.unwrap_or(BITMEX_WS_URL.to_string()).to_string(),
            credential,
            heartbeat,
            inner: None,
            rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: Arc::new(DashMap::new()),
            instrument_subscriptions: Arc::new(DashMap::new()),
            message_count: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Creates a new authenticated [`BitmexWebSocketClient`] using environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if environment variables are not set or credentials are invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        let url = get_env_var("BITMEX_WS_URL")?;
        let api_key = get_env_var("BITMEX_API_KEY")?;
        let api_secret = get_env_var("BITMEX_API_SECRET")?;

        Self::new(Some(url), Some(api_key), Some(api_secret), None)
    }

    /// Returns the websocket url being used by the client.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the public API key being used by the client.
    pub fn api_key(&self) -> Option<&str> {
        self.credential.clone().map(|c| c.api_key.as_str())
    }

    /// Returns a value indicating whether the client is active.
    pub fn is_active(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_active(),
            None => false,
        }
    }

    /// Returns a value indicating whether the client is closed.
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_closed(),
            None => true,
        }
    }

    /// Connect to the WebSocket for streaming.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails or authentication fails (if credentials provided).
    pub async fn connect(&mut self) -> Result<(), BitmexWsError> {
        let reader = self.connect_inner().await?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.rx = Some(Arc::new(rx));
        let signal = self.signal.clone();
        let message_count = self.message_count.clone();

        let stream_handle = get_runtime().spawn(async move {
            BitmexUnifiedFeedHandler::new(reader, signal, message_count, tx)
                .run()
                .await;
        });

        self.task_handle = Some(Arc::new(stream_handle));

        Ok(())
    }

    /// Connect to the WebSocket and return a message reader.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails or if authentication fails (when credentials are provided).
    async fn connect_inner(&mut self) -> Result<MessageReader, BitmexWsError> {
        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            #[cfg(feature = "python")]
            handler: Consumer::Python(None),
            #[cfg(not(feature = "python"))]
            handler: {
                let (consumer, _rx) = Consumer::rust_consumer();
                consumer
            },
            #[cfg(feature = "python")]
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };

        let keyed_quotas = vec![];
        let (reader, client) = WebSocketClient::connect_stream(config, keyed_quotas, None, None)
            .await
            .map_err(|e| BitmexWsError::ClientError(e.to_string()))?;

        self.inner = Some(Arc::new(client));

        if self.credential.is_some() {
            self.authenticate().await?;
        }

        Ok(reader)
    }

    /// Authenticate the WebSocket connection using the provided credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if authentication fails.
    ///
    /// # Panics
    ///
    /// Panics if credentials are not available when this method is called.
    async fn authenticate(&mut self) -> Result<(), BitmexWsError> {
        let credential = match &self.credential {
            Some(credential) => credential,
            None => {
                panic!("API credentials not available to authenticate");
            }
        };

        let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
        let signature = credential.sign("GET", "/realtime", expires, "");

        let auth_message = serde_json::json!({
            "op": "authKeyExpires",
            "args": [credential.api_key, expires, signature]
        });

        if let Some(inner) = &self.inner {
            inner
                .send_text(auth_message.to_string(), None)
                .await
                .map_err(|e| BitmexWsError::AuthenticationError(e.to_string()))
        } else {
            log::error!("Cannot authenticate: not connected");
            Ok(())
        }
    }

    /// Provides the internal stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the websocket is not connected.
    /// - If `stream` has already been called somewhere else (stream receiver is then taken).
    pub fn stream(&mut self) -> impl Stream<Item = NautilusWsMessage> + use<> {
        let rx = self
            .rx
            .take()
            .expect("Stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Closes the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if closing fails.
    ///
    /// # Panics
    ///
    /// Panics if the task handle cannot be unwrapped (should never happen in normal usage).
    pub async fn close(&mut self) -> Result<(), BitmexWsError> {
        if let Some(inner) = &self.inner {
            inner.disconnect().await;
        } else {
            log::error!("Error on close: not connected");
        }

        self.signal.store(true, Ordering::Relaxed);

        // Clean up stream handle with timeout
        if let Some(stream_handle) = self.task_handle.take() {
            match Arc::try_unwrap(stream_handle) {
                Ok(handle) => {
                    log::debug!("Waiting for stream handle to complete");
                    match tokio::time::timeout(Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Stream handle completed successfully"),
                        Ok(Err(e)) => log::error!("Stream handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!(
                                "Timeout waiting for stream handle, task may still be running"
                            );
                            // The task will be dropped and should clean up automatically
                        }
                    }
                }
                Err(arc_handle) => {
                    log::debug!(
                        "Cannot take ownership of stream handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            log::debug!("No stream handle to await");
        }

        log::debug!("Closed");

        Ok(())
    }

    /// Subscribe to the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the subscription message fails.
    pub async fn subscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        // Track subscriptions
        for topic in &topics {
            if let Some((channel, symbol)) = topic.split_once(':') {
                self.subscriptions
                    .entry(channel.to_string())
                    .or_default()
                    .insert(Ustr::from(symbol));
            } else {
                // Topic without symbol (e.g., "execution", "order")
                self.subscriptions.entry(topic.clone()).or_default();
            }
        }

        let message = serde_json::json!({
            "op": "subscribe",
            "args": topics
        });

        if let Some(inner) = &self.inner {
            inner
                .send_text(message.to_string(), None)
                .await
                .map_err(|e| BitmexWsError::SubscriptionError(e.to_string()))?;
        } else {
            log::error!("Cannot send message: not connected");
        }

        Ok(())
    }

    /// Unsubscribe from the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the unsubscription message fails.
    async fn unsubscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        // Remove from tracked subscriptions
        for topic in &topics {
            if let Some((channel, symbol)) = topic.split_once(':') {
                if let Some(mut entry) = self.subscriptions.get_mut(channel) {
                    entry.remove(&Ustr::from(symbol));
                    if entry.is_empty() {
                        self.subscriptions.remove(channel);
                    }
                }
            } else {
                // Topic without symbol
                self.subscriptions.remove(topic);
            }
        }

        let message = serde_json::json!({
            "op": "unsubscribe",
            "args": topics
        });

        if let Some(inner) = &self.inner {
            inner
                .send_text(message.to_string(), None)
                .await
                .map_err(|e| BitmexWsError::SubscriptionError(e.to_string()))?;
        } else {
            log::error!("Cannot send message: not connected");
        }

        Ok(())
    }

    /// Get the current number of active subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Get the total number of messages received.
    pub fn message_count(&self) -> u64 {
        self.message_count.load(Ordering::Relaxed)
    }

    /// Subscribe to instrument updates for all instruments on the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_instruments(&self) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Instrument;
        self.subscribe(vec![topic.to_string()]).await
    }

    /// Subscribe to instrument updates (mark/index prices) for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Instrument;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to order book updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to order book L2 (25 levels) updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book_25(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2_25;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to order book depth 10 updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBook10;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to quote updates for the specified instrument.
    ///
    /// Note: Index symbols (starting with '.') do not have quotes and will be silently ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.as_str();

        // Index symbols don't have quotes (bid/ask), only a single price
        if is_index_symbol(symbol) {
            tracing::warn!("Ignoring quote subscription for index symbol: {symbol}");
            return Ok(());
        }

        let topic = WsTopic::Quote;
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to trade updates for the specified instrument.
    ///
    /// Note: Index symbols (starting with '.') do not have trades and will be silently ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.as_str();

        // Index symbols don't have trades
        if is_index_symbol(symbol) {
            tracing::warn!("Ignoring trade subscription for index symbol: {symbol}");
            return Ok(());
        }

        let topic = WsTopic::Trade;
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol;
        let mut entry = self.instrument_subscriptions.entry(symbol).or_default();

        if !entry.mark_prices {
            entry.mark_prices = true;
            let needs_subscription = !entry.index_prices;
            drop(entry);

            if needs_subscription {
                self.subscribe_instrument(instrument_id).await?;
            }
        }

        Ok(())
    }

    /// Subscribe to index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol;
        let mut entry = self.instrument_subscriptions.entry(symbol).or_default();

        if !entry.index_prices {
            entry.index_prices = true;
            let needs_subscription = !entry.mark_prices;
            drop(entry);

            if needs_subscription {
                self.subscribe_instrument(instrument_id).await?;
            }
        }

        Ok(())
    }

    /// Subscribe to funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Funding;
        let symbol = instrument_id.symbol.as_str();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to bar updates for the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), BitmexWsError> {
        let topic = topic_from_bar_spec(bar_type.spec());
        let symbol = bar_type.instrument_id().symbol.to_string();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from instrument updates for all instruments on the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_instruments(&self) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Instrument;
        self.unsubscribe(vec![topic.to_string()]).await
    }

    /// Unsubscribe from instrument updates (mark/index prices) for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Instrument;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from order book updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from order book L2 (25 levels) updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book_25(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBookL2_25;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from order book depth 10 updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::OrderBook10;
        let symbol = instrument_id.symbol.as_str();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.as_str();

        // Index symbols don't have quotes
        if is_index_symbol(symbol) {
            return Ok(());
        }

        let topic = WsTopic::Quote;
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.as_str();

        // Index symbols don't have trades
        if is_index_symbol(symbol) {
            return Ok(());
        }

        let topic = WsTopic::Trade;
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol;

        if let Some(mut entry) = self.instrument_subscriptions.get_mut(&symbol)
            && entry.mark_prices
        {
            entry.mark_prices = false;
            let should_unsubscribe = !entry.index_prices;
            drop(entry);

            if should_unsubscribe {
                self.unsubscribe_instrument(instrument_id).await?;
                self.instrument_subscriptions.remove(&symbol);
            }
        }

        Ok(())
    }

    /// Unsubscribe from index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol;

        if let Some(mut entry) = self.instrument_subscriptions.get_mut(&symbol)
            && entry.index_prices
        {
            entry.index_prices = false;
            let should_unsubscribe = !entry.mark_prices;
            drop(entry);

            if should_unsubscribe {
                self.unsubscribe_instrument(instrument_id).await?;
                self.instrument_subscriptions.remove(&symbol);
            }
        }

        Ok(())
    }

    /// Unsubscribe from funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = WsTopic::Funding;
        let symbol = instrument_id.symbol.as_str();
        let topic_str = format!("{topic}:{symbol}");
        self.unsubscribe(vec![topic_str]).await
    }

    /// Unsubscribe from bar updates for the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), BitmexWsError> {
        let topic = topic_from_bar_spec(bar_type.spec());
        let symbol = bar_type.instrument_id().symbol.to_string();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }
}

struct BitmexFeedHandler {
    reader: MessageReader,
    signal: Arc<AtomicBool>,
    message_count: Arc<AtomicU64>,
}

impl BitmexFeedHandler {
    /// Creates a new [`BitmexFeedHandler`] instance.
    pub const fn new(
        reader: MessageReader,
        signal: Arc<AtomicBool>,
        message_count: Arc<AtomicU64>,
    ) -> Self {
        Self {
            reader,
            signal,
            message_count,
        }
    }

    /// Get the next message from the WebSocket stream.
    async fn next(&mut self) -> Option<WsMessage> {
        // Timeout awaiting the next message before checking signal
        let timeout_duration = Duration::from_millis(10);

        loop {
            if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::debug!("Stop signal received");
                break;
            }

            match tokio::time::timeout(timeout_duration, self.reader.next()).await {
                Ok(Some(msg)) => match msg {
                    Ok(Message::Text(text)) => {
                        self.message_count.fetch_add(1, Ordering::Relaxed);
                        tracing::trace!("Raw websocket message: {text}");

                        match serde_json::from_str(&text) {
                            Ok(msg) => match &msg {
                                WsMessage::Welcome {
                                    version,
                                    heartbeat_enabled,
                                    limit,
                                    ..
                                } => {
                                    tracing::info!(
                                        version = version,
                                        heartbeat = heartbeat_enabled,
                                        rate_limit = limit.remaining,
                                        "Welcome to the BitMEX Realtime API:",
                                    );
                                }
                                WsMessage::Subscription {
                                    success,
                                    subscribe,
                                    error,
                                } => {
                                    if let Some(subscribe) = subscribe {
                                        tracing::debug!("Subscribed to: {subscribe}");
                                    }
                                    if let Some(error) = error {
                                        tracing::error!(error);
                                    }
                                    tracing::debug!("Success: {success}");
                                }
                                WsMessage::Error { status, error, .. } => {
                                    tracing::error!(status = status, error = error);
                                    break; // TODO: Break for now
                                }
                                _ => return Some(msg),
                            },
                            Err(e) => {
                                tracing::error!("{e}: {text}");
                                break; // TODO: Break for now
                            }
                        }
                    }
                    Ok(Message::Binary(msg)) => {
                        tracing::debug!("Raw binary: {msg:?}");
                    }
                    Ok(Message::Close(_)) => {
                        tracing::debug!("Received close message");
                        return None;
                    }
                    Ok(msg) => match msg {
                        Message::Ping(data) => {
                            tracing::trace!("Received ping frame with {} bytes", data.len());
                        }
                        Message::Pong(data) => {
                            tracing::trace!("Received pong frame with {} bytes", data.len());
                        }
                        Message::Frame(frame) => {
                            tracing::debug!("Received raw frame: {frame:?}");
                        }
                        _ => {
                            tracing::warn!("Unexpected message type: {msg:?}");
                        }
                    },
                    Err(e) => {
                        tracing::error!("{e}");
                        break; // TODO: Break for now
                    }
                },
                Ok(None) => {
                    tracing::info!("WebSocket stream closed");
                    break;
                }
                Err(_) => {} // Timeout occurred awaiting a message, continue loop to check signal
            }
        }

        tracing::debug!("Stopped message streaming");
        None
    }
}

struct BitmexUnifiedFeedHandler {
    handler: BitmexFeedHandler,
    tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
}

impl BitmexUnifiedFeedHandler {
    /// Creates a new [`BitmexUnifiedFeedHandler`] instance.
    pub const fn new(
        reader: MessageReader,
        signal: Arc<AtomicBool>,
        message_count: Arc<AtomicU64>,
        tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    ) -> Self {
        let handler = BitmexFeedHandler::new(reader, signal, message_count);
        Self { handler, tx }
    }

    async fn run(&mut self) {
        while let Some(msg) = self.next().await {
            if let Err(e) = self.tx.send(msg) {
                tracing::error!("Error sending message: {e}");
                break;
            }
        }
    }

    async fn next(&mut self) -> Option<NautilusWsMessage> {
        let mut quote_cache = QuoteCache::new();

        while let Some(msg) = self.handler.next().await {
            if let WsMessage::Table(table_msg) = msg {
                let ts_init = get_atomic_clock_realtime().get_time_ns();
                let price_precision = 1; // TODO: Get actual price precision from instrument

                return Some(match table_msg {
                    // Market data messages
                    TableMessage::OrderBookL2 { action, data } => {
                        let data = parse_book_msg_vec(data, action, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::OrderBookL2_25 { action, data } => {
                        let data = parse_book_msg_vec(data, action, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::OrderBook10 { data, .. } => {
                        let data = parse_book10_msg_vec(data, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::Quote { mut data, .. } => {
                        // Index symbols may return empty quote data
                        if data.is_empty() {
                            continue;
                        }
                        let msg = data.remove(0);
                        if let Some(quote) = quote_cache.process(msg, 1) {
                            NautilusWsMessage::Data(vec![Data::Quote(quote)])
                        } else {
                            continue;
                        }
                    }
                    TableMessage::Trade { data, .. } => {
                        let data = parse_trade_msg_vec(data, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::TradeBin1m { action, data } => {
                        if action == Action::Partial {
                            continue;
                        }
                        let data = parse_trade_bin_msg_vec(data, WsTopic::TradeBin1m, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::TradeBin5m { action, data } => {
                        if action == Action::Partial {
                            continue;
                        }
                        let data = parse_trade_bin_msg_vec(data, WsTopic::TradeBin5m, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::TradeBin1h { action, data } => {
                        if action == Action::Partial {
                            continue;
                        }
                        let data = parse_trade_bin_msg_vec(data, WsTopic::TradeBin1h, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    TableMessage::TradeBin1d { action, data } => {
                        if action == Action::Partial {
                            continue;
                        }
                        let data = parse_trade_bin_msg_vec(data, WsTopic::TradeBin1d, 1, ts_init);
                        NautilusWsMessage::Data(data)
                    }
                    // Execution messages
                    TableMessage::Order { data, .. } => {
                        if let Some(order_msg) = data.into_iter().next() {
                            let report = parse::parse_order_msg(order_msg, price_precision);
                            NautilusWsMessage::OrderStatusReport(Box::new(report))
                        } else {
                            continue;
                        }
                    }
                    TableMessage::Execution { data, .. } => {
                        let mut fills = Vec::new();
                        for exec_msg in data {
                            if let Some(fill) =
                                parse::parse_execution_msg(exec_msg, price_precision)
                            {
                                fills.push(fill);
                            }
                        }
                        if !fills.is_empty() {
                            NautilusWsMessage::FillReports(fills)
                        } else {
                            continue;
                        }
                    }
                    TableMessage::Position { data, .. } => {
                        if let Some(pos_msg) = data.into_iter().next() {
                            let report = parse::parse_position_msg(pos_msg);
                            NautilusWsMessage::PositionStatusReport(Box::new(report))
                        } else {
                            continue;
                        }
                    }
                    TableMessage::Wallet { .. } => {
                        continue; // TODO: Parse to account state update
                        // if let Some(wallet_msg) = data.into_iter().next() {
                        //     let (account_id, currency, amount) =
                        //         parse::parse_wallet_msg(wallet_msg);
                        //     NautilusWsMessage::WalletUpdate {
                        //         account_id,
                        //         currency,
                        //         amount,
                        //     }
                        // } else {
                        //     continue;
                        // }
                    }
                    TableMessage::Margin { .. } => {
                        continue; // TODO: Parse to account state update
                        // if let Some(margin_msg) = data.into_iter().next() {
                        //     let (account_id, currency, available_margin) =
                        //         parse::parse_margin_msg(margin_msg);
                        //     NautilusWsMessage::MarginUpdate {
                        //         account_id,
                        //         currency,
                        //         available_margin,
                        //     }
                        // } else {
                        //     continue;
                        // }
                    }
                    TableMessage::Instrument { data, .. } => {
                        let mut data_msgs = Vec::new();
                        for msg in data {
                            let parsed = parse::parse_instrument_msg(msg);
                            data_msgs.extend(parsed);
                        }
                        if !data_msgs.is_empty() {
                            NautilusWsMessage::Data(data_msgs)
                        } else {
                            continue;
                        }
                    }
                    TableMessage::Funding { data, .. } => {
                        let mut funding_updates = Vec::new();
                        for msg in data {
                            if let Some(parsed) = parse::parse_funding_msg(msg) {
                                funding_updates.push(parsed);
                            }
                        }
                        if !funding_updates.is_empty() {
                            NautilusWsMessage::FundingRateUpdates(funding_updates)
                        } else {
                            continue;
                        }
                    }
                    _ => {
                        // Other message types not yet implemented
                        tracing::warn!("Unhandled table message type: {table_msg:?}");
                        continue;
                    }
                });
            }
        }
        None
    }
}
