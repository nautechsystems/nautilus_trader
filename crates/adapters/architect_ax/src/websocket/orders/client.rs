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

//! Orders WebSocket client for Ax.

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
use nautilus_model::{
    identifiers::{AccountId, ClientOrderId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, PingHandler, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::handler::{FeedHandler, HandlerCommand};
use crate::{
    common::enums::{AxOrderSide, AxTimeInForce},
    websocket::messages::{AxOrdersWsMessage, AxWsPlaceOrder, OrderMetadata},
};

/// Default heartbeat interval in seconds.
const DEFAULT_HEARTBEAT_SECS: u64 = 30;

/// Result type for Ax orders WebSocket operations.
pub type AxOrdersWsResult<T> = Result<T, AxOrdersWsClientError>;

/// Error type for the Ax orders WebSocket client.
#[derive(Debug, Clone)]
pub enum AxOrdersWsClientError {
    /// Transport/connection error.
    Transport(String),
    /// Channel send error.
    ChannelError(String),
    /// Authentication error.
    AuthenticationError(String),
}

impl core::fmt::Display for AxOrdersWsClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "Transport error: {msg}"),
            Self::ChannelError(msg) => write!(f, "Channel error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
        }
    }
}

impl std::error::Error for AxOrdersWsClientError {}

/// Orders WebSocket client for Ax.
///
/// Provides authenticated order management including placing, canceling,
/// and monitoring order status via WebSocket.
pub struct AxOrdersWebSocketClient {
    url: String,
    heartbeat: Option<u64>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<AxOrdersWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    auth_tracker: AuthTracker,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    request_id_counter: Arc<AtomicI64>,
    account_id: AccountId,
}

impl Debug for AxOrdersWebSocketClient {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct(stringify!(AxOrdersWebSocketClient))
            .field("url", &self.url)
            .field("heartbeat", &self.heartbeat)
            .field("account_id", &self.account_id)
            .finish()
    }
}

impl Clone for AxOrdersWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            heartbeat: self.heartbeat,
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None, // Each clone gets its own receiver
            signal: Arc::clone(&self.signal),
            task_handle: None, // Each clone gets its own task handle
            auth_tracker: self.auth_tracker.clone(),
            instruments_cache: Arc::clone(&self.instruments_cache),
            request_id_counter: Arc::clone(&self.request_id_counter),
            account_id: self.account_id,
        }
    }
}

impl AxOrdersWebSocketClient {
    /// Creates a new Ax orders WebSocket client.
    #[must_use]
    pub fn new(url: String, account_id: AccountId, heartbeat: Option<u64>) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            auth_tracker: AuthTracker::default(),
            instruments_cache: Arc::new(DashMap::new()),
            request_id_counter: Arc::new(AtomicI64::new(1)),
            account_id,
        }
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
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

    /// Generates a unique request ID.
    fn next_request_id(&self) -> i64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Caches an instrument for use during message parsing.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.symbol().inner();
        self.instruments_cache.insert(symbol, instrument.clone());

        // If connected, also send to handler
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

    /// Establishes the WebSocket connection with authentication.
    ///
    /// # Arguments
    ///
    /// * `bearer_token` - The bearer token for authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self, bearer_token: &str) -> AxOrdersWsResult<()> {
        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        // No-op ping handler: handler owns the WebSocketClient and responds to pings directly
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![
                ("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
                (
                    "Authorization".to_string(),
                    format!("Bearer {bearer_token}"),
                ),
            ],
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
        const MAX_RETRIES: u32 = 5;
        const CONNECTION_TIMEOUT_SECS: u64 = 10;

        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(500),
            Duration::from_millis(5000),
            2.0,
            250,
            false,
        )
        .map_err(|e| AxOrdersWsClientError::Transport(e.to_string()))?;

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
                        "WebSocket connection attempt failed: attempt={attempt}, max_retries={MAX_RETRIES}, url={}, error={last_error}",
                        self.url
                    );
                }
                Err(_) => {
                    last_error = format!("Connection timeout after {CONNECTION_TIMEOUT_SECS}s");
                    log::warn!(
                        "WebSocket connection attempt timed out: attempt={attempt}, max_retries={MAX_RETRIES}, url={}",
                        self.url
                    );
                }
            }

            if attempt >= MAX_RETRIES {
                return Err(AxOrdersWsClientError::Transport(format!(
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

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<AxOrdersWsMessage>();
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

        // Bearer token is passed in connection headers
        self.send_cmd(HandlerCommand::Authenticate {
            token: bearer_token.to_string(),
        })
        .await?;

        let signal = Arc::clone(&self.signal);
        let auth_tracker = self.auth_tracker.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                out_tx.clone(),
                auth_tracker.clone(),
            );

            while let Some(msg) = handler.next().await {
                if matches!(msg, AxOrdersWsMessage::Reconnected) {
                    log::info!("WebSocket reconnected");
                    // TODO: Re-authenticate on reconnect if needed
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

    /// Places an order via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the order command cannot be sent.
    #[allow(clippy::too_many_arguments)]
    pub async fn place_order(
        &self,
        client_order_id: ClientOrderId,
        symbol: Ustr,
        side: AxOrderSide,
        quantity: i64,
        price: Decimal,
        time_in_force: AxTimeInForce,
        post_only: bool,
        tag: Option<String>,
    ) -> AxOrdersWsResult<i64> {
        let request_id = self.next_request_id();

        let order = AxWsPlaceOrder {
            rid: request_id,
            t: "p".to_string(),
            s: symbol.to_string(),
            d: side,
            q: quantity,
            p: price,
            tif: time_in_force,
            po: post_only,
            tag,
        };

        let metadata = OrderMetadata {
            client_order_id,
            symbol,
        };

        self.send_cmd(HandlerCommand::PlaceOrder {
            request_id,
            order,
            metadata,
        })
        .await?;

        Ok(request_id)
    }

    /// Cancels an order via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the cancel command cannot be sent.
    pub async fn cancel_order(&self, order_id: &str) -> AxOrdersWsResult<i64> {
        let request_id = self.next_request_id();

        self.send_cmd(HandlerCommand::CancelOrder {
            request_id,
            order_id: order_id.to_string(),
        })
        .await?;

        Ok(request_id)
    }

    /// Requests open orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the request command cannot be sent.
    pub async fn get_open_orders(&self) -> AxOrdersWsResult<i64> {
        let request_id = self.next_request_id();

        self.send_cmd(HandlerCommand::GetOpenOrders { request_id })
            .await?;

        Ok(request_id)
    }

    /// Returns a stream of messages from the WebSocket.
    ///
    /// # Panics
    ///
    /// Panics if called more than once or before connecting.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = AxOrdersWsMessage> + use<'_> {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or client not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
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
        self.signal.store(true, Ordering::Relaxed);

        let _ = self.send_cmd(HandlerCommand::Disconnect).await;

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

    async fn send_cmd(&self, cmd: HandlerCommand) -> AxOrdersWsResult<()> {
        let guard = self.cmd_tx.read().await;
        guard
            .send(cmd)
            .map_err(|e| AxOrdersWsClientError::ChannelError(e.to_string()))
    }
}
