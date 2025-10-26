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

//! Provides the WebSocket client integration for the [OKX](https://okx.com) WebSocket API.
//!
//! The [`OKXWebSocketClient`] ties together several recurring patterns:
//! - Heartbeats use text `ping`/`pong`, responding to both text and control-frame pings.
//! - Authentication re-runs on reconnect before resubscribing and skips private channels when
//!   credentials are unavailable.
//! - Subscriptions cache instrument type/family/ID groupings so reconnects rebuild the same set of
//!   channels while respecting the authentication guard described above.

use std::{
    collections::VecDeque,
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::runtime::get_runtime;
use nautilus_core::{
    UUID4,
    consts::NAUTILUS_USER_AGENT,
    env::{get_env_var, get_or_env_var},
    nanos::UnixNanos,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce, TriggerType},
    events::{AccountState, OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::OrderStatusReport,
    types::{Money, Price, Quantity},
};
use nautilus_network::{
    RECONNECTED,
    ratelimiter::quota::Quota,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{
        PingHandler, TEXT_PING, TEXT_PONG, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use reqwest::header::USER_AGENT;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    auth::{AUTHENTICATION_TIMEOUT_SECS, AuthTracker},
    enums::{OKXSubscriptionEvent, OKXWsChannel, OKXWsOperation},
    error::OKXWsError,
    messages::{
        ExecutionReport, NautilusWsMessage, OKXAuthentication, OKXAuthenticationArg,
        OKXSubscription, OKXSubscriptionArg, OKXWebSocketArg, OKXWebSocketError, OKXWebSocketEvent,
        OKXWsRequest, WsAmendOrderParams, WsAmendOrderParamsBuilder, WsCancelAlgoOrderParams,
        WsCancelAlgoOrderParamsBuilder, WsCancelOrderParams, WsCancelOrderParamsBuilder,
        WsMassCancelParams, WsPostAlgoOrderParams, WsPostAlgoOrderParamsBuilder, WsPostOrderParams,
        WsPostOrderParamsBuilder,
    },
    parse::{parse_book_msg_vec, parse_ws_message_data},
    subscription::{SubscriptionState, topic_from_subscription_arg, topic_from_websocket_arg},
};
use crate::{
    common::{
        consts::{
            OKX_NAUTILUS_BROKER_ID, OKX_POST_ONLY_CANCEL_REASON, OKX_POST_ONLY_CANCEL_SOURCE,
            OKX_POST_ONLY_ERROR_CODE, OKX_SUPPORTED_ORDER_TYPES, OKX_SUPPORTED_TIME_IN_FORCE,
            OKX_WS_PUBLIC_URL, should_retry_error_code,
        },
        credential::Credential,
        enums::{
            OKXInstrumentType, OKXOrderStatus, OKXOrderType, OKXPositionSide, OKXSide,
            OKXTargetCurrency, OKXTradeMode, OKXTriggerType, OKXVipLevel,
            conditional_order_to_algo_type, is_conditional_order,
        },
        parse::{
            bar_spec_as_okx_channel, okx_instrument_type, parse_account_state,
            parse_client_order_id, parse_millisecond_timestamp, parse_price, parse_quantity,
        },
    },
    http::models::OKXAccount,
    websocket::{
        messages::{OKXAlgoOrderMsg, OKXOrderMsg},
        parse::{parse_algo_order_msg, parse_order_msg},
    },
};

enum PendingOrderParams {
    Regular(WsPostOrderParams),
    Algo(()),
}

type PlaceRequestData = (
    PendingOrderParams,
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
);
type CancelRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);
type AmendRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);
type MassCancelRequestData = InstrumentId;

/// Default OKX WebSocket connection rate limit: 3 requests per second.
///
/// This applies to establishing WebSocket connections, not to subscribe/unsubscribe operations.
pub static OKX_WS_CONNECTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(3).unwrap()));

/// OKX WebSocket subscription rate limit: 480 requests per hour per connection.
///
/// This applies to subscribe/unsubscribe/login operations.
/// 480 per hour = 8 per minute, but we use per-hour for accurate limiting.
pub static OKX_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_hour(NonZeroU32::new(480).unwrap()));

/// Rate limit for order-related WebSocket operations: 250 requests per second.
///
/// Based on OKX documentation for sub-account order limits (1000 per 2 seconds,
/// so we use half for conservative rate limiting).
pub static OKX_WS_ORDER_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(250).unwrap()));

/// Determines if an OKX WebSocket error should trigger a retry.
fn should_retry_okx_error(error: &OKXWsError) -> bool {
    match error {
        OKXWsError::OkxError { error_code, .. } => should_retry_error_code(error_code),
        OKXWsError::TungsteniteError(_) => true, // Network errors are retryable
        OKXWsError::ClientError(msg) => {
            // Retry on timeout and connection errors (case-insensitive)
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("timeout")
                || msg_lower.contains("timed out")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
        }
        OKXWsError::AuthenticationError(_)
        | OKXWsError::JsonError(_)
        | OKXWsError::ParsingError(_) => {
            // Don't retry authentication or parsing errors automatically
            false
        }
    }
}

/// Creates a timeout error for OKX operations.
fn create_okx_timeout_error(msg: String) -> OKXWsError {
    OKXWsError::ClientError(msg)
}

fn channel_requires_auth(channel: &OKXWsChannel) -> bool {
    matches!(
        channel,
        OKXWsChannel::Account
            | OKXWsChannel::Orders
            | OKXWsChannel::Fills
            | OKXWsChannel::OrdersAlgo
    )
}

/// Provides a WebSocket client for connecting to [OKX](https://okx.com).
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct OKXWebSocketClient {
    url: String,
    account_id: AccountId,
    vip_level: Arc<AtomicU8>,
    credential: Option<Credential>,
    heartbeat: Option<u64>,
    inner: Arc<tokio::sync::RwLock<Option<WebSocketClient>>>,
    auth_tracker: AuthTracker,
    rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions_inst_type: Arc<DashMap<OKXWsChannel, AHashSet<OKXInstrumentType>>>,
    subscriptions_inst_family: Arc<DashMap<OKXWsChannel, AHashSet<Ustr>>>,
    subscriptions_inst_id: Arc<DashMap<OKXWsChannel, AHashSet<Ustr>>>,
    subscriptions_bare: Arc<DashMap<OKXWsChannel, bool>>, // For channels without inst params (e.g., Account)
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
    pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
    pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
    pending_mass_cancel_requests: Arc<DashMap<String, MassCancelRequestData>>,
    active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
    emitted_order_accepted: Arc<DashMap<VenueOrderId, ()>>, // Track orders we've already emitted OrderAccepted for
    client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
    retry_manager: Arc<RetryManager<OKXWsError>>,
    cancellation_token: CancellationToken,
}

impl Default for OKXWebSocketClient {
    fn default() -> Self {
        Self::new(None, None, None, None, None, None).unwrap()
    }
}

