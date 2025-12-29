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
use nautilus_common::live::get_runtime;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, env::get_or_env_var_opt, time::get_atomic_clock_realtime,
};
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
    auth::{AuthState, DEFAULT_SESSION_NAME, send_auth_request, spawn_token_refresh_task},
    enums::{DeribitUpdateInterval, DeribitWsChannel},
    error::{DeribitWsError, DeribitWsResult},
    handler::{DeribitWsFeedHandler, HandlerCommand},
    messages::NautilusWsMessage,
};
use crate::common::{
    consts::{DERIBIT_TESTNET_WS_URL, DERIBIT_WS_URL},
    credential::Credential,
};

/// Default Deribit WebSocket subscription rate limit: 20 requests per second.
pub static DERIBIT_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(20).unwrap()));

/// Authentication timeout in seconds.
const AUTHENTICATION_TIMEOUT_SECS: u64 = 30;

/// WebSocket client for connecting to Deribit.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub struct DeribitWebSocketClient {
    url: String,
    is_testnet: bool,
    heartbeat_interval: Option<u64>,
    credential: Option<Credential>,
    is_authenticated: Arc<AtomicBool>,
    auth_state: Arc<tokio::sync::RwLock<Option<AuthState>>>,
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
            .field("has_credentials", &self.credential.is_some())
            .field(
                "is_authenticated",
                &self.is_authenticated.load(Ordering::Relaxed),
            )
            .field(
                "has_auth_state",
                &self
                    .auth_state
                    .try_read()
                    .map(|s| s.is_some())
                    .unwrap_or(false),
            )
            .field("heartbeat_interval", &self.heartbeat_interval)
            .finish_non_exhaustive()
    }
}

impl DeribitWebSocketClient {
    /// Creates a new [`DeribitWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided.
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

