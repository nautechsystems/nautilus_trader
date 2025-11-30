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
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use ahash::AHashSet;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::live::runtime::get_runtime;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT,
    env::{get_env_var, get_or_env_var},
};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, PositionSide, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        AUTHENTICATION_TIMEOUT_SECS, AuthTracker, PingHandler, SubscriptionState, TEXT_PING,
        WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use reqwest::header::USER_AGENT;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Error;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    enums::OKXWsChannel,
    error::OKXWsError,
    handler::{HandlerCommand, OKXWsFeedHandler},
    messages::{
        NautilusWsMessage, OKXAuthentication, OKXAuthenticationArg, OKXSubscriptionArg,
        WsAmendOrderParamsBuilder, WsCancelOrderParamsBuilder, WsPostAlgoOrderParamsBuilder,
        WsPostOrderParamsBuilder,
    },
    subscription::topic_from_subscription_arg,
};
use crate::common::{
    consts::{
        OKX_NAUTILUS_BROKER_ID, OKX_SUPPORTED_ORDER_TYPES, OKX_SUPPORTED_TIME_IN_FORCE,
        OKX_WS_PUBLIC_URL, OKX_WS_TOPIC_DELIMITER,
    },
    credential::Credential,
    enums::{
        OKXInstrumentType, OKXOrderType, OKXPositionSide, OKXTargetCurrency, OKXTradeMode,
        OKXTriggerType, OKXVipLevel, conditional_order_to_algo_type, is_conditional_order,
    },
    parse::{bar_spec_as_okx_channel, okx_instrument_type, okx_instrument_type_from_symbol},
};

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

/// Rate limit key for subscription operations (subscribe/unsubscribe/login).
///
/// See: <https://www.okx.com/docs-v5/en/#websocket-api-login>
/// See: <https://www.okx.com/docs-v5/en/#websocket-api-subscribe>
pub const OKX_RATE_LIMIT_KEY_SUBSCRIPTION: &str = "subscription";

/// Rate limit key for order operations (place regular and algo orders).
///
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order>
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-ws-place-algo-order>
pub const OKX_RATE_LIMIT_KEY_ORDER: &str = "order";

/// Rate limit key for cancel operations (cancel regular and algo orders, mass cancel).
///
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-cancel-order>
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-ws-cancel-algo-order>
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-mass-cancel-order>
pub const OKX_RATE_LIMIT_KEY_CANCEL: &str = "cancel";