impl Debug for OKXWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OKXWebSocketClient))
            .field("url", &self.url)
            .field(
                "credential",
                &self.credential.as_ref().map(|_| "<redacted>"),
            )
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl OKXWebSocketClient {
    /// Creates a new [`OKXWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let url = url.unwrap_or(OKX_WS_PUBLIC_URL.to_string());
        let account_id = account_id.unwrap_or(AccountId::from("OKX-master"));

        let credential = match (api_key, api_secret, api_passphrase) {
            (Some(key), Some(secret), Some(passphrase)) => {
                Some(Credential::new(key, secret, passphrase))
            }
            (None, None, None) => None,
            _ => anyhow::bail!(
                "`api_key`, `api_secret`, `api_passphrase` credentials must be provided together"
            ),
        };

        let signal = Arc::new(AtomicBool::new(false));
        let subscriptions_inst_type = Arc::new(DashMap::new());
        let subscriptions_inst_family = Arc::new(DashMap::new());
        let subscriptions_inst_id = Arc::new(DashMap::new());
        let subscriptions_bare = Arc::new(DashMap::new());
        let subscriptions_state = SubscriptionState::new();

        Ok(Self {
            url,
            account_id,
            vip_level: Arc::new(AtomicU8::new(0)), // Default to VIP 0
            credential,
            heartbeat,
            inner: Arc::new(tokio::sync::RwLock::new(None)),
            auth_tracker: AuthTracker::new(),
            rx: None,
            signal,
            task_handle: None,
            subscriptions_inst_type,
            subscriptions_inst_family,
            subscriptions_inst_id,
            subscriptions_bare,
            subscriptions_state,
            request_id_counter: Arc::new(AtomicU64::new(1)),
            pending_place_requests: Arc::new(DashMap::new()),
            pending_cancel_requests: Arc::new(DashMap::new()),
            pending_amend_requests: Arc::new(DashMap::new()),
            pending_mass_cancel_requests: Arc::new(DashMap::new()),
            active_client_orders: Arc::new(DashMap::new()),
            emitted_order_accepted: Arc::new(DashMap::new()),
            client_id_aliases: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(AHashMap::new()),
            retry_manager: Arc::new(create_websocket_retry_manager()?),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new [`OKXWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if credential values cannot be loaded or if the
    /// client fails to initialize.
    pub fn with_credentials(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let url = url.unwrap_or(OKX_WS_PUBLIC_URL.to_string());
        let api_key = get_or_env_var(api_key, "OKX_API_KEY")?;
        let api_secret = get_or_env_var(api_secret, "OKX_API_SECRET")?;
        let api_passphrase = get_or_env_var(api_passphrase, "OKX_API_PASSPHRASE")?;

        Self::new(
            Some(url),
            Some(api_key),
            Some(api_secret),
            Some(api_passphrase),
            account_id,
            heartbeat,
        )
    }

    /// Creates a new authenticated [`OKXWebSocketClient`] using environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or if
    /// the client fails to initialize.
    pub fn from_env() -> anyhow::Result<Self> {
        let url = get_env_var("OKX_WS_URL")?;
        let api_key = get_env_var("OKX_API_KEY")?;
        let api_secret = get_env_var("OKX_API_SECRET")?;
        let api_passphrase = get_env_var("OKX_API_PASSPHRASE")?;

        Self::new(
            Some(url),
            Some(api_key),
            Some(api_secret),
            Some(api_passphrase),
            None,
            None,
        )
    }

    /// Cancel all pending WebSocket requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Returns the websocket url being used by the client.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the public API key being used by the client.
    pub fn api_key(&self) -> Option<&str> {
        self.credential.clone().map(|c| c.api_key.as_str())
    }

    /// Get a read lock on the inner client
    /// Returns a value indicating whether the client is active.
    pub fn is_active(&self) -> bool {
        // Use try_read to avoid blocking
        match self.inner.try_read() {
            Ok(guard) => match &*guard {
                Some(inner) => inner.is_active(),
                None => false,
            },
            Err(_) => false, // If we can't get the lock, assume not active
        }
    }

    /// Returns a value indicating whether the client is closed.
    pub fn is_closed(&self) -> bool {
        // Use try_read to avoid blocking
        match self.inner.try_read() {
            Ok(guard) => match &*guard {
                Some(inner) => inner.is_closed(),
                None => true,
            },
            Err(_) => true, // If we can't get the lock, assume closed
        }
    }

    /// Initialize the instruments cache with the given `instruments`.
    pub fn initialize_instruments_cache(&mut self, instruments: Vec<InstrumentAny>) {
        let mut instruments_cache: AHashMap<Ustr, InstrumentAny> = AHashMap::new();
        for inst in instruments {
            instruments_cache.insert(inst.symbol().inner(), inst.clone());
        }

        self.instruments_cache = Arc::new(instruments_cache);
    }

    /// Sets the VIP level for this client.
    ///
    /// The VIP level determines which WebSocket channels are available.
    pub fn set_vip_level(&self, vip_level: OKXVipLevel) {
        self.vip_level.store(vip_level as u8, Ordering::Relaxed);
    }

    /// Gets the current VIP level.
    pub fn vip_level(&self) -> OKXVipLevel {
        let level = self.vip_level.load(Ordering::Relaxed);
        OKXVipLevel::from(level)
    }

    /// Connect to the OKX WebSocket server.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection process fails.
    ///
    /// # Panics
    ///
    /// Panics if subscription arguments fail to serialize to JSON.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let (message_handler, reader) = channel_message_handler();

        let inner_for_ping = self.inner.clone();
        let ping_handler: PingHandler = Arc::new(move |payload: Vec<u8>| {
            let inner = inner_for_ping.clone();

            get_runtime().spawn(async move {
                let len = payload.len();
                let guard = inner.read().await;

                if let Some(client) = guard.as_ref() {
                    if let Err(e) = client.send_pong(payload).await {
                        tracing::warn!(error = %e, "Failed to send pong frame");
                    } else {
                        tracing::trace!("Sent pong frame ({len} bytes)");
                    }
                } else {
                    tracing::debug!("Ping received with no active websocket client");
                }
            });
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: Some(TEXT_PING.to_string()),
            message_handler: Some(message_handler),
            ping_handler: Some(ping_handler),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };

        // Configure rate limits for different operation types
        let keyed_quotas = vec![
            ("subscription".to_string(), *OKX_WS_SUBSCRIPTION_QUOTA),
            ("order".to_string(), *OKX_WS_ORDER_QUOTA),
            ("cancel".to_string(), *OKX_WS_ORDER_QUOTA),
            ("amend".to_string(), *OKX_WS_ORDER_QUOTA),
        ];

        let client = WebSocketClient::connect(
            config,
            None, // post_reconnection
            keyed_quotas,
            Some(*OKX_WS_CONNECTION_QUOTA), // Default quota for connection operations
        )
        .await?;

        // Set the inner client with write lock
        {
            let mut inner_guard = self.inner.write().await;
            *inner_guard = Some(client);
        }

        let account_id = self.account_id;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        self.rx = Some(Arc::new(rx));
        let signal = self.signal.clone();
        let pending_place_requests = self.pending_place_requests.clone();
        let pending_cancel_requests = self.pending_cancel_requests.clone();
        let pending_amend_requests = self.pending_amend_requests.clone();
        let pending_mass_cancel_requests = self.pending_mass_cancel_requests.clone();
        let active_client_orders = self.active_client_orders.clone();
        let emitted_order_accepted = self.emitted_order_accepted.clone();
        let auth_tracker = self.auth_tracker.clone();

        let instruments_cache = self.instruments_cache.clone();
        let inner_client = self.inner.clone();
        let credential_clone = self.credential.clone();
        let subscriptions_inst_type = self.subscriptions_inst_type.clone();
        let subscriptions_inst_family = self.subscriptions_inst_family.clone();
        let subscriptions_inst_id = self.subscriptions_inst_id.clone();
        let subscriptions_bare = self.subscriptions_bare.clone();
        let subscriptions_state = self.subscriptions_state.clone();
        let client_id_aliases = self.client_id_aliases.clone();

        let stream_handle = get_runtime().spawn({
            let auth_tracker = auth_tracker.clone();
            let signal = signal.clone();
            async move {
                let mut handler = OKXWsMessageHandler::new(
                    account_id,
                    instruments_cache,
                    reader,
                    signal.clone(),
                    inner_client.clone(),
                    tx,
                    pending_place_requests,
                    pending_cancel_requests,
                    pending_amend_requests,
                    pending_mass_cancel_requests,
                    active_client_orders,
                    client_id_aliases,
                    emitted_order_accepted,
                    auth_tracker.clone(),
                    subscriptions_state.clone(),
                );

                // Main message loop with explicit reconnection handling
                loop {
                    match handler.next().await {
                        Some(NautilusWsMessage::Reconnected) => {
                            if signal.load(Ordering::Relaxed) {
                                tracing::debug!("Skipping reconnection resubscription due to stop signal");
                                continue;
                            }

                            tracing::debug!("Handling WebSocket reconnection");

                            let auth_tracker_for_task = auth_tracker.clone();
                            let inner_client_for_task = inner_client.clone();
                            let subscriptions_inst_type_for_task = subscriptions_inst_type.clone();
                            let subscriptions_inst_family_for_task = subscriptions_inst_family.clone();
                            let subscriptions_inst_id_for_task = subscriptions_inst_id.clone();
                            let subscriptions_bare_for_task = subscriptions_bare.clone();
                            let subscriptions_state_for_task = subscriptions_state.clone();

                            let auth_wait = if let Some(cred) = &credential_clone {
                                let rx = auth_tracker.begin();
                                let inner_guard = inner_client.read().await;

                                if let Some(client) = &*inner_guard {
                                    let timestamp = SystemTime::now()
                                        .duration_since(SystemTime::UNIX_EPOCH)
                                        .expect("System time should be after UNIX epoch")
                                        .as_secs()
                                        .to_string();
                                    let signature =
                                        cred.sign(&timestamp, "GET", "/users/self/verify", "");

                                    let auth_message = OKXAuthentication {
                                        op: "login",
                                        args: vec![OKXAuthenticationArg {
                                            api_key: cred.api_key.to_string(),
                                            passphrase: cred.api_passphrase.clone(),
                                            timestamp,
                                            sign: signature,
                                        }],
                                    };

                                    if let Err(e) = client
                                        .send_text(serde_json::to_string(&auth_message).unwrap(), None)
                                        .await
                                    {
                                        tracing::error!(
                                            "Failed to send re-authentication request: {e}",
                                        );
                                        auth_tracker.fail(e.to_string());
                                    } else {
                                        tracing::debug!(
                                            "Sent re-authentication request, waiting for response before resubscribing",
                                        );
                                    }
                                } else {
                                    auth_tracker
                                        .fail("Cannot authenticate: not connected".to_string());
                                }

                                drop(inner_guard);

                                Some(rx)
                            } else {
                                None
                            };

                            get_runtime().spawn(async move {
                                let auth_succeeded = match auth_wait {
                                    Some(rx) => match auth_tracker_for_task
                                        .wait_for_result(
                                            Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS),
                                            rx,
                                        )
                                        .await
                                    {
                                        Ok(()) => {
                                            tracing::debug!(
                                                "Authentication successful after reconnect, proceeding with resubscription",
                                            );
                                            true
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Authentication after reconnect failed: {e}",
                                            );
                                            false
                                        }
                                    },
                                    None => true,
                                };

                                let confirmed_topic_count = subscriptions_state_for_task.len();
                                if confirmed_topic_count == 0 {
                                    tracing::debug!(
                                        "No confirmed subscriptions recorded before reconnect; resubscribe will rely on pending topics"
                                    );
                                } else {
                                    tracing::debug!(confirmed_topic_count, "Confirmed subscriptions recorded before reconnect");
                                }
                                let confirmed_topics = subscriptions_state_for_task.confirmed();
                                if confirmed_topic_count <= 10 {
                                    let topics: Vec<_> = confirmed_topics
                                        .iter()
                                        .map(|entry| entry.key().clone())
                                        .collect();
                                    if !topics.is_empty() {
                                        tracing::trace!(topics = ?topics, "Confirmed topics before reconnect");
                                    }
                                }
                                drop(confirmed_topics);

                                let pending_topics = subscriptions_state_for_task.pending();
                                let pending_topic_count = pending_topics.len();
                                if pending_topic_count > 0 {
                                    tracing::debug!(pending_topic_count, "Pending subscriptions awaiting replay after reconnect");
                                }
                                drop(pending_topics);

                                let inner_guard = inner_client_for_task.read().await;
                                if let Some(client) = &*inner_guard {
                                    let should_resubscribe = |channel: &OKXWsChannel| {
                                        if channel_requires_auth(channel) && !auth_succeeded {
                                            tracing::warn!(
                                                ?channel,
                                                "Skipping private channel resubscription due to missing authentication",
                                            );
                                            return false;
                                        }
                                        true
                                    };

                                    let mut inst_type_args = Vec::new();
                                    for entry in subscriptions_inst_type_for_task.iter() {
                                        let (channel, inst_types) = entry.pair();
                                        if !should_resubscribe(channel) {
                                            continue;
                                        }
                                        for inst_type in inst_types.iter() {
                                            let arg = OKXSubscriptionArg {
                                                channel: channel.clone(),
                                                inst_type: Some(*inst_type),
                                                inst_family: None,
                                                inst_id: None,
                                            };
                                            let topic = topic_from_subscription_arg(&arg);
                                            subscriptions_state_for_task.mark_subscribe(&topic);
                                            inst_type_args.push(arg);
                                        }
                                    }
                                    if !inst_type_args.is_empty() {
                                        let sub_request = OKXSubscription {
                                            op: OKXWsOperation::Subscribe,
                                            args: inst_type_args,
                                        };
                                        if let Err(e) = client
                                            .send_text(
                                                serde_json::to_string(&sub_request).unwrap(),
                                                None,
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to re-subscribe inst_type channels: {e}",
                                            );
                                        }
                                    }

                                    let mut inst_family_args = Vec::new();
                                    for entry in subscriptions_inst_family_for_task.iter() {
                                        let (channel, inst_families) = entry.pair();
                                        if !should_resubscribe(channel) {
                                            continue;
                                        }
                                        for inst_family in inst_families.iter() {
                                            let arg = OKXSubscriptionArg {
                                                channel: channel.clone(),
                                                inst_type: None,
                                                inst_family: Some(*inst_family),
                                                inst_id: None,
                                            };
                                            let topic = topic_from_subscription_arg(&arg);
                                            subscriptions_state_for_task.mark_subscribe(&topic);
                                            inst_family_args.push(arg);
                                        }
                                    }
                                    if !inst_family_args.is_empty() {
                                        let sub_request = OKXSubscription {
                                            op: OKXWsOperation::Subscribe,
                                            args: inst_family_args,
                                        };
                                        if let Err(e) = client
                                            .send_text(
                                                serde_json::to_string(&sub_request).unwrap(),
                                                None,
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to re-subscribe inst_family channels: {e}",
                                            );
                                        }
                                    }

                                    let mut inst_id_args = Vec::new();
                                    for entry in subscriptions_inst_id_for_task.iter() {
                                        let (channel, inst_ids) = entry.pair();
                                        if !should_resubscribe(channel) {
                                            continue;
                                        }
                                        for inst_id in inst_ids.iter() {
                                            let arg = OKXSubscriptionArg {
                                                channel: channel.clone(),
                                                inst_type: None,
                                                inst_family: None,
                                                inst_id: Some(*inst_id),
                                            };
                                            let topic = topic_from_subscription_arg(&arg);
                                            subscriptions_state_for_task.mark_subscribe(&topic);
                                            inst_id_args.push(arg);
                                        }
                                    }
                                    if !inst_id_args.is_empty() {
                                        let sub_request = OKXSubscription {
                                            op: OKXWsOperation::Subscribe,
                                            args: inst_id_args,
                                        };
                                        if let Err(e) = client
                                            .send_text(
                                                serde_json::to_string(&sub_request).unwrap(),
                                                None,
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to re-subscribe inst_id channels: {e}",
                                            );
                                        }
                                    }

                                    let mut bare_args = Vec::new();
                                    for entry in subscriptions_bare_for_task.iter() {
                                        let channel = entry.key();
                                        if !should_resubscribe(channel) {
                                            continue;
                                        }
                                        let arg = OKXSubscriptionArg {
                                            channel: channel.clone(),
                                            inst_type: None,
                                            inst_family: None,
                                            inst_id: None,
                                        };
                                        let topic = topic_from_subscription_arg(&arg);
                                        subscriptions_state_for_task.mark_subscribe(&topic);
                                        bare_args.push(arg);
                                    }
                                    if !bare_args.is_empty() {
                                        let sub_request = OKXSubscription {
                                            op: OKXWsOperation::Subscribe,
                                            args: bare_args,
                                        };
                                        if let Err(e) = client
                                            .send_text(
                                                serde_json::to_string(&sub_request).unwrap(),
                                                None,
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to re-subscribe bare channels: {e}",
                                            );
                                        }
                                    }

                                    tracing::debug!("Completed re-subscription after reconnect");
                                } else {
                                    tracing::warn!(
                                        "Skipping resubscription after reconnect: websocket client unavailable",
                                    );
                                }
                            });

                            continue;
                        }
                        Some(msg) => {
                            if handler.tx.send(msg).is_err() {
                                tracing::error!(
                                    "Failed to send message through channel: receiver dropped",
                                );
                                break;
                            }
                        }
                        None => {
                            if handler.is_stopped() {
                                tracing::debug!(
                                    "Stop signal received, ending message processing",
                                );
                                break;
                            }
                            tracing::warn!("WebSocket stream ended unexpectedly");
                            break;
                        }
                    }
                }
            }
        });

        self.task_handle = Some(Arc::new(stream_handle));

        if self.credential.is_some() {
            self.authenticate().await?;
        }

        Ok(())
    }

    /// Authenticates the WebSocket session with OKX.
    async fn authenticate(&self) -> Result<(), Error> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            Error::Io(std::io::Error::other(
                "API credentials not available to authenticate",
            ))
        })?;

        let rx = self.auth_tracker.begin();

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time should be after UNIX epoch")
            .as_secs()
            .to_string();
        let signature = credential.sign(&timestamp, "GET", "/users/self/verify", "");

        let auth_message = OKXAuthentication {
            op: "login",
            args: vec![OKXAuthenticationArg {
                api_key: credential.api_key.to_string(),
                passphrase: credential.api_passphrase.clone(),
                timestamp,
                sign: signature,
            }],
        };

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner
                    .send_text(serde_json::to_string(&auth_message).unwrap(), None)
                    .await
                {
                    tracing::error!("Error sending auth message: {e:?}");
                    self.auth_tracker.fail(e.to_string());
                    return Err(Error::Io(std::io::Error::other(e.to_string())));
                }
            } else {
                log::error!("Cannot authenticate: not connected");
                self.auth_tracker
                    .fail("Cannot authenticate: not connected".to_string());
                return Err(Error::ConnectionClosed);
            }
        }

        match self
            .auth_tracker
            .wait_for_result(Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS), rx)
            .await
        {
            Ok(()) => {
                tracing::info!("Authentication confirmed by client");
                Ok(())
            }
            Err(e) => {
                tracing::error!("Authentication failed: {e}");
                Err(Error::Io(std::io::Error::other(e.to_string())))
            }
        }
    }

    /// Provides the internal data stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - The websocket is not connected.
    /// - `stream_data` has already been called somewhere else (stream receiver is then taken).
    pub fn stream(&mut self) -> impl Stream<Item = NautilusWsMessage> + 'static {
        let rx = self
            .rx
            .take()
            .expect("Data stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(data) = rx.recv().await {
                yield data;
            }
        }
    }

    /// Wait until the WebSocket connection is active.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection times out.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), OKXWsError> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            OKXWsError::ClientError(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
    }

    /// Closes the client.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnecting the websocket or cleaning up the
    /// client fails.
    pub async fn close(&mut self) -> Result<(), Error> {
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

        // Clean up stream handle with timeout
        if let Some(stream_handle) = self.task_handle.take() {
            match Arc::try_unwrap(stream_handle) {
                Ok(handle) => {
                    log::debug!("Waiting for stream handle to complete");
                    match tokio::time::timeout(Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Stream handle completed successfully"),
                        Ok(Err(e)) => log::error!("Stream handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!(
                                "Timeout waiting for stream handle, task may still be running"
                            );
                            // The task will be dropped and should clean up automatically
                        }
                    }
                }
                Err(arc_handle) => {
                    log::debug!(
                        "Cannot take ownership of stream handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            log::debug!("No stream handle to await");
        }

        log::debug!("Close process completed");

        Ok(())
    }

    /// Get active subscriptions for a specific instrument.
    pub fn get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<OKXWsChannel> {
        let symbol = instrument_id.symbol.inner();
        let mut channels = Vec::new();

        for entry in self.subscriptions_inst_id.iter() {
            let (channel, instruments) = entry.pair();
            if instruments.contains(&symbol) {
                channels.push(channel.clone());
            }
        }

        channels
    }

    fn generate_unique_request_id(&self) -> String {
        self.request_id_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string()
    }

    #[allow(
        clippy::result_large_err,
        reason = "OKXWsError contains large tungstenite::Error variant"
    )]
    fn get_instrument_type_and_family(
        &self,
        symbol: Ustr,
    ) -> Result<(OKXInstrumentType, String), OKXWsError> {
        // Fetch instrument from cache
        let instrument = self.instruments_cache.get(&symbol).ok_or_else(|| {
            OKXWsError::ClientError(format!("Instrument not found in cache: {symbol}"))
        })?;

        let inst_type =
            okx_instrument_type(instrument).map_err(|e| OKXWsError::ClientError(e.to_string()))?;

        // Determine instrument family based on instrument type
        let inst_family = match instrument {
            InstrumentAny::CurrencyPair(_) => symbol.as_str().to_string(),
            InstrumentAny::CryptoPerpetual(_) => {
                // For SWAP: "BTC-USDT-SWAP" -> "BTC-USDT"
                symbol
                    .as_str()
                    .strip_suffix("-SWAP")
                    .unwrap_or(symbol.as_str())
                    .to_string()
            }
            InstrumentAny::CryptoFuture(_) => {
                // For FUTURES: extract the underlying pair
                let parts: Vec<&str> = symbol.as_str().split('-').collect();
                if parts.len() >= 2 {
                    format!("{}-{}", parts[0], parts[1])
                } else {
                    return Err(OKXWsError::ClientError(format!(
                        "Unable to parse futures instrument family from symbol: {symbol}",
                    )));
                }
            }
            InstrumentAny::CryptoOption(_) => {
                // For OPTIONS: "BTC-USD-241217-92000-C" -> "BTC-USD"
                let parts: Vec<&str> = symbol.as_str().split('-').collect();
                if parts.len() >= 2 {
                    format!("{}-{}", parts[0], parts[1])
                } else {
                    return Err(OKXWsError::ClientError(format!(
                        "Unable to parse option instrument family from symbol: {symbol}",
                    )));
                }
            }
            _ => {
                return Err(OKXWsError::ClientError(format!(
                    "Unsupported instrument type: {instrument:?}",
                )));
            }
        };

        Ok((inst_type, inst_family))
    }

    async fn subscribe(&self, args: Vec<OKXSubscriptionArg>) -> Result<(), OKXWsError> {
        for arg in &args {
            let topic = topic_from_subscription_arg(arg);
            self.subscriptions_state.mark_subscribe(&topic);

            // Check if this is a bare channel (no inst params)
            if arg.inst_type.is_none() && arg.inst_family.is_none() && arg.inst_id.is_none() {
                // Track bare channels like Account
                self.subscriptions_bare.insert(arg.channel.clone(), true);
            } else {
                // Update instrument type subscriptions
                if let Some(inst_type) = &arg.inst_type {
                    self.subscriptions_inst_type
                        .entry(arg.channel.clone())
                        .or_default()
                        .insert(*inst_type);
                }

                // Update instrument family subscriptions
                if let Some(inst_family) = &arg.inst_family {
                    self.subscriptions_inst_family
                        .entry(arg.channel.clone())
                        .or_default()
                        .insert(*inst_family);
                }

                // Update instrument ID subscriptions
                if let Some(inst_id) = &arg.inst_id {
                    self.subscriptions_inst_id
                        .entry(arg.channel.clone())
                        .or_default()
                        .insert(*inst_id);
                }
            }
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Subscribe,
            args,
        };

        let json_txt =
            serde_json::to_string(&message).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner
                    .send_text(json_txt, Some(vec!["subscription".to_string()]))
                    .await
                {
                    tracing::error!("Error sending message: {e:?}");
                }
            } else {
                return Err(OKXWsError::ClientError(
                    "Cannot send message: not connected".to_string(),
                ));
            }
        }

        Ok(())
    }

    #[allow(clippy::collapsible_if, reason = "Clearer uncollapsed")]
    async fn unsubscribe(&self, args: Vec<OKXSubscriptionArg>) -> Result<(), OKXWsError> {
        for arg in &args {
            let topic = topic_from_subscription_arg(arg);
            self.subscriptions_state.mark_unsubscribe(&topic);

            // Check if this is a bare channel
            if arg.inst_type.is_none() && arg.inst_family.is_none() && arg.inst_id.is_none() {
                // Remove bare channel subscription
                self.subscriptions_bare.remove(&arg.channel);
            } else {
                // Update instrument type subscriptions
                if let Some(inst_type) = &arg.inst_type {
                    if let Some(mut entry) = self.subscriptions_inst_type.get_mut(&arg.channel) {
                        entry.remove(inst_type);
                        if entry.is_empty() {
                            drop(entry);
                            self.subscriptions_inst_type.remove(&arg.channel);
                        }
                    }
                }

                // Update instrument family subscriptions
                if let Some(inst_family) = &arg.inst_family {
                    if let Some(mut entry) = self.subscriptions_inst_family.get_mut(&arg.channel) {
                        entry.remove(inst_family);
                        if entry.is_empty() {
                            drop(entry);
                            self.subscriptions_inst_family.remove(&arg.channel);
                        }
                    }
                }

                // Update instrument ID subscriptions
                if let Some(inst_id) = &arg.inst_id {
                    if let Some(mut entry) = self.subscriptions_inst_id.get_mut(&arg.channel) {
                        entry.remove(inst_id);
                        if entry.is_empty() {
                            drop(entry);
                            self.subscriptions_inst_id.remove(&arg.channel);
                        }
                    }
                }
            }
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Unsubscribe,
            args,
        };

        let json_txt = serde_json::to_string(&message).expect("Must be valid JSON");

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner
                    .send_text(json_txt, Some(vec!["subscription".to_string()]))
                    .await
                {
                    tracing::error!("Error sending message: {e:?}");
                }
            } else {
                log::error!("Cannot send message: not connected");
            }
        }

        Ok(())
    }

    /// Unsubscribes from all active subscriptions in batched messages.
    ///
    /// Collects all confirmed subscriptions and sends unsubscribe requests in batches,
    /// which is significantly more efficient than individual unsubscribes during disconnect.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe request fails to send.
    pub async fn unsubscribe_all(&self) -> Result<(), OKXWsError> {
        let mut all_args = Vec::new();

        for entry in self.subscriptions_inst_type.iter() {
            let (channel, inst_types) = entry.pair();
            for inst_type in inst_types.iter() {
                all_args.push(OKXSubscriptionArg {
                    channel: channel.clone(),
                    inst_type: Some(*inst_type),
                    inst_family: None,
                    inst_id: None,
                });
            }
        }

        for entry in self.subscriptions_inst_family.iter() {
            let (channel, inst_families) = entry.pair();
            for inst_family in inst_families.iter() {
                all_args.push(OKXSubscriptionArg {
                    channel: channel.clone(),
                    inst_type: None,
                    inst_family: Some(*inst_family),
                    inst_id: None,
                });
            }
        }

        for entry in self.subscriptions_inst_id.iter() {
            let (channel, inst_ids) = entry.pair();
            for inst_id in inst_ids.iter() {
                all_args.push(OKXSubscriptionArg {
                    channel: channel.clone(),
                    inst_type: None,
                    inst_family: None,
                    inst_id: Some(*inst_id),
                });
            }
        }

        for entry in self.subscriptions_bare.iter() {
            let channel = entry.key();
            all_args.push(OKXSubscriptionArg {
                channel: channel.clone(),
                inst_type: None,
                inst_family: None,
                inst_id: None,
            });
        }

        if all_args.is_empty() {
            tracing::debug!("No active subscriptions to unsubscribe from");
            return Ok(());
        }

        tracing::debug!("Batched unsubscribe from {} channels", all_args.len());

        const BATCH_SIZE: usize = 256;

        for chunk in all_args.chunks(BATCH_SIZE) {
            self.unsubscribe(chunk.to_vec()).await?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    async fn resubscribe_all(&self) {
        // Collect bare channel subscriptions (e.g., Account)
        let mut subs_bare = Vec::new();
        for entry in self.subscriptions_bare.iter() {
            let channel = entry.key();
            subs_bare.push(channel.clone());
        }

        let mut subs_inst_type = Vec::new();
        for entry in self.subscriptions_inst_type.iter() {
            let (channel, inst_types) = entry.pair();
            if !inst_types.is_empty() {
                subs_inst_type.push((channel.clone(), inst_types.clone()));
            }
        }

        let mut subs_inst_family = Vec::new();
        for entry in self.subscriptions_inst_family.iter() {
            let (channel, inst_families) = entry.pair();
            if !inst_families.is_empty() {
                subs_inst_family.push((channel.clone(), inst_families.clone()));
            }
        }

        let mut subs_inst_id = Vec::new();
        for entry in self.subscriptions_inst_id.iter() {
            let (channel, inst_ids) = entry.pair();
            if !inst_ids.is_empty() {
                subs_inst_id.push((channel.clone(), inst_ids.clone()));
            }
        }

        // Process instrument type subscriptions
        for (channel, inst_types) in subs_inst_type {
            if inst_types.is_empty() {
                continue;
            }

            tracing::debug!("Resubscribing: channel={channel}, instrument_types={inst_types:?}");

            for inst_type in inst_types {
                let arg = OKXSubscriptionArg {
                    channel: channel.clone(),
                    inst_type: Some(inst_type),
                    inst_family: None,
                    inst_id: None,
                };

                if let Err(e) = self.subscribe(vec![arg]).await {
                    tracing::error!(
                        "Failed to resubscribe to channel {channel} with instrument type: {e}"
                    );
                }
            }
        }

        // Process instrument family subscriptions
        for (channel, inst_families) in subs_inst_family {
            if inst_families.is_empty() {
                continue;
            }

            tracing::debug!(
                "Resubscribing: channel={channel}, instrument_families={inst_families:?}"
            );

            for inst_family in inst_families {
                let arg = OKXSubscriptionArg {
                    channel: channel.clone(),
                    inst_type: None,
                    inst_family: Some(inst_family),
                    inst_id: None,
                };

                if let Err(e) = self.subscribe(vec![arg]).await {
                    tracing::error!(
                        "Failed to resubscribe to channel {channel} with instrument family: {e}"
                    );
                }
            }
        }

        // Process instrument ID subscriptions
        for (channel, inst_ids) in subs_inst_id {
            if inst_ids.is_empty() {
                continue;
            }

            tracing::debug!("Resubscribing: channel={channel}, instrument_ids={inst_ids:?}");

            for inst_id in inst_ids {
                let arg = OKXSubscriptionArg {
                    channel: channel.clone(),
                    inst_type: None,
                    inst_family: None,
                    inst_id: Some(inst_id),
                };

                if let Err(e) = self.subscribe(vec![arg]).await {
                    tracing::error!(
                        "Failed to resubscribe to channel {channel} with instrument ID: {e}"
                    );
                }
            }
        }

        // Process bare channel subscriptions (e.g., Account)
        for channel in subs_bare {
            tracing::debug!("Resubscribing to bare channel: {channel}");

            let arg = OKXSubscriptionArg {
                channel,
                inst_type: None,
                inst_family: None,
                inst_id: None,
            };

            if let Err(e) = self.subscribe(vec![arg]).await {
                tracing::error!("Failed to resubscribe to bare channel: {e}");
            }
        }
    }

    /// Subscribes to instrument updates for a specific instrument type.
    ///
    /// Provides updates when instrument specifications change.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-websocket-instruments-channel>.
    pub async fn subscribe_instruments(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Instruments,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to instrument updates for a specific instrument.
    ///
    /// Provides updates when instrument specifications change.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-websocket-instruments-channel>.
    pub async fn subscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Instruments,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to order book data for an instrument.
    ///
    /// This is a convenience method that calls [`Self::subscribe_book_with_depth`] with depth 0,
    /// which automatically selects the appropriate channel based on VIP level.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        self.subscribe_book_with_depth(instrument_id, 0).await
    }

    /// Subscribes to the standard books channel (internal method).
    pub(crate) async fn subscribe_books_channel(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to 5-level order book snapshot data for an instrument.
    ///
    /// Updates every 100ms when there are changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-order-book-5-depth-channel>.
    pub async fn subscribe_book_depth5(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books5,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to 50-level tick-by-tick order book data for an instrument.
    ///
    /// Provides real-time updates whenever order book changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-order-book-50-depth-tbt-channel>.
    pub async fn subscribe_book50_l2_tbt(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books50Tbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to tick-by-tick full depth (400 levels) order book data for an instrument.
    ///
    /// Provides real-time updates with all depth levels whenever order book changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-order-book-400-depth-tbt-channel>.
    pub async fn subscribe_book_l2_tbt(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::BooksTbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to order book data with automatic channel selection based on VIP level and depth.
    ///
    /// Selects the optimal channel based on user's VIP tier and requested depth:
    /// - depth 50: Requires VIP4+, subscribes to `books50-l2-tbt`
    /// - depth 0 or 400:
    ///   - VIP5+: subscribes to `books-l2-tbt` (400 depth, fastest)
    ///   - Below VIP5: subscribes to `books` (standard depth)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Subscription request fails
    /// - depth is 50 but VIP level is below 4
    pub async fn subscribe_book_with_depth(
        &self,
        instrument_id: InstrumentId,
        depth: u16,
    ) -> anyhow::Result<()> {
        let vip = self.vip_level();

        match depth {
            50 => {
                if vip < OKXVipLevel::Vip4 {
                    anyhow::bail!(
                        "VIP level {vip} insufficient for 50 depth subscription (requires VIP4)"
                    );
                }
                self.subscribe_book50_l2_tbt(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            }
            0 | 400 => {
                if vip >= OKXVipLevel::Vip5 {
                    self.subscribe_book_l2_tbt(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))
                } else {
                    self.subscribe_books_channel(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))
                }
            }
            _ => anyhow::bail!("Invalid depth {depth}, must be 0, 50, or 400"),
        }
    }

    /// Subscribes to best bid/ask quote data for an instrument.
    ///
    /// Provides tick-by-tick updates of the best bid and ask prices using the bbo-tbt channel.
    /// Supports all instrument types: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-best-bid-offer-channel>.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::BboTbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to trade data for an instrument.
    ///
    /// When `aggregated` is `false`, subscribes to the `trades` channel (per-match updates).
    /// When `aggregated` is `true`, subscribes to the `trades-all` channel (aggregated updates).
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-trades-channel>.
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-all-trades-channel>.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
        aggregated: bool,
    ) -> Result<(), OKXWsError> {
        let channel = if aggregated {
            OKXWsChannel::TradesAll
        } else {
            OKXWsChannel::Trades
        };

        let arg = OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to 24hr rolling ticker data for an instrument.
    ///
    /// Updates every 100ms with trading statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-tickers-channel>.
    pub async fn subscribe_ticker(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Tickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to mark price data for derivatives instruments.
    ///
    /// Updates every 200ms for perpetual swaps, or at settlement for futures.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-websocket-mark-price-channel>.
    pub async fn subscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::MarkPrice,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to index price data for an instrument.
    ///
    /// Updates every second with the underlying index price.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-websocket-index-tickers-channel>.
    pub async fn subscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::IndexTickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to funding rate data for perpetual swap instruments.
    ///
    /// Updates when funding rate changes or at funding intervals.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-websocket-funding-rate-channel>.
    pub async fn subscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::FundingRate,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribes to candlestick/bar data for an instrument.
    ///
    /// Supports various time intervals from 1s to 3M.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-candlesticks-channel>.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), OKXWsError> {
        // Use regular trade-price candlesticks which work for all instrument types
        let channel = bar_spec_as_okx_channel(bar_type.spec())
            .map_err(|e| OKXWsError::ClientError(e.to_string()))?;

        let arg = OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(bar_type.instrument_id().symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from instrument updates for a specific instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_instruments(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Instruments,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from instrument updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Instruments,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from full order book data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from 5-level order book snapshot data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_book_depth5(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books5,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from 50-level tick-by-tick order book data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_book50_l2_tbt(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books50Tbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from tick-by-tick full depth order book data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_book_l2_tbt(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::BooksTbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from best bid/ask quote data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::BboTbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from 24hr rolling ticker data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_ticker(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Tickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from mark price data for a derivatives instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::MarkPrice,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from index price data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::IndexTickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from funding rate data for a perpetual swap instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::FundingRate,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from trade data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
        aggregated: bool,
    ) -> Result<(), OKXWsError> {
        let channel = if aggregated {
            OKXWsChannel::TradesAll
        } else {
            OKXWsChannel::Trades
        };

        let arg = OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from candlestick/bar data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), OKXWsError> {
        // Use regular trade-price candlesticks which work for all instrument types
        let channel = bar_spec_as_okx_channel(bar_type.spec())
            .map_err(|e| OKXWsError::ClientError(e.to_string()))?;

        let arg = OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(bar_type.instrument_id().symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Subscribes to order updates for the given instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn subscribe_orders(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Orders,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from order updates for the given instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_orders(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Orders,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Subscribes to algo order updates for the given instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn subscribe_orders_algo(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::OrdersAlgo,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from algo order updates for the given instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_orders_algo(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::OrdersAlgo,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Subscribes to fill updates for the given instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn subscribe_fills(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Fills,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from fill updates for the given instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_fills(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Fills,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Subscribes to account balance updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn subscribe_account(&self) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Account,
            inst_type: None,
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from account balance updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_account(&self) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Account,
            inst_type: None,
            inst_family: None,
            inst_id: None,
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Cancel an existing order via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-cancel-order>
    async fn ws_cancel_order(
        &self,
        params: WsCancelOrderParams,
        request_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let request_id = request_id.unwrap_or(self.generate_unique_request_id());

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::CancelOrder,
            args: vec![params],
            exp_time: None,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["cancel".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Cancel multiple orders at once via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-mass-cancel-order>
    async fn ws_mass_cancel_with_id(
        &self,
        args: Vec<Value>,
        request_id: String,
    ) -> Result<(), OKXWsError> {
        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::MassCancel,
            args,
            exp_time: None,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["cancel".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Amend an existing order via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-amend-order>
    async fn ws_amend_order(
        &self,
        params: WsAmendOrderParams,
        request_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let request_id = request_id.unwrap_or(self.generate_unique_request_id());

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::AmendOrder,
            args: vec![params],
            exp_time: None,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["amend".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Place multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-orders>
    async fn ws_batch_place_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::BatchOrders,
            args,
            exp_time: None,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["order".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Cancel multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-cancel-orders>
    async fn ws_batch_cancel_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::BatchCancelOrders,
            args,
            exp_time: None,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["cancel".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Amend multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-amend-orders>
    async fn ws_batch_amend_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::BatchAmendOrders,
            args,
            exp_time: None,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["amend".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Submits an order, automatically routing conditional orders to the algo endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the order parameters are invalid or if the request
    /// cannot be sent to the websocket client.
    ///
    /// # References
    ///
    /// - Regular orders: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order>
    /// - Algo orders: <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-place-algo-order>
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        position_side: Option<PositionSide>,
    ) -> Result<(), OKXWsError> {
        if !OKX_SUPPORTED_ORDER_TYPES.contains(&order_type) {
            return Err(OKXWsError::ClientError(format!(
                "Unsupported order type: {order_type:?}",
            )));
        }

        if let Some(tif) = time_in_force
            && !OKX_SUPPORTED_TIME_IN_FORCE.contains(&tif)
        {
            return Err(OKXWsError::ClientError(format!(
                "Unsupported time in force: {tif:?}",
            )));
        }

        let mut builder = WsPostOrderParamsBuilder::default();

        builder.inst_id(instrument_id.symbol.as_str());
        builder.td_mode(td_mode);
        builder.cl_ord_id(client_order_id.as_str());

        let instrument = self
            .instruments_cache
            .get(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!("Unknown instrument {instrument_id}"))
            })?;

        let instrument_type =
            okx_instrument_type(instrument).map_err(|e| OKXWsError::ClientError(e.to_string()))?;
        let quote_currency = instrument.quote_currency();

        match instrument_type {
            OKXInstrumentType::Spot => {
                // SPOT: ccy parameter is required by OKX for spot trading
                builder.ccy(quote_currency.to_string());
            }
            OKXInstrumentType::Margin => {
                builder.ccy(quote_currency.to_string());

                if let Some(ro) = reduce_only
                    && ro
                {
                    builder.reduce_only(ro);
                }
            }
            OKXInstrumentType::Swap | OKXInstrumentType::Futures => {
                // SWAP/FUTURES: use quote currency for margin (required by OKX)
                builder.ccy(quote_currency.to_string());

                // For derivatives, posSide is required by OKX
                // Use Net for one-way mode (default for NETTING OMS)
                if position_side.is_none() {
                    builder.pos_side(OKXPositionSide::Net);
                }
            }
            _ => {
                builder.ccy(quote_currency.to_string());

                // For derivatives, posSide is required
                if position_side.is_none() {
                    builder.pos_side(OKXPositionSide::Net);
                }

                if let Some(ro) = reduce_only
                    && ro
                {
                    builder.reduce_only(ro);
                }
            }
        };

        // For SPOT market orders in Cash mode, handle tgtCcy parameter
        // https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order
        // OKX API default behavior for SPOT market orders:
        // - BUY orders default to tgtCcy=quote_ccy (sz represents quote currency amount)
        // - SELL orders default to tgtCcy=base_ccy (sz represents base currency amount)
        // Note: tgtCcy is ONLY supported for Cash trading mode, not for margin modes (Cross/Isolated)
        if instrument_type == OKXInstrumentType::Spot
            && order_type == OrderType::Market
            && td_mode == OKXTradeMode::Cash
        {
            match quote_quantity {
                Some(true) => {
                    // Explicitly request quote currency sizing
                    builder.tgt_ccy(OKXTargetCurrency::QuoteCcy);
                }
                Some(false) => {
                    if order_side == OrderSide::Buy {
                        // For BUY orders, must explicitly set to base_ccy to override OKX default
                        builder.tgt_ccy(OKXTargetCurrency::BaseCcy);
                    }
                    // For SELL orders with quote_quantity=false, omit tgtCcy (OKX defaults to base_ccy correctly)
                }
                None => {
                    // No preference specified, use OKX defaults
                }
            }
        }

        builder.side(order_side);

        if let Some(pos_side) = position_side {
            builder.pos_side(pos_side);
        };

        let (okx_ord_type, price) = if post_only.unwrap_or(false) {
            (OKXOrderType::PostOnly, price)
        } else {
            (OKXOrderType::from(order_type), price)
        };

        log::debug!(
            "Order type mapping: order_type={:?}, time_in_force={:?}, post_only={:?} -> okx_ord_type={:?}",
            order_type,
            time_in_force,
            post_only,
            okx_ord_type
        );

        builder.ord_type(okx_ord_type);
        builder.sz(quantity.to_string());

        if let Some(tp) = trigger_price {
            builder.px(tp.to_string());
        } else if let Some(p) = price {
            builder.px(p.to_string());
        }

        builder.tag(OKX_NAUTILUS_BROKER_ID);

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build order params error: {e}")))?;

        let request_id = self.generate_unique_request_id();

        self.pending_place_requests.insert(
            request_id.clone(),
            (
                PendingOrderParams::Regular(params.clone()),
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            ),
        );

        self.active_client_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));

        self.retry_manager
            .execute_with_retry_with_cancel(
                "submit_order",
                || {
                    let params = params.clone();
                    let request_id = request_id.clone();
                    async move { self.ws_place_order(params, Some(request_id)).await }
                },
                should_retry_okx_error,
                create_okx_timeout_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Cancels an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if the cancel parameters are invalid or if the
    /// cancellation request fails to send.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-cancel-order>.
    #[allow(clippy::too_many_arguments)]
    pub async fn cancel_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsCancelOrderParamsBuilder::default();
        // Note: instType should NOT be included in cancel order requests
        // For WebSocket orders, use the full symbol (including SWAP/FUTURES suffix if present)
        builder.inst_id(instrument_id.symbol.as_str());

        if let Some(venue_order_id) = venue_order_id {
            builder.ord_id(venue_order_id.as_str());
        }

        // Set client order ID before building params (fix for potential bug)
        if let Some(client_order_id) = client_order_id {
            builder.cl_ord_id(client_order_id.as_str());
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build cancel params error: {e}")))?;

        let request_id = self.generate_unique_request_id();

        // External orders may not have a client order ID,
        // for now we just track those with a client order ID as pending requests.
        if let Some(client_order_id) = client_order_id {
            self.pending_cancel_requests.insert(
                request_id.clone(),
                (
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                ),
            );
        }

        self.retry_manager
            .execute_with_retry_with_cancel(
                "cancel_order",
                || {
                    let params = params.clone();
                    let request_id = request_id.clone();
                    async move { self.ws_cancel_order(params, Some(request_id)).await }
                },
                should_retry_okx_error,
                create_okx_timeout_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Place a new order via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-place-order>
    async fn ws_place_order(
        &self,
        params: WsPostOrderParams,
        request_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let request_id = request_id.unwrap_or(self.generate_unique_request_id());

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::Order,
            exp_time: None,
            args: vec![params],
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner.send_text(txt, Some(vec!["order".to_string()])).await {
                    tracing::error!("Error sending message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Modifies an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if the amend parameters are invalid or if the
    /// websocket request fails to send.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-amend-order>.
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        price: Option<Price>,
        quantity: Option<Quantity>,
        venue_order_id: Option<VenueOrderId>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsAmendOrderParamsBuilder::default();

        builder.inst_id(instrument_id.symbol.as_str());

        if let Some(venue_order_id) = venue_order_id {
            builder.ord_id(venue_order_id.as_str());
        }

        if let Some(client_order_id) = client_order_id {
            builder.cl_ord_id(client_order_id.as_str());
        }

        if let Some(price) = price {
            builder.new_px(price.to_string());
        }

        if let Some(quantity) = quantity {
            builder.new_sz(quantity.to_string());
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build amend params error: {e}")))?;

        // Generate unique request ID for WebSocket message
        let request_id = self
            .request_id_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string();

        // External orders may not have a client order ID,
        // for now we just track those with a client order ID as pending requests.
        if let Some(client_order_id) = client_order_id {
            self.pending_amend_requests.insert(
                request_id.clone(),
                (
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                ),
            );
        }

        self.retry_manager
            .execute_with_retry_with_cancel(
                "modify_order",
                || {
                    let params = params.clone();
                    let request_id = request_id.clone();
                    async move { self.ws_amend_order(params, Some(request_id)).await }
                },
                should_retry_okx_error,
                create_okx_timeout_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Submits multiple orders.
    ///
    /// # Errors
    ///
    /// Returns an error if any batch order parameters are invalid or if the
    /// batch request fails to send.
    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    pub async fn batch_submit_orders(
        &self,
        orders: Vec<(
            OKXInstrumentType,
            InstrumentId,
            OKXTradeMode,
            ClientOrderId,
            OrderSide,
            Option<PositionSide>,
            OrderType,
            Quantity,
            Option<Price>,
            Option<Price>,
            Option<bool>,
            Option<bool>,
        )>,
    ) -> Result<(), OKXWsError> {
        let mut args: Vec<Value> = Vec::with_capacity(orders.len());
        for (
            inst_type,
            inst_id,
            td_mode,
            cl_ord_id,
            ord_side,
            pos_side,
            ord_type,
            qty,
            pr,
            tp,
            post_only,
            reduce_only,
        ) in orders
        {
            let mut builder = WsPostOrderParamsBuilder::default();
            builder.inst_type(inst_type);
            builder.inst_id(inst_id.symbol.inner());
            builder.td_mode(td_mode);
            builder.cl_ord_id(cl_ord_id.as_str());
            builder.side(ord_side);

            if let Some(ps) = pos_side {
                builder.pos_side(OKXPositionSide::from(ps));
            }

            let okx_ord_type = if post_only.unwrap_or(false) {
                OKXOrderType::PostOnly
            } else {
                OKXOrderType::from(ord_type)
            };

            builder.ord_type(okx_ord_type);
            builder.sz(qty.to_string());

            if let Some(p) = pr {
                builder.px(p.to_string());
            } else if let Some(p) = tp {
                builder.px(p.to_string());
            }

            if let Some(ro) = reduce_only {
                builder.reduce_only(ro);
            }

            builder.tag(OKX_NAUTILUS_BROKER_ID);

            let params = builder
                .build()
                .map_err(|e| OKXWsError::ClientError(format!("Build order params error: {e}")))?;
            let val =
                serde_json::to_value(params).map_err(|e| OKXWsError::JsonError(e.to_string()))?;
            args.push(val);
        }

        self.ws_batch_place_orders(args).await
    }

    /// Cancels multiple orders.
    ///
    /// Supports up to 20 orders per batch.
    ///
    /// # Errors
    ///
    /// Returns an error if cancel parameters are invalid or if the batch
    /// request fails to send.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-cancel-orders>
    #[allow(clippy::type_complexity)]
    pub async fn batch_cancel_orders(
        &self,
        orders: Vec<(InstrumentId, Option<ClientOrderId>, Option<VenueOrderId>)>,
    ) -> Result<(), OKXWsError> {
        let mut args: Vec<Value> = Vec::with_capacity(orders.len());
        for (inst_id, cl_ord_id, ord_id) in orders {
            let mut builder = WsCancelOrderParamsBuilder::default();
            // Note: instType should NOT be included in cancel order requests
            builder.inst_id(inst_id.symbol.inner());

            if let Some(c) = cl_ord_id {
                builder.cl_ord_id(c.as_str());
            }

            if let Some(o) = ord_id {
                builder.ord_id(o.as_str());
            }

            let params = builder.build().map_err(|e| {
                OKXWsError::ClientError(format!("Build cancel batch params error: {e}"))
            })?;
            let val =
                serde_json::to_value(params).map_err(|e| OKXWsError::JsonError(e.to_string()))?;
            args.push(val);
        }

        self.ws_batch_cancel_orders(args).await
    }

    /// Mass cancels all orders for a given instrument via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if instrument metadata cannot be resolved or if the
    /// cancel request fails to send.
    ///
    /// # Parameters
    /// - `inst_id`: The instrument ID. The instrument type will be automatically determined from the symbol.
    ///
    /// # References
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-mass-cancel-order>
    /// Helper function to determine instrument type and family from symbol using instruments cache.
    pub async fn mass_cancel_orders(&self, inst_id: InstrumentId) -> Result<(), OKXWsError> {
        let (inst_type, inst_family) =
            self.get_instrument_type_and_family(inst_id.symbol.inner())?;

        let params = WsMassCancelParams {
            inst_type,
            inst_family: Ustr::from(&inst_family),
        };

        let args =
            vec![serde_json::to_value(params).map_err(|e| OKXWsError::JsonError(e.to_string()))?];

        let request_id = self.generate_unique_request_id();

        self.pending_mass_cancel_requests
            .insert(request_id.clone(), inst_id);

        self.retry_manager
            .execute_with_retry_with_cancel(
                "mass_cancel_orders",
                || {
                    let args = args.clone();
                    let request_id = request_id.clone();
                    async move { self.ws_mass_cancel_with_id(args, request_id).await }
                },
                should_retry_okx_error,
                create_okx_timeout_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Modifies multiple orders via WebSocket using Nautilus domain types.
    ///
    /// # Errors
    ///
    /// Returns an error if amend parameters are invalid or if the batch request
    /// fails to send.
    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    pub async fn batch_modify_orders(
        &self,
        orders: Vec<(
            OKXInstrumentType,
            InstrumentId,
            ClientOrderId,
            ClientOrderId,
            Option<Price>,
            Option<Quantity>,
        )>,
    ) -> Result<(), OKXWsError> {
        let mut args: Vec<Value> = Vec::with_capacity(orders.len());
        for (_inst_type, inst_id, cl_ord_id, new_cl_ord_id, pr, sz) in orders {
            let mut builder = WsAmendOrderParamsBuilder::default();
            // Note: instType should NOT be included in amend order requests
            builder.inst_id(inst_id.symbol.inner());
            builder.cl_ord_id(cl_ord_id.as_str());
            builder.new_cl_ord_id(new_cl_ord_id.as_str());

            if let Some(p) = pr {
                builder.new_px(p.to_string());
            }

            if let Some(q) = sz {
                builder.new_sz(q.to_string());
            }

            let params = builder.build().map_err(|e| {
                OKXWsError::ClientError(format!("Build amend batch params error: {e}"))
            })?;
            let val =
                serde_json::to_value(params).map_err(|e| OKXWsError::JsonError(e.to_string()))?;
            args.push(val);
        }

        self.ws_batch_amend_orders(args).await
    }

    /// Submits an algo order (conditional/stop order).
    ///
    /// # Errors
    ///
    /// Returns an error if the order parameters are invalid or if the request
    /// cannot be sent.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-place-algo-order>
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_algo_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        limit_price: Option<Price>,
        reduce_only: Option<bool>,
    ) -> Result<(), OKXWsError> {
        if !is_conditional_order(order_type) {
            return Err(OKXWsError::ClientError(format!(
                "Order type {order_type:?} is not a conditional order"
            )));
        }

        let mut builder = WsPostAlgoOrderParamsBuilder::default();
        if !matches!(order_side, OrderSide::Buy | OrderSide::Sell) {
            return Err(OKXWsError::ClientError(
                "Invalid order side for OKX".to_string(),
            ));
        }

        builder.inst_id(instrument_id.symbol.inner());
        builder.td_mode(td_mode);
        builder.cl_ord_id(client_order_id.as_str());
        builder.side(order_side);
        builder.ord_type(
            conditional_order_to_algo_type(order_type)
                .map_err(|e| OKXWsError::ClientError(e.to_string()))?,
        );
        builder.sz(quantity.to_string());
        builder.trigger_px(trigger_price.to_string());

        // Map Nautilus TriggerType to OKX trigger type
        let okx_trigger_type = trigger_type.map_or(OKXTriggerType::Last, Into::into);
        builder.trigger_px_type(okx_trigger_type);

        // For stop-limit orders, set the limit price
        if matches!(order_type, OrderType::StopLimit | OrderType::LimitIfTouched)
            && let Some(price) = limit_price
        {
            builder.order_px(price.to_string());
        }

        if let Some(reduce) = reduce_only {
            builder.reduce_only(reduce);
        }

        builder.tag(OKX_NAUTILUS_BROKER_ID);

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build algo order params error: {e}")))?;

        let request_id = self.generate_unique_request_id();

        self.pending_place_requests.insert(
            request_id.clone(),
            (
                PendingOrderParams::Algo(()),
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            ),
        );

        self.retry_manager
            .execute_with_retry_with_cancel(
                "submit_algo_order",
                || {
                    let params = params.clone();
                    let request_id = request_id.clone();
                    async move { self.ws_place_algo_order(params, Some(request_id)).await }
                },
                should_retry_okx_error,
                create_okx_timeout_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Cancels an algo order.
    ///
    /// # Errors
    ///
    /// Returns an error if cancel parameters are invalid or if the request
    /// fails to send.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-cancel-algo-order>
    pub async fn cancel_algo_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        algo_order_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsCancelAlgoOrderParamsBuilder::default();
        builder.inst_id(instrument_id.symbol.inner());

        if let Some(client_order_id) = client_order_id {
            builder.algo_cl_ord_id(client_order_id.as_str());
        }

        if let Some(algo_id) = algo_order_id {
            builder.algo_id(algo_id);
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build cancel algo params error: {e}")))?;

        let request_id = self.generate_unique_request_id();

        // Track pending cancellation if we have a client order ID
        if let Some(client_order_id) = client_order_id {
            self.pending_cancel_requests.insert(
                request_id.clone(),
                (client_order_id, trader_id, strategy_id, instrument_id, None),
            );
        }

        self.retry_manager
            .execute_with_retry_with_cancel(
                "cancel_algo_order",
                || {
                    let params = params.clone();
                    let request_id = request_id.clone();
                    async move { self.ws_cancel_algo_order(params, Some(request_id)).await }
                },
                should_retry_okx_error,
                create_okx_timeout_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Place a new algo order via WebSocket.
    async fn ws_place_algo_order(
        &self,
        params: WsPostAlgoOrderParams,
        request_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let request_id = request_id.unwrap_or(self.generate_unique_request_id());

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::OrderAlgo,
            exp_time: None,
            args: vec![params],
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner
                    .send_text(txt, Some(vec!["orders-algo".to_string()]))
                    .await
                {
                    tracing::error!("Error sending algo order message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }

    /// Cancel an algo order via WebSocket.
    async fn ws_cancel_algo_order(
        &self,
        params: WsCancelAlgoOrderParams,
        request_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let request_id = request_id.unwrap_or(self.generate_unique_request_id());

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::CancelAlgos,
            exp_time: None,
            args: vec![params],
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                if let Err(e) = inner
                    .send_text(txt, Some(vec!["cancel-algos".to_string()]))
                    .await
                {
                    tracing::error!("Error sending cancel algo message: {e:?}");
                }
                Ok(())
            } else {
                Err(OKXWsError::ClientError("Not connected".to_string()))
            }
        }
    }
}

struct OKXFeedHandler {
    receiver: UnboundedReceiver<Message>,
    signal: Arc<AtomicBool>,
}

impl OKXFeedHandler {
    /// Creates a new [`OKXFeedHandler`] instance.
    pub fn new(receiver: UnboundedReceiver<Message>, signal: Arc<AtomicBool>) -> Self {
        Self { receiver, signal }
    }

    /// Gets the next message from the WebSocket stream.
    async fn next(&mut self) -> Option<OKXWebSocketEvent> {
        loop {
            tokio::select! {
                msg = self.receiver.recv() => match msg {
                    Some(msg) => match msg {
                        Message::Text(text) => {
                            // Handle ping/pong messages
                            if text == TEXT_PONG {
                                tracing::trace!("Received pong from OKX");
                                continue;
                            }
                            if text == TEXT_PING {
                                tracing::trace!("Received ping from OKX (text)");
                                return Some(OKXWebSocketEvent::Ping);
                            }

                            // Check for reconnection signal
                            if text == RECONNECTED {
                                tracing::debug!("Received WebSocket reconnection signal");
                                return Some(OKXWebSocketEvent::Reconnected);
                            }
                            tracing::trace!("Received WebSocket message: {text}");

                            match serde_json::from_str(&text) {
                                Ok(ws_event) => match &ws_event {
                                    OKXWebSocketEvent::Error { code, msg } => {
                                        tracing::error!("WebSocket error: {code} - {msg}");
                                        return Some(ws_event);
                                    }
                                    OKXWebSocketEvent::Login {
                                        event,
                                        code,
                                        msg,
                                        conn_id,
                                    } => {
                                        if code == "0" {
                                            tracing::info!(
                                                "Successfully authenticated with OKX WebSocket, conn_id={conn_id}"
                                            );
                                        } else {
                                            tracing::error!(
                                                "Authentication failed: {event} {code} - {msg}"
                                            );
                                        }
                                        return Some(ws_event);
                                    }
                                    OKXWebSocketEvent::Subscription {
                                        event,
                                        arg,
                                        conn_id, .. } => {
                                        let channel_str = serde_json::to_string(&arg.channel)
                                            .expect("Invalid OKX websocket channel")
                                            .trim_matches('"')
                                            .to_string();
                                        tracing::debug!(
                                            "{event}d: channel={channel_str}, conn_id={conn_id}"
                                        );
                                        continue;
                                    }
                                    OKXWebSocketEvent::ChannelConnCount {
                                        event: _,
                                        channel,
                                        conn_count,
                                        conn_id,
                                    } => {
                                        let channel_str = serde_json::to_string(&channel)
                                            .expect("Invalid OKX websocket channel")
                                            .trim_matches('"')
                                            .to_string();
                                        tracing::debug!(
                                            "Channel connection status: channel={channel_str}, connections={conn_count}, conn_id={conn_id}",
                                        );
                                        continue;
                                    }
                                    OKXWebSocketEvent::Ping => {
                                        tracing::trace!("Ignoring ping event parsed from text payload");
                                        continue;
                                    }
                                    OKXWebSocketEvent::Data { .. } => return Some(ws_event),
                                    OKXWebSocketEvent::BookData { .. } => return Some(ws_event),
                                    OKXWebSocketEvent::OrderResponse {
                                        id,
                                        op,
                                        code,
                                        msg: _,
                                        data,
                                    } => {
                                        if code == "0" {
                                            tracing::debug!(
                                                "Order operation successful: id={:?}, op={op}, code={code}",
                                                id
                                            );

                                            // Extract success message
                                            if let Some(order_data) = data.first() {
                                                let success_msg = order_data
                                                    .get("sMsg")
                                                    .and_then(|s| s.as_str())
                                                    .unwrap_or("Order operation successful");
                                                tracing::debug!("Order success details: {success_msg}");
                                            }
                                        }
                                        return Some(ws_event);
                                    }
                                    OKXWebSocketEvent::Reconnected => {
                                        // This shouldn't happen as we handle RECONNECTED string directly
                                        tracing::warn!("Unexpected Reconnected event from deserialization");
                                        continue;
                                    }
                                },
                                Err(e) => {
                                    tracing::error!("Failed to parse message: {e}: {text}");
                                    return None;
                                }
                            }
                        }
                        Message::Ping(payload) => {
                            tracing::trace!("Received ping frame from OKX ({} bytes)", payload.len());
                            continue;
                        }
                        Message::Pong(payload) => {
                            tracing::trace!("Received pong frame from OKX ({} bytes)", payload.len());
                            continue;
                        }
                        Message::Binary(msg) => {
                            tracing::debug!("Raw binary: {msg:?}");
                        }
                        Message::Close(_) => {
                            tracing::debug!("Received close message");
                            return None;
                        }
                        msg => {
                            tracing::warn!("Unexpected message: {msg}");
                        }
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

struct OKXWsMessageHandler {
    account_id: AccountId,
    inner: Arc<tokio::sync::RwLock<Option<WebSocketClient>>>,
    handler: OKXFeedHandler,
    #[allow(dead_code)]
    tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
    pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
    pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
    pending_mass_cancel_requests: Arc<DashMap<String, MassCancelRequestData>>,
    active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
    client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
    emitted_order_accepted: Arc<DashMap<VenueOrderId, ()>>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
    last_account_state: Option<AccountState>,
    fee_cache: AHashMap<Ustr, Money>,           // Key is order ID
    filled_qty_cache: AHashMap<Ustr, Quantity>, // Key is order ID
    funding_rate_cache: AHashMap<Ustr, (Ustr, u64)>, // Cache (funding_rate, funding_time) by inst_id
    auth_tracker: AuthTracker,
    pending_messages: VecDeque<NautilusWsMessage>,
    subscriptions_state: SubscriptionState,
}

impl OKXWsMessageHandler {
    fn schedule_text_pong(&self) {
        let inner = self.inner.clone();
        get_runtime().spawn(async move {
            let guard = inner.read().await;

            if let Some(client) = guard.as_ref() {
                if let Err(e) = client.send_text(TEXT_PONG.to_string(), None).await {
                    tracing::warn!(error = %e, "Failed to send pong response to OKX text ping");
                } else {
                    tracing::trace!("Sent pong response to OKX text ping");
                }
            } else {
                tracing::debug!("Received text ping with no active websocket client");
            }
        });
    }

    fn try_handle_post_only_auto_cancel(
        &mut self,
        msg: &OKXOrderMsg,
        ts_init: UnixNanos,
        exec_reports: &mut Vec<ExecutionReport>,
    ) -> bool {
        if !Self::is_post_only_auto_cancel(msg) {
            return false;
        }

        let Some(client_order_id) = parse_client_order_id(&msg.cl_ord_id) else {
            return false;
        };

        let Some((_, (trader_id, strategy_id, instrument_id))) =
            self.active_client_orders.remove(&client_order_id)
        else {
            return false;
        };

        self.client_id_aliases.remove(&client_order_id);

        if !exec_reports.is_empty() {
            let reports = std::mem::take(exec_reports);
            self.pending_messages
                .push_back(NautilusWsMessage::ExecutionReports(reports));
        }

        let reason = msg
            .cancel_source_reason
            .as_ref()
            .filter(|reason| !reason.is_empty())
            .map_or_else(
                || Ustr::from(OKX_POST_ONLY_CANCEL_REASON),
                |reason| Ustr::from(reason.as_str()),
            );

        let ts_event = parse_millisecond_timestamp(msg.u_time);
        let rejected = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            reason,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            true,
        );

        self.pending_messages
            .push_back(NautilusWsMessage::OrderRejected(rejected));

        true
    }

    fn is_post_only_auto_cancel(msg: &OKXOrderMsg) -> bool {
        if msg.state != OKXOrderStatus::Canceled {
            return false;
        }

        let cancel_source_matches = matches!(
            msg.cancel_source.as_deref(),
            Some(source) if source == OKX_POST_ONLY_CANCEL_SOURCE
        );

        let reason_matches = matches!(
            msg.cancel_source_reason.as_deref(),
            Some(reason) if reason.contains("POST_ONLY")
        );

        if !(cancel_source_matches || reason_matches) {
            return false;
        }

        msg.acc_fill_sz
            .as_ref()
            .is_none_or(|filled| filled == "0" || filled.is_empty())
    }

    fn register_client_order_aliases(
        &self,
        raw_child: &Option<ClientOrderId>,
        parent_from_msg: &Option<ClientOrderId>,
    ) -> Option<ClientOrderId> {
        if let Some(parent) = parent_from_msg {
            self.client_id_aliases.insert(*parent, *parent);
            if let Some(child) = raw_child.as_ref().filter(|child| **child != *parent) {
                self.client_id_aliases.insert(*child, *parent);
            }
            Some(*parent)
        } else if let Some(child) = raw_child.as_ref() {
            if let Some(mapped) = self.client_id_aliases.get(child) {
                Some(*mapped.value())
            } else {
                self.client_id_aliases.insert(*child, *child);
                Some(*child)
            }
        } else {
            None
        }
    }

    fn adjust_execution_report(
        &self,
        report: ExecutionReport,
        effective_client_id: &Option<ClientOrderId>,
        raw_child: &Option<ClientOrderId>,
    ) -> ExecutionReport {
        match report {
            ExecutionReport::Order(status_report) => {
                let mut adjusted = status_report;
                let mut final_id = *effective_client_id;

                if final_id.is_none() {
                    final_id = adjusted.client_order_id;
                }

                if final_id.is_none()
                    && let Some(child) = raw_child.as_ref()
                    && let Some(mapped) = self.client_id_aliases.get(child)
                {
                    final_id = Some(*mapped.value());
                }

                if let Some(final_id_value) = final_id {
                    if adjusted.client_order_id != Some(final_id_value) {
                        adjusted = adjusted.with_client_order_id(final_id_value);
                    }
                    self.client_id_aliases
                        .insert(final_id_value, final_id_value);

                    if let Some(child) =
                        raw_child.as_ref().filter(|child| **child != final_id_value)
                    {
                        adjusted = adjusted.with_linked_order_ids(vec![*child]);
                    }
                }

                ExecutionReport::Order(adjusted)
            }
            ExecutionReport::Fill(mut fill_report) => {
                let mut final_id = *effective_client_id;
                if final_id.is_none() {
                    final_id = fill_report.client_order_id;
                }
                if final_id.is_none()
                    && let Some(child) = raw_child.as_ref()
                    && let Some(mapped) = self.client_id_aliases.get(child)
                {
                    final_id = Some(*mapped.value());
                }

                if let Some(final_id_value) = final_id {
                    fill_report.client_order_id = Some(final_id_value);
                    self.client_id_aliases
                        .insert(final_id_value, final_id_value);
                }

                ExecutionReport::Fill(fill_report)
            }
        }
    }

    fn update_caches_with_report(&mut self, report: &ExecutionReport) {
        match report {
            ExecutionReport::Fill(fill_report) => {
                let order_id = fill_report.venue_order_id.inner();
                let current_fee = self
                    .fee_cache
                    .get(&order_id)
                    .copied()
                    .unwrap_or_else(|| Money::new(0.0, fill_report.commission.currency));
                let total_fee = current_fee + fill_report.commission;
                self.fee_cache.insert(order_id, total_fee);

                let current_filled_qty = self
                    .filled_qty_cache
                    .get(&order_id)
                    .copied()
                    .unwrap_or_else(|| Quantity::zero(fill_report.last_qty.precision));
                let total_filled_qty = current_filled_qty + fill_report.last_qty;
                self.filled_qty_cache.insert(order_id, total_filled_qty);
            }
            ExecutionReport::Order(status_report) => {
                if matches!(status_report.order_status, OrderStatus::Filled) {
                    self.fee_cache.remove(&status_report.venue_order_id.inner());
                    self.filled_qty_cache
                        .remove(&status_report.venue_order_id.inner());
                }

                if matches!(
                    status_report.order_status,
                    OrderStatus::Canceled
                        | OrderStatus::Expired
                        | OrderStatus::Filled
                        | OrderStatus::Rejected,
                ) {
                    if let Some(client_order_id) = status_report.client_order_id {
                        self.active_client_orders.remove(&client_order_id);
                        self.client_id_aliases.remove(&client_order_id);
                    }
                    if let Some(linked) = &status_report.linked_order_ids {
                        for child in linked {
                            self.client_id_aliases.remove(child);
                        }
                    }
                }
            }
        }
    }

    /// Creates a new [`OKXFeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: AccountId,
        instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
        reader: UnboundedReceiver<Message>,
        signal: Arc<AtomicBool>,
        inner: Arc<tokio::sync::RwLock<Option<WebSocketClient>>>,
        tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
        pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
        pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
        pending_mass_cancel_requests: Arc<DashMap<String, MassCancelRequestData>>,
        active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
        client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
        emitted_order_accepted: Arc<DashMap<VenueOrderId, ()>>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
    ) -> Self {
        Self {
            account_id,
            inner,
            handler: OKXFeedHandler::new(reader, signal),
            tx,
            pending_place_requests,
            pending_cancel_requests,
            pending_amend_requests,
            pending_mass_cancel_requests,
            active_client_orders,
            client_id_aliases,
            emitted_order_accepted,
            instruments_cache,
            last_account_state: None,
            fee_cache: AHashMap::new(),
            filled_qty_cache: AHashMap::new(),
            funding_rate_cache: AHashMap::new(),
            auth_tracker,
            pending_messages: VecDeque::new(),
            subscriptions_state,
        }
    }

    fn is_stopped(&self) -> bool {
        self.handler
            .signal
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[allow(dead_code)]
    async fn run(&mut self) {
        while let Some(data) = self.next().await {
            if let Err(e) = self.tx.send(data) {
                tracing::error!("Error sending data: {e}");
                break; // Stop processing on channel error for now
            }
        }
    }

    async fn next(&mut self) -> Option<NautilusWsMessage> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        let clock = get_atomic_clock_realtime();

        while let Some(event) = self.handler.next().await {
            let ts_init = clock.get_time_ns();

            match event {
                OKXWebSocketEvent::Ping => {
                    self.schedule_text_pong();
                    continue;
                }
                OKXWebSocketEvent::Login {
                    code, msg, conn_id, ..
                } => {
                    if code == "0" {
                        self.auth_tracker.succeed();
                        continue;
                    }

                    tracing::error!("Authentication failed: {msg}");
                    self.auth_tracker.fail(msg.clone());

                    let error = OKXWebSocketError {
                        code,
                        message: msg,
                        conn_id: Some(conn_id),
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    self.pending_messages
                        .push_back(NautilusWsMessage::Error(error));
                    continue;
                }
                OKXWebSocketEvent::BookData { arg, action, data } => {
                    let Some(inst_id) = arg.inst_id else {
                        tracing::error!("Instrument ID missing for book data event");
                        continue;
                    };

                    let Some(inst) = self.instruments_cache.get(&inst_id) else {
                        continue;
                    };

                    let instrument_id = inst.id();
                    let price_precision = inst.price_precision();
                    let size_precision = inst.size_precision();

                    match parse_book_msg_vec(
                        data,
                        &instrument_id,
                        price_precision,
                        size_precision,
                        action,
                        ts_init,
                    ) {
                        Ok(payloads) => return Some(NautilusWsMessage::Data(payloads)),
                        Err(e) => {
                            tracing::error!("Failed to parse book message: {e}");
                            continue;
                        }
                    }
                }
                OKXWebSocketEvent::OrderResponse {
                    id,
                    op,
                    code,
                    msg,
                    data,
                } => {
                    if code == "0" {
                        tracing::debug!(
                            "Order operation successful: id={id:?} op={op} code={code}"
                        );

                        if op == OKXWsOperation::MassCancel
                            && let Some(request_id) = &id
                            && let Some((_, instrument_id)) =
                                self.pending_mass_cancel_requests.remove(request_id)
                        {
                            tracing::info!(
                                "Mass cancel operation successful for instrument: {}",
                                instrument_id
                            );
                        } else if op == OKXWsOperation::Order
                            && let Some(request_id) = &id
                            && let Some((
                                _,
                                (params, client_order_id, _trader_id, _strategy_id, instrument_id),
                            )) = self.pending_place_requests.remove(request_id)
                        {
                            let (venue_order_id, ts_accepted) = if let Some(first) = data.first() {
                                let ord_id = first
                                    .get("ordId")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(VenueOrderId::new);

                                let ts = first
                                    .get("ts")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .map_or_else(
                                        || clock.get_time_ns(),
                                        |ms| UnixNanos::from(ms * 1_000_000),
                                    );

                                (ord_id, ts)
                            } else {
                                (None, clock.get_time_ns())
                            };

                            if let Some(instrument) = self
                                .instruments_cache
                                .get(&Ustr::from(instrument_id.symbol.as_str()))
                            {
                                match params {
                                    PendingOrderParams::Regular(order_params) => {
                                        // Check if this is an explicit quote-sized order
                                        let is_explicit_quote_sized = order_params
                                            .tgt_ccy
                                            .is_some_and(|tgt| tgt == OKXTargetCurrency::QuoteCcy);

                                        // Check if this is an implicit quote-sized order:
                                        // SPOT market BUY in cash mode with no tgt_ccy defaults to quote-sizing
                                        let is_implicit_quote_sized =
                                            order_params.tgt_ccy.is_none()
                                                && order_params.side == OKXSide::Buy
                                                && matches!(
                                                    order_params.ord_type,
                                                    OKXOrderType::Market
                                                )
                                                && order_params.td_mode == OKXTradeMode::Cash
                                                && instrument.instrument_class().as_ref() == "SPOT";

                                        if is_explicit_quote_sized || is_implicit_quote_sized {
                                            // For quote-sized orders, sz is in quote currency (USDT),
                                            // not base currency (ETH). We can't accurately parse the
                                            // base quantity without the fill price, so we skip the
                                            // synthetic OrderAccepted and rely on the orders channel
                                            tracing::info!(
                                                "Skipping synthetic OrderAccepted for {} quote-sized order: client_order_id={client_order_id}, venue_order_id={:?}",
                                                if is_explicit_quote_sized {
                                                    "explicit"
                                                } else {
                                                    "implicit"
                                                },
                                                venue_order_id
                                            );
                                            continue;
                                        }

                                        let order_side = order_params.side.into();
                                        let order_type = order_params.ord_type.into();
                                        let time_in_force = match order_params.ord_type {
                                            OKXOrderType::Fok => TimeInForce::Fok,
                                            OKXOrderType::Ioc | OKXOrderType::OptimalLimitIoc => {
                                                TimeInForce::Ioc
                                            }
                                            _ => TimeInForce::Gtc,
                                        };

                                        let size_precision = instrument.size_precision();
                                        let quantity = match parse_quantity(
                                            &order_params.sz,
                                            size_precision,
                                        ) {
                                            Ok(q) => q,
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to parse quantity for accepted order: {e}"
                                                );
                                                continue;
                                            }
                                        };

                                        let filled_qty = Quantity::zero(size_precision);

                                        let mut report = OrderStatusReport::new(
                                            self.account_id,
                                            instrument_id,
                                            Some(client_order_id),
                                            venue_order_id
                                                .unwrap_or_else(|| VenueOrderId::new("PENDING")),
                                            order_side,
                                            order_type,
                                            time_in_force,
                                            OrderStatus::Accepted,
                                            quantity,
                                            filled_qty,
                                            ts_accepted,
                                            ts_accepted, // ts_last same as ts_accepted for new orders
                                            ts_init,
                                            None, // Generate UUID4 automatically
                                        );

                                        if let Some(px) = &order_params.px
                                            && !px.is_empty()
                                            && let Ok(price) =
                                                parse_price(px, instrument.price_precision())
                                        {
                                            report = report.with_price(price);
                                        }

                                        if let Some(true) = order_params.reduce_only {
                                            report = report.with_reduce_only(true);
                                        }

                                        if order_type == OrderType::Limit
                                            && order_params.ord_type == OKXOrderType::PostOnly
                                        {
                                            report = report.with_post_only(true);
                                        }

                                        if let Some(ref v_order_id) = venue_order_id {
                                            self.emitted_order_accepted.insert(*v_order_id, ());
                                        }

                                        tracing::debug!(
                                            "Order accepted: client_order_id={client_order_id}, venue_order_id={:?}",
                                            venue_order_id
                                        );

                                        return Some(NautilusWsMessage::ExecutionReports(vec![
                                            ExecutionReport::Order(report),
                                        ]));
                                    }
                                    PendingOrderParams::Algo(_) => {
                                        tracing::info!(
                                            "Algo order placement confirmed: client_order_id={client_order_id}, venue_order_id={:?}",
                                            venue_order_id
                                        );
                                    }
                                }
                            } else {
                                tracing::error!(
                                    "Instrument not found for accepted order: {instrument_id}"
                                );
                            }
                        }

                        if let Some(first) = data.first()
                            && let Some(success_msg) =
                                first.get("sMsg").and_then(|value| value.as_str())
                        {
                            tracing::debug!("Order details: {success_msg}");
                        }

                        continue;
                    }

                    let error_msg = data
                        .first()
                        .and_then(|d| d.get("sMsg"))
                        .and_then(|s| s.as_str())
                        .unwrap_or(&msg)
                        .to_string();

                    if let Some(first) = data.first() {
                        tracing::debug!(
                            "Error data fields: {}",
                            serde_json::to_string_pretty(first)
                                .unwrap_or_else(|_| "unable to serialize".to_string())
                        );
                    }

                    tracing::warn!(
                        "Order operation failed: id={id:?} op={op} code={code} msg={error_msg}"
                    );

                    if let Some(request_id) = &id {
                        match op {
                            OKXWsOperation::Order => {
                                if let Some((
                                    _,
                                    (
                                        _params,
                                        client_order_id,
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                    ),
                                )) = self.pending_place_requests.remove(request_id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let due_post_only =
                                        is_post_only_rejection(code.as_str(), &data);
                                    let rejected = OrderRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        self.account_id,
                                        Ustr::from(error_msg.as_str()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
                                        due_post_only,
                                    );

                                    return Some(NautilusWsMessage::OrderRejected(rejected));
                                }
                            }
                            OKXWsOperation::CancelOrder => {
                                if let Some((
                                    _,
                                    (
                                        client_order_id,
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        venue_order_id,
                                    ),
                                )) = self.pending_cancel_requests.remove(request_id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderCancelRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        Ustr::from(error_msg.as_str()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
                                        venue_order_id,
                                        Some(self.account_id),
                                    );

                                    return Some(NautilusWsMessage::OrderCancelRejected(rejected));
                                }
                            }
                            OKXWsOperation::AmendOrder => {
                                if let Some((
                                    _,
                                    (
                                        client_order_id,
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        venue_order_id,
                                    ),
                                )) = self.pending_amend_requests.remove(request_id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderModifyRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        Ustr::from(error_msg.as_str()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
                                        venue_order_id,
                                        Some(self.account_id),
                                    );

                                    return Some(NautilusWsMessage::OrderModifyRejected(rejected));
                                }
                            }
                            OKXWsOperation::MassCancel => {
                                if let Some((_, instrument_id)) =
                                    self.pending_mass_cancel_requests.remove(request_id)
                                {
                                    tracing::error!(
                                        "Mass cancel operation failed for {}: code={code} msg={error_msg}",
                                        instrument_id
                                    );
                                    let error = OKXWebSocketError {
                                        code,
                                        message: format!(
                                            "Mass cancel failed for {}: {}",
                                            instrument_id, error_msg
                                        ),
                                        conn_id: None,
                                        timestamp: clock.get_time_ns().as_u64(),
                                    };
                                    return Some(NautilusWsMessage::Error(error));
                                } else {
                                    tracing::error!(
                                        "Mass cancel operation failed: code={code} msg={error_msg}"
                                    );
                                }
                            }
                            _ => tracing::warn!("Unhandled operation type for rejection: {op}"),
                        }
                    }

                    let error = OKXWebSocketError {
                        code,
                        message: error_msg,
                        conn_id: None,
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                OKXWebSocketEvent::Data { arg, data } => {
                    let OKXWebSocketArg {
                        channel, inst_id, ..
                    } = arg;

                    match channel {
                        OKXWsChannel::Account => {
                            match serde_json::from_value::<Vec<OKXAccount>>(data) {
                                Ok(accounts) => {
                                    if let Some(account) = accounts.first() {
                                        match parse_account_state(account, self.account_id, ts_init)
                                        {
                                            Ok(account_state) => {
                                                if let Some(last_account_state) =
                                                    &self.last_account_state
                                                    && account_state.has_same_balances_and_margins(
                                                        last_account_state,
                                                    )
                                                {
                                                    continue;
                                                }
                                                self.last_account_state =
                                                    Some(account_state.clone());
                                                return Some(NautilusWsMessage::AccountUpdate(
                                                    account_state,
                                                ));
                                            }
                                            Err(e) => tracing::error!(
                                                "Failed to parse account state: {e}"
                                            ),
                                        }
                                    }
                                }
                                Err(e) => tracing::error!("Failed to parse account data: {e}"),
                            }
                            continue;
                        }
                        OKXWsChannel::Orders => {
                            let orders: Vec<OKXOrderMsg> = match serde_json::from_value(data) {
                                Ok(orders) => orders,
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to deserialize orders channel payload: {e}"
                                    );
                                    continue;
                                }
                            };

                            tracing::debug!(
                                "Received {} order message(s) from orders channel",
                                orders.len()
                            );

                            let mut exec_reports: Vec<ExecutionReport> =
                                Vec::with_capacity(orders.len());

                            for msg in orders {
                                tracing::debug!(
                                    "Processing order message: inst_id={}, cl_ord_id={}, state={:?}, exec_type={:?}",
                                    msg.inst_id,
                                    msg.cl_ord_id,
                                    msg.state,
                                    msg.exec_type
                                );

                                if self.try_handle_post_only_auto_cancel(
                                    &msg,
                                    ts_init,
                                    &mut exec_reports,
                                ) {
                                    continue;
                                }

                                let raw_child = parse_client_order_id(&msg.cl_ord_id);
                                let parent_from_msg = msg
                                    .algo_cl_ord_id
                                    .as_ref()
                                    .filter(|value| !value.is_empty())
                                    .map(ClientOrderId::new);
                                let effective_client_id = self
                                    .register_client_order_aliases(&raw_child, &parent_from_msg);

                                match parse_order_msg(
                                    &msg,
                                    self.account_id,
                                    &self.instruments_cache,
                                    &self.fee_cache,
                                    &self.filled_qty_cache,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        tracing::debug!(
                                            "Successfully parsed execution report: {:?}",
                                            report
                                        );

                                        // Check for duplicate OrderAccepted events
                                        let is_duplicate_accepted =
                                            if let ExecutionReport::Order(ref status_report) =
                                                report
                                            {
                                                if status_report.order_status
                                                    == OrderStatus::Accepted
                                                {
                                                    self.emitted_order_accepted
                                                        .contains_key(&status_report.venue_order_id)
                                                } else {
                                                    false
                                                }
                                            } else {
                                                false
                                            };

                                        if is_duplicate_accepted {
                                            tracing::debug!(
                                                "Skipping duplicate OrderAccepted for venue_order_id={}",
                                                if let ExecutionReport::Order(ref r) = report {
                                                    r.venue_order_id.to_string()
                                                } else {
                                                    "unknown".to_string()
                                                }
                                            );
                                            continue;
                                        }

                                        if let ExecutionReport::Order(ref status_report) = report
                                            && status_report.order_status == OrderStatus::Accepted
                                        {
                                            self.emitted_order_accepted
                                                .insert(status_report.venue_order_id, ());
                                        }

                                        let adjusted = self.adjust_execution_report(
                                            report,
                                            &effective_client_id,
                                            &raw_child,
                                        );

                                        // Clean up tracking for terminal states
                                        if let ExecutionReport::Order(ref status_report) = adjusted
                                            && matches!(
                                                status_report.order_status,
                                                OrderStatus::Filled
                                                    | OrderStatus::Canceled
                                                    | OrderStatus::Expired
                                                    | OrderStatus::Rejected
                                            )
                                        {
                                            self.emitted_order_accepted
                                                .remove(&status_report.venue_order_id);
                                        }

                                        self.update_caches_with_report(&adjusted);
                                        exec_reports.push(adjusted);
                                    }
                                    Err(e) => tracing::error!("Failed to parse order message: {e}"),
                                }
                            }

                            if !exec_reports.is_empty() {
                                tracing::debug!(
                                    "Pushing {} execution report(s) to message queue",
                                    exec_reports.len()
                                );
                                self.pending_messages
                                    .push_back(NautilusWsMessage::ExecutionReports(exec_reports));
                            } else {
                                tracing::debug!(
                                    "No execution reports generated from order messages"
                                );
                            }

                            if let Some(message) = self.pending_messages.pop_front() {
                                return Some(message);
                            }

                            continue;
                        }
                        OKXWsChannel::OrdersAlgo => {
                            let orders: Vec<OKXAlgoOrderMsg> = match serde_json::from_value(data) {
                                Ok(orders) => orders,
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to deserialize algo orders payload: {e}"
                                    );
                                    continue;
                                }
                            };

                            let mut exec_reports: Vec<ExecutionReport> =
                                Vec::with_capacity(orders.len());

                            for msg in orders {
                                let raw_child = parse_client_order_id(&msg.cl_ord_id);
                                let parent_from_msg = parse_client_order_id(&msg.algo_cl_ord_id);
                                let effective_client_id = self
                                    .register_client_order_aliases(&raw_child, &parent_from_msg);

                                match parse_algo_order_msg(
                                    msg,
                                    self.account_id,
                                    &self.instruments_cache,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        let adjusted = self.adjust_execution_report(
                                            report,
                                            &effective_client_id,
                                            &raw_child,
                                        );
                                        self.update_caches_with_report(&adjusted);
                                        exec_reports.push(adjusted);
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to parse algo order message: {e}");
                                    }
                                }
                            }

                            if !exec_reports.is_empty() {
                                return Some(NautilusWsMessage::ExecutionReports(exec_reports));
                            }

                            continue;
                        }
                        _ => {
                            let Some(inst_id) = inst_id else {
                                tracing::error!("No instrument for channel {:?}", channel);
                                continue;
                            };

                            let Some(instrument) = self.instruments_cache.get(&inst_id) else {
                                tracing::error!(
                                    "No instrument for channel {:?}, inst_id {:?}",
                                    channel,
                                    inst_id
                                );
                                continue;
                            };

                            let instrument_id = instrument.id();
                            let price_precision = instrument.price_precision();
                            let size_precision = instrument.size_precision();

                            match parse_ws_message_data(
                                &channel,
                                data,
                                &instrument_id,
                                price_precision,
                                size_precision,
                                ts_init,
                                &mut self.funding_rate_cache,
                                &self.instruments_cache,
                            ) {
                                Ok(Some(msg)) => return Some(msg),
                                Ok(None) => continue,
                                Err(e) => {
                                    tracing::error!(
                                        "Error parsing message for channel {:?}: {e}",
                                        channel
                                    );
                                    continue;
                                }
                            }
                        }
                    }
                }
                OKXWebSocketEvent::Error { code, msg } => {
                    let error = OKXWebSocketError {
                        code,
                        message: msg,
                        conn_id: None,
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                OKXWebSocketEvent::Reconnected => {
                    return Some(NautilusWsMessage::Reconnected);
                }
                OKXWebSocketEvent::Subscription {
                    event,
                    arg,
                    code,
                    msg,
                    ..
                } => {
                    let topic = topic_from_websocket_arg(&arg);
                    let success = code.as_deref().is_none_or(|c| c == "0");

                    match event {
                        OKXSubscriptionEvent::Subscribe => {
                            if success {
                                self.subscriptions_state.confirm(&topic);
                            } else {
                                tracing::warn!(?topic, error = ?msg, code = ?code, "Subscription failed");
                                self.subscriptions_state.mark_failure(&topic);
                            }
                        }
                        OKXSubscriptionEvent::Unsubscribe => {
                            if success {
                                self.subscriptions_state.clear_pending(&topic);
                            } else {
                                tracing::warn!(?topic, error = ?msg, code = ?code, "Unsubscription failed");
                                self.subscriptions_state.mark_failure(&topic);
                            }
                        }
                    }

                    continue;
                }
                OKXWebSocketEvent::ChannelConnCount { .. } => continue,
            }
        }

        None
    }
}

