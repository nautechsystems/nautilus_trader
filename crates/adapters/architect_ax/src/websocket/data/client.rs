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
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering},
    },
    time::Duration,
};

use ahash::AHashSet;
use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_core::{AtomicMap, consts::NAUTILUS_USER_AGENT};
use nautilus_live::SocketControl;
use nautilus_network::{
    http::USER_AGENT,
    mode::ConnectionMode,
    websocket::{
        InitialConnectRetryPolicy, PingHandler, ReconnectHeaders, SubscriptionState,
        TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    AxMdSubscriptionSpec,
    handler::{AxMdWsFeedHandler, HandlerCommand},
};
use crate::{
    common::enums::{AxCandleWidth, AxMarketDataLevel},
    websocket::messages::AxDataWsMessage,
};

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
pub struct SymbolDataTypes {
    pub quotes: bool,
    pub trades: bool,
    pub mark_prices: bool,
    pub instrument_status: bool,
    pub book_level: Option<AxMarketDataLevel>,
}

impl SymbolDataTypes {
    fn effective_subscription(&self) -> Option<AxMdSubscriptionSpec> {
        let ticker = self.mark_prices || self.instrument_status;
        let book_level = self.book_level.or({
            if self.quotes || ticker {
                Some(AxMarketDataLevel::Level1)
            } else {
                None
            }
        });

        if let Some(level) = book_level {
            return Some(AxMdSubscriptionSpec::new(
                level,
                Some(self.trades),
                Some(ticker),
            ));
        }

        if self.trades {
            return Some(AxMdSubscriptionSpec::new(
                AxMarketDataLevel::Trades,
                None,
                None,
            ));
        }

        None
    }

    fn is_empty(&self) -> bool {
        !self.quotes
            && !self.trades
            && !self.mark_prices
            && !self.instrument_status
            && self.book_level.is_none()
    }
}

/// Market data WebSocket client for Ax.
///
/// Provides streaming market data including tickers, trades, order books, and candles.
/// Requires Bearer token authentication obtained via the HTTP `/api/authenticate` endpoint.
pub struct AxMdWebSocketClient {
    url: String,
    heartbeat: Option<u64>,
    auth_token: Arc<Mutex<Option<String>>>,
    reconnect_headers: Arc<Mutex<Option<ReconnectHeaders>>>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<AxDataWsMessage>>>,
    signal: Arc<AtomicBool>,
    cancellation_token: Arc<ArcSwap<CancellationToken>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    subscriptions: SubscriptionState,
    request_id_counter: Arc<AtomicI64>,
    subscribe_lock: Arc<tokio::sync::Mutex<()>>,
    symbol_data_types: Arc<AtomicMap<String, SymbolDataTypes>>,
    status_invalidations: Arc<Mutex<AHashSet<Ustr>>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
    socket_control: Option<SocketControl>,
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
            auth_token: Arc::clone(&self.auth_token),
            reconnect_headers: Arc::clone(&self.reconnect_headers),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            signal: Arc::clone(&self.signal),
            cancellation_token: Arc::clone(&self.cancellation_token),
            task_handle: None,
            subscriptions: self.subscriptions.clone(),
            subscribe_lock: Arc::clone(&self.subscribe_lock),
            request_id_counter: Arc::clone(&self.request_id_counter),
            symbol_data_types: Arc::clone(&self.symbol_data_types),
            status_invalidations: Arc::clone(&self.status_invalidations),
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
            socket_control: self.socket_control.clone(),
        }
    }
}

impl AxMdWebSocketClient {
    fn initial_connect_retry_policy() -> InitialConnectRetryPolicy {
        InitialConnectRetryPolicy {
            max_attempts: NonZeroU32::new(5).expect("initial connect attempts must be non-zero"),
            delay_initial: Duration::from_millis(500),
            delay_max: Duration::from_secs(5),
            backoff_factor: 2.0,
            jitter_ms: 250,
        }
    }

