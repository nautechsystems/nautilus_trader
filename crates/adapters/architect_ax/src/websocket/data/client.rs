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

//! Market data WebSocket client for Ax.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    websocket::{
        PingHandler, SubscriptionState, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use ustr::Ustr;

use super::handler::{FeedHandler, HandlerCommand};
use crate::{
    common::enums::{AxCandleWidth, AxMarketDataLevel},
    websocket::messages::NautilusDataWsMessage,
};

/// Default heartbeat interval in seconds.
const DEFAULT_HEARTBEAT_SECS: u64 = 30;

/// Subscription topic delimiter for Ax.
const AX_TOPIC_DELIMITER: char = ':';

/// Result type for Ax WebSocket operations.
pub type AxWsResult<T> = Result<T, AxWsClientError>;

/// Error type for the Ax WebSocket client.
#[derive(Debug, Clone)]
pub enum AxWsClientError {
    /// Transport/connection error.
    Transport(String),
    /// Channel send error.
    ChannelError(String),
}

impl core::fmt::Display for AxWsClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "Transport error: {msg}"),
            Self::ChannelError(msg) => write!(f, "Channel error: {msg}"),
        }
    }
}

impl std::error::Error for AxWsClientError {}

#[derive(Debug, Default, Clone)]
pub(crate) struct SymbolDataTypes {
    pub(crate) quotes: bool,
    pub(crate) trades: bool,
    pub(crate) book_level: Option<AxMarketDataLevel>,
}

impl SymbolDataTypes {
    pub(crate) fn effective_level(&self) -> Option<AxMarketDataLevel> {
        if let Some(level) = self.book_level {
            return Some(level);
        }

        if self.quotes || self.trades {
            return Some(AxMarketDataLevel::Level1);
        }
        None
    }

    fn is_empty(&self) -> bool {
        !self.quotes && !self.trades && self.book_level.is_none()
    }
}

/// Market data WebSocket client for Ax.
///
/// Provides streaming market data including tickers, trades, order books, and candles.
/// Requires Bearer token authentication obtained via the HTTP `/api/authenticate` endpoint.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.architect",
        from_py_object
    )
)]
pub struct AxMdWebSocketClient {
    url: String,
    heartbeat: Option<u64>,
    auth_token: Option<String>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusDataWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    subscriptions: SubscriptionState,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    request_id_counter: Arc<AtomicI64>,
    subscribe_lock: Arc<tokio::sync::Mutex<()>>,
    symbol_data_types: Arc<DashMap<String, SymbolDataTypes>>,
}

impl Debug for AxMdWebSocketClient {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct(stringify!(AxMdWebSocketClient))
            .field("url", &self.url)
            .field("heartbeat", &self.heartbeat)
            .field("confirmed_subscriptions", &self.subscriptions.len())
            .finish()
    }
}

impl Clone for AxMdWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            heartbeat: self.heartbeat,
            auth_token: self.auth_token.clone(),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            signal: Arc::clone(&self.signal),
            task_handle: None,
            subscriptions: self.subscriptions.clone(),
            subscribe_lock: Arc::clone(&self.subscribe_lock),
            instruments_cache: Arc::clone(&self.instruments_cache),
            request_id_counter: Arc::clone(&self.request_id_counter),
            symbol_data_types: Arc::clone(&self.symbol_data_types),
        }
    }
}

