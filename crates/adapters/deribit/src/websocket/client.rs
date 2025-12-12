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

//! WebSocket client for the Deribit API.
//!
//! The [`DeribitWebSocketClient`] provides connectivity to Deribit's WebSocket API using
//! JSON-RPC 2.0. It supports subscribing to market data channels including trades, order books,
//! and tickers.

use std::{
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::live::runtime::get_runtime;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, env::get_or_env_var_opt};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::USER_AGENT,
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        AuthTracker, PingHandler, SubscriptionState, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    enums::DeribitWsChannel,
    error::{DeribitWsError, DeribitWsResult},
    handler::{DeribitWsFeedHandler, HandlerCommand},
    messages::NautilusWsMessage,
};
use crate::common::consts::{DERIBIT_TESTNET_WS_URL, DERIBIT_WS_URL};

/// Default Deribit WebSocket subscription rate limit: 20 requests per second.
pub static DERIBIT_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(20).unwrap()));

/// Delimiter used in Deribit channel names.
pub const DERIBIT_WS_TOPIC_DELIMITER: &str = ".";

/// WebSocket client for connecting to Deribit.
#[derive(Clone)]
#[allow(dead_code)] // Fields reserved for future authentication support
pub struct DeribitWebSocketClient {
    url: String,
    is_testnet: bool,
    heartbeat_interval: Option<u64>,
    api_key: Option<String>,
    api_secret: Option<String>,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    auth_tracker: AuthTracker,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions_state: SubscriptionState,
    subscribed_channels: Arc<DashMap<String, ()>>,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    request_id_counter: Arc<AtomicU64>,
    cancellation_token: CancellationToken,
}

impl Debug for DeribitWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeribitWebSocketClient")
            .field("url", &self.url)
            .field("is_testnet", &self.is_testnet)
            .field("has_credentials", &self.api_key.is_some())
            .field("heartbeat_interval", &self.heartbeat_interval)
            .finish_non_exhaustive()
    }
}

