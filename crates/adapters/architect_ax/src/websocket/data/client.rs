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

/// Market data WebSocket client for Ax.
///
/// Provides streaming market data including tickers, trades, order books, and candles.
/// Requires Bearer token authentication obtained via the HTTP `/api/authenticate` endpoint.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub struct AxMdWebSocketClient {
    url: String,
    heartbeat: Option<u64>,
    auth_token: Option<String>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusDataWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    request_id_counter: Arc<AtomicI64>,
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
            out_rx: None, // Each clone gets its own receiver
            signal: Arc::clone(&self.signal),
            task_handle: None, // Each clone gets its own task handle
            subscriptions: self.subscriptions.clone(),
            instruments_cache: Arc::clone(&self.instruments_cache),
            request_id_counter: Arc::clone(&self.request_id_counter),
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

    /// Generates a unique request ID.
    fn next_request_id(&self) -> i64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
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

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                out_tx.clone(),
                subscriptions.clone(),
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

        self.task_handle = Some(Arc::new(stream_handle));

        Ok(())
    }

    /// Subscribes to market data for a symbol at the specified level.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe(&self, symbol: &str, level: AxMarketDataLevel) -> AxWsResult<()> {
        let request_id = self.next_request_id();
        let topic = format!("{symbol}:{level:?}");

        self.subscriptions.mark_subscribe(&topic);

        self.send_cmd(HandlerCommand::Subscribe {
            request_id,
            symbol: symbol.to_string(),
            level,
        })
        .await
    }

    /// Unsubscribes from market data for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe(&self, symbol: &str) -> AxWsResult<()> {
        let request_id = self.next_request_id();

        for level in [
            AxMarketDataLevel::Level1,
            AxMarketDataLevel::Level2,
            AxMarketDataLevel::Level3,
        ] {
            let topic = format!("{symbol}:{level:?}");
            self.subscriptions.mark_unsubscribe(&topic);
        }

        self.send_cmd(HandlerCommand::Unsubscribe {
            request_id,
            symbol: symbol.to_string(),
        })
        .await
    }

    /// Subscribes to candle data for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription command cannot be sent.
    pub async fn subscribe_candles(&self, symbol: &str, width: AxCandleWidth) -> AxWsResult<()> {
        let request_id = self.next_request_id();
        let topic = format!("candles:{symbol}:{width:?}");

        self.subscriptions.mark_subscribe(&topic);

        self.send_cmd(HandlerCommand::SubscribeCandles {
            request_id,
            symbol: symbol.to_string(),
            width,
        })
        .await
    }

    /// Unsubscribes from candle data for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe command cannot be sent.
    pub async fn unsubscribe_candles(&self, symbol: &str, width: AxCandleWidth) -> AxWsResult<()> {
        let request_id = self.next_request_id();
        let topic = format!("candles:{symbol}:{width:?}");

        self.subscriptions.mark_unsubscribe(&topic);

        self.send_cmd(HandlerCommand::UnsubscribeCandles {
            request_id,
            symbol: symbol.to_string(),
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

            match tokio::time::timeout(CLOSE_TIMEOUT, async {
                loop {
                    if Arc::strong_count(&handle) == 1 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            {
                Ok(()) => log::debug!("Handler task completed gracefully"),
                Err(_) => {
                    log::warn!("Handler task did not complete within timeout, aborting");
                    handle.abort();
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
