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

use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::{AtomicMap, AtomicSet, UUID4, consts::NAUTILUS_USER_AGENT};
use nautilus_model::{
    data::BarType,
    enums::{AggregationSource, OrderSide, OrderType, PriceType, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, PingHandler, SubscriptionState, TransportBackend, WebSocketClient,
        WebSocketConfig, channel_message_handler,
    },
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{BYBIT_NAUTILUS_BROKER_ID, BYBIT_WS_TOPIC_DELIMITER},
        credential::Credential,
        enums::{
            BybitEnvironment, BybitOrderSide, BybitOrderType, BybitPositionIdx, BybitProductType,
            BybitTimeInForce, BybitTpSlMode, BybitWsOrderRequestOp, resolve_trigger_type,
        },
        parse::{
            bar_spec_to_bybit_interval, extract_base_coin, extract_raw_symbol, map_time_in_force,
            spot_leverage, spot_market_unit, trigger_direction,
        },
        symbol::BybitSymbol,
        urls::{bybit_ws_private_url, bybit_ws_public_url, bybit_ws_trade_url},
    },
    websocket::{
        dispatch::PendingOperation,
        enums::{BybitWsOperation, BybitWsPrivateChannel, BybitWsPublicChannel},
        error::{BybitWsError, BybitWsResult},
        handler::{BybitWsFeedHandler, HandlerCommand},
        messages::{
            BybitAuthRequest, BybitSubscription, BybitWsAmendOrderParams, BybitWsBatchCancelItem,
            BybitWsBatchCancelOrderArgs, BybitWsBatchPlaceItem, BybitWsBatchPlaceOrderArgs,
            BybitWsCancelOrderParams, BybitWsHeader, BybitWsMessage, BybitWsPlaceOrderParams,
            BybitWsRequest,
        },
    },
};

const WEBSOCKET_AUTH_WINDOW_MS: i64 = 5_000;
const AUTH_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
pub const BATCH_PROCESSING_LIMIT: usize = 20;

/// Tracks a pending Python execution request for OrderResponse correlation.
#[derive(Debug, Clone)]
pub struct PendingPyRequest {
    pub client_order_id: ClientOrderId,
    pub operation: PendingOperation,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub venue_order_id: Option<VenueOrderId>,
}

/// Public/market data WebSocket client for Bybit.
#[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
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
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<BybitWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    account_id: Option<AccountId>,
    mm_level: Arc<AtomicU8>,
    bar_types_cache: Arc<AtomicMap<String, BarType>>,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    trade_subs: Arc<AtomicSet<InstrumentId>>,
    option_greeks_subs: Arc<AtomicSet<InstrumentId>>,
    bars_timestamp_on_close: Arc<AtomicBool>,
    pending_py_requests: Arc<DashMap<String, Vec<PendingPyRequest>>>,
    transport_backend: TransportBackend,
    cancellation_token: CancellationToken,
    proxy_url: Option<String>,
}

impl Debug for BybitWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BybitWebSocketClient))
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
            account_id: self.account_id,
            mm_level: Arc::clone(&self.mm_level),
            bar_types_cache: Arc::clone(&self.bar_types_cache),
            instruments_cache: Arc::clone(&self.instruments_cache),
            trade_subs: Arc::clone(&self.trade_subs),
            option_greeks_subs: Arc::clone(&self.option_greeks_subs),
            bars_timestamp_on_close: Arc::clone(&self.bars_timestamp_on_close),
            pending_py_requests: Arc::clone(&self.pending_py_requests),
            transport_backend: self.transport_backend,
            cancellation_token: self.cancellation_token.clone(),
            proxy_url: self.proxy_url.clone(),
        }
    }
}

impl BybitWebSocketClient {
    /// Creates a new Bybit public WebSocket client.
    #[must_use]
    pub fn new_public(url: Option<String>, heartbeat: u64) -> Self {
        Self::new_public_with(
            BybitProductType::Linear,
            BybitEnvironment::Mainnet,
            url,
            heartbeat,
            TransportBackend::default(),
            None,
        )
    }

