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

//! Bybit WebSocket client providing public market data streaming.
//!
//! Bybit API reference <https://bybit-exchange.github.io/docs/>.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::runtime::get_runtime;
use nautilus_core::{UUID4, consts::NAUTILUS_USER_AGENT, env::get_or_env_var_opt};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, PingHandler, SubscriptionState, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            BYBIT_BASE_COIN, BYBIT_NAUTILUS_BROKER_ID, BYBIT_QUOTE_COIN, BYBIT_WS_TOPIC_DELIMITER,
        },
        credential::Credential,
        enums::{
            BybitEnvironment, BybitOrderSide, BybitOrderType, BybitProductType, BybitTimeInForce,
            BybitTriggerDirection, BybitTriggerType, BybitWsOrderRequestOp,
        },
        parse::{extract_raw_symbol, make_bybit_symbol},
        symbol::BybitSymbol,
        urls::{bybit_ws_private_url, bybit_ws_public_url, bybit_ws_trade_url},
    },
    websocket::{
        enums::{BybitWsOperation, BybitWsPrivateChannel, BybitWsPublicChannel},
        error::{BybitWsError, BybitWsResult},
        handler::{FeedHandler, HandlerCommand},
        messages::{
            BybitAuthRequest, BybitSubscription, BybitWsAmendOrderParams, BybitWsBatchCancelItem,
            BybitWsBatchCancelOrderArgs, BybitWsBatchPlaceItem, BybitWsBatchPlaceOrderArgs,
            BybitWsCancelOrderParams, BybitWsHeader, BybitWsPlaceOrderParams, BybitWsRequest,
            NautilusWsMessage,
        },
    },
};

const DEFAULT_HEARTBEAT_SECS: u64 = 20;
const WEBSOCKET_AUTH_WINDOW_MS: i64 = 5_000;
const BATCH_PROCESSING_LIMIT: usize = 20;

/// Type alias for the funding rate cache.
type FundingCache = Arc<tokio::sync::RwLock<AHashMap<Ustr, (Option<String>, Option<String>)>>>;

/// Resolves credentials from provided values or environment variables.
///
/// Priority for environment variables based on environment:
/// - Demo: `BYBIT_DEMO_API_KEY`, `BYBIT_DEMO_API_SECRET`
/// - Testnet: `BYBIT_TESTNET_API_KEY`, `BYBIT_TESTNET_API_SECRET`
/// - Mainnet: `BYBIT_API_KEY`, `BYBIT_API_SECRET`
fn resolve_credential(
    environment: BybitEnvironment,
    api_key: Option<String>,
    api_secret: Option<String>,
) -> Option<Credential> {
    let (api_key_env, api_secret_env) = match environment {
        BybitEnvironment::Demo => ("BYBIT_DEMO_API_KEY", "BYBIT_DEMO_API_SECRET"),
        BybitEnvironment::Testnet => ("BYBIT_TESTNET_API_KEY", "BYBIT_TESTNET_API_SECRET"),
        BybitEnvironment::Mainnet => ("BYBIT_API_KEY", "BYBIT_API_SECRET"),
    };

    let key = get_or_env_var_opt(api_key, api_key_env);
    let secret = get_or_env_var_opt(api_secret, api_secret_env);

    match (key, secret) {
        (Some(k), Some(s)) => Some(Credential::new(k, s)),
        _ => None,
    }
}

/// Public/market data WebSocket client for Bybit.
#[cfg_attr(feature = "python", pyo3::pyclass)]
pub struct BybitWebSocketClient {
    url: String,
    environment: BybitEnvironment,
    product_type: Option<BybitProductType>,
    credential: Option<Credential>,
    requires_auth: bool,
    auth_tracker: AuthTracker,
    heartbeat: Option<u64>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    is_authenticated: Arc<AtomicBool>,
    account_id: Option<AccountId>,
    mm_level: Arc<AtomicU8>,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    funding_cache: FundingCache,
    cancellation_token: CancellationToken,
}

impl Debug for BybitWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BybitWebSocketClient")
            .field("url", &self.url)
            .field("environment", &self.environment)
            .field("product_type", &self.product_type)
            .field("requires_auth", &self.requires_auth)
            .field("heartbeat", &self.heartbeat)
            .field("confirmed_subscriptions", &self.subscriptions.len())
            .finish()
    }
}

impl Clone for BybitWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            environment: self.environment,
            product_type: self.product_type,
            credential: self.credential.clone(),
            requires_auth: self.requires_auth,
            auth_tracker: self.auth_tracker.clone(),
            heartbeat: self.heartbeat,
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None, // Each clone gets its own receiver
            signal: Arc::clone(&self.signal),
            task_handle: None, // Each clone gets its own task handle
            subscriptions: self.subscriptions.clone(),
            is_authenticated: Arc::clone(&self.is_authenticated),
            account_id: self.account_id,
            mm_level: Arc::clone(&self.mm_level),
            instruments_cache: Arc::clone(&self.instruments_cache),
            funding_cache: Arc::clone(&self.funding_cache),
            cancellation_token: self.cancellation_token.clone(),
        }
    }
}

impl BybitWebSocketClient {
    /// Creates a new Bybit public WebSocket client.
    #[must_use]
    pub fn new_public(url: Option<String>, heartbeat: Option<u64>) -> Self {
        Self::new_public_with(
            BybitProductType::Linear,
            BybitEnvironment::Mainnet,
            url,
            heartbeat,
        )
    }