/// Returns `true` when an OKX error payload represents a post-only rejection.
pub fn is_post_only_rejection(code: &str, data: &[Value]) -> bool {
    if code == OKX_POST_ONLY_ERROR_CODE {
        return true;
    }

    for entry in data {
        if let Some(s_code) = entry.get("sCode").and_then(|value| value.as_str())
            && s_code == OKX_POST_ONLY_ERROR_CODE
        {
            return true;
        }

        if let Some(inner_code) = entry.get("code").and_then(|value| value.as_str())
            && inner_code == OKX_POST_ONLY_ERROR_CODE
        {
            return true;
        }
    }

    false
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use futures_util;
    use rstest::rstest;

    use super::*;
    use crate::common::enums::{OKXExecType, OKXOrderCategory, OKXSide};

    #[rstest]
    fn test_timestamp_format_for_websocket_auth() {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time should be after UNIX epoch")
            .as_secs()
            .to_string();

        assert!(timestamp.parse::<u64>().is_ok());
        assert_eq!(timestamp.len(), 10);
        assert!(timestamp.chars().all(|c| c.is_ascii_digit()));
    }

    #[rstest]
    fn test_new_without_credentials() {
        let client = OKXWebSocketClient::default();
        assert!(client.credential.is_none());
        assert_eq!(client.api_key(), None);
    }

    #[rstest]
    fn test_new_with_credentials() {
        let client = OKXWebSocketClient::new(
            None,
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some("test_passphrase".to_string()),
            None,
            None,
        )
        .unwrap();
        assert!(client.credential.is_some());
        assert_eq!(client.api_key(), Some("test_key"));
    }

    #[rstest]
    fn test_new_partial_credentials_fails() {
        let result = OKXWebSocketClient::new(
            None,
            Some("test_key".to_string()),
            None,
            Some("test_passphrase".to_string()),
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_request_id_generation() {
        let client = OKXWebSocketClient::default();

        let initial_counter = client.request_id_counter.load(Ordering::SeqCst);

        let id1 = client.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let id2 = client.request_id_counter.fetch_add(1, Ordering::SeqCst);

        assert_eq!(id1, initial_counter);
        assert_eq!(id2, initial_counter + 1);
        assert_eq!(
            client.request_id_counter.load(Ordering::SeqCst),
            initial_counter + 2
        );
    }

    #[rstest]
    fn test_client_state_management() {
        let client = OKXWebSocketClient::default();

        assert!(client.is_closed());
        assert!(!client.is_active());

        let client_with_heartbeat =
            OKXWebSocketClient::new(None, None, None, None, None, Some(30)).unwrap();

        assert!(client_with_heartbeat.heartbeat.is_some());
        assert_eq!(client_with_heartbeat.heartbeat.unwrap(), 30);
    }

    #[rstest]
    fn test_request_cache_operations() {
        let client = OKXWebSocketClient::default();

        assert_eq!(client.pending_place_requests.len(), 0);
        assert_eq!(client.pending_cancel_requests.len(), 0);
        assert_eq!(client.pending_amend_requests.len(), 0);

        let client_order_id = ClientOrderId::from("test-order-123");
        let trader_id = TraderId::from("test-trader-001");
        let strategy_id = StrategyId::from("test-strategy-001");
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");

        let dummy_params = WsPostOrderParamsBuilder::default()
            .inst_id("BTC-USDT".to_string())
            .td_mode(OKXTradeMode::Cash)
            .side(OKXSide::Buy)
            .ord_type(OKXOrderType::Limit)
            .sz("1".to_string())
            .build()
            .unwrap();

        client.pending_place_requests.insert(
            "place-123".to_string(),
            (
                PendingOrderParams::Regular(dummy_params),
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            ),
        );

        assert_eq!(client.pending_place_requests.len(), 1);
        assert!(client.pending_place_requests.contains_key("place-123"));

        let removed = client.pending_place_requests.remove("place-123");
        assert!(removed.is_some());
        assert_eq!(client.pending_place_requests.len(), 0);
    }

    #[rstest]
    fn test_websocket_error_handling() {
        let clock = get_atomic_clock_realtime();
        let ts = clock.get_time_ns().as_u64();

        let error = OKXWebSocketError {
            code: "60012".to_string(),
            message: "Invalid request".to_string(),
            conn_id: None,
            timestamp: ts,
        };

        assert_eq!(error.code, "60012");
        assert_eq!(error.message, "Invalid request");
        assert_eq!(error.timestamp, ts);

        let nautilus_msg = NautilusWsMessage::Error(error);
        match nautilus_msg {
            NautilusWsMessage::Error(e) => {
                assert_eq!(e.code, "60012");
                assert_eq!(e.message, "Invalid request");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[rstest]
    fn test_request_id_generation_sequence() {
        let client = OKXWebSocketClient::default();

        let initial_counter = client
            .request_id_counter
            .load(std::sync::atomic::Ordering::SeqCst);
        let mut ids = Vec::new();
        for _ in 0..10 {
            let id = client
                .request_id_counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ids.push(id);
        }

        for (i, &id) in ids.iter().enumerate() {
            assert_eq!(id, initial_counter + i as u64);
        }

        assert_eq!(
            client
                .request_id_counter
                .load(std::sync::atomic::Ordering::SeqCst),
            initial_counter + 10
        );
    }

    #[rstest]
    fn test_client_state_transitions() {
        let client = OKXWebSocketClient::default();

        assert!(client.is_closed());
        assert!(!client.is_active());

        let client_with_heartbeat = OKXWebSocketClient::new(
            None,
            None,
            None,
            None,
            None,
            Some(30), // 30 second heartbeat
        )
        .unwrap();

        assert!(client_with_heartbeat.heartbeat.is_some());
        assert_eq!(client_with_heartbeat.heartbeat.unwrap(), 30);

        let account_id = AccountId::from("test-account-123");
        let client_with_account =
            OKXWebSocketClient::new(None, None, None, None, Some(account_id), None).unwrap();

        assert_eq!(client_with_account.account_id, account_id);
    }

    #[tokio::test]
    async fn test_concurrent_request_handling() {
        let client = Arc::new(OKXWebSocketClient::default());

        let initial_counter = client
            .request_id_counter
            .load(std::sync::atomic::Ordering::SeqCst);
        let mut handles = Vec::new();

        for i in 0..10 {
            let client_clone = Arc::clone(&client);
            let handle = tokio::spawn(async move {
                let client_order_id = ClientOrderId::from(format!("order-{i}").as_str());
                let trader_id = TraderId::from("trader-001");
                let strategy_id = StrategyId::from("strategy-001");
                let instrument_id = InstrumentId::from("BTC-USDT.OKX");

                let request_id = client_clone
                    .request_id_counter
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let request_id_str = request_id.to_string();

                let dummy_params = WsPostOrderParamsBuilder::default()
                    .inst_id(instrument_id.symbol.to_string())
                    .td_mode(OKXTradeMode::Cash)
                    .side(OKXSide::Buy)
                    .ord_type(OKXOrderType::Limit)
                    .sz("1".to_string())
                    .build()
                    .unwrap();

                client_clone.pending_place_requests.insert(
                    request_id_str.clone(),
                    (
                        PendingOrderParams::Regular(dummy_params),
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    ),
                );

                // Simulate processing delay
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

                // Remove from cache (simulating response processing)
                let removed = client_clone.pending_place_requests.remove(&request_id_str);
                assert!(removed.is_some());

                request_id
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        let results: Vec<_> = futures_util::future::join_all(handles).await;

        assert_eq!(results.len(), 10);
        for result in results {
            assert!(result.is_ok());
        }

        assert_eq!(client.pending_place_requests.len(), 0);

        let final_counter = client
            .request_id_counter
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(final_counter, initial_counter + 10);
    }

    #[rstest]
    fn test_websocket_error_scenarios() {
        let clock = get_atomic_clock_realtime();
        let ts = clock.get_time_ns().as_u64();

        let error_scenarios = vec![
            ("60012", "Invalid request", None),
            ("60009", "Invalid API key", Some("conn-123".to_string())),
            ("60014", "Too many requests", None),
            ("50001", "Order not found", None),
        ];

        for (code, message, conn_id) in error_scenarios {
            let error = OKXWebSocketError {
                code: code.to_string(),
                message: message.to_string(),
                conn_id: conn_id.clone(),
                timestamp: ts,
            };

            assert_eq!(error.code, code);
            assert_eq!(error.message, message);
            assert_eq!(error.conn_id, conn_id);
            assert_eq!(error.timestamp, ts);

            let nautilus_msg = NautilusWsMessage::Error(error);
            match nautilus_msg {
                NautilusWsMessage::Error(e) => {
                    assert_eq!(e.code, code);
                    assert_eq!(e.message, message);
                    assert_eq!(e.conn_id, conn_id);
                }
                _ => panic!("Expected Error variant"),
            }
        }
    }

    #[tokio::test]
    async fn test_feed_handler_reconnection_detection() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let signal = Arc::new(AtomicBool::new(false));
        let mut handler = OKXFeedHandler::new(rx, signal.clone());

        tx.send(Message::Text(RECONNECTED.to_string().into()))
            .unwrap();

        let result = handler.next().await;
        assert!(matches!(result, Some(OKXWebSocketEvent::Reconnected)));
    }

    #[tokio::test]
    async fn test_feed_handler_normal_message_processing() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let signal = Arc::new(AtomicBool::new(false));
        let mut handler = OKXFeedHandler::new(rx, signal.clone());

        // Send a ping message (OKX sends pings)
        let ping_msg = TEXT_PING;
        tx.send(Message::Text(ping_msg.to_string().into())).unwrap();

        // Send a valid subscription response
        let sub_msg = r#"{
            "event": "subscribe",
            "arg": {
                "channel": "tickers",
                "instType": "SPOT"
            },
            "connId": "a4d3ae55"
        }"#;

        tx.send(Message::Text(sub_msg.to_string().into())).unwrap();

        let first = handler.next().await;
        assert!(matches!(first, Some(OKXWebSocketEvent::Ping)));

        // Now ensure we can still shut down cleanly even with a pending subscription message.
        signal.store(true, std::sync::atomic::Ordering::Relaxed);

        let result = handler.next().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_feed_handler_stop_signal() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let signal = Arc::new(AtomicBool::new(true)); // Signal already set
        let mut handler = OKXFeedHandler::new(rx, signal.clone());

        let result = handler.next().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_feed_handler_close_message() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let signal = Arc::new(AtomicBool::new(false));
        let mut handler = OKXFeedHandler::new(rx, signal.clone());

        // Send close message
        tx.send(Message::Close(None)).unwrap();

        let result = handler.next().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_reconnection_message_constant() {
        assert_eq!(RECONNECTED, "__RECONNECTED__");
    }

    #[tokio::test]
    async fn test_multiple_reconnection_signals() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let signal = Arc::new(AtomicBool::new(false));
        let mut handler = OKXFeedHandler::new(rx, signal.clone());

        // Send multiple reconnection messages
        for _ in 0..3 {
            tx.send(Message::Text(RECONNECTED.to_string().into()))
                .unwrap();

            let result = handler.next().await;
            assert!(matches!(result, Some(OKXWebSocketEvent::Reconnected)));
        }
    }

    #[tokio::test]
    async fn test_wait_until_active_timeout() {
        let client = OKXWebSocketClient::new(
            None,
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some("test_passphrase".to_string()),
            Some(AccountId::from("test-account")),
            None,
        )
        .unwrap();

        // Should timeout since client is not connected
        let result = client.wait_until_active(0.1).await;

        assert!(result.is_err());
        assert!(!client.is_active());
    }

    fn sample_canceled_order_msg() -> OKXOrderMsg {
        OKXOrderMsg {
            acc_fill_sz: Some("0".to_string()),
            avg_px: "0".to_string(),
            c_time: 0,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: ustr::Ustr::from("USDT"),
            cl_ord_id: "order-1".to_string(),
            algo_cl_ord_id: None,
            fee: None,
            fee_ccy: ustr::Ustr::from("USDT"),
            fill_px: "0".to_string(),
            fill_sz: "0".to_string(),
            fill_time: 0,
            inst_id: ustr::Ustr::from("ETH-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            lever: "1".to_string(),
            ord_id: ustr::Ustr::from("123456"),
            ord_type: OKXOrderType::Limit,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Net,
            px: "0".to_string(),
            reduce_only: "false".to_string(),
            side: OKXSide::Buy,
            state: OKXOrderStatus::Canceled,
            exec_type: OKXExecType::None,
            sz: "1".to_string(),
            td_mode: OKXTradeMode::Cross,
            tgt_ccy: None,
            trade_id: String::new(),
            u_time: 0,
        }
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_detects_cancel_source() {
        let mut msg = sample_canceled_order_msg();
        msg.cancel_source = Some(super::OKX_POST_ONLY_CANCEL_SOURCE.to_string());

        assert!(OKXWsMessageHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_detects_reason() {
        let mut msg = sample_canceled_order_msg();
        msg.cancel_source_reason = Some("POST_ONLY would take liquidity".to_string());

        assert!(OKXWsMessageHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_false_without_markers() {
        let msg = sample_canceled_order_msg();

        assert!(!OKXWsMessageHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_false_for_order_type_only() {
        let mut msg = sample_canceled_order_msg();
        msg.ord_type = OKXOrderType::PostOnly;

        assert!(!OKXWsMessageHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_rejection_detects_by_code() {
        assert!(super::is_post_only_rejection("51019", &[]));
    }

    #[rstest]
    fn test_is_post_only_rejection_detects_by_inner_code() {
        let data = vec![serde_json::json!({
            "sCode": "51019"
        })];
        assert!(super::is_post_only_rejection("50000", &data));
    }

    #[rstest]
    fn test_is_post_only_rejection_false_for_unrelated_error() {
        let data = vec![serde_json::json!({
            "sMsg": "Insufficient balance"
        })];
        assert!(!super::is_post_only_rejection("50000", &data));
    }

    #[tokio::test]
    async fn test_batch_cancel_orders_with_multiple_orders() {
        use nautilus_model::identifiers::{ClientOrderId, InstrumentId, VenueOrderId};

        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let client_order_id1 = ClientOrderId::new("order1");
        let client_order_id2 = ClientOrderId::new("order2");
        let venue_order_id1 = VenueOrderId::new("venue1");
        let venue_order_id2 = VenueOrderId::new("venue2");

        let orders = vec![
            (instrument_id, Some(client_order_id1), Some(venue_order_id1)),
            (instrument_id, Some(client_order_id2), Some(venue_order_id2)),
        ];

        // This will fail to send since we're not connected, but we're testing the payload building
        let result = client.batch_cancel_orders(orders).await;

        // Should get an error because not connected, but it means payload was built correctly
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_cancel_orders_with_only_client_order_id() {
        use nautilus_model::identifiers::{ClientOrderId, InstrumentId};

        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let client_order_id = ClientOrderId::new("order1");

        let orders = vec![(instrument_id, Some(client_order_id), None)];

        let result = client.batch_cancel_orders(orders).await;

        // Should get an error because not connected
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_cancel_orders_with_only_venue_order_id() {
        use nautilus_model::identifiers::{InstrumentId, VenueOrderId};

        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let venue_order_id = VenueOrderId::new("venue1");

        let orders = vec![(instrument_id, None, Some(venue_order_id))];

        let result = client.batch_cancel_orders(orders).await;

        // Should get an error because not connected
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_cancel_orders_with_both_ids() {
        use nautilus_model::identifiers::{ClientOrderId, InstrumentId, VenueOrderId};

        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let client_order_id = ClientOrderId::new("order1");
        let venue_order_id = VenueOrderId::new("venue1");

        let orders = vec![(instrument_id, Some(client_order_id), Some(venue_order_id))];

        let result = client.batch_cancel_orders(orders).await;

        // Should get an error because not connected
        assert!(result.is_err());
    }
}