    /// Creates a new Ax market data WebSocket client.
    ///
    /// The `auth_token` is a Bearer token obtained from the HTTP `/api/authenticate` endpoint.
    #[must_use]
    pub fn new(
        url: String,
        auth_token: String,
        heartbeat: u64,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat: Some(heartbeat),
            auth_token: Arc::new(Mutex::new(Some(auth_token))),
            reconnect_headers: Arc::new(Mutex::new(None)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            cancellation_token: Arc::new(ArcSwap::from_pointee(CancellationToken::new())),
            task_handle: None,
            subscriptions: SubscriptionState::new(AX_TOPIC_DELIMITER),
            request_id_counter: Arc::new(AtomicI64::new(1)),
            subscribe_lock: Arc::new(tokio::sync::Mutex::new(())),
            symbol_data_types: Arc::new(AtomicMap::new()),
            status_invalidations: Arc::new(Mutex::new(AHashSet::new())),
            transport_backend,
            proxy_url,
            socket_control: None,
        }
    }

    /// Creates a new Ax market data WebSocket client without authentication.
    ///
    /// Use [`set_auth_token`](Self::set_auth_token) to set the token before connecting.
    #[must_use]
    pub fn without_auth(
        url: String,
        heartbeat: u64,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat: Some(heartbeat),
            auth_token: Arc::new(Mutex::new(None)),
            reconnect_headers: Arc::new(Mutex::new(None)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            cancellation_token: Arc::new(ArcSwap::from_pointee(CancellationToken::new())),
            task_handle: None,
            subscriptions: SubscriptionState::new(AX_TOPIC_DELIMITER),
            request_id_counter: Arc::new(AtomicI64::new(1)),
            subscribe_lock: Arc::new(tokio::sync::Mutex::new(())),
            symbol_data_types: Arc::new(AtomicMap::new()),
            status_invalidations: Arc::new(Mutex::new(AHashSet::new())),
            transport_backend,
            proxy_url,
            socket_control: None,
        }
    }

    /// Configures socket state reporting and reconnect control.
    #[must_use]
    pub fn with_socket_control(mut self, control: SocketControl) -> Self {
        self.socket_control = Some(control);
        self
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Sets the authentication token for subsequent connections.
    ///
    /// This should be called before `connect()` if authentication is required.
    pub fn set_auth_token(&self, token: String) {
        *self
            .auth_token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(token);
    }

    /// Updates the token used by future automatic reconnect attempts.
    ///
    /// Updating the token does not interrupt the active WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the reconnect header cannot be updated.
    pub fn update_auth_token(&self, token: String) -> AxWsResult<()> {
        let value = format!("Bearer {token}");

        if let Some(headers) = self
            .reconnect_headers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            headers
                .update("Authorization", &value)
                .map_err(|e| AxWsClientError::Transport(e.to_string()))?;
        }
        self.set_auth_token(token);
        Ok(())
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

    /// Returns the symbol data types map (shared with handler).
    #[must_use]
    pub fn symbol_data_types(&self) -> Arc<AtomicMap<String, SymbolDataTypes>> {
        Arc::clone(&self.symbol_data_types)
    }

    /// Returns the shared set of symbols whose instrument status cache has been invalidated.
    pub fn status_invalidations(&self) -> Arc<Mutex<AHashSet<Ustr>>> {
        Arc::clone(&self.status_invalidations)
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

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    pub async fn connect(&mut self) -> AxWsResult<()> {
        self.signal.store(false, Ordering::Release);
        let cancellation_token = CancellationToken::new();
        self.cancellation_token
            .store(Arc::new(cancellation_token.clone()));

        let (raw_handler, raw_rx) = channel_message_handler();

        // No-op: ping responses are handled internally by the WebSocketClient
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {});

        let mut headers = vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())];

        let auth_token = self
            .auth_token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        if let Some(token) = auth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers,
            heartbeat_interval_secs: self.heartbeat,
            heartbeat_payload: None, // Ax server sends heartbeats
            connect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let client = WebSocketClient::builder()
            .config(config.clone())
            .message_handler(raw_handler.clone())
            .ping_handler(ping_handler.clone())
            .initial_connect_retry_policy(Self::initial_connect_retry_policy())
            .cancellation_token(cancellation_token)
            .maybe_state_sink(self.socket_control.as_ref().map(SocketControl::sink))
            .connect()
            .await
            .map_err(|e| {
                AxWsClientError::Transport(format!("Failed to connect to {}: {e}", self.url))
            })?;

        self.connection_mode.store(client.connection_mode_atomic());
        let reconnect_handle = client.reconnect_handle();
        *self
            .reconnect_headers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(client.reconnect_headers());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<AxDataWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        self.send_cmd(HandlerCommand::SetClient(client)).await?;