    /// Creates a new Bybit public WebSocket client targeting the specified product/environment.
    #[must_use]
    pub fn new_public_with(
        product_type: BybitProductType,
        environment: BybitEnvironment,
        url: Option<String>,
        heartbeat: u64,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
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
            heartbeat: Some(heartbeat),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER),
            bar_types_cache: Arc::new(AtomicMap::new()),
            instruments_cache: Arc::new(AtomicMap::new()),
            trade_subs: Arc::new(AtomicSet::new()),
            option_greeks_subs: Arc::new(AtomicSet::new()),
            bars_timestamp_on_close: Arc::new(AtomicBool::new(true)),
            pending_py_requests: Arc::new(DashMap::new()),
            account_id: None,
            mm_level: Arc::new(AtomicU8::new(0)),
            transport_backend,
            cancellation_token: CancellationToken::new(),
            proxy_url,
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
        heartbeat: u64,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let credential = Credential::resolve(api_key, api_secret, environment);

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
            heartbeat: Some(heartbeat),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER),
            bar_types_cache: Arc::new(AtomicMap::new()),
            instruments_cache: Arc::new(AtomicMap::new()),
            trade_subs: Arc::new(AtomicSet::new()),
            option_greeks_subs: Arc::new(AtomicSet::new()),
            bars_timestamp_on_close: Arc::new(AtomicBool::new(true)),
            pending_py_requests: Arc::new(DashMap::new()),
            account_id: None,
            mm_level: Arc::new(AtomicU8::new(0)),
            transport_backend,
            cancellation_token: CancellationToken::new(),
            proxy_url,
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
        heartbeat: u64,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let credential = Credential::resolve(api_key, api_secret, environment);

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
            heartbeat: Some(heartbeat),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER),
            bar_types_cache: Arc::new(AtomicMap::new()),
            instruments_cache: Arc::new(AtomicMap::new()),
            trade_subs: Arc::new(AtomicSet::new()),
            option_greeks_subs: Arc::new(AtomicSet::new()),
            bars_timestamp_on_close: Arc::new(AtomicBool::new(true)),
            pending_py_requests: Arc::new(DashMap::new()),
            account_id: None,
            mm_level: Arc::new(AtomicU8::new(0)),
            transport_backend,
            cancellation_token: CancellationToken::new(),
            proxy_url,
        }
    }

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying WebSocket connection cannot be established,
    /// after retrying multiple times with exponential backoff.
    pub async fn connect(&mut self) -> BybitWsResult<()> {
        const MAX_RETRIES: u32 = 5;
        const CONNECTION_TIMEOUT_SECS: u64 = 10;

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
            req_id: None,
        })?;

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: Self::default_headers(),
            heartbeat: self.heartbeat,
            heartbeat_msg: Some(ping_msg),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        // Retry initial connection with exponential backoff to handle transient DNS/network issues
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
                    last_error = format!(
                        "Connection timeout after {CONNECTION_TIMEOUT_SECS}s (possible DNS resolution failure)"
                    );
                    log::warn!(
                        "WebSocket connection attempt timed out: attempt={attempt}, max_retries={MAX_RETRIES}, url={}",
                        self.url
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
            log::debug!(
                "Retrying in {delay:?} (attempt {}/{MAX_RETRIES})",
                attempt + 1
            );
            tokio::time::sleep(delay).await;
        };

        self.connection_mode.store(client.connection_mode_atomic());
        client.set_auth_tracker(self.auth_tracker.clone(), self.requires_auth);

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<BybitWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        let cmd = HandlerCommand::SetClient(client);

        self.send_cmd(cmd).await?;

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let credential = self.credential.clone();
        let requires_auth = self.requires_auth;
        let cmd_tx_for_reconnect = cmd_tx.clone();
        let auth_tracker = self.auth_tracker.clone();
        let auth_tracker_for_handler = auth_tracker.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = BybitWsFeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                auth_tracker_for_handler,
                subscriptions.clone(),
            );

            // Helper closure to resubscribe all tracked subscriptions after reconnection
            let resubscribe_all = || async {
                let topics = subscriptions.all_topics();

                if topics.is_empty() {
                    return;
                }

                log::debug!(
                    "Resubscribing to confirmed subscriptions: count={}",
                    topics.len()
                );

                for topic in &topics {
                    subscriptions.mark_subscribe(topic.as_str());
                }

                let mut payloads = Vec::with_capacity(topics.len());
                for topic in &topics {
                    let message = BybitSubscription {
                        op: BybitWsOperation::Subscribe,
                        args: vec![topic.clone()],
                        req_id: Some(topic.clone()),
                    };

                    if let Ok(payload) = serde_json::to_string(&message) {
                        payloads.push(payload);
                    }
                }

                let cmd = HandlerCommand::Subscribe { topics: payloads };

                if let Err(e) = cmd_tx_for_reconnect.send(cmd) {
                    log::error!("Failed to send resubscribe command: {e}");
                }
            };

            // Run message processing with reconnection handling
            loop {
                match handler.next().await {
                    Some(BybitWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }

                        log::info!("WebSocket reconnected");

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
                            log::debug!(
                                "Marking confirmed subscriptions as pending for replay: count={}",
                                confirmed_topics.len()
                            );

                            for topic in confirmed_topics {
                                subscriptions.mark_failure(&topic);
                            }
                        }

                        if requires_auth {
                            log::debug!("Re-authenticating after reconnection");

                            if let Some(cred) = &credential {
                                // Begin auth attempt so succeed() will update state
                                let _rx = auth_tracker.begin();

                                let expires = chrono::Utc::now().timestamp_millis()
                                    + WEBSOCKET_AUTH_WINDOW_MS;
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
                                        log::error!(
                                            "Failed to send reconnection auth command: error={e}"
                                        );
                                    }
                                } else {
                                    log::error!("Failed to serialize reconnection auth message");
                                }
                            }
                        }

                        // Unauthenticated sessions resubscribe immediately after reconnection,
                        // authenticated sessions wait for Auth message
                        if !requires_auth {
                            log::debug!("No authentication required, resubscribing immediately");
                            resubscribe_all().await;
                        }

                        // Forward to out_tx so caller sees the Reconnected message
                        if out_tx.send(BybitWsMessage::Reconnected).is_err() {
                            log::debug!("Receiver dropped, stopping");
                            break;
                        }
                    }
                    Some(BybitWsMessage::Auth(ref auth)) => {
                        let is_success = auth.success.unwrap_or(false) || auth.ret_code == Some(0);
                        if is_success {
                            log::debug!("Authenticated, resubscribing");
                            resubscribe_all().await;
                        }

                        if out_tx.send(BybitWsMessage::Auth(auth.clone())).is_err() {
                            log::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    Some(msg) => {
                        if out_tx.send(msg).is_err() {
                            log::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        // Stream ended - check if it's a stop signal
                        if handler.is_stopped() {
                            log::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        // Otherwise it's an unexpected stream end
                        log::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            log::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        if requires_auth && let Err(e) = self.authenticate_if_required().await {
            return Err(e);
        }

        Ok(())
    }

    /// Disconnects the WebSocket client and stops the background task.
    pub async fn close(&mut self) -> BybitWsResult<()> {
        log::debug!("Starting close process");

        self.signal.store(true, Ordering::Relaxed);

        let cmd = HandlerCommand::Disconnect;
        if let Err(e) = self.cmd_tx.read().await.send(cmd) {
            log::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

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

        self.auth_tracker.invalidate();

        log::debug!("Closed");

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

        log::debug!("Subscribing to topics: {topics:?}");

        // Use reference counting to deduplicate subscriptions
        let mut topics_to_send = Vec::new();

        for topic in topics {
            // Returns true if this is the first subscription (ref count 0 -> 1)
            if self.subscriptions.add_reference(&topic) {
                self.subscriptions.mark_subscribe(&topic);
                topics_to_send.push(topic.clone());
            } else {
                log::debug!("Already subscribed to {topic}, skipping duplicate subscription");
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
                req_id: Some(topic.clone()),
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

        log::debug!("Attempting to unsubscribe from topics: {topics:?}");

        if self.signal.load(Ordering::Relaxed) {
            log::debug!("Shutdown signal detected, skipping unsubscribe");
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
                log::debug!("Topic {topic} still has active subscriptions, not unsubscribing");
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
                req_id: Some(topic.clone()),
            };

            if let Ok(payload) = serde_json::to_string(&message) {
                payloads.push(payload);
            }
        }

        let cmd = HandlerCommand::Unsubscribe { topics: payloads };
        if let Err(e) = self.cmd_tx.read().await.send(cmd) {
            log::debug!("Failed to send unsubscribe command: error={e}");
        }

        Ok(())
    }

    /// Returns a stream of venue-typed [`BybitWsMessage`] items.
    ///
    /// # Panics
    ///
    /// Panics if called before [`Self::connect`] or if the stream has already been taken.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = BybitWsMessage> + use<> {
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

    /// Sets the account ID for account message parsing.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Sets the account market maker level.
    pub fn set_mm_level(&self, mm_level: u8) {
        self.mm_level.store(mm_level, Ordering::Relaxed);
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

    /// Returns a reference to the bar types cache.
    #[must_use]
    pub fn bar_types_cache(&self) -> &Arc<AtomicMap<String, BarType>> {
        &self.bar_types_cache
    }

    /// Adds an instrument to the shared instruments cache.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.id().symbol.inner(), instrument);
    }

    /// Returns a snapshot of the instruments cache keyed by symbol.
    #[must_use]
    pub fn instruments_snapshot(&self) -> ahash::AHashMap<Ustr, InstrumentAny> {
        (**self.instruments_cache.load()).clone()
    }

    /// Sets whether bar timestamps use the close time.
    pub fn set_bars_timestamp_on_close(&self, value: bool) {
        self.bars_timestamp_on_close.store(value, Ordering::Relaxed);
    }

    /// Returns whether bar timestamps use the close time.
    #[must_use]
    pub fn bars_timestamp_on_close(&self) -> bool {
        self.bars_timestamp_on_close.load(Ordering::Relaxed)
    }

    /// Adds an instrument ID to the option greeks subscription set.
    pub fn add_option_greeks_sub(&self, instrument_id: InstrumentId) {
        self.option_greeks_subs.insert(instrument_id);
    }

    /// Removes an instrument ID from the option greeks subscription set.
    pub fn remove_option_greeks_sub(&self, instrument_id: &InstrumentId) {
        self.option_greeks_subs.remove(instrument_id);
    }

    /// Returns a reference to the option greeks subscription set.
    #[must_use]
    pub fn option_greeks_subs(&self) -> &Arc<AtomicSet<InstrumentId>> {
        &self.option_greeks_subs
    }

    /// Returns a reference to the trade subscriptions set.
    #[must_use]
    pub fn trade_subs(&self) -> &Arc<AtomicSet<InstrumentId>> {
        &self.trade_subs
    }

    /// Returns a reference to the pending Python requests map.
    #[must_use]
    pub fn pending_py_requests(&self) -> &Arc<DashMap<String, Vec<PendingPyRequest>>> {
        &self.pending_py_requests
    }

    /// Returns a reference to the live instruments cache Arc.
    #[must_use]
    pub fn instruments_cache_ref(&self) -> &Arc<AtomicMap<Ustr, InstrumentAny>> {
        &self.instruments_cache
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
        self.trade_subs.insert(instrument_id);
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        // Bybit option trades use baseCoin topic (e.g. publicTrade.BTC)
        let topic_symbol = match self.product_type {
            Some(BybitProductType::Option) => extract_base_coin(raw_symbol),
            _ => raw_symbol,
        };
        let topic = format!(
            "{}.{topic_symbol}",
            BybitWsPublicChannel::PublicTrade.as_ref()
        );
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        self.trade_subs.remove(&instrument_id);
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic_symbol = match self.product_type {
            Some(BybitProductType::Option) => extract_base_coin(raw_symbol),
            _ => raw_symbol,
        };
        let topic = format!(
            "{}.{topic_symbol}",
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
    pub async fn subscribe_bars(&self, bar_type: BarType) -> BybitWsResult<()> {
        if self.product_type == Some(BybitProductType::Option) {
            return Err(BybitWsError::ClientError(
                "Bybit does not support kline/bar data for options".to_string(),
            ));
        }

        let spec = bar_type.spec();

        if spec.price_type != PriceType::Last {
            return Err(BybitWsError::ClientError(format!(
                "Invalid bar type: Bybit bars only support LAST price type, received {}",
                spec.price_type
            )));
        }

        if bar_type.aggregation_source() != AggregationSource::External {
            return Err(BybitWsError::ClientError(format!(
                "Invalid bar type: Bybit bars only support EXTERNAL aggregation source, received {}",
                bar_type.aggregation_source()
            )));
        }

        let interval = bar_spec_to_bybit_interval(spec.aggregation, spec.step.get() as u64)
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;

        let instrument_id = bar_type.instrument_id();
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{}.{raw_symbol}",
            BybitWsPublicChannel::Kline.as_ref(),
            interval
        );

        // Coordinate with reference counting to avoid duplicate cache entries
        if self.subscriptions.get_reference_count(&topic) == 0 {
            self.bar_types_cache.insert(topic.clone(), bar_type);
        }

        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from kline/candlestick updates for a specific instrument.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> BybitWsResult<()> {
        let spec = bar_type.spec();
        let interval = bar_spec_to_bybit_interval(spec.aggregation, spec.step.get() as u64)
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;

        let instrument_id = bar_type.instrument_id();
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!(
            "{}.{}.{raw_symbol}",
            BybitWsPublicChannel::Kline.as_ref(),
            interval
        );

        // Coordinate with reference counting to preserve cache for other subscribers
        if self.subscriptions.get_reference_count(&topic) == 1 {
            self.bar_types_cache.remove(&topic);
        }

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

    /// Waits for the session to be authenticated, aborting early if the client
    /// enters a terminal state (closed or disconnecting) during the wait.
    async fn require_authenticated(&self) -> BybitWsResult<()> {
        if self.is_closed() {
            return Err(BybitWsError::ClientError(
                "WebSocket client is closed".to_string(),
            ));
        }

        if self.auth_tracker.is_authenticated() {
            return Ok(());
        }

        tokio::select! {
            authenticated = self.auth_tracker.wait_for_authenticated(AUTH_WAIT_TIMEOUT) => {
                if authenticated {
                    Ok(())
                } else {
                    Err(BybitWsError::Authentication(
                        "Must be authenticated".to_string(),
                    ))
                }
            }
            () = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    if self.is_closed() {
                        return;
                    }
                }
            } => {
                Err(BybitWsError::ClientError(
                    "WebSocket client closed during authentication wait".to_string(),
                ))
            }
        }
    }

    /// Places an order via WebSocket, returning the request ID for correlation.
    ///
    /// # Errors
    ///
    /// Returns an error if the order request fails or if not authenticated.
    pub async fn place_order(&self, params: BybitWsPlaceOrderParams) -> BybitWsResult<String> {
        self.require_authenticated().await?;

        let req_id = UUID4::new().to_string();

        let referer = if self.include_referer_header(params.time_in_force) {
            Some(BYBIT_NAUTILUS_BROKER_ID.to_string())
        } else {
            None
        };

        let request = BybitWsRequest {
            req_id: Some(req_id.clone()),
            op: BybitWsOrderRequestOp::Create,
            header: BybitWsHeader::with_referer(referer),
            args: vec![params],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        self.send_text(&payload).await?;

        Ok(req_id)
    }

    /// Amends an existing order via WebSocket, returning the request ID for correlation.
    ///
    /// # Errors
    ///
    /// Returns an error if the amend request fails or if not authenticated.
    pub async fn amend_order(&self, params: BybitWsAmendOrderParams) -> BybitWsResult<String> {
        self.require_authenticated().await?;

        let req_id = UUID4::new().to_string();

        let request = BybitWsRequest {
            req_id: Some(req_id.clone()),
            op: BybitWsOrderRequestOp::Amend,
            header: BybitWsHeader::now(),
            args: vec![params],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        self.send_text(&payload).await?;

        Ok(req_id)
    }

    /// Cancels an order via WebSocket, returning the request ID for correlation.
    ///
    /// # Errors
    ///
    /// Returns an error if the cancel request fails or if not authenticated.
    pub async fn cancel_order(&self, params: BybitWsCancelOrderParams) -> BybitWsResult<String> {
        self.require_authenticated().await?;

        let req_id = UUID4::new().to_string();

        let request = BybitWsRequest {
            req_id: Some(req_id.clone()),
            op: BybitWsOrderRequestOp::Cancel,
            header: BybitWsHeader::now(),
            args: vec![params],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        self.send_text(&payload).await?;

        Ok(req_id)
    }

    /// Batch creates multiple orders via WebSocket, returning the request ID for correlation.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_place_orders(
        &self,
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> BybitWsResult<Vec<String>> {
        self.require_authenticated().await?;

        if orders.is_empty() {
            log::warn!("Batch place orders called with empty orders list");
            return Ok(vec![]);
        }

        let mut req_ids = Vec::new();

        for chunk in orders.chunks(BATCH_PROCESSING_LIMIT) {
            let req_id = self.batch_place_orders_chunk(chunk.to_vec()).await?;
            req_ids.push(req_id);
        }

        Ok(req_ids)
    }

    async fn batch_place_orders_chunk(
        &self,
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> BybitWsResult<String> {
        let category = orders[0].category;
        let batch_req_id = UUID4::new().to_string();

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
                order_iv: order.order_iv,
                mmp: order.mmp,
                position_idx: order.position_idx,
            })
            .collect();

        let args = BybitWsBatchPlaceOrderArgs {
            category,
            request: request_items,
        };

        let request = BybitWsRequest {
            req_id: Some(batch_req_id.clone()),
            op: BybitWsOrderRequestOp::CreateBatch,
            header: BybitWsHeader::with_referer(referer),
            args: vec![args],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        self.send_text(&payload).await?;

        Ok(batch_req_id)
    }

    /// Batch amends multiple orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_amend_orders(
        &self,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> BybitWsResult<Vec<String>> {
        self.require_authenticated().await?;

        if orders.is_empty() {
            log::warn!("Batch amend orders called with empty orders list");
            return Ok(vec![]);
        }

        let mut req_ids = Vec::new();

        for chunk in orders.chunks(BATCH_PROCESSING_LIMIT) {
            let req_id = self.batch_amend_orders_chunk(chunk.to_vec()).await?;
            req_ids.push(req_id);
        }

        Ok(req_ids)
    }

    async fn batch_amend_orders_chunk(
        &self,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> BybitWsResult<String> {
        let batch_req_id = UUID4::new().to_string();

        let request = BybitWsRequest {
            req_id: Some(batch_req_id.clone()),
            op: BybitWsOrderRequestOp::AmendBatch,
            header: BybitWsHeader::now(),
            args: orders,
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        self.send_text(&payload).await?;

        Ok(batch_req_id)
    }

    /// Batch cancels multiple orders via WebSocket, returning the request ID for correlation.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_cancel_orders(
        &self,
        orders: Vec<BybitWsCancelOrderParams>,
    ) -> BybitWsResult<Vec<String>> {
        self.require_authenticated().await?;

        if orders.is_empty() {
            log::warn!("Batch cancel orders called with empty orders list");
            return Ok(vec![]);
        }

        let mut req_ids = Vec::new();

        for chunk in orders.chunks(BATCH_PROCESSING_LIMIT) {
            let req_id = self.batch_cancel_orders_chunk(chunk.to_vec()).await?;
            req_ids.push(req_id);
        }

        Ok(req_ids)
    }

    async fn batch_cancel_orders_chunk(
        &self,
        orders: Vec<BybitWsCancelOrderParams>,
    ) -> BybitWsResult<String> {
        if orders.is_empty() {
            return Ok(String::new());
        }

        let category = orders[0].category;
        let batch_req_id = UUID4::new().to_string();

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
            req_id: Some(batch_req_id.clone()),
            op: BybitWsOrderRequestOp::CancelBatch,
            header: BybitWsHeader::now(),
            args: vec![args],
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        self.send_text(&payload).await?;

        Ok(batch_req_id)
    }

    /// Submits an order using Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if order submission fails or if not authenticated.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_order(
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
        trigger_type: Option<TriggerType>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
        position_idx: Option<BybitPositionIdx>,
    ) -> BybitWsResult<String> {
        let params = self.build_place_order_params(
            product_type,
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            is_quote_quantity,
            time_in_force,
            price,
            trigger_price,
            trigger_type,
            post_only,
            reduce_only,
            is_leverage,
            None,
            None,
            position_idx,
        )?;

        self.place_order(params).await
    }

    /// Modifies an existing order using Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if modification fails or if not authenticated.
    pub async fn modify_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> BybitWsResult<String> {
        let params = self.build_amend_order_params(
            product_type,
            instrument_id,
            venue_order_id,
            Some(client_order_id),
            quantity,
            price,
        )?;

        self.amend_order(params).await
    }

    /// Cancels an order using Nautilus domain objects.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails or if not authenticated.
    pub async fn cancel_order_by_id(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> BybitWsResult<String> {
        let params = self.build_cancel_order_params(
            product_type,
            instrument_id,
            venue_order_id,
            Some(client_order_id),
        )?;

        self.cancel_order(params).await
    }

    /// Builds order params for placing an order.
    #[expect(clippy::too_many_arguments)]
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
        trigger_type: Option<TriggerType>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        is_leverage: bool,
        take_profit: Option<Price>,
        stop_loss: Option<Price>,
        position_idx: Option<BybitPositionIdx>,
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

        let bybit_tif =
            map_time_in_force(bybit_order_type, time_in_force, post_only).map_err(|tif| {
                BybitWsError::ClientError(format!("Unsupported time in force: {tif:?}"))
            })?;
        let market_unit = spot_market_unit(product_type, bybit_order_type, is_quote_quantity);
        let is_leverage_value = spot_leverage(product_type, is_leverage);
        let trigger_dir =
            trigger_direction(order_type, order_side, is_stop_order).map(|d| d as i32);

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
                time_in_force: bybit_tif,
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: trigger_price.map(|p| p.to_string()),
                trigger_by: Some(resolve_trigger_type(trigger_type)),
                trigger_direction: trigger_dir,
                tpsl_mode: if take_profit.is_some() || stop_loss.is_some() {
                    Some(BybitTpSlMode::Full)
                } else {
                    None
                },
                take_profit: take_profit.map(|p| p.to_string()),
                stop_loss: stop_loss.map(|p| p.to_string()),
                tp_trigger_by: take_profit.map(|_| resolve_trigger_type(trigger_type)),
                sl_trigger_by: stop_loss.map(|_| resolve_trigger_type(trigger_type)),
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
                order_iv: None,
                mmp: None,
                position_idx,
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
                time_in_force: bybit_tif,
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: None,
                trigger_by: None,
                trigger_direction: None,
                tpsl_mode: if take_profit.is_some() || stop_loss.is_some() {
                    Some(BybitTpSlMode::Full)
                } else {
                    None
                },
                take_profit: take_profit.map(|p| p.to_string()),
                stop_loss: stop_loss.map(|p| p.to_string()),
                tp_trigger_by: take_profit.map(|_| resolve_trigger_type(trigger_type)),
                sl_trigger_by: stop_loss.map(|_| resolve_trigger_type(trigger_type)),
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
                order_iv: None,
                mmp: None,
                position_idx,
            }
        };

        Ok(params)
    }

    /// Builds order params for amending an order.
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
            order_iv: None,
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

    fn include_referer_header(&self, time_in_force: Option<BybitTimeInForce>) -> bool {
        let is_post_only = matches!(time_in_force, Some(BybitTimeInForce::PostOnly));
        let mm_level = self.mm_level.load(Ordering::Relaxed);
        !(is_post_only && mm_level > 0)
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

        // Begin auth attempt so succeed() will update state
        let _rx = self.auth_tracker.begin();

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Authenticate { payload })
            .map_err(|e| BybitWsError::Send(format!("Failed to send auth command: {e}")))?;

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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{enums::BybitMarketUnit, testing::load_test_json},
        websocket::{messages::BybitWsFrame, parse_bybit_ws_frame},
    };

    #[rstest]
    fn classify_orderbook_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json"))
            .expect("invalid fixture");
        let frame = parse_bybit_ws_frame(json);
        assert!(matches!(frame, BybitWsFrame::Orderbook(_)));
    }

    #[rstest]
    fn classify_trade_snapshot() {
        let json: Value =
            serde_json::from_str(&load_test_json("ws_public_trade.json")).expect("invalid fixture");
        let frame = parse_bybit_ws_frame(json);
        assert!(matches!(frame, BybitWsFrame::Trade(_)));
    }

    #[rstest]
    fn classify_ticker_linear_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_linear.json"))
            .expect("invalid fixture");
        let frame = parse_bybit_ws_frame(json);
        assert!(matches!(frame, BybitWsFrame::TickerLinear(_)));
    }

    #[rstest]
    fn classify_ticker_option_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_option.json"))
            .expect("invalid fixture");
        let frame = parse_bybit_ws_frame(json);
        assert!(matches!(frame, BybitWsFrame::TickerOption(_)));
    }

    #[rstest]
    fn test_race_unsubscribe_failure_recovery() {
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);
        let topic = "publicTrade.BTCUSDT";

        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        subscriptions.mark_unsubscribe(topic);
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        subscriptions.confirm_unsubscribe(topic);
        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);

        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert!(subscriptions.pending_subscribe_topics().is_empty());

        let all = subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic.to_string()));
    }

    #[rstest]
    fn test_race_resubscribe_before_unsubscribe_ack() {
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);
        let topic = "orderbook.50.BTCUSDT";

        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        subscriptions.mark_unsubscribe(topic);
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        subscriptions.mark_subscribe(topic);
        assert_eq!(subscriptions.pending_subscribe_topics(), vec![topic]);

        subscriptions.confirm_unsubscribe(topic);
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(subscriptions.pending_subscribe_topics(), vec![topic]);

        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions.pending_subscribe_topics().is_empty());

        let all = subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic.to_string()));
    }

    #[rstest]
    fn test_race_late_subscribe_confirmation_after_unsubscribe() {
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);
        let topic = "tickers.ETHUSDT";

        subscriptions.mark_subscribe(topic);
        assert_eq!(subscriptions.pending_subscribe_topics(), vec![topic]);

        subscriptions.mark_unsubscribe(topic);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(subscriptions.pending_unsubscribe_topics(), vec![topic]);

        subscriptions.confirm_unsubscribe(topic);

        assert!(subscriptions.is_empty());
        assert!(subscriptions.all_topics().is_empty());
    }

    #[rstest]
    fn test_race_reconnection_with_pending_states() {
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);

        let trade_btc = "publicTrade.BTCUSDT";
        subscriptions.mark_subscribe(trade_btc);
        subscriptions.confirm_subscribe(trade_btc);

        let trade_eth = "publicTrade.ETHUSDT";
        subscriptions.mark_subscribe(trade_eth);

        let book_btc = "orderbook.50.BTCUSDT";
        subscriptions.mark_subscribe(book_btc);
        subscriptions.confirm_subscribe(book_btc);
        subscriptions.mark_unsubscribe(book_btc);

        let topics_to_restore = subscriptions.all_topics();

        assert_eq!(topics_to_restore.len(), 2);
        assert!(topics_to_restore.contains(&trade_btc.to_string()));
        assert!(topics_to_restore.contains(&trade_eth.to_string()));
        assert!(!topics_to_restore.contains(&book_btc.to_string()));
    }

    #[rstest]
    fn test_race_duplicate_subscribe_messages_idempotent() {
        let subscriptions = SubscriptionState::new(BYBIT_WS_TOPIC_DELIMITER);
        let topic = "publicTrade.BTCUSDT";

        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

        subscriptions.mark_subscribe(topic);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert_eq!(subscriptions.len(), 1);

        subscriptions.confirm_subscribe(topic);
        assert_eq!(subscriptions.len(), 1);

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
            20,
            TransportBackend::default(),
            None,
        );

        let params = client
            .build_place_order_params(
                product_type,
                instrument_id,
                client_order_id,
                OrderSide::Buy,
                OrderType::Limit,
                quantity,
                false,
                Some(TimeInForce::Gtc),
                Some(Price::from("50000.0")),
                None,
                None,
                None,
                None,
                is_leverage,
                None,
                None,
                None,
            )
            .expect("Failed to build params");

        assert_eq!(params.is_leverage, expected);
    }

    #[rstest]
    #[case::spot_market_quote_quantity(
        BybitProductType::Spot,
        OrderType::Market,
        true,
        Some(BybitMarketUnit::QuoteCoin)
    )]
    #[case::spot_market_base_quantity(
        BybitProductType::Spot,
        OrderType::Market,
        false,
        Some(BybitMarketUnit::BaseCoin)
    )]
    #[case::spot_limit_no_unit(BybitProductType::Spot, OrderType::Limit, false, None)]
    #[case::spot_limit_quote(BybitProductType::Spot, OrderType::Limit, true, None)]
    #[case::linear_market_no_unit(BybitProductType::Linear, OrderType::Market, false, None)]
    #[case::inverse_market_no_unit(BybitProductType::Inverse, OrderType::Market, true, None)]
    fn test_is_quote_quantity_parameter(
        #[case] product_type: BybitProductType,
        #[case] order_type: OrderType,
        #[case] is_quote_quantity: bool,
        #[case] expected: Option<BybitMarketUnit>,
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
            20,
            TransportBackend::default(),
            None,
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
                None,
                false,
                None,
                None,
                None,
            )
            .expect("Failed to build params");

        assert_eq!(params.market_unit, expected);
    }
}