impl DeribitWebSocketClient {
    /// Creates a new [`DeribitWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat_interval: Option<u64>,
        is_testnet: bool,
    ) -> anyhow::Result<Self> {
        let url = url.unwrap_or_else(|| {
            if is_testnet {
                DERIBIT_TESTNET_WS_URL.to_string()
            } else {
                DERIBIT_WS_URL.to_string()
            }
        });

        let signal = Arc::new(AtomicBool::new(false));
        let subscriptions_state = SubscriptionState::new('.');

        Ok(Self {
            url,
            is_testnet,
            heartbeat_interval,
            api_key,
            api_secret,
            signal,
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed.as_u8(),
            ))),
            auth_tracker: AuthTracker::new(),
            cmd_tx: {
                let (tx, _) = tokio::sync::mpsc::unbounded_channel();
                Arc::new(tokio::sync::RwLock::new(tx))
            },
            out_rx: None,
            task_handle: None,
            subscriptions_state,
            subscribed_channels: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(DashMap::new()),
            request_id_counter: Arc::new(AtomicU64::new(1)),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new public (unauthenticated) client.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    pub fn new_public(is_testnet: bool) -> anyhow::Result<Self> {
        let heartbeat_interval = 10;
        Self::new(None, None, None, Some(heartbeat_interval), is_testnet)
    }

    /// Creates a client from environment variables.
    ///
    /// Uses `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET` for testnet,
    /// or `DERIBIT_API_KEY` and `DERIBIT_API_SECRET` for mainnet.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    pub fn from_env() -> anyhow::Result<Self> {
        // Default to testnet
        let api_key = get_or_env_var_opt(None, "DERIBIT_TESTNET_API_KEY");
        let api_secret = get_or_env_var_opt(None, "DERIBIT_TESTNET_API_SECRET");

        let heartbeat_interval = 10;
        Self::new(None, api_key, api_secret, Some(heartbeat_interval), true)
    }

    /// Returns the current connection mode.
    fn connection_mode(&self) -> ConnectionMode {
        let mode_u8 = self.connection_mode.load().load(Ordering::Relaxed);
        ConnectionMode::from_u8(mode_u8)
    }

    /// Returns whether the client is actively connected.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.connection_mode() == ConnectionMode::Active
    }

    /// Returns whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.connection_mode() == ConnectionMode::Disconnect
    }

    /// Waits until the client is active or timeout expires.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeout expires before the client becomes active.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> DeribitWsResult<()> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            DeribitWsError::Timeout(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
    }

    /// Caches instruments for use during message parsing.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        self.instruments_cache.clear();
        for inst in instruments {
            self.instruments_cache
                .insert(inst.raw_symbol().inner(), inst);
        }
        tracing::debug!("Cached {} instruments", self.instruments_cache.len());
    }

    /// Caches a single instrument.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.raw_symbol().inner();
        self.instruments_cache.insert(symbol, instrument);

        // If connected, send update to handler
        if self.is_active() {
            let tx = self.cmd_tx.clone();
            let inst = self.instruments_cache.get(&symbol).map(|r| r.clone());
            if let Some(inst) = inst {
                tokio::spawn(async move {
                    let _ = tx
                        .read()
                        .await
                        .send(HandlerCommand::UpdateInstrument(Box::new(inst)));
                });
            }
        }
    }

    /// Connects to the Deribit WebSocket API.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        tracing::info!("Connecting to Deribit WebSocket: {}", self.url);

        // Reset stop signal
        self.signal.store(false, Ordering::Relaxed);

        // Create message handler and channel
        let (message_handler, raw_rx) = channel_message_handler();

        // No-op ping handler: handler responds to pings directly
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally
        });

        // Configure WebSocket client
        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat_interval,
            heartbeat_msg: None, // Deribit uses JSON-RPC heartbeat, not text ping
            message_handler: Some(message_handler),
            ping_handler: Some(ping_handler),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };

        // Configure rate limits
        let keyed_quotas = vec![("subscription".to_string(), *DERIBIT_WS_SUBSCRIPTION_QUOTA)];

        // Connect the WebSocket
        let ws_client = WebSocketClient::connect(
            config,
            None, // post_reconnection
            keyed_quotas,
            Some(*DERIBIT_WS_SUBSCRIPTION_QUOTA), // Default quota
        )
        .await?;

        // Store connection mode
        self.connection_mode
            .store(ws_client.connection_mode_atomic());

        // Create message channels
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();

        // Store command sender and output receiver
        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(Arc::new(out_rx));

        // Create handler
        let mut handler = DeribitWsFeedHandler::new(
            self.signal.clone(),
            cmd_rx,
            raw_rx,
            out_tx,
            self.auth_tracker.clone(),
            self.subscriptions_state.clone(),
        );

        // Send client to handler
        let _ = cmd_tx.send(HandlerCommand::SetClient(ws_client));

        // Replay cached instruments
        let instruments: Vec<InstrumentAny> =
            self.instruments_cache.iter().map(|r| r.clone()).collect();
        if !instruments.is_empty() {
            let _ = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments));
        }

        // Enable heartbeat if configured
        if let Some(interval) = self.heartbeat_interval {
            let _ = cmd_tx.send(HandlerCommand::SetHeartbeat { interval });
        }

        // Spawn handler task
        let subscriptions_state = self.subscriptions_state.clone();
        let subscribed_channels = self.subscribed_channels.clone();

        let task_handle = get_runtime().spawn(async move {
            loop {
                match handler.next().await {
                    Some(msg) => {
                        // Handle reconnection
                        if matches!(msg, NautilusWsMessage::Reconnected) {
                            tracing::info!("Reconnected - resubscribing to channels");

                            // Resubscribe to all tracked channels
                            let channels: Vec<String> = subscribed_channels
                                .iter()
                                .map(|r| r.key().clone())
                                .collect();

                            // Mark each channel as failed and pending resubscription
                            for channel in &channels {
                                subscriptions_state.mark_failure(channel);
                            }

                            if !channels.is_empty() {
                                let _ = cmd_tx.send(HandlerCommand::Subscribe { channels });
                            }
                        }
                    }
                    None => {
                        tracing::debug!("Handler returned None, stopping task");
                        break;
                    }
                }
            }
        });

        self.task_handle = Some(Arc::new(task_handle));
        tracing::info!("Connected to Deribit WebSocket");

        Ok(())
    }

    /// Closes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the close operation fails.
    pub async fn close(&self) -> DeribitWsResult<()> {
        tracing::info!("Closing Deribit WebSocket connection");
        self.signal.store(true, Ordering::Relaxed);

        let _ = self.cmd_tx.read().await.send(HandlerCommand::Disconnect);

        // Wait for task to complete
        if let Some(_handle) = &self.task_handle {
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                // Can't actually await the handle since we don't own it
                tokio::time::sleep(Duration::from_millis(100)).await;
            })
            .await;
        }

        Ok(())
    }

    /// Returns a stream of WebSocket messages.
    ///
    /// # Panics
    ///
    /// Panics if called before `connect()` or if called twice.
    pub fn stream(&mut self) -> impl Stream<Item = NautilusWsMessage> + 'static {
        let rx = self
            .out_rx
            .take()
            .expect("Data stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");

        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    // ------------------------------------------------------------------------------------------------
    // Subscription Methods
    // ------------------------------------------------------------------------------------------------

    async fn send_subscribe(&self, channels: Vec<String>) -> DeribitWsResult<()> {
        // Track subscriptions
        for channel in &channels {
            self.subscribed_channels.insert(channel.clone(), ());
        }

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Subscribe {
                channels: channels.clone(),
            })
            .map_err(|e| DeribitWsError::Send(e.to_string()))?;

        tracing::debug!("Sent subscribe for {} channels", channels.len());
        Ok(())
    }

    async fn send_unsubscribe(&self, channels: Vec<String>) -> DeribitWsResult<()> {
        // Remove from tracked subscriptions
        for channel in &channels {
            self.subscribed_channels.remove(channel);
        }

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Unsubscribe {
                channels: channels.clone(),
            })
            .map_err(|e| DeribitWsError::Send(e.to_string()))?;

        tracing::debug!("Sent unsubscribe for {} channels", channels.len());
        Ok(())
    }

    /// Subscribes to trade updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Trades.format_channel(instrument_id.symbol.as_str(), None);
        self.send_subscribe(vec![channel]).await
    }

    /// Unsubscribes from trade updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Trades.format_channel(instrument_id.symbol.as_str(), None);
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to order book updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Book.format_channel(instrument_id.symbol.as_str(), None);
        self.send_subscribe(vec![channel]).await
    }

    /// Unsubscribes from order book updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Book.format_channel(instrument_id.symbol.as_str(), None);
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to ticker updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails.
    pub async fn subscribe_ticker(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Ticker.format_channel(instrument_id.symbol.as_str(), None);
        self.send_subscribe(vec![channel]).await
    }

    /// Unsubscribes from ticker updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_ticker(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Ticker.format_channel(instrument_id.symbol.as_str(), None);
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to quote (best bid/ask) updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Quote.format_channel(instrument_id.symbol.as_str(), None);
        self.send_subscribe(vec![channel]).await
    }

    /// Unsubscribes from quote updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_quotes(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        let channel = DeribitWsChannel::Quote.format_channel(instrument_id.symbol.as_str(), None);
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to multiple channels at once.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails.
    pub async fn subscribe(&self, channels: Vec<String>) -> DeribitWsResult<()> {
        self.send_subscribe(channels).await
    }

    /// Unsubscribes from multiple channels at once.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe(&self, channels: Vec<String>) -> DeribitWsResult<()> {
        self.send_unsubscribe(channels).await
    }
}
