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
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;
use nautilus_common::runtime::get_runtime;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{PingHandler, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{BYBIT_NAUTILUS_BROKER_ID, BYBIT_PONG},
        credential::Credential,
        enums::{
            BybitEnvironment, BybitOrderSide, BybitOrderType, BybitProductType, BybitTimeInForce,
            BybitTriggerType, BybitWsOrderRequestOp,
        },
        parse::extract_raw_symbol,
        symbol::BybitSymbol,
        urls::{bybit_ws_private_url, bybit_ws_public_url, bybit_ws_trade_url},
    },
    websocket::{
        auth::{AUTHENTICATION_TIMEOUT_SECS, AuthTracker},
        cache,
        enums::BybitWsOperation,
        error::{BybitWsError, BybitWsResult},
        messages::{
            BybitAuthRequest, BybitSubscription, BybitWebSocketError, BybitWebSocketMessage,
            BybitWsAccountExecutionMsg, BybitWsAccountOrderMsg, BybitWsAccountPositionMsg,
            BybitWsAccountWalletMsg, BybitWsAmendOrderParams, BybitWsAuthResponse,
            BybitWsCancelOrderParams, BybitWsHeader, BybitWsKlineMsg, BybitWsOrderbookDepthMsg,
            BybitWsPlaceOrderParams, BybitWsRequest, BybitWsResponse, BybitWsSubscriptionMsg,
            BybitWsTickerLinearMsg, BybitWsTickerOptionMsg, BybitWsTradeMsg,
        },
        subscription::SubscriptionState,
    },
};

const MAX_ARGS_PER_SUBSCRIPTION_REQUEST: usize = 10;
const DEFAULT_HEARTBEAT_SECS: u64 = 20;
const WEBSOCKET_AUTH_WINDOW_MS: i64 = 5_000;

/// Determines if a Bybit WebSocket error should trigger a retry.
fn should_retry_bybit_error(error: &BybitWsError) -> bool {
    match error {
        BybitWsError::Transport(_) => true, // Network errors are retryable
        BybitWsError::Send(_) => true,      // Send errors are retryable
        BybitWsError::ClientError(msg) => {
            // Retry on timeout and connection errors (case-insensitive)
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("timeout")
                || msg_lower.contains("timed out")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
        }
        BybitWsError::NotConnected => true, // Connection issues are retryable
        BybitWsError::Authentication(_) | BybitWsError::Json(_) => {
            // Don't retry authentication or parsing errors automatically
            false
        }
    }
}

/// Creates a timeout error for Bybit operations.
fn create_bybit_timeout_error(msg: String) -> BybitWsError {
    BybitWsError::ClientError(msg)
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
    inner: Arc<RwLock<Option<WebSocketClient>>>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<BybitWebSocketMessage>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    subscriptions: SubscriptionState,
    is_authenticated: Arc<AtomicBool>,
    instruments_cache: Arc<DashMap<InstrumentId, InstrumentAny>>,
    account_id: Option<AccountId>,
    quote_cache: Arc<RwLock<cache::QuoteCache>>,
    retry_manager: Arc<RetryManager<BybitWsError>>,
    cancellation_token: CancellationToken,
}