impl AxMdWebSocketClient {
    /// Creates a new Ax market data WebSocket client.
    ///
    /// The `auth_token` is a Bearer token obtained from the HTTP `/api/authenticate` endpoint.
    #[must_use]
    pub fn new(url: String, auth_token: String, heartbeat: Option<u64>) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            auth_token: Some(auth_token),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(AX_TOPIC_DELIMITER),
            instruments_cache: Arc::new(DashMap::new()),
            request_id_counter: Arc::new(AtomicI64::new(1)),
            subscribe_lock: Arc::new(tokio::sync::Mutex::new(())),
            symbol_data_types: Arc::new(DashMap::new()),
        }
    }

    /// Creates a new Ax market data WebSocket client without authentication.
    ///
    /// Use [`set_auth_token`](Self::set_auth_token) to set the token before connecting.
    #[must_use]
    pub fn without_auth(url: String, heartbeat: Option<u64>) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            auth_token: None,
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(AX_TOPIC_DELIMITER),
            instruments_cache: Arc::new(DashMap::new()),
            request_id_counter: Arc::new(AtomicI64::new(1)),
            subscribe_lock: Arc::new(tokio::sync::Mutex::new(())),
            symbol_data_types: Arc::new(DashMap::new()),
        }
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Sets the authentication token for subsequent connections.
    ///
    /// This should be called before `connect()` if authentication is required.
    pub fn set_auth_token(&mut self, token: String) {
        self.auth_token = Some(token);
    }

    /// Returns whether the client is currently connected and active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Acquire)
    }

    /// Returns whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Acquire)
    }

    /// Returns the number of confirmed subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    fn next_request_id(&self) -> i64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    fn is_subscribed_topic(&self, topic: &str) -> bool {
        let (channel, symbol) = topic
            .split_once(AX_TOPIC_DELIMITER)
            .map_or((topic, None), |(c, s)| (c, Some(s)));
        let channel_ustr = Ustr::from(channel);
        let symbol_ustr = symbol.map_or_else(|| Ustr::from(""), Ustr::from);
        self.subscriptions
            .is_subscribed(&channel_ustr, &symbol_ustr)
    }

    /// Caches an instrument for use during message parsing.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.symbol().inner();
        self.instruments_cache.insert(symbol, instrument.clone());

        if self.is_active() {
            let cmd = HandlerCommand::UpdateInstrument(Box::new(instrument));
            let cmd_tx = self.cmd_tx.clone();
            get_runtime().spawn(async move {
                let guard = cmd_tx.read().await;
                let _ = guard.send(cmd);
            });
        }
    }

    /// Returns a cached instrument by symbol.
    #[must_use]
    pub fn get_cached_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).map(|r| r.clone())
    }

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> AxWsResult<()> {
        const MAX_RETRIES: u32 = 5;
        const CONNECTION_TIMEOUT_SECS: u64 = 10;

        self.signal.store(false, Ordering::Release);

        let (raw_handler, raw_rx) = channel_message_handler();

        // No-op: ping responses are handled internally by the WebSocketClient
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {});

        let mut headers = vec![("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string())];

        if let Some(ref token) = self.auth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers,
            heartbeat: self.heartbeat,
            heartbeat_msg: None, // Ax server sends heartbeats
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
        };

        // Retry initial connection with exponential backoff
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(500),
            Duration::from_millis(5000),
            2.0,
            250,
            false,
        )
        .map_err(|e| AxWsClientError::Transport(e.to_string()))?;

        let mut last_error: String;
        let mut attempt = 0;

        let client = loop {
            attempt += 1;

            match tokio::time::timeout(
                Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                WebSocketClient::connect(
                    config.clone(),
                    Some(raw_handler.clone()),
                    Some(ping_handler.clone()),
                    None,
                    vec![],
                    None,
                ),
            )
            .await
            {
                Ok(Ok(client)) => {
                    if attempt > 1 {
                        log::info!("WebSocket connection established after {attempt} attempts");
                    }
                    break client;
                }
                Ok(Err(e)) => {
                    last_error = e.to_string();
                    log::warn!(
                        "WebSocket connection attempt failed: attempt={attempt}/{MAX_RETRIES}, url={}, error={last_error}",
                        self.url
                    );
                }
                Err(_) => {
                    last_error = format!("Connection timeout after {CONNECTION_TIMEOUT_SECS}s");
                    log::warn!(
                        "WebSocket connection attempt timed out: attempt={attempt}/{MAX_RETRIES}, url={}",
                        self.url
                    );
                }
            }

            if attempt >= MAX_RETRIES {
                return Err(AxWsClientError::Transport(format!(
                    "Failed to connect to {} after {MAX_RETRIES} attempts: {}",
                    self.url,
                    if last_error.is_empty() {
                        "unknown error"
                    } else {
                        &last_error
                    }
                )));
            }

            let delay = backoff.next_duration();
            log::debug!(
                "Retrying in {delay:?} (attempt {}/{MAX_RETRIES})",
                attempt + 1
            );
            tokio::time::sleep(delay).await;
        };

        self.connection_mode.store(client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusDataWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        self.send_cmd(HandlerCommand::SetClient(client)).await?;

        if !self.instruments_cache.is_empty() {
            let cached_instruments: Vec<InstrumentAny> = self
                .instruments_cache
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            self.send_cmd(HandlerCommand::InitializeInstruments(cached_instruments))
                .await?;
        }

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let symbol_data_types = Arc::clone(&self.symbol_data_types);

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                out_tx.clone(),
                subscriptions.clone(),
                symbol_data_types,
            );

            while let Some(msg) = handler.next().await {
                if matches!(msg, NautilusDataWsMessage::Reconnected) {
                    log::info!("WebSocket reconnected, subscriptions will be replayed");
                }

                if out_tx.send(msg).is_err() {
                    log::debug!("Output channel closed");
                    break;
                }
            }

            log::debug!("Handler loop exited");
        });

        self.task_handle = Some(stream_handle);

        Ok(())
    }

    /// Subscribes to order book deltas for a symbol.
    ///
    /// Uses reference counting so the underlying AX subscription is only
    /// removed when all data types have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_book_deltas(
        &self,
        symbol: &str,
        level: AxMarketDataLevel,
    ) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let entry = self
            .symbol_data_types
            .entry(symbol.to_string())
            .or_default();

        // AX allows only one subscription per symbol, skip if book already subscribed
        if entry.book_level.is_some() {
            log::debug!("Book deltas already subscribed for {symbol}, skipping");
            return Ok(());
        }

        let old_level = entry.effective_level();
        let mut next = entry.clone();
        next.book_level = Some(level);
        let new_level = next.effective_level();
        drop(entry);

        self.update_data_subscription(symbol, old_level, new_level)
            .await?;

        self.symbol_data_types
            .entry(symbol.to_string())
            .or_default()
            .book_level = Some(level);

        Ok(())
    }

    /// Subscribes to quote data for a symbol.
    ///
    /// Uses reference counting so the underlying AX subscription is only
    /// removed when all data types have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_quotes(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let entry = self
            .symbol_data_types
            .entry(symbol.to_string())
            .or_default();
        let old_level = entry.effective_level();
        let mut next = entry.clone();
        next.quotes = true;
        let new_level = next.effective_level();
        drop(entry);

        self.update_data_subscription(symbol, old_level, new_level)
            .await?;

        self.symbol_data_types
            .entry(symbol.to_string())
            .or_default()
            .quotes = true;

        Ok(())
    }

    /// Subscribes to trade data for a symbol.
    ///
    /// Uses reference counting so the underlying AX subscription is only
    /// removed when all data types have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_trades(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let entry = self
            .symbol_data_types
            .entry(symbol.to_string())
            .or_default();
        let old_level = entry.effective_level();
        let mut next = entry.clone();
        next.trades = true;
        let new_level = next.effective_level();
        drop(entry);

        self.update_data_subscription(symbol, old_level, new_level)
            .await?;

        self.symbol_data_types
            .entry(symbol.to_string())
            .or_default()
            .trades = true;

        Ok(())
    }

    /// Unsubscribes from order book deltas for a symbol.
    ///
    /// The underlying AX subscription is only removed when all data types
    /// (quotes, trades, book) have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_book_deltas(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let Some(entry) = self.symbol_data_types.get(symbol) else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe book deltas");
            return Ok(());
        };
        let old_level = entry.effective_level();
        let mut next = entry.clone();
        next.book_level = None;
        let new_level = next.effective_level();
        drop(entry);

        self.update_data_subscription(symbol, old_level, new_level)
            .await?;

        if let Some(mut entry) = self.symbol_data_types.get_mut(symbol) {
            entry.book_level = None;
            if entry.is_empty() {
                drop(entry);
                self.symbol_data_types.remove(symbol);
            }
        }

        Ok(())
    }

    /// Unsubscribes from quote data for a symbol.
    ///
    /// The underlying AX subscription is only removed when all data types
    /// (quotes, trades, book) have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_quotes(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let Some(entry) = self.symbol_data_types.get(symbol) else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe quotes");
            return Ok(());
        };
        let old_level = entry.effective_level();
        let mut next = entry.clone();
        next.quotes = false;
        let new_level = next.effective_level();
        drop(entry);

        self.update_data_subscription(symbol, old_level, new_level)
            .await?;

        if let Some(mut entry) = self.symbol_data_types.get_mut(symbol) {
            entry.quotes = false;
            if entry.is_empty() {
                drop(entry);
                self.symbol_data_types.remove(symbol);
            }
        }

        Ok(())
    }

    /// Unsubscribes from trade data for a symbol.
    ///
    /// The underlying AX subscription is only removed when all data types
    /// (quotes, trades, book) have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_trades(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let Some(entry) = self.symbol_data_types.get(symbol) else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe trades");
            return Ok(());
        };
        let old_level = entry.effective_level();
        let mut next = entry.clone();
        next.trades = false;
        let new_level = next.effective_level();
        drop(entry);

        self.update_data_subscription(symbol, old_level, new_level)
            .await?;

        if let Some(mut entry) = self.symbol_data_types.get_mut(symbol) {
            entry.trades = false;
            if entry.is_empty() {
                drop(entry);
                self.symbol_data_types.remove(symbol);
            }
        }

        Ok(())
    }

    async fn update_data_subscription(
        &self,
        symbol: &str,
        old_level: Option<AxMarketDataLevel>,
        new_level: Option<AxMarketDataLevel>,
    ) -> AxWsResult<()> {
        if old_level == new_level {
            return Ok(());
        }

        match (old_level, new_level) {
            (None, Some(level)) => {
                log::debug!("Subscribing {symbol} at {level:?}");
                self.send_subscribe(symbol, level).await
            }
            (Some(_), None) => {
                log::debug!("Unsubscribing {symbol} (no remaining data types)");
                self.send_unsubscribe(symbol).await
            }
            (Some(old), Some(new)) => {
                log::debug!("Resubscribing {symbol}: {old:?} -> {new:?}");
                self.send_unsubscribe(symbol).await?;
                if let Err(e) = self.send_subscribe(symbol, new).await {
                    log::warn!("Resubscribe failed for {symbol} at {new:?}: {e}");
                    if let Err(restore_err) = self.send_subscribe(symbol, old).await {
                        // Channel dead, mark old topic for reconnection replay
                        log::error!(
                            "Failed to restore {symbol} at {old:?}: {restore_err}, \
                             reconnection required"
                        );
                        let old_topic = format!("{symbol}:{old:?}");
                        self.subscriptions.mark_subscribe(&old_topic);
                    }
                    return Err(e);
                }
                Ok(())
            }
            (None, None) => Ok(()),
        }
    }

    async fn send_subscribe(&self, symbol: &str, level: AxMarketDataLevel) -> AxWsResult<()> {
        let topic = format!("{symbol}:{level:?}");
        let request_id = self.next_request_id();

        self.subscriptions.mark_subscribe(&topic);

        if let Err(e) = self
            .send_cmd(HandlerCommand::Subscribe {
                request_id,
                symbol: Ustr::from(symbol),
                level,
            })
            .await
        {
            self.subscriptions.mark_unsubscribe(&topic);
            return Err(e);
        }

        Ok(())
    }

    async fn send_unsubscribe(&self, symbol: &str) -> AxWsResult<()> {
        let request_id = self.next_request_id();

        self.send_cmd(HandlerCommand::Unsubscribe {
            request_id,
            symbol: Ustr::from(symbol),
        })
        .await?;

        for level in [
            AxMarketDataLevel::Level1,
            AxMarketDataLevel::Level2,
            AxMarketDataLevel::Level3,
        ] {
            let topic = format!("{symbol}:{level:?}");
            self.subscriptions.mark_unsubscribe(&topic);
        }

        Ok(())
    }

    /// Subscribes to candle data for a symbol.
    ///
    /// Skips sending if already subscribed or subscription is pending.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_candles(&self, symbol: &str, width: AxCandleWidth) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;
        let topic = format!("candles:{symbol}:{width:?}");

        // Skip if already subscribed or pending
        if self.is_subscribed_topic(&topic) {
            log::debug!("Already subscribed to {topic}, skipping");
            return Ok(());
        }

        let request_id = self.next_request_id();

        // Mark pending BEFORE sending to prevent race conditions with concurrent subscribes
        self.subscriptions.mark_subscribe(&topic);

        if let Err(e) = self
            .send_cmd(HandlerCommand::SubscribeCandles {
                request_id,
                symbol: Ustr::from(symbol),
                width,
            })
            .await
        {
            // Rollback pending state on send failure
            self.subscriptions.mark_unsubscribe(&topic);
            return Err(e);
        }

        Ok(())
    }

    /// Unsubscribes from candle data for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_candles(&self, symbol: &str, width: AxCandleWidth) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;
        let request_id = self.next_request_id();
        let topic = format!("candles:{symbol}:{width:?}");

        self.subscriptions.mark_unsubscribe(&topic);

        self.send_cmd(HandlerCommand::UnsubscribeCandles {
            request_id,
            symbol: Ustr::from(symbol),
            width,
        })
        .await
    }

    /// Returns a stream of WebSocket messages.
    ///
    /// # Panics
    ///
    /// Panics if called before `connect()` or if the stream has already been taken.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = NautilusDataWsMessage> + 'static {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or client not connected - stream() can only be called once");
        let mut rx = Arc::try_unwrap(rx).expect(
            "Cannot take ownership of stream - client was cloned and other references exist",
        );
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Disconnects the WebSocket connection gracefully.
    pub async fn disconnect(&self) {
        log::debug!("Disconnecting WebSocket");
        let _ = self.send_cmd(HandlerCommand::Disconnect).await;
    }

    /// Closes the WebSocket connection and cleans up resources.
    pub async fn close(&mut self) {
        log::debug!("Closing WebSocket client");

        // Send disconnect first to allow graceful cleanup before signal
        let _ = self.send_cmd(HandlerCommand::Disconnect).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.signal.store(true, Ordering::Release);

        if let Some(handle) = self.task_handle.take() {
            const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
            let abort_handle = handle.abort_handle();

            match tokio::time::timeout(CLOSE_TIMEOUT, handle).await {
                Ok(Ok(())) => log::debug!("Handler task completed gracefully"),
                Ok(Err(e)) => log::warn!("Handler task panicked: {e}"),
                Err(_) => {
                    log::warn!("Handler task did not complete within timeout, aborting");
                    abort_handle.abort();
                }
            }
        }
    }

    async fn send_cmd(&self, cmd: HandlerCommand) -> AxWsResult<()> {
        let guard = self.cmd_tx.read().await;
        guard
            .send(cmd)
            .map_err(|e| AxWsClientError::ChannelError(e.to_string()))
    }
}