        if let Some(control) = &self.socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                AxMdWsFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions.clone());

            while let Some(msg) = handler.next().await {
                if matches!(msg, AxDataWsMessage::Reconnected) {
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

        let current = self
            .symbol_data_types
            .load()
            .get(symbol)
            .cloned()
            .unwrap_or_default();

        if current.book_level == Some(level) {
            log::debug!("Book deltas already subscribed for {symbol} at {level:?}, skipping");
            return Ok(());
        }

        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.book_level = Some(level);
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            let entry = m.entry(symbol.to_string()).or_default();
            entry.book_level = Some(level);
        });

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

        let current = self
            .symbol_data_types
            .load()
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.quotes = true;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            m.entry(symbol.to_string()).or_default().quotes = true;
        });

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

        let current = self
            .symbol_data_types
            .load()
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.trades = true;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            m.entry(symbol.to_string()).or_default().trades = true;
        });

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

        let Some(current) = self.symbol_data_types.load().get(symbol).cloned() else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe book deltas");
            return Ok(());
        };
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.book_level = None;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            if let Some(entry) = m.get_mut(symbol) {
                entry.book_level = None;
                if entry.is_empty() {
                    m.remove(symbol);
                }
            }
        });

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

        let Some(current) = self.symbol_data_types.load().get(symbol).cloned() else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe quotes");
            return Ok(());
        };
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.quotes = false;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            if let Some(entry) = m.get_mut(symbol) {
                entry.quotes = false;
                if entry.is_empty() {
                    m.remove(symbol);
                }
            }
        });

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

        let Some(current) = self.symbol_data_types.load().get(symbol).cloned() else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe trades");
            return Ok(());
        };
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.trades = false;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            if let Some(entry) = m.get_mut(symbol) {
                entry.trades = false;
                if entry.is_empty() {
                    m.remove(symbol);
                }
            }
        });

        Ok(())
    }

    /// Subscribes to mark prices for a symbol.
    ///
    /// Ensures at least an L1 subscription so that ticker messages
    /// (which carry the mark price field) are received.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_mark_prices(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let current = self
            .symbol_data_types
            .load()
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.mark_prices = true;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            m.entry(symbol.to_string()).or_default().mark_prices = true;
        });

        Ok(())
    }

    /// Unsubscribes from mark prices for a symbol.
    ///
    /// The underlying AX subscription is only removed when all data types
    /// have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_mark_prices(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let Some(current) = self.symbol_data_types.load().get(symbol).cloned() else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe mark prices");
            return Ok(());
        };
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.mark_prices = false;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            if let Some(entry) = m.get_mut(symbol) {
                entry.mark_prices = false;
                if entry.is_empty() {
                    m.remove(symbol);
                }
            }
        });

        Ok(())
    }

    /// Subscribes to instrument status for a symbol.
    ///
    /// Ensures at least an L1 subscription so that ticker messages
    /// (which carry the instrument state field) are received.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_instrument_status(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let current = self
            .symbol_data_types
            .load()
            .get(symbol)
            .cloned()
            .unwrap_or_default();
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.instrument_status = true;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            m.entry(symbol.to_string()).or_default().instrument_status = true;
        });

        Ok(())
    }

    /// Unsubscribes from instrument status for a symbol.
    ///
    /// The underlying AX subscription is only removed when all data types
    /// have been unsubscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_instrument_status(&self, symbol: &str) -> AxWsResult<()> {
        let _guard = self.subscribe_lock.lock().await;

        let Some(current) = self.symbol_data_types.load().get(symbol).cloned() else {
            log::debug!("Symbol {symbol} not subscribed, skipping unsubscribe instrument status");
            return Ok(());
        };
        let old_spec = current.effective_subscription();
        let mut next = current.clone();
        next.instrument_status = false;
        let new_spec = next.effective_subscription();

        self.update_data_subscription(symbol, old_spec, new_spec)
            .await?;

        self.symbol_data_types.rcu(|m| {
            if let Some(entry) = m.get_mut(symbol) {
                entry.instrument_status = false;
                if entry.is_empty() {
                    m.remove(symbol);
                }
            }
        });

        if let Ok(mut invalidations) = self.status_invalidations.lock() {
            invalidations.insert(Ustr::from(symbol));
        }

        Ok(())
    }

    async fn update_data_subscription(
        &self,
        symbol: &str,
        old_spec: Option<AxMdSubscriptionSpec>,
        new_spec: Option<AxMdSubscriptionSpec>,
    ) -> AxWsResult<()> {
        if old_spec == new_spec {
            return Ok(());
        }

        match (old_spec, new_spec) {
            (None, Some(spec)) => {
                log::debug!("Subscribing {symbol} at {spec:?}");
                self.send_subscribe(symbol, spec).await
            }
            (Some(old), None) => {
                log::debug!("Unsubscribing {symbol} (no remaining data types)");
                self.send_unsubscribe(symbol, old).await
            }
            (Some(old), Some(new)) => {
                log::debug!("Resubscribing {symbol}: {old:?} -> {new:?}");
                self.send_unsubscribe(symbol, old).await?;
                if let Err(e) = self.send_subscribe(symbol, new).await {
                    log::warn!("Resubscribe failed for {symbol} at {new:?}: {e}");
                    if let Err(restore_err) = self.send_subscribe(symbol, old).await {
                        log::error!(
                            "Failed to restore {symbol} at {old:?}: {restore_err}, \
                             reconnection required"
                        );
                        self.subscriptions.mark_subscribe(&old.topic(symbol));
                    }
                    return Err(e);
                }
                Ok(())
            }
            (None, None) => Ok(()),
        }
    }

    async fn send_subscribe(&self, symbol: &str, spec: AxMdSubscriptionSpec) -> AxWsResult<()> {
        let topic = spec.topic(symbol);
        let request_id = self.next_request_id();

        self.subscriptions.mark_subscribe(&topic);

        if let Err(e) = self
            .send_cmd(HandlerCommand::Subscribe {
                request_id,
                symbol: Ustr::from(symbol),
                spec,
            })
            .await
        {
            self.subscriptions.mark_unsubscribe(&topic);
            return Err(e);
        }

        Ok(())
    }

    async fn send_unsubscribe(&self, symbol: &str, spec: AxMdSubscriptionSpec) -> AxWsResult<()> {
        let request_id = self.next_request_id();
        let topic = spec.topic(symbol);
        let was_pending = self
            .subscriptions
            .pending_subscribe_topics()
            .contains(&topic);

        self.subscriptions.mark_unsubscribe(&topic);

        if let Err(e) = self
            .send_cmd(HandlerCommand::Unsubscribe {
                request_id,
                symbol: Ustr::from(symbol),
                topic: topic.clone(),
            })
            .await
        {
            self.restore_unsubscribe_state(&topic, was_pending);
            return Err(e);
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
        let was_pending = self
            .subscriptions
            .pending_subscribe_topics()
            .contains(&topic);

        if !self.is_subscribed_topic(&topic) {
            log::debug!("Not subscribed to {topic}, skipping unsubscribe");
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&topic);

        if let Err(e) = self
            .send_cmd(HandlerCommand::UnsubscribeCandles {
                request_id,
                symbol: Ustr::from(symbol),
                width,
                topic: topic.clone(),
            })
            .await
        {
            self.restore_unsubscribe_state(&topic, was_pending);
            return Err(e);
        }

        Ok(())
    }

    fn restore_unsubscribe_state(&self, topic: &str, was_pending: bool) {
        self.subscriptions.confirm_unsubscribe(topic);
        self.subscriptions.mark_subscribe(topic);
        if !was_pending {
            self.subscriptions.confirm_subscribe(topic);
        }
    }

    /// Returns a stream of WebSocket messages.
    ///
    /// # Panics
    ///
    /// Panics if called before `connect()` or if the stream has already been taken.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = AxDataWsMessage> + 'static {
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
        self.cancellation_token.load().cancel();
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

        *self
            .reconnect_headers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
    }

    async fn send_cmd(&self, cmd: HandlerCommand) -> AxWsResult<()> {
        let guard = self.cmd_tx.read().await;
        guard
            .send(cmd)
            .map_err(|e| AxWsClientError::ChannelError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_effective_subscription_empty_returns_none() {
        let sdt = SymbolDataTypes::default();
        assert_eq!(sdt.effective_subscription(), None);
        assert!(sdt.is_empty());
    }

    #[rstest]
    fn test_effective_subscription_book_level_takes_precedence() {
        let sdt = SymbolDataTypes {
            book_level: Some(AxMarketDataLevel::Level2),
            quotes: true,
            ..Default::default()
        };
        assert_eq!(
            sdt.effective_subscription(),
            Some(AxMdSubscriptionSpec::new(
                AxMarketDataLevel::Level2,
                Some(false),
                Some(false),
            ))
        );
        assert!(!sdt.is_empty());
    }

    #[rstest]
    #[case(
        true,
        false,
        false,
        false,
        AxMarketDataLevel::Level1,
        Some(false),
        Some(false)
    )]
    #[case(false, true, false, false, AxMarketDataLevel::Trades, None, None)]
    #[case(
        false,
        false,
        true,
        false,
        AxMarketDataLevel::Level1,
        Some(false),
        Some(true)
    )]
    #[case(
        false,
        false,
        false,
        true,
        AxMarketDataLevel::Level1,
        Some(false),
        Some(true)
    )]
    fn test_effective_subscription_for_single_data_type(
        #[case] quotes: bool,
        #[case] trades: bool,
        #[case] mark_prices: bool,
        #[case] instrument_status: bool,
        #[case] level: AxMarketDataLevel,
        #[case] include_trades: Option<bool>,
        #[case] include_ticker: Option<bool>,
    ) {
        let sdt = SymbolDataTypes {
            quotes,
            trades,
            mark_prices,
            instrument_status,
            book_level: None,
        };
        assert_eq!(
            sdt.effective_subscription(),
            Some(AxMdSubscriptionSpec::new(
                level,
                include_trades,
                include_ticker,
            ))
        );
        assert!(!sdt.is_empty());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn test_unsubscribe_send_failure_restores_subscription(#[case] was_pending: bool) {
        let client = AxMdWebSocketClient::new(
            "ws://localhost:9999/md/ws".to_string(),
            "test_token".to_string(),
            30,
            TransportBackend::default(),
            None,
        );
        let symbol = "EURUSD-PERP";
        let spec = AxMdSubscriptionSpec::new(AxMarketDataLevel::Level2, Some(false), Some(false));
        let topic = spec.topic(symbol);
        client.subscriptions.mark_subscribe(&topic);
        if !was_pending {
            client.subscriptions.confirm_subscribe(&topic);
        }

        let error = client.send_unsubscribe(symbol, spec).await.unwrap_err();

        assert_eq!(error.to_string(), "Channel error: channel closed");
        assert_eq!(client.subscription_count(), usize::from(!was_pending));
        assert_eq!(client.subscriptions.all_topics(), vec![topic]);
        assert_eq!(
            client.subscriptions.pending_subscribe_topics().len(),
            usize::from(was_pending)
        );
        assert!(client.subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn test_unsubscribe_candles_send_failure_restores_subscription(
        #[case] was_pending: bool,
    ) {
        let client = AxMdWebSocketClient::new(
            "ws://localhost:9999/md/ws".to_string(),
            "test_token".to_string(),
            30,
            TransportBackend::default(),
            None,
        );
        let symbol = "EURUSD-PERP";
        let width = AxCandleWidth::Minutes1;
        let topic = format!("candles:{symbol}:{width:?}");
        client.subscriptions.mark_subscribe(&topic);
        if !was_pending {
            client.subscriptions.confirm_subscribe(&topic);
        }

        let error = client.unsubscribe_candles(symbol, width).await.unwrap_err();

        assert_eq!(error.to_string(), "Channel error: channel closed");
        assert_eq!(client.subscription_count(), usize::from(!was_pending));
        assert_eq!(client.subscriptions.all_topics(), vec![topic]);
        assert_eq!(
            client.subscriptions.pending_subscribe_topics().len(),
            usize::from(was_pending)
        );
        assert!(client.subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_unsubscribe_candles_skips_untracked_topic() {
        let client = AxMdWebSocketClient::new(
            "ws://localhost:9999/md/ws".to_string(),
            "test_token".to_string(),
            30,
            TransportBackend::default(),
            None,
        );

        client
            .unsubscribe_candles("EURUSD-PERP", AxCandleWidth::Minutes1)
            .await
            .unwrap();

        assert_eq!(client.subscription_count(), 0);
        assert!(client.subscriptions.all_topics().is_empty());
        assert!(client.subscriptions.pending_subscribe_topics().is_empty());
        assert!(client.subscriptions.pending_unsubscribe_topics().is_empty());
    }
}