impl fmt::Debug for BybitWebSocketClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            inner: Arc::clone(&self.inner),
            rx: None, // Each clone gets its own receiver
            signal: Arc::clone(&self.signal),
            task_handle: None, // Each clone gets its own task handle
            subscriptions: self.subscriptions.clone(),
            is_authenticated: Arc::clone(&self.is_authenticated),
            instruments_cache: Arc::clone(&self.instruments_cache),
            account_id: self.account_id,
            quote_cache: Arc::clone(&self.quote_cache),
            retry_manager: Arc::clone(&self.retry_manager),
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
    ///
    /// # Panics
    ///
    /// Panics if the retry manager cannot be created.
    #[must_use]
    pub fn new_public_with(
        product_type: BybitProductType,
        environment: BybitEnvironment,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        Self {
            url: url.unwrap_or_else(|| bybit_ws_public_url(product_type, environment)),
            environment,
            product_type: Some(product_type),
            credential: None,
            requires_auth: false,
            auth_tracker: AuthTracker::new(),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            inner: Arc::new(RwLock::new(None)),
            rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(),
            is_authenticated: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            quote_cache: Arc::new(RwLock::new(cache::QuoteCache::new())),
            retry_manager: Arc::new(
                create_websocket_retry_manager().expect("Failed to create retry manager"),
            ),
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Creates a new Bybit private WebSocket client.
    ///
    /// # Panics
    ///
    /// Panics if the retry manager cannot be created.
    #[must_use]
    pub fn new_private(
        environment: BybitEnvironment,
        credential: Credential,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        Self {
            url: url.unwrap_or_else(|| bybit_ws_private_url(environment).to_string()),
            environment,
            product_type: None,
            credential: Some(credential),
            requires_auth: true,
            auth_tracker: AuthTracker::new(),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            inner: Arc::new(RwLock::new(None)),
            rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(),
            is_authenticated: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            quote_cache: Arc::new(RwLock::new(cache::QuoteCache::new())),
            retry_manager: Arc::new(
                create_websocket_retry_manager().expect("Failed to create retry manager"),
            ),
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Creates a new Bybit trade WebSocket client for order operations.
    ///
    /// # Panics
    ///
    /// Panics if the retry manager cannot be created.
    #[must_use]
    pub fn new_trade(
        environment: BybitEnvironment,
        credential: Credential,
        url: Option<String>,
        heartbeat: Option<u64>,
    ) -> Self {
        Self {
            url: url.unwrap_or_else(|| bybit_ws_trade_url(environment).to_string()),
            environment,
            product_type: None,
            credential: Some(credential),
            requires_auth: true,
            auth_tracker: AuthTracker::new(),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            inner: Arc::new(RwLock::new(None)),
            rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: SubscriptionState::new(),
            is_authenticated: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            quote_cache: Arc::new(RwLock::new(cache::QuoteCache::new())),
            retry_manager: Arc::new(
                create_websocket_retry_manager().expect("Failed to create retry manager"),
            ),
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying WebSocket connection cannot be established.
    ///
    /// # Panics
    ///
    /// Panics if the ping message cannot be serialized to JSON.
    pub async fn connect(&mut self) -> BybitWsResult<()> {
        let (message_handler, mut message_rx) = channel_message_handler();

        let inner_for_ping = Arc::clone(&self.inner);
        let ping_handler: PingHandler = Arc::new(move |payload: Vec<u8>| {
            let inner = Arc::clone(&inner_for_ping);
            get_runtime().spawn(async move {
                let len = payload.len();
                let guard = inner.read().await;
                if let Some(client) = guard.as_ref() {
                    if let Err(e) = client.send_pong(payload).await {
                        tracing::warn!(error = %e, "Failed to send pong frame");
                    } else {
                        tracing::trace!("Sent pong frame ({len} bytes)");
                    }
                }
            });
        });

        let ping_msg = serde_json::to_string(&BybitSubscription {
            op: BybitWsOperation::Ping,
            args: vec![],
        })
        .expect("Failed to serialize ping message");

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: Self::default_headers(),
            message_handler: Some(message_handler),
            heartbeat: self.heartbeat,
            heartbeat_msg: Some(ping_msg),
            ping_handler: Some(ping_handler),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .map_err(BybitWsError::from)?;

        {
            let mut guard = self.inner.write().await;
            *guard = Some(client);
        }

        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<BybitWebSocketMessage>();
        self.rx = Some(event_rx);
        self.signal.store(false, Ordering::Relaxed);

        let inner = Arc::clone(&self.inner);
        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let auth_tracker = self.auth_tracker.clone();
        let credential = self.credential.clone();
        let requires_auth = self.requires_auth;
        let is_authenticated = Arc::clone(&self.is_authenticated);
        let quote_cache = Arc::clone(&self.quote_cache);

        let task_handle = get_runtime().spawn(async move {
            while let Some(message) = message_rx.recv().await {
                if signal.load(Ordering::Relaxed) {
                    break;
                }

                match Self::handle_message(
                    &inner,
                    &subscriptions,
                    &auth_tracker,
                    requires_auth,
                    &is_authenticated,
                    message,
                )
                .await
                {
                    Ok(Some(BybitWebSocketMessage::Reconnected)) => {
                        tracing::info!("Handling WebSocket reconnection");

                        let inner_for_task = inner.clone();
                        let subscriptions_for_task = subscriptions.clone();
                        let auth_tracker_for_task = auth_tracker.clone();
                        let is_authenticated_for_task = is_authenticated.clone();
                        let credential_for_task = credential.clone();
                        let quote_cache_for_task = quote_cache.clone();
                        let event_tx_for_task = event_tx.clone();

                        get_runtime().spawn(async move {
                            // Authenticate if required
                            let auth_succeeded = if requires_auth {
                                match Self::authenticate_inner(
                                    &inner_for_task,
                                    requires_auth,
                                    credential_for_task,
                                    &auth_tracker_for_task,
                                    &is_authenticated_for_task,
                                )
                                .await
                                {
                                    Ok(()) => {
                                        tracing::info!(
                                            "Authentication successful after reconnect, proceeding with resubscription"
                                        );
                                        true
                                    }
                                    Err(e) => {
                                        tracing::error!("Authentication after reconnect failed: {e}");
                                        let error = BybitWebSocketError::from_message(e.to_string());
                                        let _ = event_tx_for_task.send(BybitWebSocketMessage::Error(error));
                                        false
                                    }
                                }
                            } else {
                                true
                            };

                            if !auth_succeeded {
                                return;
                            }

                            // Clear the quote cache to prevent stale data after reconnection
                            quote_cache_for_task.write().await.clear();

                            // Resubscribe to all topics
                            if let Err(e) = Self::resubscribe_all_inner(
                                &inner_for_task,
                                &subscriptions_for_task,
                            )
                            .await
                            {
                                tracing::error!("Failed to restore subscriptions after reconnection: {e}");
                                let error = BybitWebSocketError::from_message(e.to_string());
                                let _ = event_tx_for_task.send(BybitWebSocketMessage::Error(error));
                            } else {
                                tracing::info!("Restored subscriptions after reconnection");
                                let _ = event_tx_for_task.send(BybitWebSocketMessage::Reconnected);
                            }
                        });
                    }
                    Ok(Some(event)) => {
                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let error = BybitWebSocketError::from_message(e.to_string());
                        if event_tx.send(BybitWebSocketMessage::Error(error)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        self.task_handle = Some(task_handle);

        self.authenticate_if_required().await?;

        // Resubscribe to any pre-registered topics (e.g. configured before connect).
        if !self.subscriptions.is_empty() {
            Self::resubscribe_all_inner(&self.inner, &self.subscriptions).await?;
        }

        Ok(())
    }

    /// Disconnects the WebSocket client and stops the background task.
    pub async fn close(&mut self) -> BybitWsResult<()> {
        self.signal.store(true, Ordering::Relaxed);

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = inner_guard.as_ref() {
                inner.disconnect().await;
            }
        }

        if let Some(handle) = self.task_handle.take()
            && let Err(e) = handle.await
        {
            tracing::error!(error = %e, "Bybit websocket task terminated with error");
        }

        self.rx = None;
        self.is_authenticated.store(false, Ordering::Relaxed);

        Ok(())
    }

    /// Returns `true` when the underlying client is active.
    #[must_use]
    pub async fn is_active(&self) -> bool {
        let guard = self.inner.read().await;
        guard.as_ref().is_some_and(WebSocketClient::is_active)
    }

    /// Waits until the WebSocket client becomes active or times out.
    ///
    /// # Errors
    ///
    /// Returns an error if the timeout is exceeded before the client becomes active.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> BybitWsResult<()> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active().await {
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

        Self::send_topics_inner(&self.inner, BybitWsOperation::Subscribe, topics_to_send).await
    }

    /// Unsubscribe from the provided topics.
    pub async fn unsubscribe(&self, topics: Vec<String>) -> BybitWsResult<()> {
        if topics.is_empty() {
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

        Self::send_topics_inner(&self.inner, BybitWsOperation::Unsubscribe, topics_to_send).await
    }

    /// Returns a stream of parsed [`BybitWebSocketMessage`] items.
    ///
    /// # Panics
    ///
    /// Panics if called before [`Self::connect`] or if the stream has already been taken.
    pub fn stream(
        &mut self,
    ) -> impl futures_util::Stream<Item = BybitWebSocketMessage> + Send + 'static {
        let rx = self
            .rx
            .take()
            .expect("Stream receiver already taken or client not connected");

        async_stream::stream! {
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                yield event;
            }
        }
    }

    /// Returns the number of currently registered subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Adds an instrument to the cache for parsing WebSocket messages.
    pub fn add_instrument(&self, instrument: InstrumentAny) {
        let instrument_id = instrument.id();
        self.instruments_cache.insert(instrument_id, instrument);
        tracing::debug!("Added instrument {instrument_id} to WebSocket client cache");
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<InstrumentId, InstrumentAny>> {
        &self.instruments_cache
    }

    /// Sets the account ID for account message parsing.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
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

    /// Returns a reference to the quote cache.
    #[must_use]
    pub fn quote_cache(&self) -> &Arc<RwLock<cache::QuoteCache>> {
        &self.quote_cache
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
        let topic = format!("orderbook.{}.{}", depth, raw_symbol);
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    pub async fn unsubscribe_orderbook(
        &self,
        instrument_id: InstrumentId,
        depth: u32,
    ) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("orderbook.{}.{}", depth, raw_symbol);
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
        let topic = format!("publicTrade.{}", raw_symbol);
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("publicTrade.{}", raw_symbol);
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
        let topic = format!("tickers.{}", raw_symbol);
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from ticker updates for a specific instrument.
    pub async fn unsubscribe_ticker(&self, instrument_id: InstrumentId) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("tickers.{}", raw_symbol);
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
        let topic = format!("kline.{}.{}", interval.into(), raw_symbol);
        self.subscribe(vec![topic]).await
    }

    /// Unsubscribes from kline/candlestick updates for a specific instrument.
    pub async fn unsubscribe_klines(
        &self,
        instrument_id: InstrumentId,
        interval: impl Into<String>,
    ) -> BybitWsResult<()> {
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("kline.{}.{}", interval.into(), raw_symbol);
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
        self.subscribe(vec!["order".to_string()]).await
    }

    /// Unsubscribes from order updates.
    pub async fn unsubscribe_orders(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec!["order".to_string()]).await
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
        self.subscribe(vec!["execution".to_string()]).await
    }

    /// Unsubscribes from execution/fill updates.
    pub async fn unsubscribe_executions(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec!["execution".to_string()]).await
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
        self.subscribe(vec!["position".to_string()]).await
    }

    /// Unsubscribes from position updates.
    pub async fn unsubscribe_positions(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec!["position".to_string()]).await
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
        self.subscribe(vec!["wallet".to_string()]).await
    }

    /// Unsubscribes from wallet/balance updates.
    pub async fn unsubscribe_wallet(&self) -> BybitWsResult<()> {
        self.unsubscribe(vec!["wallet".to_string()]).await
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
    pub async fn place_order(&self, params: BybitWsPlaceOrderParams) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to place orders".to_string(),
            ));
        }

        self.retry_manager
            .execute_with_retry_with_cancel(
                "place_order",
                || {
                    let params = params.clone();
                    async move {
                        let request = BybitWsRequest {
                            op: BybitWsOrderRequestOp::Create,
                            header: BybitWsHeader::now(),
                            args: vec![params],
                        };

                        let payload =
                            serde_json::to_string(&request).map_err(BybitWsError::from)?;
                        tracing::debug!("Sending order WebSocket message: {}", payload);
                        Self::send_text_inner(&self.inner, &payload).await
                    }
                },
                should_retry_bybit_error,
                create_bybit_timeout_error,
                &self.cancellation_token,
            )
            .await
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
    pub async fn amend_order(&self, params: BybitWsAmendOrderParams) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to amend orders".to_string(),
            ));
        }

        self.retry_manager
            .execute_with_retry_with_cancel(
                "amend_order",
                || {
                    let params = params.clone();
                    async move {
                        let request = BybitWsRequest {
                            op: BybitWsOrderRequestOp::Amend,
                            header: BybitWsHeader::now(),
                            args: vec![params],
                        };

                        let payload =
                            serde_json::to_string(&request).map_err(BybitWsError::from)?;
                        Self::send_text_inner(&self.inner, &payload).await
                    }
                },
                should_retry_bybit_error,
                create_bybit_timeout_error,
                &self.cancellation_token,
            )
            .await
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
    pub async fn cancel_order(&self, params: BybitWsCancelOrderParams) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to cancel orders".to_string(),
            ));
        }

        self.retry_manager
            .execute_with_retry_with_cancel(
                "cancel_order",
                || {
                    let params = params.clone();
                    async move {
                        let request = BybitWsRequest {
                            op: BybitWsOrderRequestOp::Cancel,
                            header: BybitWsHeader::now(),
                            args: vec![params],
                        };

                        let payload =
                            serde_json::to_string(&request).map_err(BybitWsError::from)?;
                        Self::send_text_inner(&self.inner, &payload).await
                    }
                },
                should_retry_bybit_error,
                create_bybit_timeout_error,
                &self.cancellation_token,
            )
            .await
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
        orders: Vec<BybitWsPlaceOrderParams>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to place orders".to_string(),
            ));
        }

        if orders.len() > 20 {
            return Err(BybitWsError::ClientError(
                "Batch order limit is 20 orders per request".to_string(),
            ));
        }

        let request = BybitWsRequest {
            op: BybitWsOrderRequestOp::CreateBatch,
            header: BybitWsHeader::now(),
            args: orders,
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        Self::send_text_inner(&self.inner, &payload).await
    }

    /// Batch amends multiple orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_amend_orders(
        &self,
        orders: Vec<BybitWsAmendOrderParams>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to amend orders".to_string(),
            ));
        }

        if orders.len() > 20 {
            return Err(BybitWsError::ClientError(
                "Batch amend limit is 20 orders per request".to_string(),
            ));
        }

        let request = BybitWsRequest {
            op: BybitWsOrderRequestOp::AmendBatch,
            header: BybitWsHeader::now(),
            args: orders,
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        Self::send_text_inner(&self.inner, &payload).await
    }

    /// Batch cancels multiple orders via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails or if not authenticated.
    pub async fn batch_cancel_orders(
        &self,
        orders: Vec<BybitWsCancelOrderParams>,
    ) -> BybitWsResult<()> {
        if !self.is_authenticated.load(Ordering::Relaxed) {
            return Err(BybitWsError::Authentication(
                "Must be authenticated to cancel orders".to_string(),
            ));
        }

        if orders.len() > 20 {
            return Err(BybitWsError::ClientError(
                "Batch cancel limit is 20 orders per request".to_string(),
            ));
        }

        let request = BybitWsRequest {
            op: BybitWsOrderRequestOp::CancelBatch,
            header: BybitWsHeader::now(),
            args: orders,
        };

        let payload = serde_json::to_string(&request).map_err(BybitWsError::from)?;
        Self::send_text_inner(&self.inner, &payload).await
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
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
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

        // Determine the base order type for Bybit API
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

        // If post_only is true, use PostOnly time in force, otherwise use provided time_in_force
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

        let params = if is_stop_order {
            // For conditional orders, ALL types use triggerPrice field
            // sl_trigger_price/tp_trigger_price are only for TP/SL attached to regular orders
            BybitWsPlaceOrderParams {
                category: product_type,
                symbol: raw_symbol,
                side: bybit_side,
                order_type: bybit_order_type,
                qty: quantity.to_string(),
                price: price.map(|p| p.to_string()),
                time_in_force: bybit_tif,
                order_link_id: Some(client_order_id.to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: trigger_price.map(|p| p.to_string()),
                trigger_by: Some(BybitTriggerType::LastPrice),
                trigger_direction: None,
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
                price: price.map(|p| p.to_string()),
                time_in_force: bybit_tif,
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
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
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
            order_link_id: client_order_id.map(|id| id.to_string()),
            qty: quantity.map(|q| q.to_string()),
            price: price.map(|p| p.to_string()),
            trigger_price: None,
            take_profit: None,
            stop_loss: None,
            tp_trigger_by: None,
            sl_trigger_by: None,
        };

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
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> BybitWsResult<()> {
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())
            .map_err(|e| BybitWsError::ClientError(e.to_string()))?;
        let raw_symbol = Ustr::from(bybit_symbol.raw_symbol());

        let params = BybitWsCancelOrderParams {
            category: product_type,
            symbol: raw_symbol,
            order_id: venue_order_id.map(|id| id.to_string()),
            order_link_id: client_order_id.map(|id| id.to_string()),
        };

        self.cancel_order(params).await
    }

    fn default_headers() -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Referer".to_string(), BYBIT_NAUTILUS_BROKER_ID.to_string()),
        ]
    }

    async fn authenticate_if_required(&self) -> BybitWsResult<()> {
        Self::authenticate_inner(
            &self.inner,
            self.requires_auth,
            self.credential.clone(),
            &self.auth_tracker,
            &self.is_authenticated,
        )
        .await
    }

    async fn send_text_inner(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        text: &str,
    ) -> BybitWsResult<()> {
        let guard = inner.read().await;
        let client = guard.as_ref().ok_or(BybitWsError::NotConnected)?;
        client
            .send_text(text.to_string(), None)
            .await
            .map_err(BybitWsError::from)
    }

    async fn send_pong_inner(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        payload: Vec<u8>,
    ) -> BybitWsResult<()> {
        let guard = inner.read().await;
        let client = guard.as_ref().ok_or(BybitWsError::NotConnected)?;
        client.send_pong(payload).await.map_err(BybitWsError::from)
    }

    async fn send_topics_inner(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        op: BybitWsOperation,
        topics: Vec<String>,
    ) -> BybitWsResult<()> {
        if topics.is_empty() {
            return Ok(());
        }

        for chunk in topics.chunks(MAX_ARGS_PER_SUBSCRIPTION_REQUEST) {
            let subscription = BybitSubscription {
                op: op.clone(),
                args: chunk.to_vec(),
            };
            let payload = serde_json::to_string(&subscription)?;
            Self::send_text_inner(inner, &payload).await?;
        }

        Ok(())
    }

    async fn resubscribe_all_inner(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        subscriptions: &SubscriptionState,
    ) -> BybitWsResult<()> {
        let topics = subscriptions.all_topics();
        if topics.is_empty() {
            return Ok(());
        }

        tracing::info!(
            "Restoring {} subscriptions after reconnection",
            topics.len()
        );
        Self::send_topics_inner(inner, BybitWsOperation::Subscribe, topics).await
    }

    async fn handle_message(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        subscriptions: &SubscriptionState,
        auth_tracker: &AuthTracker,
        requires_auth: bool,
        is_authenticated: &Arc<AtomicBool>,
        message: Message,
    ) -> BybitWsResult<Option<BybitWebSocketMessage>> {
        match message {
            Message::Text(text) => {
                tracing::trace!("Bybit WS message: {text}");

                if text == RECONNECTED {
                    tracing::debug!("Bybit websocket reconnected signal received");
                    return Ok(Some(BybitWebSocketMessage::Reconnected));
                }

                if text.trim().eq_ignore_ascii_case(BYBIT_PONG) {
                    return Ok(Some(BybitWebSocketMessage::Pong));
                }

                let value: Value = serde_json::from_str(&text).map_err(BybitWsError::from)?;

                // Handle ping/pong
                if let Ok(op) = serde_json::from_value::<BybitWsOperation>(
                    value.get("op").cloned().unwrap_or(Value::Null),
                ) {
                    match op {
                        BybitWsOperation::Ping => {
                            let pong = BybitSubscription {
                                op: BybitWsOperation::Pong,
                                args: vec![],
                            };
                            let payload = serde_json::to_string(&pong)?;
                            Self::send_text_inner(inner, &payload).await?;
                            return Ok(None);
                        }
                        BybitWsOperation::Pong => {
                            return Ok(Some(BybitWebSocketMessage::Pong));
                        }
                        _ => {}
                    }
                }

                if let Some(event) = Self::classify_message(&value) {
                    // Log raw JSON for error events to aid debugging
                    if matches!(event, BybitWebSocketMessage::Error(_)) {
                        tracing::debug!(
                            json = %serde_json::to_string(&value).unwrap_or_default(),
                            "Received error event from Bybit"
                        );
                    }

                    if let BybitWebSocketMessage::Auth(auth) = &event {
                        // Auth is successful if either success=true OR retCode=0
                        let is_success =
                            auth.success.unwrap_or(false) || auth.ret_code.unwrap_or(-1) == 0;

                        if is_success {
                            is_authenticated.store(true, Ordering::Relaxed);
                            auth_tracker.succeed();
                        } else {
                            is_authenticated.store(false, Ordering::Relaxed);
                            let message = auth
                                .ret_msg
                                .clone()
                                .unwrap_or_else(|| "Authentication failed".to_string());
                            auth_tracker.fail(message);
                        }
                    } else if let BybitWebSocketMessage::Subscription(sub_msg) = &event {
                        // Handle subscription/unsubscription confirmation
                        match sub_msg.op {
                            BybitWsOperation::Subscribe => {
                                let pending_topics = subscriptions.pending_subscribe_topics();
                                // Handle subscribe acknowledgment
                                if sub_msg.success {
                                    for topic in pending_topics {
                                        subscriptions.confirm_subscribe(&topic);
                                        tracing::debug!(topic = topic, "Subscription confirmed");
                                    }
                                } else {
                                    for topic in pending_topics {
                                        subscriptions.mark_failure(&topic);
                                        tracing::warn!(
                                            topic = topic,
                                            error = ?sub_msg.ret_msg,
                                            "Subscription failed, will retry on reconnect"
                                        );
                                    }
                                }
                            }
                            BybitWsOperation::Unsubscribe => {
                                let pending_topics = subscriptions.pending_unsubscribe_topics();
                                // Handle unsubscribe acknowledgment
                                if sub_msg.success {
                                    for topic in pending_topics {
                                        subscriptions.confirm_unsubscribe(&topic);
                                        tracing::debug!(topic = topic, "Unsubscription confirmed");
                                    }
                                } else {
                                    // Unsubscribe failed - venue still considers us subscribed
                                    // Clear from pending_unsubscribe and restore to confirmed
                                    for topic in pending_topics {
                                        subscriptions.confirm_unsubscribe(&topic); // Clear from pending_unsubscribe
                                        subscriptions.confirm_subscribe(&topic); // Restore to confirmed
                                        tracing::warn!(
                                            topic = topic,
                                            error = ?sub_msg.ret_msg,
                                            "Unsubscription failed, topic remains subscribed"
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if let BybitWebSocketMessage::Error(e) = &event
                        && requires_auth
                        && !is_authenticated.load(Ordering::Relaxed)
                    {
                        auth_tracker.fail(e.message.clone());
                    }
                    if let BybitWebSocketMessage::Error(e) = &event {
                        tracing::warn!(
                            code = e.code,
                            message = %e.message,
                            conn_id = ?e.conn_id,
                            topic = ?e.topic,
                            req_id = ?e.req_id,
                            "Bybit websocket error"
                        );
                    }
                    return Ok(Some(event));
                }

                Ok(Some(BybitWebSocketMessage::Raw(value)))
            }
            Message::Ping(payload) => {
                Self::send_pong_inner(inner, payload.to_vec()).await?;
                Ok(None)
            }
            Message::Pong(_) => Ok(Some(BybitWebSocketMessage::Pong)),
            Message::Binary(_) => Ok(None),
            Message::Close(_) => Ok(None),
            Message::Frame(_) => Ok(None),
        }
    }

    fn classify_message(value: &Value) -> Option<BybitWebSocketMessage> {
        // Check for auth response first (by op field) to avoid confusion with subscription messages
        if let Ok(op) = serde_json::from_value::<BybitWsOperation>(
            value.get("op").cloned().unwrap_or(Value::Null),
        ) && op == BybitWsOperation::Auth
        {
            tracing::debug!(json = %value, "Detected auth message by op field");
            if let Ok(auth) = serde_json::from_value::<BybitWsAuthResponse>(value.clone()) {
                // Auth is successful if either success=true OR retCode=0
                let is_success = auth.success.unwrap_or(false) || auth.ret_code.unwrap_or(-1) == 0;

                if is_success {
                    tracing::debug!("Auth successful, returning Auth message");
                    return Some(BybitWebSocketMessage::Auth(auth));
                }
                let resp = BybitWsResponse {
                    op: Some(auth.op.clone()),
                    topic: None,
                    success: auth.success,
                    conn_id: auth.conn_id.clone(),
                    req_id: None,
                    ret_code: auth.ret_code,
                    ret_msg: auth.ret_msg,
                };
                let error = BybitWebSocketError::from_response(&resp);
                return Some(BybitWebSocketMessage::Error(error));
            }
        }

        if let Some(success) = value.get("success").and_then(Value::as_bool) {
            if success {
                if let Ok(msg) = serde_json::from_value::<BybitWsSubscriptionMsg>(value.clone()) {
                    return Some(BybitWebSocketMessage::Subscription(msg));
                }
            } else if let Ok(resp) = serde_json::from_value::<BybitWsResponse>(value.clone()) {
                let error = BybitWebSocketError::from_response(&resp);
                return Some(BybitWebSocketMessage::Error(error));
            }
        }

        if (value.get("ret_code").is_some() || value.get("retCode").is_some())
            && let Ok(resp) = serde_json::from_value::<BybitWsResponse>(value.clone())
        {
            if resp.ret_code.unwrap_or_default() != 0 {
                let error = BybitWebSocketError::from_response(&resp);
                return Some(BybitWebSocketMessage::Error(error));
            }
            return Some(BybitWebSocketMessage::Response(resp));
        }

        if let Ok(auth) = serde_json::from_value::<BybitWsAuthResponse>(value.clone())
            && auth.op == BybitWsOperation::Auth
        {
            if auth.success.unwrap_or(false) {
                return Some(BybitWebSocketMessage::Auth(auth));
            }
            let resp = BybitWsResponse {
                op: Some(auth.op.clone()),
                topic: None,
                success: auth.success,
                conn_id: auth.conn_id.clone(),
                req_id: None,
                ret_code: auth.ret_code,
                ret_msg: auth.ret_msg,
            };
            let error = BybitWebSocketError::from_response(&resp);
            return Some(BybitWebSocketMessage::Error(error));
        }

        if let Some(topic) = value.get("topic").and_then(Value::as_str) {
            if topic.starts_with("orderbook") {
                if let Ok(msg) = serde_json::from_value::<BybitWsOrderbookDepthMsg>(value.clone()) {
                    return Some(BybitWebSocketMessage::Orderbook(msg));
                }
            } else if topic.contains("publicTrade") || topic.starts_with("trade") {
                if let Ok(msg) = serde_json::from_value::<BybitWsTradeMsg>(value.clone()) {
                    return Some(BybitWebSocketMessage::Trade(msg));
                }
            } else if topic.contains("kline") {
                if let Ok(msg) = serde_json::from_value::<BybitWsKlineMsg>(value.clone()) {
                    return Some(BybitWebSocketMessage::Kline(msg));
                }
            } else if topic.contains("tickers") {
                if let Ok(msg) = serde_json::from_value::<BybitWsTickerOptionMsg>(value.clone()) {
                    return Some(BybitWebSocketMessage::TickerOption(msg));
                }
                if let Ok(msg) = serde_json::from_value::<BybitWsTickerLinearMsg>(value.clone()) {
                    return Some(BybitWebSocketMessage::TickerLinear(msg));
                }
            } else if topic == "order" || topic.starts_with("order.") {
                match serde_json::from_value::<BybitWsAccountOrderMsg>(value.clone()) {
                    Ok(msg) => return Some(BybitWebSocketMessage::AccountOrder(msg)),
                    Err(e) => tracing::warn!("Failed to deserialize order message: {e}\n{value}"),
                }
            } else if topic == "execution" || topic.starts_with("execution.") {
                match serde_json::from_value::<BybitWsAccountExecutionMsg>(value.clone()) {
                    Ok(msg) => return Some(BybitWebSocketMessage::AccountExecution(msg)),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize execution message: {e}\n{value}");
                    }
                }
            } else if topic == "wallet" || topic.starts_with("wallet.") {
                match serde_json::from_value::<BybitWsAccountWalletMsg>(value.clone()) {
                    Ok(msg) => return Some(BybitWebSocketMessage::AccountWallet(msg)),
                    Err(e) => tracing::warn!("Failed to deserialize wallet message: {e}\n{value}"),
                }
            } else if topic == "position" || topic.starts_with("position.") {
                match serde_json::from_value::<BybitWsAccountPositionMsg>(value.clone()) {
                    Ok(msg) => return Some(BybitWebSocketMessage::AccountPosition(msg)),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize position message: {e}\n{value}");
                    }
                }
            }
        }

        None
    }

    async fn authenticate_inner(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        requires_auth: bool,
        credential: Option<Credential>,
        auth_tracker: &AuthTracker,
        is_authenticated: &Arc<AtomicBool>,
    ) -> BybitWsResult<()> {
        if !requires_auth {
            return Ok(());
        }

        is_authenticated.store(false, Ordering::Relaxed);

        let credential = credential.ok_or_else(|| {
            BybitWsError::Authentication(
                "API credentials not provided for authentication".to_string(),
            )
        })?;

        let receiver = auth_tracker.begin();

        let now_ns = get_atomic_clock_realtime().get_time_ns().as_i64();
        let now_ms = now_ns / 1_000_000;
        let expires = now_ms + WEBSOCKET_AUTH_WINDOW_MS;
        let signature = credential.sign_websocket_auth(expires);

        let auth_request = BybitAuthRequest {
            op: BybitWsOperation::Auth,
            args: vec![
                json!(credential.api_key().as_str()),
                json!(expires),
                json!(signature),
            ],
        };

        let payload = serde_json::to_string(&auth_request)?;

        if let Err(e) = Self::send_text_inner(inner, &payload).await {
            auth_tracker.fail(e.to_string());
            return Err(e);
        }

        match auth_tracker
            .wait_for_result(Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS), receiver)
            .await
        {
            Ok(()) => {
                is_authenticated.store(true, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                is_authenticated.store(false, Ordering::Relaxed);
                Err(e)
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn classify_orderbook_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json"))
            .expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected orderbook message");
        assert!(matches!(message, BybitWebSocketMessage::Orderbook(_)));
    }

    #[rstest]
    fn classify_trade_snapshot() {
        let json: Value =
            serde_json::from_str(&load_test_json("ws_public_trade.json")).expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected trade message");
        assert!(matches!(message, BybitWebSocketMessage::Trade(_)));
    }

    #[rstest]
    fn classify_ticker_linear_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_linear.json"))
            .expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected ticker message");
        assert!(matches!(message, BybitWebSocketMessage::TickerLinear(_)));
    }

    #[rstest]
    fn classify_ticker_option_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_option.json"))
            .expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected ticker message");
        assert!(matches!(message, BybitWebSocketMessage::TickerOption(_)));
    }
}
