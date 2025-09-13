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
//! This module defines and implements a [`BitmexWebSocketClient`] for
//! connecting to BitMEX WebSocket streams. It handles authentication (when credentials
//! are provided), manages subscriptions to market data and account update channels,
//! and parses incoming messages into structured Nautilus domain objects.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::runtime::get_runtime;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, env::get_env_var, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Data, bar::BarType},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{WebSocketClient, WebSocketConfig, channel_message_handler},
};
use reqwest::header::USER_AGENT;
use tokio::{sync::RwLock, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    cache::QuoteCache,
    enums::{
        BitmexAction, BitmexWsAuthAction, BitmexWsAuthChannel, BitmexWsOperation, BitmexWsTopic,
    },
    error::BitmexWsError,
    messages::{
        BitmexAuthentication, BitmexSubscription, BitmexTableMessage, BitmexWsMessage,
        NautilusWsMessage, OrderData,
    },
    parse::{
        is_index_symbol, parse_book_msg_vec, parse_book10_msg_vec, parse_order_update_msg,
        parse_trade_bin_msg_vec, parse_trade_msg_vec, parse_wallet_msg, topic_from_bar_spec,
    },
};
use crate::{
    common::{
        consts::BITMEX_WS_URL, credential::Credential, enums::BitmexExecType,
        parse::parse_account_state,
    },
    websocket::parse::{
        parse_execution_msg, parse_funding_msg, parse_instrument_msg, parse_order_msg,
        parse_position_msg,
    },
};

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
    inner: Arc<RwLock<Option<WebSocketClient>>>,
    rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: Arc<DashMap<String, AHashSet<Ustr>>>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
    account_id: AccountId,
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
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => anyhow::bail!("Both `api_key` and `api_secret` must be provided together"),
        };

        let account_id = account_id.unwrap_or(AccountId::from("BITMEX-master"));

        Ok(Self {
            url: url.unwrap_or(BITMEX_WS_URL.to_string()),
            credential,
            heartbeat,
            inner: Arc::new(RwLock::new(None)),
            rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(AHashMap::new()),
            account_id,
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

        Self::new(Some(url), Some(api_key), Some(api_secret), None, None)
    }

    /// Returns the websocket url being used by the client.
    #[must_use]
    pub const fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the public API key being used by the client.
    #[must_use]
    pub fn api_key(&self) -> Option<&str> {
        self.credential.as_ref().map(|c| c.api_key.as_str())
    }

    /// Returns a value indicating whether the client is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        match self.inner.try_read() {
            Ok(guard) => match &*guard {
                Some(inner) => inner.is_active(),
                None => false,
            },
            Err(_) => false,
        }
    }

    /// Returns a value indicating whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self.inner.try_read() {
            Ok(guard) => match &*guard {
                Some(inner) => inner.is_closed(),
                None => true,
            },
            Err(_) => true,
        }
    }

    /// Sets the account ID.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }

    /// Initialize the instruments cache with the given `instruments`.
    pub fn initialize_instruments_cache(&mut self, instruments: Vec<InstrumentAny>) {
        let mut instruments_cache: AHashMap<Ustr, InstrumentAny> = AHashMap::new();
        for inst in instruments {
            instruments_cache.insert(inst.symbol().inner(), inst.clone());
        }

        self.instruments_cache = Arc::new(instruments_cache);
    }

    /// Connect to the BitMEX WebSocket server.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails or authentication fails (if credentials provided).
    ///
    /// # Panics
    ///
    /// Panics if subscription or authentication messages fail to serialize to JSON.
    pub async fn connect(&mut self) -> Result<(), BitmexWsError> {
        let reader = self.connect_inner().await?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.rx = Some(Arc::new(rx));
        let signal = self.signal.clone();

        let instruments_cache = self.instruments_cache.clone();
        let account_id = self.account_id;
        let inner_client = self.inner.clone();
        let credential = self.credential.clone();
        let subscriptions = self.subscriptions.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                BitmexWsMessageHandler::new(reader, signal, tx, instruments_cache, account_id);

            // Run message processing with reconnection handling
            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        log::info!("Reconnecting WebSocket");

                        let inner_guard = inner_client.read().await;
                        if let Some(inner) = &*inner_guard {
                            // Re-authenticate if we have credentials
                            if let Some(cred) = &credential {
                                let expires = (chrono::Utc::now() + chrono::Duration::seconds(30))
                                    .timestamp();
                                let signature = cred.sign("GET", "/realtime", expires, "");

                                let auth_message = BitmexAuthentication {
                                    op: BitmexWsAuthAction::AuthKeyExpires,
                                    args: (cred.api_key.to_string(), expires, signature),
                                };

                                let auth_json = match serde_json::to_string(&auth_message) {
                                    Ok(json) => json,
                                    Err(e) => {
                                        tracing::error!(error = %e, "Failed to serialize auth message");
                                        continue;
                                    }
                                };
                                if let Err(e) = inner
                                    .send_text(auth_json, None)
                                    .await
                                {
                                    log::error!(
                                        "Failed to re-authenticate after reconnection: {e}"
                                    );
                                } else {
                                    log::info!("Re-authenticated after reconnection");
                                }
                            }

                            // Always resubscribe to instruments
                            let subscribe_msg = BitmexSubscription {
                                op: BitmexWsOperation::Subscribe,
                                args: vec!["instrument".to_string()],
                            };

                            let subscribe_json = match serde_json::to_string(&subscribe_msg) {
                                Ok(json) => json,
                                Err(e) => {
                                    tracing::error!(error = %e, "Failed to serialize subscribe message");
                                    continue;
                                }
                            };
                            if let Err(e) = inner
                                .send_text(subscribe_json, None)
                                .await
                            {
                                log::error!(
                                    "Failed to subscribe to instruments after reconnection: {e}"
                                );
                            }

                            // Restore all tracked subscriptions
                            let mut topics_to_restore = Vec::new();
                            for entry in subscriptions.iter() {
                                let (channel, symbols) = entry.pair();
                                if symbols.is_empty() {
                                    topics_to_restore.push(channel.clone());
                                } else {
                                    for symbol in symbols.iter() {
                                        topics_to_restore.push(format!("{channel}:{symbol}"));
                                    }
                                }
                            }

                            if !topics_to_restore.is_empty() {
                                let message = BitmexSubscription {
                                    op: BitmexWsOperation::Subscribe,
                                    args: topics_to_restore.clone(),
                                };

                                let msg_json = match serde_json::to_string(&message) {
                                    Ok(json) => json,
                                    Err(e) => {
                                        tracing::error!(error = %e, topics = ?topics_to_restore, "Failed to serialize message");
                                        continue;
                                    }
                                };
                                if let Err(e) = inner
                                    .send_text(msg_json, None)
                                    .await
                                {
                                    log::error!(
                                        "Failed to restore subscriptions after reconnection: {e}"
                                    );
                                } else {
                                    log::info!(
                                        "Restored {} subscriptions after reconnection",
                                        topics_to_restore.len()
                                    );
                                }
                            }
                        }
                    }
                    Some(msg) => {
                        if let Err(e) = handler.tx.send(msg) {
                            tracing::error!("Error sending message: {e}");
                            break;
                        }
                    }
                    None => {
                        // Stream ended - check if it's a stop signal
                        if handler.handler.signal.load(Ordering::Relaxed) {
                            tracing::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        // Otherwise it's an unexpected stream end
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }
        });

        self.task_handle = Some(Arc::new(stream_handle));

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                let subscribe_msg = BitmexSubscription {
                    op: BitmexWsOperation::Subscribe,
                    args: vec!["instrument".to_string()],
                };

                match serde_json::to_string(&subscribe_msg) {
                    Ok(subscribe_json) => {
                        if let Err(e) = inner.send_text(subscribe_json, None).await {
                            log::error!("Failed to subscribe to instruments: {e}");
                        } else {
                            log::debug!("Subscribed to all instruments");
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize resubscribe message");
                    }
                }
            }
        }

        Ok(())
    }

    /// Connect to the WebSocket and return a message receiver.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails or if authentication fails (when credentials are provided).
    async fn connect_inner(
        &mut self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<Message>, BitmexWsError> {
        let (message_handler, rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            message_handler: Some(message_handler),
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };

        let keyed_quotas = vec![];
        let client = WebSocketClient::connect(
            config,
            None, // post_reconnection
            keyed_quotas,
            None, // default_quota
        )
        .await
        .map_err(|e| BitmexWsError::ClientError(e.to_string()))?;

        {
            let mut inner_guard = self.inner.write().await;
            *inner_guard = Some(client);
        }

        if self.credential.is_some() {
            self.authenticate().await?;
        }

        Ok(rx)
    }

    /// Authenticate the WebSocket connection using the provided credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, if authentication fails,
    /// or if credentials are not available.
    async fn authenticate(&self) -> Result<(), BitmexWsError> {
        let credential = match &self.credential {
            Some(credential) => credential,
            None => {
                return Err(BitmexWsError::AuthenticationError(
                    "API credentials not available to authenticate".to_string(),
                ));
            }
        };

        let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
        let signature = credential.sign("GET", "/realtime", expires, "");

        let auth_message = BitmexAuthentication {
            op: BitmexWsAuthAction::AuthKeyExpires,
            args: (credential.api_key.to_string(), expires, signature),
        };

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                let auth_json = serde_json::to_string(&auth_message).map_err(|e| {
                    BitmexWsError::AuthenticationError(format!(
                        "Failed to serialize auth message: {e}"
                    ))
                })?;
                inner
                    .send_text(auth_json, None)
                    .await
                    .map_err(|e| BitmexWsError::AuthenticationError(e.to_string()))
            } else {
                log::error!("Cannot authenticate: not connected");
                Ok(())
            }
        }
    }

    /// Wait until the WebSocket connection is active.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection times out.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), BitmexWsError> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            BitmexWsError::ClientError(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
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
        log::debug!("Starting close process");

        self.signal.store(true, Ordering::Relaxed);

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                log::debug!("Disconnecting websocket");

                match tokio::time::timeout(Duration::from_secs(3), inner.disconnect()).await {
                    Ok(()) => log::debug!("Websocket disconnected successfully"),
                    Err(_) => {
                        log::warn!(
                            "Timeout waiting for websocket disconnect, continuing with cleanup"
                        );
                    }
                }
            } else {
                log::debug!("No active connection to disconnect");
            }
        }

        // Clean up task handle with timeout
        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    log::debug!("Waiting for task handle to complete");
                    match tokio::time::timeout(Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => log::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!(
                                "Timeout waiting for task handle, task may still be running"
                            );
                            // The task will be dropped and should clean up automatically
                        }
                    }
                }
                Err(arc_handle) => {
                    log::debug!(
                        "Cannot take ownership of task handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            log::debug!("No task handle to await");
        }

        log::debug!("Closed");

        Ok(())
    }

    /// Subscribe to the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the subscription message fails.
    ///
    /// # Panics
    ///
    /// Panics if serialization of WebSocket messages fails (should never happen).
    pub async fn subscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        log::debug!("Subscribing to topics: {topics:?}");

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

        let message = BitmexSubscription {
            op: BitmexWsOperation::Subscribe,
            args: topics.clone(),
        };

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                let msg_json = serde_json::to_string(&message).map_err(|e| {
                    BitmexWsError::SubscriptionError(format!(
                        "Failed to serialize subscribe message: {e}"
                    ))
                })?;
                inner
                    .send_text(msg_json, None)
                    .await
                    .map_err(|e| BitmexWsError::SubscriptionError(e.to_string()))?;
            } else {
                log::error!("Cannot send message: not connected");
            }
        }

        Ok(())
    }

    /// Unsubscribe from the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the unsubscription message fails.
    async fn unsubscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        log::debug!("Attempting to unsubscribe from topics: {topics:?}");

        if self.signal.load(Ordering::Relaxed) {
            log::debug!("Shutdown signal detected, skipping unsubscribe");
            return Ok(());
        }

        for topic in &topics {
            if let Some((channel, symbol)) = topic.split_once(':') {
                if let Some(mut entry) = self.subscriptions.get_mut(channel) {
                    entry.remove(&Ustr::from(symbol));
                    if entry.is_empty() {
                        drop(entry);
                        self.subscriptions.remove(channel);
                    }
                }
            } else {
                self.subscriptions.remove(topic);
            }
        }

        let message = BitmexSubscription {
            op: BitmexWsOperation::Unsubscribe,
            args: topics.clone(),
        };

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                let msg_json = match serde_json::to_string(&message) {
                    Ok(json) => json,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize unsubscribe message");
                        return Ok(());
                    }
                };
                if let Err(e) = inner.send_text(msg_json, None).await {
                    log::debug!("Error sending unsubscribe message: {e}");
                }
            } else {
                log::debug!("Cannot send unsubscribe message: not connected");
            }
        }

        Ok(())
    }

    /// Get the current number of active subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Get active subscriptions for a specific instrument.
    #[must_use]
    pub fn get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<String> {
        let symbol = instrument_id.symbol.inner();
        let mut channels = Vec::with_capacity(self.subscriptions.len());

        for entry in self.subscriptions.iter() {
            let (channel, symbols) = entry.pair();
            if symbols.contains(&symbol) {
                // Return the full topic string (e.g., "orderBookL2:XBTUSD")
                channels.push(format!("{channel}:{symbol}"));
            } else if symbols.is_empty() && (channel == "execution" || channel == "order") {
                // These are account-level subscriptions without symbols
                channels.push(channel.clone());
            }
        }

        channels
    }

    /// Subscribe to instrument updates for all instruments on the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_instruments(&self) -> Result<(), BitmexWsError> {
        // Already subscribed automatically on connection
        log::debug!("Already subscribed to all instruments on connection, skipping");
        Ok(())
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
        // Already subscribed to all instruments on connection
        log::debug!(
            "Already subscribed to all instruments on connection (includes {instrument_id}), skipping"
        );
        Ok(())
    }

    /// Subscribe to order book updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBookL2;
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
        let topic = BitmexWsTopic::OrderBookL2_25;
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
        let topic = BitmexWsTopic::OrderBook10;
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
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have quotes (bid/ask), only a single price
        if is_index_symbol(&instrument_id.symbol.inner()) {
            tracing::warn!("Ignoring quote subscription for index symbol: {symbol}");
            return Ok(());
        }

        let topic = BitmexWsTopic::Quote;
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
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have trades
        if is_index_symbol(&symbol) {
            tracing::warn!("Ignoring trade subscription for index symbol: {symbol}");
            return Ok(());
        }

        let topic = BitmexWsTopic::Trade;
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
        self.subscribe_instrument(instrument_id).await
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
        self.subscribe_instrument(instrument_id).await
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
        let topic = BitmexWsTopic::Funding;
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
        // No-op: instruments are required for proper operation
        log::debug!(
            "Instruments subscription maintained for proper operation, skipping unsubscribe"
        );
        Ok(())
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
        // No-op: instruments are required for proper operation
        log::debug!(
            "Instruments subscription maintained for proper operation (includes {instrument_id}), skipping unsubscribe"
        );
        Ok(())
    }

    /// Unsubscribe from order book updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBookL2;
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
        let topic = BitmexWsTopic::OrderBookL2_25;
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
        let topic = BitmexWsTopic::OrderBook10;
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
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have quotes
        if is_index_symbol(&symbol) {
            return Ok(());
        }

        let topic = BitmexWsTopic::Quote;
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
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have trades
        if is_index_symbol(&symbol) {
            return Ok(());
        }

        let topic = BitmexWsTopic::Trade;
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
        // No-op: instrument channel shared with index prices
        log::debug!(
            "Mark prices for {instrument_id} uses shared instrument channel, skipping unsubscribe"
        );
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
        // No-op: instrument channel shared with mark prices
        log::debug!(
            "Index prices for {instrument_id} uses shared instrument channel, skipping unsubscribe"
        );
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
        // No-op: unsubscribing during shutdown causes race conditions
        log::debug!(
            "Funding rates for {instrument_id}, skipping unsubscribe to avoid shutdown race"
        );
        Ok(())
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

    /// Subscribe to order updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_orders(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Order.to_string()])
            .await
    }

    /// Subscribe to execution updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_executions(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Execution.to_string()])
            .await
    }

    /// Subscribe to position updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_positions(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Position.to_string()])
            .await
    }

    /// Subscribe to margin updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_margin(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Margin.to_string()])
            .await
    }

    /// Subscribe to wallet updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_wallet(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Wallet.to_string()])
            .await
    }

    /// Unsubscribe from order updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_orders(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Order.to_string()])
            .await
    }

    /// Unsubscribe from execution updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_executions(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Execution.to_string()])
            .await
    }

    /// Unsubscribe from position updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_positions(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Position.to_string()])
            .await
    }

    /// Unsubscribe from margin updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_margin(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Margin.to_string()])
            .await
    }

    /// Unsubscribe from wallet updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_wallet(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Wallet.to_string()])
            .await
    }
}