    /// Creates a new Bybit public WebSocket client targeting the specified product/environment.
    #[must_use]
    pub fn new_public_with(
        product_type: BybitProductType,
        environment: BybitEnvironment,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        // We don't have a handler yet; this placeholder keeps cache_instrument() working.
        // connect() swaps in the real channel and replays any queued instruments so the
        // handler sees them once it starts.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url: url.unwrap_or_else(|| bybit_ws_public_url(product_type, environment)),
            environment,
            product_type: Some(product_type),
            credential: None,
            requires_auth: false,
            auth_tracker: AuthTracker::new(),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER),
            is_authenticated: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            funding_cache: Arc::new(tokio::sync::RwLock::new(AHashMap::new())),
            cancellation_token: CancellationToken::new(),
            mm_level: Arc::new(AtomicU8::new(0)),
        }
    }

    /// Creates a new Bybit private WebSocket client.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from
    /// environment variables based on the environment:
    /// - Demo: `BYBIT_DEMO_API_KEY`, `BYBIT_DEMO_API_SECRET`
    /// - Testnet: `BYBIT_TESTNET_API_KEY`, `BYBIT_TESTNET_API_SECRET`
    /// - Mainnet: `BYBIT_API_KEY`, `BYBIT_API_SECRET`
    #[must_use]
    pub fn new_private(
        environment: BybitEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        let credential = resolve_credential(environment, api_key, api_secret);

        // We don't have a handler yet; this placeholder keeps cache_instrument() working.
        // connect() swaps in the real channel and replays any queued instruments so the
        // handler sees them once it starts.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url: url.unwrap_or_else(|| bybit_ws_private_url(environment).to_string()),
            environment,
            product_type: None,
            credential,
            requires_auth: true,
            auth_tracker: AuthTracker::new(),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER),
            is_authenticated: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            funding_cache: Arc::new(tokio::sync::RwLock::new(AHashMap::new())),
            cancellation_token: CancellationToken::new(),
            mm_level: Arc::new(AtomicU8::new(0)),
        }
    }

    /// Creates a new Bybit trade WebSocket client for order operations.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from
    /// environment variables based on the environment:
    /// - Demo: `BYBIT_DEMO_API_KEY`, `BYBIT_DEMO_API_SECRET`
    /// - Testnet: `BYBIT_TESTNET_API_KEY`, `BYBIT_TESTNET_API_SECRET`
    /// - Mainnet: `BYBIT_API_KEY`, `BYBIT_API_SECRET`
    #[must_use]
    pub fn new_trade(
        environment: BybitEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        let credential = resolve_credential(environment, api_key, api_secret);

        // We don't have a handler yet; this placeholder keeps cache_instrument() working.
        // connect() swaps in the real channel and replays any queued instruments so the
        // handler sees them once it starts.
        let (cmd_tx, _) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url: url.unwrap_or_else(|| bybit_ws_trade_url(environment).to_string()),
            environment,
            product_type: None,
            credential,
            requires_auth: true,
            auth_tracker: AuthTracker::new(),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER),
            is_authenticated: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            funding_cache: Arc::new(tokio::sync::RwLock::new(AHashMap::new())),
            cancellation_token: CancellationToken::new(),
            mm_level: Arc::new(AtomicU8::new(0)),
        }
    }

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying WebSocket connection cannot be established,
    /// after retrying multiple times with exponential backoff.
    pub async fn connect(&mut self) -> BybitWsResult<()> {
        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        // No-op ping handler: handler owns the WebSocketClient and responds to pings directly
        // in the message loop for minimal latency (see handler.rs pong response)
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
        });

        let ping_msg = serde_json::to_string(&BybitSubscription {
            op: BybitWsOperation::Ping,
            args: vec![],
        })?;

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: Self::default_headers(),
            message_handler: Some(raw_handler),
            heartbeat: self.heartbeat,
            heartbeat_msg: Some(ping_msg),
            ping_handler: Some(ping_handler),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
        };

        // Retry initial connection with exponential backoff to handle transient DNS/network issues
        // TODO: Eventually expose client config options for this
        const MAX_RETRIES: u32 = 5;
        const CONNECTION_TIMEOUT_SECS: u64 = 10;

        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(500),
            Duration::from_millis(5000),
            2.0,
            250,
            false,
        )
        .map_err(|e| BybitWsError::ClientError(e.to_string()))?;

        #[allow(unused_assignments)]
        let mut last_error = String::new();
        let mut attempt = 0;
        let client = loop {
            attempt += 1;

            match tokio::time::timeout(
                Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                WebSocketClient::connect(config.clone(), None, vec![], None),
            )
            .await
            {
                Ok(Ok(client)) => {
                    if attempt > 1 {
                        tracing::info!("WebSocket connection established after {attempt} attempts");
                    }
                    break client;
                }
                Ok(Err(e)) => {
                    last_error = e.to_string();
                    tracing::warn!(
                        attempt,
                        max_retries = MAX_RETRIES,
                        url = %self.url,
                        error = %last_error,
                        "WebSocket connection attempt failed"
                    );
                }
                Err(_) => {
                    last_error = format!(
                        "Connection timeout after {CONNECTION_TIMEOUT_SECS}s (possible DNS resolution failure)"
                    );
                    tracing::warn!(
                        attempt,
                        max_retries = MAX_RETRIES,
                        url = %self.url,
                        "WebSocket connection attempt timed out"
                    );
                }
            }

            if attempt >= MAX_RETRIES {
                return Err(BybitWsError::Transport(format!(
                    "Failed to connect to {} after {MAX_RETRIES} attempts: {}. \
                    If this is a DNS error, check your network configuration and DNS settings.",
                    self.url,
                    if last_error.is_empty() {
                        "unknown error"
                    } else {
                        &last_error
                    }
                )));
            }

            let delay = backoff.next_duration();
            tracing::debug!(
                "Retrying in {delay:?} (attempt {}/{MAX_RETRIES})",
                attempt + 1
            );
            tokio::time::sleep(delay).await;
        };

        self.connection_mode.store(client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        let cmd = HandlerCommand::SetClient(client);

        self.send_cmd(cmd).await?;

        // Replay cached instruments to the new handler via the new channel
        if !self.instruments_cache.is_empty() {
            let cached_instruments: Vec<InstrumentAny> = self
                .instruments_cache
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            let cmd = HandlerCommand::InitializeInstruments(cached_instruments);
            self.send_cmd(cmd).await?;
        }

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let credential = self.credential.clone();
        let requires_auth = self.requires_auth;
        let funding_cache = Arc::clone(&self.funding_cache);
        let account_id = self.account_id;
        let product_type = self.product_type;
        let mm_level = Arc::clone(&self.mm_level);
        let cmd_tx_for_reconnect = cmd_tx.clone();
        let auth_tracker = self.auth_tracker.clone();
        let is_authenticated = Arc::clone(&self.is_authenticated);

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                out_tx.clone(),
                account_id,
                product_type,
                mm_level.clone(),
                auth_tracker,
                subscriptions.clone(),
                funding_cache.clone(),
            );

            // Helper closure to resubscribe all tracked subscriptions after reconnection
            let resubscribe_all = || async {
                let topics = subscriptions.all_topics();

                if topics.is_empty() {
                    return;
                }

                tracing::debug!(count = topics.len(), "Resubscribing to confirmed subscriptions");

                for topic in &topics {
                    subscriptions.mark_subscribe(topic.as_str());
                }

                let mut payloads = Vec::with_capacity(topics.len());
                for topic in &topics {
                    let message = BybitSubscription {
                        op: BybitWsOperation::Subscribe,
                        args: vec![topic.clone()],
                    };
                    if let Ok(payload) = serde_json::to_string(&message) {
                        payloads.push(payload);
                    }
                }

                let cmd = HandlerCommand::Subscribe { topics: payloads };

                if let Err(e) = cmd_tx_for_reconnect.send(cmd) {
                    tracing::error!("Failed to send resubscribe command: {e}");
                }
            };

            // Run message processing with reconnection handling
            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }

                        tracing::info!("WebSocket reconnected");

                        // Mark all confirmed subscriptions as failed so they transition to pending state
                        let confirmed_topics: Vec<String> = {
                            let confirmed = subscriptions.confirmed();
                            let mut topics = Vec::new();
                            for entry in confirmed.iter() {
                                let (channel, symbols) = entry.pair();
                                for symbol in symbols {
                                    if symbol.is_empty() {
                                        topics.push(channel.to_string());
                                    } else {
                                        topics.push(format!("{channel}.{symbol}"));
                                    }
                                }
                            }
                            topics
                        };

                        if !confirmed_topics.is_empty() {
                            tracing::debug!(count = confirmed_topics.len(), "Marking confirmed subscriptions as pending for replay");
                            for topic in confirmed_topics {
                                subscriptions.mark_failure(&topic);
                            }
                        }

                        // Clear caches to prevent stale data after reconnection
                        funding_cache.write().await.clear();

                        if requires_auth {
                            is_authenticated.store(false, Ordering::Relaxed);
                            tracing::debug!("Re-authenticating after reconnection");

                            if let Some(cred) = &credential {
                                let expires = chrono::Utc::now().timestamp_millis() + WEBSOCKET_AUTH_WINDOW_MS;
                                let signature = cred.sign_websocket_auth(expires);

                                let auth_message = BybitAuthRequest {
                                    op: BybitWsOperation::Auth,
                                    args: vec![
                                        Value::String(cred.api_key().to_string()),
                                        Value::Number(expires.into()),
                                        Value::String(signature),
                                    ],
                                };

                                if let Ok(payload) = serde_json::to_string(&auth_message) {
                                    let cmd = HandlerCommand::Authenticate { payload };
                                    if let Err(e) = cmd_tx_for_reconnect.send(cmd) {
                                        tracing::error!(error = %e, "Failed to send reconnection auth command");
                                    }
                                } else {
                                    tracing::error!("Failed to serialize reconnection auth message");
                                }
                            }
                        }

                        // Unauthenticated sessions resubscribe immediately after reconnection,
                        // authenticated sessions wait for Authenticated message
                        if !requires_auth {
                            tracing::debug!("No authentication required, resubscribing immediately");
                            resubscribe_all().await;
                        }

                        // Forward to out_tx so caller sees the Reconnected message
                        if out_tx.send(NautilusWsMessage::Reconnected).is_err() {
                            tracing::debug!("Receiver dropped, stopping");
                            break;
                        }
                        continue;
                    }
                    Some(NautilusWsMessage::Authenticated) => {
                        tracing::debug!("Authenticated, resubscribing");
                        is_authenticated.store(true, Ordering::Relaxed);
                        resubscribe_all().await;
                        continue;
                    }
                    Some(msg) => {
                        if out_tx.send(msg).is_err() {
                            tracing::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        // Stream ended - check if it's a stop signal
                        if handler.is_stopped() {
                            tracing::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        // Otherwise it's an unexpected stream end
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            tracing::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        if requires_auth && let Err(e) = self.authenticate_if_required().await {
            return Err(e);
        }

        Ok(())
    }

    /// Disconnects the WebSocket client and stops the background task.
    pub async fn close(&mut self) -> BybitWsResult<()> {
        tracing::debug!("Starting close process");

        self.signal.store(true, Ordering::Relaxed);

        let cmd = HandlerCommand::Disconnect;
        if let Err(e) = self.cmd_tx.read().await.send(cmd) {
            tracing::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    tracing::debug!("Waiting for task handle to complete");
                    match tokio::time::timeout(Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => tracing::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => tracing::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            tracing::warn!(
                                "Timeout waiting for task handle, task may still be running"
                            );
                            // The task will be dropped and should clean up automatically
                        }
                    }
                }
                Err(arc_handle) => {
                    tracing::debug!(
                        "Cannot take ownership of task handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            tracing::debug!("No task handle to await");
        }

        self.is_authenticated.store(false, Ordering::Relaxed);

        tracing::debug!("Closed");

        Ok(())
    }

    /// Returns a value indicating whether the client is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Relaxed)
    }

    /// Returns a value indicating whether the client is closed.
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Relaxed)
    }

    /// Waits until the WebSocket client becomes active or times out.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeout is exceeded before the client becomes active.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> BybitWsResult<()> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            BybitWsError::ClientError(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
    }

    /// Subscribe to the provided topic strings.
    pub async fn subscribe(&self, topics: Vec<String>) -> BybitWsResult<()> {
        if topics.is_empty() {
            return Ok(());
        }

        tracing::debug!("Subscribing to topics: {topics:?}");

        // Use reference counting to deduplicate subscriptions
        let mut topics_to_send = Vec::new();

        for topic in topics {
            // Returns true if this is the first subscription (ref count 0 -> 1)
            if self.subscriptions.add_reference(&topic) {
                self.subscriptions.mark_subscribe(&topic);
                topics_to_send.push(topic.clone());
            } else {
                tracing::debug!("Already subscribed to {topic}, skipping duplicate subscription");
            }
        }

        if topics_to_send.is_empty() {
            return Ok(());
        }

        // Serialize subscription messages
        let mut payloads = Vec::with_capacity(topics_to_send.len());
        for topic in &topics_to_send {
            let message = BybitSubscription {
                op: BybitWsOperation::Subscribe,
                args: vec![topic.clone()],
            };
            let payload = serde_json::to_string(&message).map_err(|e| {
                BybitWsError::Json(format!("Failed to serialize subscription: {e}"))
            })?;
            payloads.push(payload);
        }

        let cmd = HandlerCommand::Subscribe { topics: payloads };
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| BybitWsError::Send(format!("Failed to send subscribe command: {e}")))?;

        Ok(())
    }

    /// Unsubscribe from the provided topics.
    pub async fn unsubscribe(&self, topics: Vec<String>) -> BybitWsResult<()> {
        if topics.is_empty() {
            return Ok(());
        }

        tracing::debug!("Attempting to unsubscribe from topics: {topics:?}");

        if self.signal.load(Ordering::Relaxed) {
            tracing::debug!("Shutdown signal detected, skipping unsubscribe");
            return Ok(());
        }

        // Use reference counting to avoid unsubscribing while other consumers still need the topic
        let mut topics_to_send = Vec::new();

        for topic in topics {
            // Returns true if this was the last subscription (ref count 1 -> 0)
            if self.subscriptions.remove_reference(&topic) {
                self.subscriptions.mark_unsubscribe(&topic);
                topics_to_send.push(topic.clone());
            } else {
                tracing::debug!("Topic {topic} still has active subscriptions, not unsubscribing");
            }
        }

        if topics_to_send.is_empty() {
            return Ok(());
        }

        // Serialize unsubscription messages
        let mut payloads = Vec::with_capacity(topics_to_send.len());
        for topic in &topics_to_send {
            let message = BybitSubscription {
                op: BybitWsOperation::Unsubscribe,
                args: vec![topic.clone()],
            };
            if let Ok(payload) = serde_json::to_string(&message) {
                payloads.push(payload);
            }
        }

        let cmd = HandlerCommand::Unsubscribe { topics: payloads };
        if let Err(e) = self.cmd_tx.read().await.send(cmd) {
            tracing::debug!(error = %e, "Failed to send unsubscribe command");
        }

        Ok(())
    }

    /// Returns a stream of parsed [`NautilusWsMessage`] items.
    ///
    /// # Panics
    ///
    /// Panics if called before [`Self::connect`] or if the stream has already been taken.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = NautilusWsMessage> + use<> {
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

    /// Returns the number of currently registered subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns the credential associated with this client, if any.
    #[must_use]
    pub fn credential(&self) -> Option<&Credential> {
        self.credential.as_ref()
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same ID will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument.clone());

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let cmd = HandlerCommand::UpdateInstrument(instrument);
            if let Err(e) = cmd_tx.send(cmd) {
                tracing::debug!("Failed to send instrument update to handler: {e}");
            }
        }
    }

    /// Caches multiple instruments.
    ///
    /// Clears the existing cache first, then adds all provided instruments.
    pub fn cache_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        self.instruments_cache.clear();
        let mut count = 0;

        tracing::debug!("Initializing Bybit instrument cache");

        for inst in instruments {
            let symbol = inst.symbol().inner();
            self.instruments_cache.insert(symbol, inst.clone());
            tracing::debug!("Cached instrument: {symbol}");
            count += 1;
        }

        tracing::info!("Bybit instrument cache initialized with {count} instruments");
    }

    /// Sets the account ID for account message parsing.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Sets the account market maker level.
    pub fn set_mm_level(&self, mm_level: u8) {
        self.mm_level.store(mm_level, Ordering::Relaxed);
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments_cache
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    /// Returns the product type for public connections.
    #[must_use]
    pub fn product_type(&self) -> Option<BybitProductType> {
        self.product_type
    }

    /// Subscribes to orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/orderbook>
    pub async fn subscribe_orderbook(
        &self,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{depth}.{raw_symbol}",
            BybitWsPublicChannel::OrderBook.as_ref()
        );
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    pub async fn unsubscribe_orderbook(
        &self,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{depth}.{raw_symbol}",
            BybitWsPublicChannel::OrderBook.as_ref()
        );
        self.unsubscribe(vec![topic]).await
    }

    /// Subscribes to public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/trade>
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{raw_symbol}",
            BybitWsPublicChannel::PublicTrade.as_ref()
        );
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{raw_symbol}",
            BybitWsPublicChannel::PublicTrade.as_ref()
        );
        self.unsubscribe(vec![topic]).await
    }

    /// Subscribes to ticker updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/ticker>
    pub async fn subscribe_ticker(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{}.{raw_symbol}", BybitWsPublicChannel::Tickers.as_ref());
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from ticker updates for a specific instrument.
    pub async fn unsubscribe_ticker(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{}.{raw_symbol}", BybitWsPublicChannel::Tickers.as_ref());

        // Clear funding rate cache to ensure fresh data on resubscribe
        let symbol = self.product_type.map_or_else(
            || instrument_id.symbol.inner(),
            |pt| make_bybit_symbol(raw_symbol, pt),
        );
        self.funding_cache.write().await.remove(&symbol);

        self.unsubscribe(vec![topic]).await
    }

    /// Subscribes to kline/candlestick updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/public/kline>
    pub async fn subscribe_klines(
        &self,
        instrument_id: InstrumentId,
        interval: impl Into<String>,
    ) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{}.{raw_symbol}",
            BybitWsPublicChannel::Kline.as_ref(),
            interval.into()
        );
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from kline/candlestick updates for a specific instrument.
    pub async fn unsubscribe_klines(
        &self,
        instrument_id: InstrumentId,
        interval: impl Into<String>,
    ) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{}.{raw_symbol}",
            BybitWsPublicChannel::Kline.as_ref(),
            interval.into()
        );
        self.unsubscribe(vec![topic]).await
    }

    /// Subscribes to order updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/order>
    pub async fn subscribe_orders(&self) -> BybitWsResult<()> {
        if !self.requires_auth {
            return Err(BybitWsError::Authentication(
                "Order subscription requires authentication".to_string(),
            ));
        }
        self.subscribe(vec![BybitWsPrivateChannel::Order.as_ref().to_string()])
            .await
    }

    /// Unsubscribes from order updates.
    pub async fn unsubscribe_orders(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec![BybitWsPrivateChannel::Order.as_ref().to_string()])
            .await
    }

    /// Subscribes to execution/fill updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/execution>
    pub async fn subscribe_executions(&self) -> BybitWsResult<()> {
        if !self.requires_auth {
            return Err(BybitWsError::Authentication(
                "Execution subscription requires authentication".to_string(),
            ));
        }
        self.subscribe(vec![BybitWsPrivateChannel::Execution.as_ref().to_string()])
            .await
    }

    /// Unsubscribes from execution/fill updates.
    pub async fn unsubscribe_executions(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec![BybitWsPrivateChannel::Execution.as_ref().to_string()])
            .await
    }

    /// Subscribes to position updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/position>
    pub async fn subscribe_positions(&self) -> BybitWsResult<()> {
        if !self.requires_auth {
            return Err(BybitWsError::Authentication(
                "Position subscription requires authentication".to_string(),
            ));
        }
        self.subscribe(vec![BybitWsPrivateChannel::Position.as_ref().to_string()])
            .await
    }

    /// Unsubscribes from position updates.
    pub async fn unsubscribe_positions(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec![BybitWsPrivateChannel::Position.as_ref().to_string()])
            .await
    }

    /// Subscribes to wallet/balance updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/private/wallet>
    pub async fn subscribe_wallet(&self) -> BybitWsResult<()> {
        if !self.requires_auth {
            return Err(BybitWsError::Authentication(
                "Wallet subscription requires authentication".to_string(),
            ));
        }
        self.subscribe(vec![BybitWsPrivateChannel::Wallet.as_ref().to_string()])
            .await
    }

    /// Unsubscribes from wallet/balance updates.
    pub async fn unsubscribe_wallet(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec![BybitWsPrivateChannel::Wallet.as_ref().to_string()])
            .await
    }

    /// Places an order via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the order request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/trade/guideline>
    pub async fn place_order(
        &self,
        params: BybitWsPlaceOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to place orders".to_string(),
            ));
        }

        let cmd = HandlerCommand::PlaceOrder {
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
        };

        self.send_cmd(cmd).await
    }

    /// Amends an existing order via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the amend request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/trade/guideline>
    pub async fn amend_order(
        &self,
        params: BybitWsAmendOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to amend orders".to_string(),
            ));
        }

        let cmd = HandlerCommand::AmendOrder {
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
            venue_order_id,
        };

        self.send_cmd(cmd).await
    }

    /// Cancels an order via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the cancel request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/trade/guideline>
    pub async fn cancel_order(
        &self,
        params: BybitWsCancelOrderParams,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to cancel orders".to_string(),
            ));
        }

        let cmd = HandlerCommand::CancelOrder {
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
            venue_order_id,
        };

        self.send_cmd(cmd).await
    }

    /// Batch creates multiple orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/websocket/trade/guideline>
    pub async fn batch_place_orders(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to place orders".to_string(),
            ));
        }

        if orders.is_empty() {
            tracing::warn!("Batch place orders called with empty orders list");
            return Ok(());
        }

        for chunk in orders.chunks(BATCH_PROCESSING_LIMIT) {
            self.batch_place_orders_chunk(trader_id, strategy_id, chunk.to_vec())
                .await?;
        }

        Ok(())
    }

    async fn batch_place_orders_chunk(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> BybitWsResult<()> {
        let category = orders[0].category;
        let batch_req_id = UUID4::new().to_string();

        // Extract order tracking data before consuming orders to register with handler
        let mut batch_order_data = Vec::new();
        for order in &orders {
            if let Some(order_link_id_str) = &order.order_link_id {
                let client_order_id = ClientOrderId::from(order_link_id_str.as_str());
                let cache_key = make_bybit_symbol(order.symbol.as_str(), category);
                let instrument_id = self
                    .instruments_cache
                    .get(&cache_key)
                    .map(|inst| inst.id())
                    .ok_or_else(|| {
                        BybitWsError::ClientError(format!(
                            "Instrument {cache_key} not found in cache"
                        ))
                    })?;
                batch_order_data.push((
                    client_order_id,
                    (client_order_id, trader_id, strategy_id, instrument_id),
                ));
            }
        }

        if !batch_order_data.is_empty() {
            let cmd = HandlerCommand::RegisterBatchPlace {
                req_id: batch_req_id.clone(),
                orders: batch_order_data,
            };
            let cmd_tx = self.cmd_tx.read().await;
            if let Err(e) = cmd_tx.send(cmd) {
                tracing::error!("Failed to send RegisterBatchPlace command: {e}");
            }
        }

        let mm_level = self.mm_level.load(Ordering::Relaxed);
        let has_non_post_only = orders
            .iter()
            .any(|o| !matches!(o.time_in_force, Some(BybitTimeInForce::PostOnly)));
        let referer = if has_non_post_only || mm_level == 0 {
            Some(BYBIT_NAUTILUS_BROKER_ID.to_string())
        } else {
            None
        };

        let request_items: Vec<BybitWsBatchPlaceItem> = orders
            .into_iter()
            .map(|order| BybitWsBatchPlaceItem {
                symbol: order.symbol,
                side: order.side,
                order_type: order.order_type,
                qty: order.qty,
                is_leverage: order.is_leverage,
                market_unit: order.market_unit,
                price: order.price,
                time_in_force: order.time_in_force,
                order_link_id: order.order_link_id,
                reduce_only: order.reduce_only,
                close_on_trigger: order.close_on_trigger,
                trigger_price: order.trigger_price,
                trigger_by: order.trigger_by,
                trigger_direction: order.trigger_direction,
                tpsl_mode: order.tpsl_mode,
                take_profit: order.take_profit,
                stop_loss: order.stop_loss,
                tp_trigger_by: order.tp_trigger_by,
                sl_trigger_by: order.sl_trigger_by,
                sl_trigger_price: order.sl_trigger_price,
                tp_trigger_price: order.tp_trigger_price,
                sl_order_type: order.sl_order_type,
                tp_order_type: order.tp_order_type,
                sl_limit_price: order.sl_limit_price,
                tp_limit_price: order.tp_limit_price,
            })
            .collect();

        let args = BybitWsBatchPlaceOrderArgs {
            category,
            request: request_items,
        };

        let request = BybitWsRequest {
            req_id: Some(batch_req_id),
            op: BybitWsOrderRequestOp::CreateBatch,
            header: BybitWsHeader::with_referer(referer),
            args: vec![args],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;

        self.send_text(&payload).await
    }

    /// Batch amends multiple orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_amend_orders(
        &self,
        #[allow(unused_variables)] trader_id: TraderId,
        #[allow(unused_variables)] strategy_id: StrategyId,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to amend orders".to_string(),
            ));
        }

        if orders.is_empty() {
            tracing::warn!("Batch amend orders called with empty orders list");
            return Ok(());
        }

        for chunk in orders.chunks(BATCH_PROCESSING_LIMIT) {
            self.batch_amend_orders_chunk(trader_id, strategy_id, chunk.to_vec())
                .await?;
        }

        Ok(())
    }

    async fn batch_amend_orders_chunk(
        &self,
        #[allow(unused_variables)] trader_id: TraderId,
        #[allow(unused_variables)] strategy_id: StrategyId,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> BybitWsResult<()> {
        let request = BybitWsRequest {
            req_id: None,
            op: BybitWsOrderRequestOp::AmendBatch,
            header: BybitWsHeader::now(),
            args: orders,
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;

        self.send_text(&payload).await
    }

    /// Batch cancels multiple orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_cancel_orders(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsCancelOrderParams>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to cancel orders".to_string(),
            ));
        }

        if orders.is_empty() {
            tracing::warn!("Batch cancel orders called with empty orders list");
            return Ok(());
        }

        for chunk in orders.chunks(BATCH_PROCESSING_LIMIT) {
            self.batch_cancel_orders_chunk(trader_id, strategy_id, chunk.to_vec())
                .await?;
        }

        Ok(())
    }

    async fn batch_cancel_orders_chunk(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<BybitWsCancelOrderParams>,
    ) -> BybitWsResult<()> {
        if orders.is_empty() {
            return Ok(());
        }

        let category = orders[0].category;
        let batch_req_id = UUID4::new().to_string();

        let mut validated_data = Vec::new();

        for order in &orders {
            if let Some(order_link_id_str) = &order.order_link_id {
                let cache_key = make_bybit_symbol(order.symbol.as_str(), category);
                let instrument_id = self
                    .instruments_cache
                    .get(&cache_key)
                    .map(|inst| inst.id())
                    .ok_or_else(|| {
                        BybitWsError::ClientError(format!(
                            "Instrument {cache_key} not found in cache"
                        ))
                    })?;

                let venue_order_id = order
                    .order_id
                    .as_ref()
                    .map(|id| VenueOrderId::from(id.as_str()));

                validated_data.push((order_link_id_str.clone(), instrument_id, venue_order_id));
            }
        }

        let batch_cancel_data: Vec<_> = validated_data
            .iter()
            .map(|(order_link_id_str, instrument_id, venue_order_id)| {
                let client_order_id = ClientOrderId::from(order_link_id_str.as_str());
                (
                    client_order_id,
                    (
                        client_order_id,
                        trader_id,
                        strategy_id,
                        *instrument_id,
                        *venue_order_id,
                    ),
                )
            })
            .collect();

        if !batch_cancel_data.is_empty() {
            let cmd = HandlerCommand::RegisterBatchCancel {
                req_id: batch_req_id.clone(),
                cancels: batch_cancel_data,
            };
            let cmd_tx = self.cmd_tx.read().await;
            if let Err(e) = cmd_tx.send(cmd) {
                tracing::error!("Failed to send RegisterBatchCancel command: {e}");
            }
        }

        let request_items: Vec<BybitWsBatchCancelItem> = orders
            .into_iter()
            .map(|order| BybitWsBatchCancelItem {
                symbol: order.symbol,
                order_id: order.order_id,
                order_link_id: order.order_link_id,
            })
            .collect();

        let args = BybitWsBatchCancelOrderArgs {
            category,
            request: request_items,
        };

        let request = BybitWsRequest {
            req_id: Some(batch_req_id),
            op: BybitWsOrderRequestOp::CancelBatch,
            header: BybitWsHeader::now(),
            args: vec![args],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;

        self.send_text(&payload).await
    }

    /// Submits an order using Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if order submission fails or if not authenticated.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        is_quote_quantity: bool,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
    ) -> BybitWsResult<()> {
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        let bybit_side = match order_side {
            OrderSide::Buy => BybitOrderSide::Buy,
            OrderSide::Sell => BybitOrderSide::Sell,
            _ => {
                return Err(BybitWsError::ClientError(format!(
                    "Invalid order side: {order_side:?}"
                )));
            }
        };

        // For stop/conditional orders, Bybit uses Market/Limit with trigger parameters
        let (bybit_order_type, is_stop_order) = match order_type {
            OrderType::Market => (BybitOrderType::Market, false),
            OrderType::Limit => (BybitOrderType::Limit, false),
            OrderType::StopMarket | OrderType::MarketIfTouched => (BybitOrderType::Market, true),
            OrderType::StopLimit | OrderType::LimitIfTouched => (BybitOrderType::Limit, true),
            _ => {
                return Err(BybitWsError::ClientError(format!(
                    "Unsupported order type: {order_type:?}"
                )));
            }
        };

        let bybit_tif = if bybit_order_type == BybitOrderType::Market {
            None
        } else if post_only == Some(true) {
            Some(BybitTimeInForce::PostOnly)
        } else if let Some(tif) = time_in_force {
            Some(match tif {
                TimeInForce::Gtc => BybitTimeInForce::Gtc,
                TimeInForce::Ioc => BybitTimeInForce::Ioc,
                TimeInForce::Fok => BybitTimeInForce::Fok,
                _ => {
                    return Err(BybitWsError::ClientError(format!(
                        "Unsupported time in force: {tif:?}"
                    )));
                }
            })
        } else {
            None
        };

        // For SPOT market orders, specify baseCoin to interpret qty as base currency.
        // This ensures Nautilus quantities (always in base currency) are interpreted correctly.
        let market_unit = if product_type == BybitProductType::Spot
            && bybit_order_type == BybitOrderType::Market
        {
            if is_quote_quantity {
                Some(BYBIT_QUOTE_COIN.to_string())
            } else {
                Some(BYBIT_BASE_COIN.to_string())
            }
        } else {
            None
        };

        // Only SPOT products support is_leverage parameter
        let is_leverage_value = if product_type == BybitProductType::Spot {
            Some(i32::from(is_leverage))
        } else {
            None
        };

        // Stop semantics: Buy stops trigger on rise (breakout), sell stops trigger on fall (breakdown)
        // MIT semantics: Buy MIT triggers on fall (pullback entry), sell MIT triggers on rise (rally entry)
        let trigger_direction = if is_stop_order {
            match (order_type, order_side) {
                (OrderType::StopMarket | OrderType::StopLimit, OrderSide::Buy) => {
                    Some(BybitTriggerDirection::RisesTo as i32)
                }
                (OrderType::StopMarket | OrderType::StopLimit, OrderSide::Sell) => {
                    Some(BybitTriggerDirection::FallsTo as i32)
                }
                (OrderType::MarketIfTouched | OrderType::LimitIfTouched, OrderSide::Buy) => {
                    Some(BybitTriggerDirection::FallsTo as i32)
                }
                (OrderType::MarketIfTouched | OrderType::LimitIfTouched, OrderSide::Sell) => {
                    Some(BybitTriggerDirection::RisesTo as i32)
                }
                _ => None,
            }
        } else {
            None
        };

        let params = if is_stop_order {
            // For conditional orders, ALL types use triggerPrice field
            // sl_trigger_price/tp_trigger_price are only for TP/SL attached to regular orders
            BybitWsPlaceOrderParams {
                category: product_type,
                symbol: raw_symbol,
                side: bybit_side,
                order_type: bybit_order_type,
                qty: quantity.to_string(),
                is_leverage: is_leverage_value,
                market_unit: market_unit.clone(),
                price: price.map(|p| p.to_string()),
                time_in_force: bybit_tif,
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: trigger_price.map(|p| p.to_string()),
                trigger_by: Some(BybitTriggerType::LastPrice),
                trigger_direction,
                tpsl_mode: None, // Not needed for standalone conditional orders
                take_profit: None,
                stop_loss: None,
                tp_trigger_by: None,
                sl_trigger_by: None,
                sl_trigger_price: None, // Not used for standalone stop orders
                tp_trigger_price: None, // Not used for standalone stop orders
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
            }
        } else {
            // Regular market/limit orders
            BybitWsPlaceOrderParams {
                category: product_type,
                symbol: raw_symbol,
                side: bybit_side,
                order_type: bybit_order_type,
                qty: quantity.to_string(),
                is_leverage: is_leverage_value,
                market_unit,
                price: price.map(|p| p.to_string()),
                time_in_force: if bybit_order_type == BybitOrderType::Market {
                    None
                } else {
                    bybit_tif
                },
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: None,
                trigger_by: None,
                trigger_direction: None,
                tpsl_mode: None,
                take_profit: None,
                stop_loss: None,
                tp_trigger_by: None,
                sl_trigger_by: None,
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
            }
        };

        self.place_order(
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
        )
        .await
    }

    /// Modifies an existing order using Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if modification fails or if not authenticated.
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> BybitWsResult<()> {
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        let params = BybitWsAmendOrderParams {
            category: product_type,
            symbol: raw_symbol,
            order_id: venue_order_id.map(|id| id.to_string()),
            order_link_id: Some(client_order_id.to_string()),
            qty: quantity.map(|q| q.to_string()),
            price: price.map(|p| p.to_string()),
            trigger_price: None,
            take_profit: None,
            stop_loss: None,
            tp_trigger_by: None,
            sl_trigger_by: None,
        };

        self.amend_order(
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
            venue_order_id,
        )
        .await
    }

    /// Cancels an order using Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails or if not authenticated.
    pub async fn cancel_order_by_id(
        &self,
        product_type: BybitProductType,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> BybitWsResult<()> {
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        let params = BybitWsCancelOrderParams {
            category: product_type,
            symbol: raw_symbol,
            order_id: venue_order_id.map(|id| id.to_string()),
            order_link_id: Some(client_order_id.to_string()),
        };

        self.cancel_order(
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
            venue_order_id,
        )
        .await
    }

    /// Builds order params for placing an order.
    #[allow(clippy::too_many_arguments)]
    pub fn build_place_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        is_quote_quantity: bool,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
    ) -> BybitWsResult<BybitWsPlaceOrderParams> {
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        let bybit_side = match order_side {
            OrderSide::Buy => BybitOrderSide::Buy,
            OrderSide::Sell => BybitOrderSide::Sell,
            _ => {
                return Err(BybitWsError::ClientError(format!(
                    "Invalid order side: {order_side:?}"
                )));
            }
        };

        let (bybit_order_type, is_stop_order) = match order_type {
            OrderType::Market => (BybitOrderType::Market, false),
            OrderType::Limit => (BybitOrderType::Limit, false),
            OrderType::StopMarket | OrderType::MarketIfTouched => (BybitOrderType::Market, true),
            OrderType::StopLimit | OrderType::LimitIfTouched => (BybitOrderType::Limit, true),
            _ => {
                return Err(BybitWsError::ClientError(format!(
                    "Unsupported order type: {order_type:?}"
                )));
            }
        };

        let bybit_tif = if post_only == Some(true) {
            Some(BybitTimeInForce::PostOnly)
        } else if let Some(tif) = time_in_force {
            Some(match tif {
                TimeInForce::Gtc => BybitTimeInForce::Gtc,
                TimeInForce::Ioc => BybitTimeInForce::Ioc,
                TimeInForce::Fok => BybitTimeInForce::Fok,
                _ => {
                    return Err(BybitWsError::ClientError(format!(
                        "Unsupported time in force: {tif:?}"
                    )));
                }
            })
        } else {
            None
        };

        let market_unit = if product_type == BybitProductType::Spot
            && bybit_order_type == BybitOrderType::Market
        {
            if is_quote_quantity {
                Some(BYBIT_QUOTE_COIN.to_string())
            } else {
                Some(BYBIT_BASE_COIN.to_string())
            }
        } else {
            None
        };

        // Only SPOT products support is_leverage parameter
        let is_leverage_value = if product_type == BybitProductType::Spot {
            Some(i32::from(is_leverage))
        } else {
            None
        };

        // Stop semantics: Buy stops trigger on rise (breakout), sell stops trigger on fall (breakdown)
        // MIT semantics: Buy MIT triggers on fall (pullback entry), sell MIT triggers on rise (rally entry)
        let trigger_direction = if is_stop_order {
            match (order_type, order_side) {
                (OrderType::StopMarket | OrderType::StopLimit, OrderSide::Buy) => {
                    Some(BybitTriggerDirection::RisesTo as i32)
                }
                (OrderType::StopMarket | OrderType::StopLimit, OrderSide::Sell) => {
                    Some(BybitTriggerDirection::FallsTo as i32)
                }
                (OrderType::MarketIfTouched | OrderType::LimitIfTouched, OrderSide::Buy) => {
                    Some(BybitTriggerDirection::FallsTo as i32)
                }
                (OrderType::MarketIfTouched | OrderType::LimitIfTouched, OrderSide::Sell) => {
                    Some(BybitTriggerDirection::RisesTo as i32)
                }
                _ => None,
            }
        } else {
            None
        };

        let params = if is_stop_order {
            BybitWsPlaceOrderParams {
                category: product_type,
                symbol: raw_symbol,
                side: bybit_side,
                order_type: bybit_order_type,
                qty: quantity.to_string(),
                is_leverage: is_leverage_value,
                market_unit,
                price: price.map(|p| p.to_string()),
                time_in_force: if bybit_order_type == BybitOrderType::Market {
                    None
                } else {
                    bybit_tif
                },
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: trigger_price.map(|p| p.to_string()),
                trigger_by: Some(BybitTriggerType::LastPrice),
                trigger_direction,
                tpsl_mode: None,
                take_profit: None,
                stop_loss: None,
                tp_trigger_by: None,
                sl_trigger_by: None,
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
            }
        } else {
            BybitWsPlaceOrderParams {
                category: product_type,
                symbol: raw_symbol,
                side: bybit_side,
                order_type: bybit_order_type,
                qty: quantity.to_string(),
                is_leverage: is_leverage_value,
                market_unit,
                price: price.map(|p| p.to_string()),
                time_in_force: if bybit_order_type == BybitOrderType::Market {
                    None
                } else {
                    bybit_tif
                },
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: None,
                trigger_by: None,
                trigger_direction: None,
                tpsl_mode: None,
                take_profit: None,
                stop_loss: None,
                tp_trigger_by: None,
                sl_trigger_by: None,
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
            }
        };

        Ok(params)
    }

    /// Builds order params for amending an order.
    #[allow(clippy::too_many_arguments)]
    pub fn build_amend_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> BybitWsResult<BybitWsAmendOrderParams> {
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        Ok(BybitWsAmendOrderParams {
            category: product_type,
            symbol: raw_symbol,
            order_id: venue_order_id.map(|v| v.to_string()),
            order_link_id: client_order_id.map(|c| c.to_string()),
            qty: quantity.map(|q| q.to_string()),
            price: price.map(|p| p.to_string()),
            trigger_price: None,
            take_profit: None,
            stop_loss: None,
            tp_trigger_by: None,
            sl_trigger_by: None,
        })
    }

    /// Builds order params for canceling an order via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if symbol parsing fails or if neither venue_order_id
    /// nor client_order_id is provided.
    pub fn build_cancel_order_params(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> BybitWsResult<BybitWsCancelOrderParams> {
        if venue_order_id.is_none() && client_order_id.is_none() {
            return Err(BybitWsError::ClientError(
                "Either venue_order_id or client_order_id must be provided".to_string(),
            ));
        }

        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        Ok(BybitWsCancelOrderParams {
            category: product_type,
            symbol: raw_symbol,
            order_id: venue_order_id.map(|v| v.to_string()),
            order_link_id: client_order_id.map(|c| c.to_string()),
        })
    }

    fn default_headers() -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
        ]
    }

    async fn authenticate_if_required(&self) -> BybitWsResult<()> {
        if !self.requires_auth {
            return Ok(());
        }

        let credential = self.credential.as_ref().ok_or_else(|| {
            BybitWsError::Authentication("Credentials required for authentication".to_string())
        })?;

        let expires = chrono::Utc::now().timestamp_millis() + WEBSOCKET_AUTH_WINDOW_MS;
        let signature = credential.sign_websocket_auth(expires);

        let auth_message = BybitAuthRequest {
            op: BybitWsOperation::Auth,
            args: vec![
                Value::String(credential.api_key().to_string()),
                Value::Number(expires.into()),
                Value::String(signature),
            ],
        };

        let payload = serde_json::to_string(&auth_message)?;

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Authenticate { payload })
            .map_err(|e| BybitWsError::Send(format!("Failed to send auth command: {e}")))?;

        // Authentication will be processed asynchronously by the handler
        // The handler will emit NautilusWsMessage::Authenticated when successful
        Ok(())
    }

    async fn send_text(&self, text: &str) -> BybitWsResult<()> {
        let cmd = HandlerCommand::SendText {
            payload: text.to_string(),
        };

        self.send_cmd(cmd).await
    }

    async fn send_cmd(&self, cmd: HandlerCommand) -> BybitWsResult<()> {
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| BybitWsError::Send(e.to_string()))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::testing::load_test_json,
        websocket::{handler::FeedHandler, messages::BybitWsMessage},
    };

    #[rstest]
    fn classify_orderbook_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json"))
            .expect("invalid fixture");
        let message =
            FeedHandler::classify_bybit_message(&json).expect("expected orderbook message");
        assert!(matches!(message, BybitWsMessage::Orderbook(_)));
    }

    #[rstest]
    fn classify_trade_snapshot() {
        let json: Value =
            serde_json::from_str(&load_test_json("ws_public_trade.json")).expect("invalid fixture");
        let message = FeedHandler::classify_bybit_message(&json).expect("expected trade message");
        assert!(matches!(message, BybitWsMessage::Trade(_)));
    }

    #[rstest]
    fn classify_ticker_linear_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_linear.json"))
            .expect("invalid fixture");
        let message = FeedHandler::classify_bybit_message(&json).expect("expected ticker message");
        assert!(matches!(message, BybitWsMessage::TickerLinear(_)));
    }

    #[rstest]
    fn classify_ticker_option_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_option.json"))
            .expect("invalid fixture");
        let message = FeedHandler::classify_bybit_message(&json).expect("expected ticker message");
        assert!(matches!(message, BybitWsMessage::TickerOption(_)));
    }

    #[rstest]
    fn test_race_unsubscribe_failure_recovery() {
        // Simulates the race condition where venue rejects an unsubscribe request.
        // The adapter must perform the 3-step recovery:
        // 1. confirm_unsubscribe() - clear pending_unsubscribe
        // 2. mark_subscribe() - mark as subscribing again
        // 3. confirm_subscribe() - restore to confirmed state
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER); // Bybit uses dot delimiter

        let topic = "publicTrade.BTCUSDT";

        // Initial subscribe flow
        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        // User unsubscribes
        subscriptions.mark_unsubscribe(topic);
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        // Venue REJECTS the unsubscribe (error message)
        // Adapter must perform 3-step recovery (from lines 2181-2183)
        subscriptions.confirm_unsubscribe(topic); // Step 1: clear pending_unsubscribe
        subscriptions.mark_subscribe(topic); // Step 2: mark as subscribing
        subscriptions.confirm_subscribe(topic); // Step 3: confirm subscription

        // Verify recovery: topic should be back in confirmed state
        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert!(subscriptions.pending_subscribe_topics().is_empty());

        // Verify topic is in all_topics() for reconnect
        let all = subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic.to_string()));
    }

    #[rstest]
    fn test_race_resubscribe_before_unsubscribe_ack() {
        // Simulates: User unsubscribes, then immediately resubscribes before
        // the unsubscribe ACK arrives from the venue.
        // This is the race condition fixed in the subscription tracker.
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER); // Bybit uses dot delimiter

        let topic = "orderbook.50.BTCUSDT";

        // Initial subscribe
        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        // User unsubscribes
        subscriptions.mark_unsubscribe(topic);
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        // User immediately changes mind and resubscribes (before unsubscribe ACK)
        subscriptions.mark_subscribe(topic);
        assert_eq!(subscriptions.pending_subscribe_topics(), vec![topic]);

        // NOW the unsubscribe ACK arrives - should NOT clear pending_subscribe
        subscriptions.confirm_unsubscribe(topic);
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(subscriptions.pending_subscribe_topics(), vec![topic]);

        // Subscribe ACK arrives
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions.pending_subscribe_topics().is_empty());

        // Verify final state is correct
        let all = subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic.to_string()));
    }

    #[rstest]
    fn test_race_late_subscribe_confirmation_after_unsubscribe() {
        // Simulates: User subscribes, then unsubscribes before subscribe ACK arrives.
        // The late subscribe ACK should be ignored.
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER); // Bybit uses dot delimiter

        let topic = "tickers.ETHUSDT";

        // User subscribes
        subscriptions.mark_subscribe(topic);
        assert_eq!(subscriptions.pending_subscribe_topics(), vec![topic]);

        // User immediately unsubscribes (before subscribe ACK)
        subscriptions.mark_unsubscribe(topic);
        assert!(subscriptions.pending_subscribe_topics().is_empty()); // Cleared
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        // Late subscribe confirmation arrives - should be IGNORED
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 0); // Not added to confirmed
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        // Unsubscribe ACK arrives
        subscriptions.confirm_unsubscribe(topic);

        // Final state: completely empty
        assert!(subscriptions.is_empty());
        assert!(subscriptions.all_topics().is_empty());
    }

    #[rstest]
    fn test_race_reconnection_with_pending_states() {
        // Simulates reconnection with mixed pending states.
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER); // Bybit uses dot delimiter

        // Set up mixed state before reconnection
        // Confirmed: publicTrade.BTCUSDT
        let trade_btc = "publicTrade.BTCUSDT";
        subscriptions.mark_subscribe(trade_btc);
        subscriptions.confirm_subscribe(trade_btc);

        // Pending subscribe: publicTrade.ETHUSDT
        let trade_eth = "publicTrade.ETHUSDT";
        subscriptions.mark_subscribe(trade_eth);

        // Pending unsubscribe: orderbook.50.BTCUSDT (user cancelled)
        let book_btc = "orderbook.50.BTCUSDT";
        subscriptions.mark_subscribe(book_btc);
        subscriptions.confirm_subscribe(book_btc);
        subscriptions.mark_unsubscribe(book_btc);

        // Get topics for reconnection
        let topics_to_restore = subscriptions.all_topics();

        // Should include: confirmed + pending_subscribe (NOT pending_unsubscribe)
        assert_eq!(topics_to_restore.len(), 2);
        assert!(topics_to_restore.contains(&trade_btc.to_string()));
        assert!(topics_to_restore.contains(&trade_eth.to_string()));
        assert!(!topics_to_restore.contains(&book_btc.to_string())); // Excluded
    }

    #[rstest]
    fn test_race_duplicate_subscribe_messages_idempotent() {
        // Simulates duplicate subscribe requests (e.g., from reconnection logic).
        // The subscription tracker should be idempotent and not create duplicate state.
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER); // Bybit uses dot delimiter

        let topic = "publicTrade.BTCUSDT";

        // Subscribe and confirm
        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        // Duplicate mark_subscribe on already-confirmed topic (should be no-op)
        subscriptions.mark_subscribe(topic);
        assert!(subscriptions.pending_subscribe_topics().is_empty()); // Not re-added
        assert_eq!(subscriptions.len(), 1); // Still just 1

        // Duplicate confirm_subscribe (should be idempotent)
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        // Verify final state
        let all = subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], topic);
    }

    #[rstest]
    #[case::spot_with_leverage(BybitProductType::Spot, true, Some(1))]
    #[case::spot_without_leverage(BybitProductType::Spot, false, Some(0))]
    #[case::linear_with_leverage(BybitProductType::Linear, true, None)]
    #[case::linear_without_leverage(BybitProductType::Linear, false, None)]
    #[case::inverse_with_leverage(BybitProductType::Inverse, true, None)]
    #[case::option_with_leverage(BybitProductType::Option, true, None)]
    fn test_is_leverage_parameter(
        #[case] product_type: BybitProductType,
        #[case] is_leverage: bool,
        #[case] expected: Option<i32>,
    ) {
        let symbol = match product_type {
            BybitProductType::Spot => "BTCUSDT-SPOT.BYBIT",
            BybitProductType::Linear => "ETHUSDT-LINEAR.BYBIT",
            BybitProductType::Inverse => "BTCUSD-INVERSE.BYBIT",
            BybitProductType::Option => "BTC-31MAY24-50000-C-OPTION.BYBIT",
        };

        let instrument_id = InstrumentId::from(symbol);
        let client_order_id = ClientOrderId::from("test-order-1");
        let quantity = Quantity::from("1.0");

        let client = BybitWebSocketClient::new_trade(
            BybitEnvironment::Testnet,
            Some("test-key".to_string()),
            Some("test-secret".to_string()),
            None,
            Some(20),
        );

        let params = client
            .build_place_order_params(
                product_type,
                instrument_id,
                client_order_id,
                OrderSide::Buy,
                OrderType::Limit,
                quantity,
                false, // is_quote_quantity
                Some(TimeInForce::Gtc),
                Some(Price::from("50000.0")),
                None,
                None,
                None,
                is_leverage,
            )
            .expect("Failed to build params");

        assert_eq!(params.is_leverage, expected);
    }

    #[rstest]
    #[case::spot_market_quote_quantity(BybitProductType::Spot, OrderType::Market, true, Some(BYBIT_QUOTE_COIN.to_string()))]
    #[case::spot_market_base_quantity(BybitProductType::Spot, OrderType::Market, false, Some(BYBIT_BASE_COIN.to_string()))]
    #[case::spot_limit_no_unit(BybitProductType::Spot, OrderType::Limit, false, None)]
    #[case::spot_limit_quote(BybitProductType::Spot, OrderType::Limit, true, None)]
    #[case::linear_market_no_unit(BybitProductType::Linear, OrderType::Market, false, None)]
    #[case::inverse_market_no_unit(BybitProductType::Inverse, OrderType::Market, true, None)]
    fn test_is_quote_quantity_parameter(
        #[case] product_type: BybitProductType,
        #[case] order_type: OrderType,
        #[case] is_quote_quantity: bool,
        #[case] expected: Option<String>,
    ) {
        let symbol = match product_type {
            BybitProductType::Spot => "BTCUSDT-SPOT.BYBIT",
            BybitProductType::Linear => "ETHUSDT-LINEAR.BYBIT",
            BybitProductType::Inverse => "BTCUSD-INVERSE.BYBIT",
            BybitProductType::Option => "BTC-31MAY24-50000-C-OPTION.BYBIT",
        };

        let instrument_id = InstrumentId::from(symbol);
        let client_order_id = ClientOrderId::from("test-order-1");
        let quantity = Quantity::from("1.0");

        let client = BybitWebSocketClient::new_trade(
            BybitEnvironment::Testnet,
            Some("test-key".to_string()),
            Some("test-secret".to_string()),
            None,
            Some(20),
        );

        let params = client
            .build_place_order_params(
                product_type,
                instrument_id,
                client_order_id,
                OrderSide::Buy,
                order_type,
                quantity,
                is_quote_quantity,
                Some(TimeInForce::Gtc),
                if order_type == OrderType::Market {
                    None
                } else {
                    Some(Price::from("50000.0"))
                },
                None,
                None,
                None,
                false,
            )
            .expect("Failed to build params");

        assert_eq!(params.market_unit, expected);
    }
}