/// Rate limit key for amend operations (amend orders).
///
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-amend-order>
pub const OKX_RATE_LIMIT_KEY_AMEND: &str = "amend";

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
    auth_tracker: AuthTracker,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions_inst_type: Arc<DashMap<OKXWsChannel, AHashSet<OKXInstrumentType>>>,
    subscriptions_inst_family: Arc<DashMap<OKXWsChannel, AHashSet<Ustr>>>,
    subscriptions_inst_id: Arc<DashMap<OKXWsChannel, AHashSet<Ustr>>>,
    subscriptions_bare: Arc<DashMap<OKXWsChannel, bool>>, // For channels without inst params (e.g., Account)
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
    emitted_order_accepted: Arc<DashMap<VenueOrderId, ()>>, // Track orders we've already emitted OrderAccepted for
    client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
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
        let subscriptions_state = SubscriptionState::new(OKX_WS_TOPIC_DELIMITER);

        Ok(Self {
            url,
            account_id,
            vip_level: Arc::new(AtomicU8::new(0)), // Default to VIP 0
            credential,
            heartbeat,
            auth_tracker: AuthTracker::new(),
            signal,
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed.as_u8(),
            ))),
            cmd_tx: {
                // Placeholder channel until connect() creates the real handler and replays queued instruments
                let (tx, _) = tokio::sync::mpsc::unbounded_channel();
                Arc::new(tokio::sync::RwLock::new(tx))
            },
            out_rx: None,
            task_handle: None,
            subscriptions_inst_type,
            subscriptions_inst_family,
            subscriptions_inst_id,
            subscriptions_bare,
            subscriptions_state,
            request_id_counter: Arc::new(AtomicU64::new(1)),
            active_client_orders: Arc::new(DashMap::new()),
            emitted_order_accepted: Arc::new(DashMap::new()),
            client_id_aliases: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(DashMap::new()),
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

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> Option<String> {
        self.credential.clone().map(|c| c.api_key_masked())
    }

    /// Returns a value indicating whether the client is active.
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

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same symbols will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for inst in &instruments {
            self.instruments_cache
                .insert(inst.symbol().inner(), inst.clone());
        }

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if !instruments.is_empty()
            && let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments))
        {
            log::debug!("Failed to send bulk instrument update to handler: {e}");
        }
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument.clone());

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(HandlerCommand::UpdateInstrument(instrument))
        {
            log::debug!("Failed to send instrument update to handler: {e}");
        }
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
        let (message_handler, raw_rx) = channel_message_handler();

        // No-op ping handler: handler owns the WebSocketClient and responds to pings directly
        // in the message loop for minimal latency (see handler.rs TEXT_PONG response)
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
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
            reconnect_max_attempts: None,
        };

        // Configure rate limits for different operation types
        let keyed_quotas = vec![
            (
                OKX_RATE_LIMIT_KEY_SUBSCRIPTION.to_string(),
                *OKX_WS_SUBSCRIPTION_QUOTA,
            ),
            (OKX_RATE_LIMIT_KEY_ORDER.to_string(), *OKX_WS_ORDER_QUOTA),
            (OKX_RATE_LIMIT_KEY_CANCEL.to_string(), *OKX_WS_ORDER_QUOTA),
            (OKX_RATE_LIMIT_KEY_AMEND.to_string(), *OKX_WS_ORDER_QUOTA),
        ];

        let client = WebSocketClient::connect(
            config,
            None, // post_reconnection
            keyed_quotas,
            Some(*OKX_WS_CONNECTION_QUOTA), // Default quota for connection operations
        )
        .await?;

        // Replace connection state so all clones see the underlying WebSocketClient's state
        self.connection_mode.store(client.connection_mode_atomic());

        let account_id = self.account_id;
        let (msg_tx, rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        self.out_rx = Some(Arc::new(rx));

        // Create fresh command channel for this connection
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        // Replay cached instruments to the new handler via the new channel
        if !self.instruments_cache.is_empty() {
            let cached_instruments: Vec<InstrumentAny> = self
                .instruments_cache
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            if let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(cached_instruments)) {
                tracing::error!("Failed to replay instruments to handler: {e}");
            }
        }

        let signal = self.signal.clone();
        let active_client_orders = self.active_client_orders.clone();
        let emitted_order_accepted = self.emitted_order_accepted.clone();
        let auth_tracker = self.auth_tracker.clone();
        let subscriptions_state = self.subscriptions_state.clone();
        let client_id_aliases = self.client_id_aliases.clone();

        let stream_handle = get_runtime().spawn({
            let auth_tracker = auth_tracker.clone();
            let signal = signal.clone();
            let credential = self.credential.clone();
            let cmd_tx_for_reconnect = cmd_tx.clone();
            let subscriptions_bare = self.subscriptions_bare.clone();
            let subscriptions_inst_type = self.subscriptions_inst_type.clone();
            let subscriptions_inst_family = self.subscriptions_inst_family.clone();
            let subscriptions_inst_id = self.subscriptions_inst_id.clone();
            let mut has_reconnected = false;

            async move {
                let mut handler = OKXWsFeedHandler::new(
                    account_id,
                    signal.clone(),
                    cmd_rx,
                    raw_rx,
                    msg_tx,
                    active_client_orders,
                    client_id_aliases,
                    emitted_order_accepted,
                    auth_tracker.clone(),
                    subscriptions_state.clone(),
                );

                // Helper closure to resubscribe all tracked subscriptions after reconnection
                let resubscribe_all = || {
                    for entry in subscriptions_inst_id.iter() {
                        let (channel, inst_ids) = entry.pair();
                        for inst_id in inst_ids {
                            let arg = OKXSubscriptionArg {
                                channel: channel.clone(),
                                inst_type: None,
                                inst_family: None,
                                inst_id: Some(*inst_id),
                            };
                            if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe { args: vec![arg] }) {
                                tracing::error!(error = %e, "Failed to send resubscribe command");
                            }
                        }
                    }

                    for entry in subscriptions_bare.iter() {
                        let channel = entry.key();
                        let arg = OKXSubscriptionArg {
                            channel: channel.clone(),
                            inst_type: None,
                            inst_family: None,
                            inst_id: None,
                        };
                        if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe { args: vec![arg] }) {
                            tracing::error!(error = %e, "Failed to send resubscribe command");
                        }
                    }

                    for entry in subscriptions_inst_type.iter() {
                        let (channel, inst_types) = entry.pair();
                        for inst_type in inst_types {
                            let arg = OKXSubscriptionArg {
                                channel: channel.clone(),
                                inst_type: Some(*inst_type),
                                inst_family: None,
                                inst_id: None,
                            };
                            if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe { args: vec![arg] }) {
                                tracing::error!(error = %e, "Failed to send resubscribe command");
                            }
                        }
                    }

                    for entry in subscriptions_inst_family.iter() {
                        let (channel, inst_families) = entry.pair();
                        for inst_family in inst_families {
                            let arg = OKXSubscriptionArg {
                                channel: channel.clone(),
                                inst_type: None,
                                inst_family: Some(*inst_family),
                                inst_id: None,
                            };
                            if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe { args: vec![arg] }) {
                                tracing::error!(error = %e, "Failed to send resubscribe command");
                            }
                        }
                    }
                };

                // Main message loop with explicit reconnection handling
                loop {
                    match handler.next().await {
                        Some(NautilusWsMessage::Reconnected) => {
                            if signal.load(Ordering::Relaxed) {
                                continue;
                            }

                            has_reconnected = true;

                            // Mark all confirmed subscriptions as failed so they transition to pending state
                            let confirmed_topics_vec: Vec<String> = {
                                let confirmed = subscriptions_state.confirmed();
                                let mut topics = Vec::new();
                                for entry in confirmed.iter() {
                                    let channel = entry.key();
                                    for symbol in entry.value() {
                                        if symbol.as_str() == "#" {
                                            topics.push(channel.to_string());
                                        } else {
                                            topics.push(format!("{channel}{OKX_WS_TOPIC_DELIMITER}{symbol}"));
                                        }
                                    }
                                }
                                topics
                            };

                            if !confirmed_topics_vec.is_empty() {
                                tracing::debug!(count = confirmed_topics_vec.len(), "Marking confirmed subscriptions as pending for replay");
                                for topic in confirmed_topics_vec {
                                    subscriptions_state.mark_failure(&topic);
                                }
                            }

                            if let Some(cred) = &credential {
                                tracing::debug!("Re-authenticating after reconnection");
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                    .expect("System time should be after UNIX epoch")
                                    .as_secs()
                                    .to_string();
                                let signature = cred.sign(&timestamp, "GET", "/users/self/verify", "");

                                let auth_message = super::messages::OKXAuthentication {
                                    op: "login",
                                    args: vec![super::messages::OKXAuthenticationArg {
                                        api_key: cred.api_key.to_string(),
                                        passphrase: cred.api_passphrase.clone(),
                                        timestamp,
                                        sign: signature,
                                    }],
                                };

                                if let Ok(payload) = serde_json::to_string(&auth_message) {
                                    if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Authenticate { payload }) {
                                        tracing::error!(error = %e, "Failed to send reconnection auth command");
                                    }
                                } else {
                                    tracing::error!("Failed to serialize reconnection auth message");
                                }
                            }

                            // Unauthenticated sessions resubscribe immediately after reconnection,
                            // authenticated sessions wait for Authenticated message
                            if credential.is_none() {
                                tracing::debug!("No authentication required, resubscribing immediately");
                                resubscribe_all();
                            }

                            // TODO: Implement proper Reconnected event forwarding to consumers.
                            // Currently intercepted for internal housekeeping only. Will add new
                            // message type from WebSocketClient to notify consumers of reconnections.

                            continue;
                        }
                        Some(NautilusWsMessage::Authenticated) => {
                            if has_reconnected {
                                resubscribe_all();
                            }

                            // NOTE: Not forwarded to out_tx as it's only used internally for
                            // reconnection flow coordination. Downstream consumers have access to
                            // authentication state via AuthTracker if needed. The execution client's
                            // Authenticated handler only logs at debug level (no critical logic).
                            continue;
                        }
                        Some(msg) => {
                            if handler.send(msg).is_err() {
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

                tracing::debug!("Handler task exiting");
            }
        });

        self.task_handle = Some(Arc::new(stream_handle));

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SetClient(client))
            .map_err(|e| {
                OKXWsError::ClientError(format!("Failed to send WebSocket client to handler: {e}"))
            })?;
        tracing::debug!("Sent WebSocket client to handler");

        if self.credential.is_some()
            && let Err(e) = self.authenticate().await
        {
            anyhow::bail!("Authentication failed: {e}");
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

        let payload = serde_json::to_string(&auth_message).map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Failed to serialize auth message: {e}"
            )))
        })?;

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Authenticate { payload })
            .map_err(|e| {
                Error::Io(std::io::Error::other(format!(
                    "Failed to send authenticate command: {e}"
                )))
            })?;

        match self
            .auth_tracker
            .wait_for_result::<OKXWsError>(Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS), rx)
            .await
        {
            Ok(()) => {
                tracing::info!("WebSocket authenticated");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "WebSocket authentication failed");
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
            .out_rx
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

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::warn!("Failed to send disconnect command to handler: {e}");
        } else {
            log::debug!("Sent disconnect command to handler");
        }

        // Handler drops the WebSocketClient on Disconnect
        {
            if false {
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

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Subscribe { args })
            .map_err(|e| OKXWsError::ClientError(format!("Failed to send subscribe command: {e}")))
    }

    #[allow(clippy::collapsible_if)]
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

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Unsubscribe { args })
            .map_err(|e| {
                OKXWsError::ClientError(format!("Failed to send unsubscribe command: {e}"))
            })
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
            for inst_type in inst_types {
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
            for inst_family in inst_families {
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
            for inst_id in inst_ids {
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
    /// Since OKX doesn't support subscribing to individual instruments via `instId`,
    /// this method subscribes to the entire instrument type if not already subscribed.
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
        let inst_type = okx_instrument_type_from_symbol(instrument_id.symbol.as_str());

        let already_subscribed = self
            .subscriptions_inst_type
            .get(&OKXWsChannel::Instruments)
            .is_some_and(|types| types.contains(&inst_type));

        if already_subscribed {
            tracing::debug!(
                "Already subscribed to instrument type {inst_type:?} for {instrument_id}"
            );
            return Ok(());
        }

        tracing::info!("Subscribing to instrument type {inst_type:?} for {instrument_id}");
        self.subscribe_instruments(inst_type).await
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

    /// Subscribes to position updates for a specific instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#websocket-api-private-channel-positions-channel>
    pub async fn subscribe_positions(
        &self,
        inst_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Positions,
            inst_type: Some(inst_type),
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from position updates for a specific instrument type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_positions(
        &self,
        inst_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Positions,
            inst_type: Some(inst_type),
            inst_family: None,
            inst_id: None,
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Place multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-orders>
    async fn ws_batch_place_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();
        let cmd = HandlerCommand::BatchPlaceOrders { args, request_id };

        self.send_cmd(cmd).await
    }

    /// Cancel multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-cancel-orders>
    async fn ws_batch_cancel_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();
        let cmd = HandlerCommand::BatchCancelOrders { args, request_id };

        self.send_cmd(cmd).await
    }

    /// Amend multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-amend-orders>
    async fn ws_batch_amend_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();
        let cmd = HandlerCommand::BatchAmendOrders { args, request_id };

        self.send_cmd(cmd).await
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
            okx_instrument_type(&instrument).map_err(|e| OKXWsError::ClientError(e.to_string()))?;
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

        // OKX implements FOK/IOC as order types rather than separate time-in-force
        // Market + FOK is unsupported (FOK requires a limit price)
        let (okx_ord_type, price) = if post_only.unwrap_or(false) {
            (OKXOrderType::PostOnly, price)
        } else if let Some(tif) = time_in_force {
            match (order_type, tif) {
                (OrderType::Market, TimeInForce::Fok) => {
                    return Err(OKXWsError::ClientError(
                        "Market orders with FOK time-in-force are not supported by OKX. Use Limit order with FOK instead.".to_string()
                    ));
                }
                (OrderType::Market, TimeInForce::Ioc) => (OKXOrderType::OptimalLimitIoc, price),
                (OrderType::Limit, TimeInForce::Fok) => (OKXOrderType::Fok, price),
                (OrderType::Limit, TimeInForce::Ioc) => (OKXOrderType::Ioc, price),
                _ => (OKXOrderType::from(order_type), price),
            }
        } else {
            (OKXOrderType::from(order_type), price)
        };

        log::debug!(
            "Order type mapping: order_type={order_type:?}, time_in_force={time_in_force:?}, post_only={post_only:?} -> okx_ord_type={okx_ord_type:?}"
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

        self.active_client_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));

        let cmd = HandlerCommand::PlaceOrder {
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
        };

        self.send_cmd(cmd).await
    }

    /// Place a new order via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-place-order>
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

        // External orders may not have a client order ID,
        // for now we just send commands for orders with a client order ID.
        if let Some(client_order_id) = client_order_id {
            let cmd = HandlerCommand::AmendOrder {
                params,
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            };

            self.send_cmd(cmd).await
        } else {
            // For external orders without client_order_id, we can't track them properly yet
            Err(OKXWsError::ClientError(
                "Cannot amend order without client_order_id".to_string(),
            ))
        }
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
        let cmd = HandlerCommand::CancelOrder {
            client_order_id,
            venue_order_id,
            instrument_id,
            trader_id,
            strategy_id,
        };

        self.send_cmd(cmd).await
    }

    /// Mass cancels all orders for a given instrument via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if instrument metadata cannot be resolved or if the
    /// cancel request fails to send.
    ///
    /// # References
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-mass-cancel-order>
    pub async fn mass_cancel_orders(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let cmd = HandlerCommand::MassCancel { instrument_id };

        self.send_cmd(cmd).await
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

    /// Modifies multiple orders.
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

        self.active_client_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));

        let cmd = HandlerCommand::PlaceAlgoOrder {
            params,
            client_order_id,
            trader_id,
            strategy_id,
            instrument_id,
        };

        self.send_cmd(cmd).await
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
        let cmd = HandlerCommand::CancelAlgoOrder {
            client_order_id,
            algo_order_id: algo_order_id.map(|id| VenueOrderId::from(id.as_str())),
            instrument_id,
            trader_id,
            strategy_id,
        };

        self.send_cmd(cmd).await
    }

    /// Sends a command to the handler.
    async fn send_cmd(&self, cmd: HandlerCommand) -> Result<(), OKXWsError> {
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| OKXWsError::ClientError(format!("Handler not available: {e}")))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_network::RECONNECTED;
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::{
        common::{
            consts::OKX_POST_ONLY_CANCEL_SOURCE,
            enums::{OKXExecType, OKXOrderCategory, OKXOrderStatus, OKXSide},
        },
        websocket::{
            handler::OKXWsFeedHandler,
            messages::{OKXOrderMsg, OKXWebSocketError, OKXWsMessage},
        },
    };

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

    // NOTE: This test was removed because pending_amend_requests moved to the handler
    // and is no longer directly accessible from the client. The handler owns all pending
    // request state in its private AHashMap for lock-free operation.

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

    #[rstest]
    fn test_feed_handler_reconnection_detection() {
        let msg = Message::Text(RECONNECTED.to_string().into());
        let result = OKXWsFeedHandler::parse_raw_message(msg);
        assert!(matches!(result, Some(OKXWsMessage::Reconnected)));
    }

    #[rstest]
    fn test_feed_handler_normal_message_processing() {
        // Test ping message
        let ping_msg = Message::Text(TEXT_PING.to_string().into());
        let result = OKXWsFeedHandler::parse_raw_message(ping_msg);
        assert!(matches!(result, Some(OKXWsMessage::Ping)));

        // Test valid subscription response
        let sub_msg = r#"{
            "event": "subscribe",
            "arg": {
                "channel": "tickers",
                "instType": "SPOT"
            },
            "connId": "a4d3ae55"
        }"#;

        let sub_result =
            OKXWsFeedHandler::parse_raw_message(Message::Text(sub_msg.to_string().into()));
        assert!(matches!(
            sub_result,
            Some(OKXWsMessage::Subscription { .. })
        ));
    }

    #[rstest]
    fn test_feed_handler_close_message() {
        // Close messages return None (filtered out)
        let result = OKXWsFeedHandler::parse_raw_message(Message::Close(None));
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconnection_message_constant() {
        assert_eq!(RECONNECTED, "__RECONNECTED__");
    }

    #[rstest]
    fn test_multiple_reconnection_signals() {
        // Test that multiple reconnection messages are properly parsed
        for _ in 0..3 {
            let msg = Message::Text(RECONNECTED.to_string().into());
            let result = OKXWsFeedHandler::parse_raw_message(msg);
            assert!(matches!(result, Some(OKXWsMessage::Reconnected)));
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
        msg.cancel_source = Some(OKX_POST_ONLY_CANCEL_SOURCE.to_string());

        assert!(OKXWsFeedHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_detects_reason() {
        let mut msg = sample_canceled_order_msg();
        msg.cancel_source_reason = Some("POST_ONLY would take liquidity".to_string());

        assert!(OKXWsFeedHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_false_without_markers() {
        let msg = sample_canceled_order_msg();

        assert!(!OKXWsFeedHandler::is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_false_for_order_type_only() {
        let mut msg = sample_canceled_order_msg();
        msg.ord_type = OKXOrderType::PostOnly;

        assert!(!OKXWsFeedHandler::is_post_only_auto_cancel(&msg));
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

    #[rstest]
    fn test_race_unsubscribe_failure_recovery() {
        // Simulates the race condition where venue rejects an unsubscribe request.
        // The adapter must perform the 3-step recovery:
        // 1. confirm_unsubscribe() - clear pending_unsubscribe
        // 2. mark_subscribe() - mark as subscribing again
        // 3. confirm_subscribe() - restore to confirmed state
        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let topic = "trades:BTC-USDT-SWAP";

        // Initial subscribe flow
        client.subscriptions_state.mark_subscribe(topic);
        client.subscriptions_state.confirm_subscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 1);

        // User unsubscribes
        client.subscriptions_state.mark_unsubscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 0);
        assert_eq!(
            client.subscriptions_state.pending_unsubscribe_topics(),
            vec![topic]
        );

        // Venue REJECTS the unsubscribe (error message)
        // Adapter must perform 3-step recovery (from lines 4444-4446)
        client.subscriptions_state.confirm_unsubscribe(topic); // Step 1: clear pending_unsubscribe
        client.subscriptions_state.mark_subscribe(topic); // Step 2: mark as subscribing
        client.subscriptions_state.confirm_subscribe(topic); // Step 3: confirm subscription

        // Verify recovery: topic should be back in confirmed state
        assert_eq!(client.subscriptions_state.len(), 1);
        assert!(
            client
                .subscriptions_state
                .pending_unsubscribe_topics()
                .is_empty()
        );
        assert!(
            client
                .subscriptions_state
                .pending_subscribe_topics()
                .is_empty()
        );

        // Verify topic is in all_topics() for reconnect
        let all = client.subscriptions_state.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic.to_string()));
    }

    #[rstest]
    fn test_race_resubscribe_before_unsubscribe_ack() {
        // Simulates: User unsubscribes, then immediately resubscribes before
        // the unsubscribe ACK arrives from the venue.
        // This is the race condition fixed in the subscription tracker.
        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let topic = "books:BTC-USDT";

        // Initial subscribe
        client.subscriptions_state.mark_subscribe(topic);
        client.subscriptions_state.confirm_subscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 1);

        // User unsubscribes
        client.subscriptions_state.mark_unsubscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 0);
        assert_eq!(
            client.subscriptions_state.pending_unsubscribe_topics(),
            vec![topic]
        );

        // User immediately changes mind and resubscribes (before unsubscribe ACK)
        client.subscriptions_state.mark_subscribe(topic);
        assert_eq!(
            client.subscriptions_state.pending_subscribe_topics(),
            vec![topic]
        );

        // NOW the unsubscribe ACK arrives - should NOT clear pending_subscribe
        client.subscriptions_state.confirm_unsubscribe(topic);
        assert!(
            client
                .subscriptions_state
                .pending_unsubscribe_topics()
                .is_empty()
        );
        assert_eq!(
            client.subscriptions_state.pending_subscribe_topics(),
            vec![topic]
        );

        // Subscribe ACK arrives
        client.subscriptions_state.confirm_subscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 1);
        assert!(
            client
                .subscriptions_state
                .pending_subscribe_topics()
                .is_empty()
        );

        // Verify final state is correct
        let all = client.subscriptions_state.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic.to_string()));
    }

    #[rstest]
    fn test_race_late_subscribe_confirmation_after_unsubscribe() {
        // Simulates: User subscribes, then unsubscribes before subscribe ACK arrives.
        // The late subscribe ACK should be ignored.
        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let topic = "tickers:ETH-USDT";

        // User subscribes
        client.subscriptions_state.mark_subscribe(topic);
        assert_eq!(
            client.subscriptions_state.pending_subscribe_topics(),
            vec![topic]
        );

        // User immediately unsubscribes (before subscribe ACK)
        client.subscriptions_state.mark_unsubscribe(topic);
        assert!(
            client
                .subscriptions_state
                .pending_subscribe_topics()
                .is_empty()
        ); // Cleared
        assert_eq!(
            client.subscriptions_state.pending_unsubscribe_topics(),
            vec![topic]
        );

        // Late subscribe confirmation arrives - should be IGNORED
        client.subscriptions_state.confirm_subscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 0); // Not added to confirmed
        assert_eq!(
            client.subscriptions_state.pending_unsubscribe_topics(),
            vec![topic]
        );

        // Unsubscribe ACK arrives
        client.subscriptions_state.confirm_unsubscribe(topic);

        // Final state: completely empty
        assert!(client.subscriptions_state.is_empty());
        assert!(client.subscriptions_state.all_topics().is_empty());
    }

    #[rstest]
    fn test_race_reconnection_with_pending_states() {
        // Simulates reconnection with mixed pending states.
        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some("test_passphrase".to_string()),
            Some(AccountId::new("OKX-TEST")),
            None,
        )
        .expect("Failed to create client");

        // Set up mixed state before reconnection
        // Confirmed: trades:BTC-USDT-SWAP
        let trade_btc = "trades:BTC-USDT-SWAP";
        client.subscriptions_state.mark_subscribe(trade_btc);
        client.subscriptions_state.confirm_subscribe(trade_btc);

        // Pending subscribe: trades:ETH-USDT-SWAP
        let trade_eth = "trades:ETH-USDT-SWAP";
        client.subscriptions_state.mark_subscribe(trade_eth);

        // Pending unsubscribe: books:BTC-USDT (user cancelled)
        let book_btc = "books:BTC-USDT";
        client.subscriptions_state.mark_subscribe(book_btc);
        client.subscriptions_state.confirm_subscribe(book_btc);
        client.subscriptions_state.mark_unsubscribe(book_btc);

        // Get topics for reconnection
        let topics_to_restore = client.subscriptions_state.all_topics();

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
        let client = OKXWebSocketClient::new(
            Some("wss://test.okx.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client");

        let topic = "trades:BTC-USDT-SWAP";

        // Subscribe and confirm
        client.subscriptions_state.mark_subscribe(topic);
        client.subscriptions_state.confirm_subscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 1);

        // Duplicate mark_subscribe on already-confirmed topic (should be no-op)
        client.subscriptions_state.mark_subscribe(topic);
        assert!(
            client
                .subscriptions_state
                .pending_subscribe_topics()
                .is_empty()
        ); // Not re-added
        assert_eq!(client.subscriptions_state.len(), 1); // Still just 1

        // Duplicate confirm_subscribe (should be idempotent)
        client.subscriptions_state.confirm_subscribe(topic);
        assert_eq!(client.subscriptions_state.len(), 1);

        // Verify final state
        let all = client.subscriptions_state.all_topics();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], topic);
    }
}