struct BitmexFeedHandler {
    receiver: tokio::sync::mpsc::UnboundedReceiver<Message>,
    signal: Arc<AtomicBool>,
}

impl BitmexFeedHandler {
    /// Creates a new [`BitmexFeedHandler`] instance.
    pub fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<Message>,
        signal: Arc<AtomicBool>,
    ) -> Self {
        Self { receiver, signal }
    }

    /// Get the next message from the WebSocket stream.
    async fn next(&mut self) -> Option<BitmexWsMessage> {
        loop {
            tokio::select! {
                msg = self.receiver.recv() => match msg {
                    Some(msg) => match msg {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                tracing::info!("Received WebSocket reconnection signal");
                                return Some(BitmexWsMessage::Reconnected);
                            }

                            tracing::trace!("Raw websocket message: {text}");

                            match serde_json::from_str(&text) {
                                Ok(msg) => match &msg {
                                    BitmexWsMessage::Welcome {
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
                                    BitmexWsMessage::Subscription {
                                        success: _,
                                        subscribe: _,
                                        error,
                                    } => {
                                        if let Some(error) = error {
                                            tracing::error!("Subscription error: {error}");
                                        }
                                    }
                                    BitmexWsMessage::Error { status, error, .. } => {
                                        tracing::error!(
                                            status = status,
                                            error = error,
                                            "Received error from BitMEX"
                                        );
                                    }
                                    _ => return Some(msg),
                                },
                                Err(e) => {
                                    tracing::error!("Failed to parse WebSocket message: {e}: {text}");
                                }
                            }
                        }
                        Message::Binary(msg) => {
                            tracing::debug!("Raw binary: {msg:?}");
                        }
                        Message::Close(_) => {
                            tracing::debug!("Received close message");
                            return None;
                        }
                        msg => match msg {
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
                    }
                    None => {
                        tracing::info!("WebSocket stream closed");
                        return None;
                    }
                },
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }
}

struct BitmexWsMessageHandler {
    handler: BitmexFeedHandler,
    tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
    #[allow(dead_code)] // May be needed for future account-specific processing
    account_id: AccountId,
}

impl BitmexWsMessageHandler {
    /// Creates a new [`BitmexWsMessageHandler`] instance.
    pub fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<Message>,
        signal: Arc<AtomicBool>,
        tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
        account_id: AccountId,
    ) -> Self {
        let handler = BitmexFeedHandler::new(receiver, signal);
        Self {
            handler,
            tx,
            instruments_cache,
            account_id,
        }
    }

    // Run is now handled inline in the connect() method where we have access to reconnection resources

    /// Get price precision for a symbol from the instruments cache.
    ///
    /// # Panics
    ///
    /// Panics if the instrument is not found in the cache.
    #[inline]
    fn get_price_precision(&self, symbol: &Ustr) -> u8 {
        self.instruments_cache
            .get(symbol).map_or_else(|| panic!("Instrument '{symbol}' not found in cache; ensure all instruments are loaded before starting websocket"), Instrument::price_precision)
    }

    async fn next(&mut self) -> Option<NautilusWsMessage> {
        let clock = get_atomic_clock_realtime();
        let mut quote_cache = QuoteCache::new();

        while let Some(msg) = self.handler.next().await {
            match msg {
                BitmexWsMessage::Reconnected => {
                    // Return reconnection signal to outer loop
                    return Some(NautilusWsMessage::Reconnected);
                }
                BitmexWsMessage::Table(table_msg) => {
                    let ts_init = clock.get_time_ns();

                    let msg = match table_msg {
                        BitmexTableMessage::OrderBookL2 { action, data } => {
                            if data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_book_msg_vec(data, action, price_precision, ts_init);

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::OrderBookL2_25 { action, data } => {
                            if data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_book_msg_vec(data, action, price_precision, ts_init);

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::OrderBook10 { data, .. } => {
                            if data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_book10_msg_vec(data, price_precision, ts_init);

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::Quote { mut data, .. } => {
                            // Index symbols may return empty quote data
                            if data.is_empty() {
                                continue;
                            }

                            let msg = data.remove(0);
                            let price_precision = self.get_price_precision(&msg.symbol);

                            if let Some(quote) = quote_cache.process(&msg, price_precision, ts_init)
                            {
                                NautilusWsMessage::Data(vec![Data::Quote(quote)])
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Trade { data, .. } => {
                            if data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_trade_msg_vec(data, price_precision, ts_init);

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin1m { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin1m,
                                price_precision,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin5m { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin5m,
                                price_precision,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin1h { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin1h,
                                price_precision,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin1d { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let price_precision = self.get_price_precision(&data[0].symbol);
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin1d,
                                price_precision,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        // Execution messages
                        // Note: BitMEX may send duplicate order status updates for the same order
                        // (e.g., immediate response + stream update). This is expected behavior.
                        BitmexTableMessage::Order { data, .. } => {
                            // Process all orders in the message
                            let mut reports = Vec::with_capacity(data.len());

                            for order_data in data {
                                match order_data {
                                    OrderData::Full(order_msg) => {
                                        let price_precision =
                                            self.get_price_precision(&order_msg.symbol);
                                        match parse_order_msg(&order_msg, price_precision) {
                                            Ok(report) => reports.push(report),
                                            Err(e) => {
                                                tracing::error!(
                                                    error = %e,
                                                    symbol = %order_msg.symbol,
                                                    order_id = %order_msg.order_id,
                                                    time_in_force = ?order_msg.time_in_force,
                                                    "Failed to parse full order message - potential data loss"
                                                );
                                                // TODO: Add metric counter for parse failures
                                                continue;
                                            }
                                        }
                                    }
                                    OrderData::Update(msg) => {
                                        let price_precision = self.get_price_precision(&msg.symbol);

                                        if let Some(event) = parse_order_update_msg(
                                            &msg,
                                            price_precision,
                                            self.account_id,
                                        ) {
                                            return Some(NautilusWsMessage::OrderUpdated(event));
                                        } else {
                                            tracing::warn!(
                                                order_id = %msg.order_id,
                                                price = ?msg.price,
                                                "Skipped order update message (insufficient data)"
                                            );
                                        }
                                    }
                                }
                            }

                            if reports.is_empty() {
                                continue;
                            }

                            NautilusWsMessage::OrderStatusReports(reports)
                        }
                        BitmexTableMessage::Execution { data, .. } => {
                            let mut fills = Vec::with_capacity(data.len());

                            for exec_msg in data {
                                // Skip if symbol is missing (shouldn't happen for valid trades)
                                let Some(symbol) = &exec_msg.symbol else {
                                    // Only log as warn for Trade executions, debug for others to reduce noise
                                    if exec_msg.exec_type == Some(BitmexExecType::Trade) {
                                        tracing::warn!(
                                            "Execution message missing symbol: {:?}",
                                            exec_msg.exec_id
                                        );
                                    } else {
                                        tracing::debug!(
                                            "Execution message missing symbol: {:?} (exec_type: {:?})",
                                            exec_msg.exec_id,
                                            exec_msg.exec_type
                                        );
                                    }
                                    continue;
                                };
                                let price_precision = self.get_price_precision(symbol);

                                if let Some(fill) = parse_execution_msg(exec_msg, price_precision) {
                                    fills.push(fill);
                                }
                            }

                            if fills.is_empty() {
                                continue;
                            }
                            NautilusWsMessage::FillReports(fills)
                        }
                        BitmexTableMessage::Position { data, .. } => {
                            if let Some(pos_msg) = data.into_iter().next() {
                                let report = parse_position_msg(pos_msg);
                                NautilusWsMessage::PositionStatusReport(report)
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Wallet { data, .. } => {
                            if let Some(wallet_msg) = data.into_iter().next() {
                                let account_state = parse_wallet_msg(wallet_msg, ts_init);
                                NautilusWsMessage::AccountState(account_state)
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Margin { data, .. } => {
                            if let Some(margin_msg) = data.into_iter().next() {
                                tracing::debug!(
                                    "Processing margin message: wallet={:?}, available={:?}, init={:?}, maint={:?}",
                                    margin_msg.wallet_balance,
                                    margin_msg.available_margin,
                                    margin_msg.init_margin,
                                    margin_msg.maint_margin
                                );
                                // TODO: Catching panics for now for initial development
                                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    parse_account_state(&margin_msg, self.account_id, ts_init)
                                })) {
                                    Ok(Ok(account_state)) => {
                                        tracing::debug!("Successfully parsed margin message");
                                        NautilusWsMessage::AccountState(account_state)
                                    }
                                    Ok(Err(e)) => {
                                        tracing::debug!("Failed to parse margin message: {e:?}");
                                        continue;
                                    }
                                    Err(panic_info) => {
                                        tracing::error!(
                                            "PANIC while parsing margin message: {:?}",
                                            panic_info
                                        );
                                        continue;
                                    }
                                }
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Instrument { data, .. } => {
                            let ts_init = clock.get_time_ns();
                            let mut data_msgs = Vec::with_capacity(data.len());

                            for msg in data {
                                let parsed =
                                    parse_instrument_msg(msg, &self.instruments_cache, ts_init);
                                data_msgs.extend(parsed);
                            }

                            if data_msgs.is_empty() {
                                continue;
                            }
                            NautilusWsMessage::Data(data_msgs)
                        }
                        BitmexTableMessage::Funding { data, .. } => {
                            let ts_init = clock.get_time_ns();
                            let mut funding_updates = Vec::with_capacity(data.len());

                            for msg in data {
                                if let Some(parsed) = parse_funding_msg(msg, ts_init) {
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
                    };

                    return Some(msg);
                }
                _ => {
                    // Other BitmexWsMessage types (Welcome, Subscription, Error) are
                    // already handled in BitmexFeedHandler::next()
                    continue;
                }
            }
        }

        None
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_reconnect_topics_restoration_logic() {
        // Create real client with credentials
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Populate subscriptions like they would be during normal operation
        client.subscriptions.insert("trade".to_string(), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set.insert(Ustr::from("ETHUSD"));
            set
        });

        client.subscriptions.insert("orderBookL2".to_string(), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set
        });

        // Private channels (no symbols)
        client
            .subscriptions
            .insert("order".to_string(), AHashSet::new());
        client
            .subscriptions
            .insert("position".to_string(), AHashSet::new());

        // Test the actual reconnection topic building logic from lines 258-268
        let mut topics_to_restore = Vec::new();
        for entry in client.subscriptions.iter() {
            let (channel, symbols) = entry.pair();
            if symbols.is_empty() {
                topics_to_restore.push(channel.clone());
            } else {
                for symbol in symbols.iter() {
                    topics_to_restore.push(format!("{channel}:{symbol}"));
                }
            }
        }

        // Verify it builds the correct restoration topics
        assert!(topics_to_restore.contains(&"trade:XBTUSD".to_string()));
        assert!(topics_to_restore.contains(&"trade:ETHUSD".to_string()));
        assert!(topics_to_restore.contains(&"orderBookL2:XBTUSD".to_string()));
        assert!(topics_to_restore.contains(&"order".to_string()));
        assert!(topics_to_restore.contains(&"position".to_string()));
        assert_eq!(topics_to_restore.len(), 5);
    }

    #[rstest]
    fn test_reconnect_auth_message_building() {
        // Test with credentials
        let client_with_creds = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Test the actual auth message building logic from lines 220-228
        if let Some(cred) = &client_with_creds.credential {
            let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
            let signature = cred.sign("GET", "/realtime", expires, "");

            let auth_message = BitmexAuthentication {
                op: BitmexWsAuthAction::AuthKeyExpires,
                args: (cred.api_key.to_string(), expires, signature),
            };

            // Verify auth message structure
            assert_eq!(auth_message.op, BitmexWsAuthAction::AuthKeyExpires);
            assert_eq!(auth_message.args.0, "test_key");
            assert!(auth_message.args.1 > 0); // expires should be positive
            assert!(!auth_message.args.2.is_empty()); // signature should exist
        } else {
            panic!("Client should have credentials");
        }

        // Test without credentials
        let client_no_creds = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            None,
            None,
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        assert!(client_no_creds.credential.is_none());
    }

    #[rstest]
    fn test_subscription_state_after_unsubscribe() {
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Set up initial subscriptions
        client.subscriptions.insert("trade".to_string(), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set.insert(Ustr::from("ETHUSD"));
            set
        });

        client.subscriptions.insert("orderBookL2".to_string(), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set
        });

        // Simulate unsubscribe logic (like from unsubscribe() method lines 586-599)
        let topic = "trade:ETHUSD";
        if let Some((channel, symbol)) = topic.split_once(':')
            && let Some(mut entry) = client.subscriptions.get_mut(channel)
        {
            entry.remove(&Ustr::from(symbol));
            if entry.is_empty() {
                drop(entry);
                client.subscriptions.remove(channel);
            }
        }

        // Build restoration topics after unsubscribe
        let mut topics_to_restore = Vec::new();
        for entry in client.subscriptions.iter() {
            let (channel, symbols) = entry.pair();
            if symbols.is_empty() {
                topics_to_restore.push(channel.clone());
            } else {
                for symbol in symbols.iter() {
                    topics_to_restore.push(format!("{channel}:{symbol}"));
                }
            }
        }

        // Should have XBTUSD trade but not ETHUSD trade
        assert!(topics_to_restore.contains(&"trade:XBTUSD".to_string()));
        assert!(!topics_to_restore.contains(&"trade:ETHUSD".to_string()));
        assert!(topics_to_restore.contains(&"orderBookL2:XBTUSD".to_string()));
        assert_eq!(topics_to_restore.len(), 2);
    }
}