        // Create credential from api_key and api_secret if both provided
        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => anyhow::bail!("Both api_key and api_secret must be provided together, or neither"),
        };

        let signal = Arc::new(AtomicBool::new(false));
        let subscriptions_state = SubscriptionState::new('.');

        Ok(Self {
            url,
            is_testnet,
            heartbeat_interval,
            credential,
            is_authenticated: Arc::new(AtomicBool::new(false)),
            auth_state: Arc::new(tokio::sync::RwLock::new(None)),
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

    /// Creates an authenticated client with credentials.
    ///
    /// Uses environment variables to load credentials:
    /// - Testnet: `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET`
    /// - Mainnet: `DERIBIT_API_KEY` and `DERIBIT_API_SECRET`
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are not found in environment variables.
    pub fn with_credentials(is_testnet: bool) -> anyhow::Result<Self> {
        let (key_env, secret_env) = if is_testnet {
            ("DERIBIT_TESTNET_API_KEY", "DERIBIT_TESTNET_API_SECRET")
        } else {
            ("DERIBIT_API_KEY", "DERIBIT_API_SECRET")
        };

        let api_key = get_or_env_var_opt(None, key_env)
            .ok_or_else(|| anyhow::anyhow!("Missing environment variable: {key_env}"))?;
        let api_secret = get_or_env_var_opt(None, secret_env)
            .ok_or_else(|| anyhow::anyhow!("Missing environment variable: {secret_env}"))?;

        let heartbeat_interval = 10;
        Self::new(
            None,
            Some(api_key),
            Some(api_secret),
            Some(heartbeat_interval),
            is_testnet,
        )
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

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.connection_mode() == ConnectionMode::Disconnect
    }

    /// Cancel all pending WebSocket requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Returns the cancellation token for this client.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
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
                get_runtime().spawn(async move {
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
            Some(message_handler),
            Some(ping_handler),
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
        let credential = self.credential.clone();
        let is_authenticated = self.is_authenticated.clone();
        let auth_state = self.auth_state.clone();
        let request_id_counter = self.request_id_counter.clone();

        let task_handle = get_runtime().spawn(async move {
            // Track if we're waiting for re-authentication after reconnection
            let mut pending_reauth = false;

            loop {
                match handler.next().await {
                    Some(msg) => match msg {
                        NautilusWsMessage::Reconnected => {
                            tracing::info!("Reconnected to Deribit WebSocket");

                            // Mark all subscriptions as failed
                            let channels: Vec<String> = subscribed_channels
                                .iter()
                                .map(|r| r.key().clone())
                                .collect();

                            // Mark each channel as failed and pending resubscription
                            for channel in &channels {
                                subscriptions_state.mark_failure(channel);
                            }

                            // Check if we need to re-authenticate
                            if let Some(cred) = &credential {
                                tracing::info!("Re-authenticating after reconnection...");

                                // Reset authenticated state
                                is_authenticated.store(false, Ordering::Release);
                                pending_reauth = true;

                                // Get the previously used scope for re-authentication
                                let previous_scope = auth_state
                                    .read()
                                    .await
                                    .as_ref()
                                    .map(|s| s.scope.clone());

                                // Send re-authentication request
                                send_auth_request(cred, previous_scope, &cmd_tx, &request_id_counter);
                            } else {
                                // No credentials - resubscribe immediately
                                if !channels.is_empty() {
                                    let _ = cmd_tx.send(HandlerCommand::Subscribe { channels });
                                }
                            }
                        }
                        NautilusWsMessage::Authenticated(result) => {
                            let timestamp = get_atomic_clock_realtime().get_time_ms();
                            let new_auth_state = AuthState::from_auth_result(&result, timestamp);
                            *auth_state.write().await = Some(new_auth_state);

                            // Spawn background token refresh task
                            spawn_token_refresh_task(
                                result.expires_in,
                                result.refresh_token.clone(),
                                cmd_tx.clone(),
                                request_id_counter.clone(),
                            );

                            if pending_reauth {
                                pending_reauth = false;
                                is_authenticated.store(true, Ordering::Release);
                                tracing::info!(
                                    "Re-authentication successful (scope: {}), resubscribing to channels",
                                    result.scope
                                );

                                // Now resubscribe to all channels
                                let channels: Vec<String> = subscribed_channels
                                    .iter()
                                    .map(|r| r.key().clone())
                                    .collect();

                                if !channels.is_empty() {
                                    let _ = cmd_tx.send(HandlerCommand::Subscribe { channels });
                                }
                            } else {
                                // Initial authentication completed
                                is_authenticated.store(true, Ordering::Release);
                                tracing::debug!(
                                    "Auth state stored: scope={}, expires_in={}s",
                                    result.scope,
                                    result.expires_in
                                );
                            }
                        }
                        _ => {}
                    },
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

    /// Returns whether the client has credentials configured.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Returns whether the client is authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.is_authenticated.load(Ordering::Acquire)
    }

    /// Authenticates the WebSocket session with Deribit.
    ///
    /// Uses the `client_signature` grant type with HMAC-SHA256 signature.
    /// This must be called before subscribing to raw data streams.
    ///
    /// # Arguments
    ///
    /// * `session_name` - Optional session name for session-scoped authentication.
    ///   When provided, uses `session:<name>` scope which allows skipping `access_token`
    ///   in subsequent private requests. When `None`, uses default `connection` scope.
    ///   Recommended to use session scope for order execution compatibility.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No credentials are configured
    /// - The authentication request fails
    /// - The authentication times out
    pub async fn authenticate(&self, session_name: Option<&str>) -> DeribitWsResult<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            DeribitWsError::Authentication("API credentials not configured".to_string())
        })?;

        // Determine scope
        let scope = session_name.map(|name| format!("session:{name}"));

        tracing::info!(
            "Authenticating WebSocket with API key: {}, scope: {}",
            credential.api_key_masked(),
            scope.as_deref().unwrap_or("connection (default)")
        );

        let rx = self.auth_tracker.begin();

        // Send authentication request
        let cmd_tx = self.cmd_tx.read().await;
        send_auth_request(credential, scope, &cmd_tx, &self.request_id_counter);
        drop(cmd_tx);

        // Wait for authentication result with timeout
        match self
            .auth_tracker
            .wait_for_result::<DeribitWsError>(Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS), rx)
            .await
        {
            Ok(()) => {
                self.is_authenticated.store(true, Ordering::Release);
                tracing::info!("WebSocket authenticated successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "WebSocket authentication failed");
                Err(e)
            }
        }
    }

    /// Authenticates with session scope using default session name.
    ///
    /// Convenience method equivalent to `authenticate(Some("nautilus"))`.
    /// Session-scoped authentication is recommended for order execution as it
    /// allows skipping `access_token` in private method payloads.
    ///
    /// # Errors
    ///
    /// Returns an error if authentication fails.
    pub async fn authenticate_session(&self) -> DeribitWsResult<()> {
        self.authenticate(Some(DEFAULT_SESSION_NAME)).await
    }

    /// Returns the current authentication state containing tokens.
    ///
    /// Returns `None` if not authenticated or tokens haven't been stored yet.
    pub async fn auth_state(&self) -> Option<AuthState> {
        self.auth_state.read().await.clone()
    }

    /// Returns the current access token if available.
    pub async fn access_token(&self) -> Option<String> {
        self.auth_state
            .read()
            .await
            .as_ref()
            .map(|s| s.access_token.clone())
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
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails or raw is requested without authentication.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> DeribitWsResult<()> {
        let interval = interval.unwrap_or_default();
        self.check_auth_requirement(interval)?;
        let channel =
            DeribitWsChannel::Trades.format_channel(instrument_id.symbol.as_str(), Some(interval));
        self.send_subscribe(vec![channel]).await
    }

    /// Subscribes to raw trade updates (requires authentication).
    ///
    /// Convenience method equivalent to `subscribe_trades(id, Some(DeribitUpdateInterval::Raw))`.
    ///
    /// # Errors
    ///
    /// Returns an error if not authenticated or subscription fails.
    pub async fn subscribe_trades_raw(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        self.subscribe_trades(instrument_id, Some(DeribitUpdateInterval::Raw))
            .await
    }

    /// Unsubscribes from trade updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> DeribitWsResult<()> {
        let interval = interval.unwrap_or_default();
        let channel =
            DeribitWsChannel::Trades.format_channel(instrument_id.symbol.as_str(), Some(interval));
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to order book updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails or raw is requested without authentication.
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> DeribitWsResult<()> {
        let interval = interval.unwrap_or_default();
        self.check_auth_requirement(interval)?;
        let channel =
            DeribitWsChannel::Book.format_channel(instrument_id.symbol.as_str(), Some(interval));
        self.send_subscribe(vec![channel]).await
    }

    /// Subscribes to raw order book updates (requires authentication).
    ///
    /// Convenience method equivalent to `subscribe_book(id, Some(DeribitUpdateInterval::Raw))`.
    ///
    /// # Errors
    ///
    /// Returns an error if not authenticated or subscription fails.
    pub async fn subscribe_book_raw(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        self.subscribe_book(instrument_id, Some(DeribitUpdateInterval::Raw))
            .await
    }

    /// Unsubscribes from order book updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_book(
        &self,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> DeribitWsResult<()> {
        let interval = interval.unwrap_or_default();
        let channel =
            DeribitWsChannel::Book.format_channel(instrument_id.symbol.as_str(), Some(interval));
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to ticker updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails or raw is requested without authentication.
    pub async fn subscribe_ticker(
        &self,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> DeribitWsResult<()> {
        let interval = interval.unwrap_or_default();
        self.check_auth_requirement(interval)?;
        let channel =
            DeribitWsChannel::Ticker.format_channel(instrument_id.symbol.as_str(), Some(interval));
        self.send_subscribe(vec![channel]).await
    }

    /// Subscribes to raw ticker updates (requires authentication).
    ///
    /// Convenience method equivalent to `subscribe_ticker(id, Some(DeribitUpdateInterval::Raw))`.
    ///
    /// # Errors
    ///
    /// Returns an error if not authenticated or subscription fails.
    pub async fn subscribe_ticker_raw(&self, instrument_id: InstrumentId) -> DeribitWsResult<()> {
        self.subscribe_ticker(instrument_id, Some(DeribitUpdateInterval::Raw))
            .await
    }

    /// Unsubscribes from ticker updates for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe_ticker(
        &self,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> DeribitWsResult<()> {
        let interval = interval.unwrap_or_default();
        let channel =
            DeribitWsChannel::Ticker.format_channel(instrument_id.symbol.as_str(), Some(interval));
        self.send_unsubscribe(vec![channel]).await
    }

    /// Subscribes to quote (best bid/ask) updates for an instrument.
    ///
    /// Note: Quote channel does not support interval parameter.
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

    /// Checks if authentication is required for the given interval.
    ///
    /// # Errors
    ///
    /// Returns an error if raw interval is requested but client is not authenticated.
    fn check_auth_requirement(&self, interval: DeribitUpdateInterval) -> DeribitWsResult<()> {
        if interval.requires_auth() && !self.is_authenticated() {
            return Err(DeribitWsError::Authentication(
                "Raw streams require authentication. Call authenticate() first.".to_string(),
            ));
        }
        Ok(())
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
