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

use ahash::{AHashMap, AHashSet};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    AtomicMap,
    consts::NAUTILUS_USER_AGENT,
    env::{get_env_var, get_or_env_var},
    string::secret::REDACTED,
};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, PositionSide, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::USER_AGENT,
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        AUTHENTICATION_TIMEOUT_SECS, AuthTracker, PingHandler, SubscriptionState, TEXT_PING,
        TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Error;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    enums::OKXWsChannel,
    error::OKXWsError,
    handler::{HandlerCommand, OKXWsFeedHandler},
    messages::{
        OKXAuthentication, OKXAuthenticationArg, OKXSubscriptionArg, OKXWsMessage, OKXWsRequest,
        WsAmendOrderParamsBuilder, WsAttachAlgoOrdParams, WsCancelOrderParamsBuilder,
        WsMassCancelParams, WsPostAlgoOrderParamsBuilder, WsPostOrderParamsBuilder,
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
        OKXGreeksType, OKXInstrumentType, OKXOrderType, OKXPositionSide, OKXTargetCurrency,
        OKXTradeMode, OKXTriggerType, OKXVipLevel, conditional_order_to_algo_type,
        is_conditional_order,
    },
    parse::{
        bar_spec_as_okx_channel, okx_instrument_type, okx_instrument_type_from_symbol,
        parse_base_quote_from_symbol,
    },
};

/// Default OKX WebSocket connection rate limit: 3 requests per second.
///
/// This applies to establishing WebSocket connections, not to subscribe/unsubscribe operations.
pub static OKX_WS_CONNECTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(3).expect("non-zero")).expect("valid constant")
});

/// OKX WebSocket subscription rate limit: 480 requests per hour per connection.
///
/// This applies to subscribe/unsubscribe/login operations.
/// 480 per hour = 8 per minute, but we use per-hour for accurate limiting.
pub static OKX_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_hour(NonZeroU32::new(480).expect("non-zero")));

/// Rate limit for order-related WebSocket operations: 250 requests per second.
///
/// Based on OKX documentation for sub-account order limits (1000 per 2 seconds,
/// so we use half for conservative rate limiting).
pub static OKX_WS_ORDER_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(250).expect("non-zero")).expect("valid constant")
});

/// Pre-interned rate limit key for subscription operations (subscribe/unsubscribe/login).
///
/// See: <https://www.okx.com/docs-v5/en/#websocket-api-login>
/// See: <https://www.okx.com/docs-v5/en/#websocket-api-subscribe>
pub static OKX_RATE_LIMIT_KEY_SUBSCRIPTION: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("subscription")]);

/// Pre-interned rate limit key for order operations (place regular and algo orders).
///
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order>
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-ws-place-algo-order>
pub static OKX_RATE_LIMIT_KEY_ORDER: LazyLock<[Ustr; 1]> = LazyLock::new(|| [Ustr::from("order")]);

/// Pre-interned rate limit key for cancel operations (cancel regular and algo orders, mass cancel).
///
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-cancel-order>
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-ws-cancel-algo-order>
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-mass-cancel-order>
pub static OKX_RATE_LIMIT_KEY_CANCEL: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("cancel")]);

/// Pre-interned rate limit key for amend operations (amend orders).
///
/// See: <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-amend-order>
pub static OKX_RATE_LIMIT_KEY_AMEND: LazyLock<[Ustr; 1]> = LazyLock::new(|| [Ustr::from("amend")]);

/// Context stored at order submission time for correlating venue responses.
///
/// Fields are read in `python/websocket.rs` (behind the `python` feature gate).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct PendingOrderInfo {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
}

/// Provides a WebSocket client for connecting to [OKX](https://okx.com).
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.okx", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.okx")
)]
pub struct OKXWebSocketClient {
    url: String,
    #[allow(dead_code)] // Read by Python bindings
    pub(crate) account_id: AccountId,
    vip_level: Arc<AtomicU8>,
    credential: Option<Credential>,
    heartbeat: Option<u64>,
    auth_timeout_secs: u64,
    auth_tracker: AuthTracker,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<OKXWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions_inst_type: Arc<DashMap<OKXWsChannel, AHashSet<OKXInstrumentType>>>,
    subscriptions_inst_family: Arc<DashMap<OKXWsChannel, AHashSet<Ustr>>>,
    subscriptions_inst_id: Arc<DashMap<OKXWsChannel, AHashSet<Ustr>>>,
    subscriptions_bare: Arc<DashMap<OKXWsChannel, bool>>,
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    inst_id_code_cache: Arc<AtomicMap<Ustr, u64>>,
    pub(crate) pending_orders: Arc<DashMap<String, PendingOrderInfo>>,
    pub(crate) pending_cancels: Arc<DashMap<String, PendingOrderInfo>>,
    pub(crate) pending_amends: Arc<DashMap<String, PendingOrderInfo>>,
    option_greeks_subs: Arc<AtomicMap<InstrumentId, AHashSet<OKXGreeksType>>>,
    /// Per-base-pair refcount for the `index-tickers` channel. Multiple
    /// instruments commonly share one base pair (e.g. `BTC-USDT-SWAP` and
    /// `BTC-USDT-240628` both depend on `BTC-USDT`), so the venue
    /// (un)subscribe must only fire on the 0↔1 transitions. Without this
    /// refcount, a Python caller unsubscribing one instrument would tear
    /// down the channel for every other subscriber on the same pair.
    index_pair_subscribers: Arc<DashMap<Ustr, usize>>,
    /// Serializes index-tickers transitions so a concurrent
    /// subscribe/unsubscribe pair on the same base pair cannot interleave
    /// the refcount check with the venue send and leave the channel
    /// unsubscribed while the local count says it is live.
    index_pair_transition: Arc<tokio::sync::Mutex<()>>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    transport_backend: TransportBackend,
    /// Optional proxy URL for the WebSocket transport.
    proxy_url: Option<String>,
    cancellation_token: CancellationToken,
}

impl Default for OKXWebSocketClient {
    fn default() -> Self {
        Self::new(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            TransportBackend::default(),
            None,
        )
        .unwrap()
    }
}

impl Debug for OKXWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OKXWebSocketClient))
            .field("url", &self.url)
            .field("credential", &self.credential.as_ref().map(|_| REDACTED))
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        auth_timeout_secs: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
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
            vip_level: Arc::new(AtomicU8::new(0)),
            credential,
            heartbeat,
            auth_timeout_secs: auth_timeout_secs.unwrap_or(AUTHENTICATION_TIMEOUT_SECS),
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
            instruments_cache: Arc::new(AtomicMap::new()),
            inst_id_code_cache: Arc::new(AtomicMap::new()),
            pending_orders: Arc::new(DashMap::new()),
            pending_cancels: Arc::new(DashMap::new()),
            pending_amends: Arc::new(DashMap::new()),
            option_greeks_subs: Arc::new(AtomicMap::new()),
            index_pair_subscribers: Arc::new(DashMap::new()),
            index_pair_transition: Arc::new(tokio::sync::Mutex::new(())),
            transport_backend,
            proxy_url,
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new [`OKXWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if credential values cannot be loaded or if the
    /// client fails to initialize.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        auth_timeout_secs: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
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
            auth_timeout_secs,
            transport_backend,
            proxy_url,
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
            None,
            TransportBackend::default(),
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
        self.credential.as_ref().map(|c| c.api_key())
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> Option<String> {
        self.credential.as_ref().map(|c| c.api_key_masked())
    }

    /// Returns a value indicating whether the client is active.
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Acquire)
    }

    /// Returns a value indicating whether the client is closed.
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Acquire)
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same symbols will be replaced.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments_cache.rcu(|m| {
            for inst in instruments {
                m.insert(inst.symbol().inner(), inst.clone());
            }
        });
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument);
    }

    /// Returns a snapshot of the instruments cache as an `AHashMap`.
    pub fn instruments_snapshot(&self) -> AHashMap<Ustr, InstrumentAny> {
        (**self.instruments_cache.load()).clone()
    }

    /// Caches the instIdCode mapping for an instrument.
    ///
    /// The instIdCode is required for WebSocket order operations per OKX API deprecation.
    pub fn cache_inst_id_code(&self, inst_id: Ustr, inst_id_code: u64) {
        self.inst_id_code_cache.insert(inst_id, inst_id_code);
    }

    /// Caches multiple instIdCode mappings for instruments.
    ///
    /// This is typically called after loading instruments from the HTTP API.
    pub fn cache_inst_id_codes(&self, mappings: impl IntoIterator<Item = (Ustr, u64)>) {
        let entries: Vec<_> = mappings.into_iter().collect();
        self.inst_id_code_cache.rcu(|m| {
            for (inst_id, inst_id_code) in &entries {
                m.insert(*inst_id, *inst_id_code);
            }
        });
    }

    /// Gets the instIdCode for an instrument.
    ///
    /// Returns `None` if the instrument is not in the cache.
    #[must_use]
    pub fn get_inst_id_code(&self, inst_id: &Ustr) -> Option<u64> {
        self.inst_id_code_cache.load().get(inst_id).copied()
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
        // Reset signal so is_active()/is_closed() work after a previous close()
        self.signal.store(false, Ordering::Release);

        let (message_handler, raw_rx) = channel_message_handler();

        // No-op ping handler: handler owns the WebSocketClient and responds to pings directly
        // in the message loop for minimal latency (see handler.rs TEXT_PONG response)
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
        });

        let headers = vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())];

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers,
            heartbeat: self.heartbeat,
            heartbeat_msg: Some(TEXT_PING.to_string()),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let keyed_quotas = vec![
            (
                OKX_RATE_LIMIT_KEY_SUBSCRIPTION[0].as_str().to_string(),
                *OKX_WS_SUBSCRIPTION_QUOTA,
            ),
            (
                OKX_RATE_LIMIT_KEY_ORDER[0].as_str().to_string(),
                *OKX_WS_ORDER_QUOTA,
            ),
            (
                OKX_RATE_LIMIT_KEY_CANCEL[0].as_str().to_string(),
                *OKX_WS_ORDER_QUOTA,
            ),
            (
                OKX_RATE_LIMIT_KEY_AMEND[0].as_str().to_string(),
                *OKX_WS_ORDER_QUOTA,
            ),
        ];

        let client = WebSocketClient::connect(
            config,
            Some(message_handler),
            Some(ping_handler),
            None, // post_reconnection
            keyed_quotas,
            Some(*OKX_WS_CONNECTION_QUOTA), // Default quota for connection operations
        )
        .await?;

        // Replace connection state so all clones see the underlying WebSocketClient's state
        self.connection_mode.store(client.connection_mode_atomic());

        let (msg_tx, rx) = tokio::sync::mpsc::unbounded_channel::<OKXWsMessage>();

        self.out_rx = Some(Arc::new(rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        let signal = self.signal.clone();
        let auth_tracker = self.auth_tracker.clone();
        let subscriptions_state = self.subscriptions_state.clone();

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
                    signal.clone(),
                    cmd_rx,
                    raw_rx,
                    msg_tx,
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
                                log::error!("Failed to send resubscribe command: error={e}");
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
                            log::error!("Failed to send resubscribe command: error={e}");
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
                                log::error!("Failed to send resubscribe command: error={e}");
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
                                log::error!("Failed to send resubscribe command: error={e}");
                            }
                        }
                    }
                };

                loop {
                    match handler.next().await {
                        Some(OKXWsMessage::Reconnected) => {
                            if signal.load(Ordering::Acquire) {
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
                                log::debug!("Marking confirmed subscriptions as pending for replay: count={}", confirmed_topics_vec.len());
                                for topic in confirmed_topics_vec {
                                    subscriptions_state.mark_failure(&topic);
                                }
                            }

                            if let Some(cred) = &credential {
                                log::debug!("Re-authenticating after reconnection");
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                    .expect("System time should be after UNIX epoch")
                                    .as_secs()
                                    .to_string();
                                let signature = cred.sign(&timestamp, "GET", "/users/self/verify", "");

                                let auth_message = super::messages::OKXAuthentication {
                                    op: "login",
                                    args: vec![super::messages::OKXAuthenticationArg {
                                        api_key: cred.api_key().to_string(),
                                        passphrase: cred.api_passphrase().to_string(),
                                        timestamp,
                                        sign: signature,
                                    }],
                                };

                                if let Ok(payload) = serde_json::to_string(&auth_message) {
                                    if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Authenticate { payload }) {
                                        log::error!("Failed to send reconnection auth command: error={e}");
                                    }
                                } else {
                                    log::error!("Failed to serialize reconnection auth message");
                                }
                            }

                            // Unauthenticated sessions resubscribe immediately after reconnection,
                            // authenticated sessions wait for Authenticated message
                            if credential.is_none() {
                                log::debug!("No authentication required, resubscribing immediately");
                                resubscribe_all();
                            }

                            // Forward Reconnected to consumers so they can reset state
                            if handler.send(OKXWsMessage::Reconnected).is_err() {
                                log::error!("Failed to send Reconnected through channel: receiver dropped");
                                break;
                            }
                        }
                        Some(OKXWsMessage::Authenticated) => {
                            if has_reconnected {
                                resubscribe_all();
                            }
                        }
                        Some(msg) => {
                            if handler.send(msg).is_err() {
                                log::error!(
                                    "Failed to send message through channel: receiver dropped",
                                );
                                break;
                            }
                        }
                        None => {
                            if handler.is_stopped() {
                                log::debug!(
                                    "Stop signal received, ending message processing",
                                );
                                break;
                            }
                            log::debug!("WebSocket stream closed");
                            break;
                        }
                    }
                }

                log::debug!("Handler task exiting");
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
        log::debug!("Sent WebSocket client to handler");

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
                api_key: credential.api_key().to_string(),
                passphrase: credential.api_passphrase().to_string(),
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
            .wait_for_result::<OKXWsError>(Duration::from_secs(self.auth_timeout_secs), rx)
            .await
        {
            Ok(()) => {
                log::info!("WebSocket authenticated");
                Ok(())
            }
            Err(e) => {
                log::error!("WebSocket authentication failed: error={e}");
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
    pub fn stream(&mut self) -> impl Stream<Item = OKXWsMessage> + 'static {
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

        self.signal.store(true, Ordering::Release);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::warn!("Failed to send disconnect command to handler: {e}");
        } else {
            log::debug!("Sent disconnect command to handler");
        }

        if let Some(stream_handle) = self.task_handle.take() {
            match Arc::try_unwrap(stream_handle) {
                Ok(handle) => {
                    log::debug!("Waiting for stream handle to complete");
                    let abort_handle = handle.abort_handle();
                    match tokio::time::timeout(Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Stream handle completed successfully"),
                        Ok(Err(e)) => log::error!("Stream handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!("Timeout waiting for stream handle, aborting task");
                            abort_handle.abort();
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

        // Wipe per-base-pair refcounts so a subsequent reconnect can re-arm
        // the index-tickers channel. Otherwise the stale count short-circuits
        // every future `subscribe_index_prices` call and the feed stays dark.
        self.index_pair_subscribers.clear();

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

    async fn subscribe(&self, args: Vec<OKXSubscriptionArg>) -> Result<(), OKXWsError> {
        // Send the command first; only update local state on success
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Subscribe { args: args.clone() })
            .map_err(|e| {
                OKXWsError::ClientError(format!("Failed to send subscribe command: {e}"))
            })?;

        for arg in &args {
            let topic = topic_from_subscription_arg(arg);
            self.subscriptions_state.mark_subscribe(&topic);

            // Check if this is a bare channel (no inst params)
            if arg.inst_type.is_none() && arg.inst_family.is_none() && arg.inst_id.is_none() {
                self.subscriptions_bare.insert(arg.channel.clone(), true);
            } else {
                if let Some(inst_type) = &arg.inst_type {
                    self.subscriptions_inst_type
                        .entry(arg.channel.clone())
                        .or_default()
                        .insert(*inst_type);
                }

                if let Some(inst_family) = &arg.inst_family {
                    self.subscriptions_inst_family
                        .entry(arg.channel.clone())
                        .or_default()
                        .insert(*inst_family);
                }

                if let Some(inst_id) = &arg.inst_id {
                    self.subscriptions_inst_id
                        .entry(arg.channel.clone())
                        .or_default()
                        .insert(*inst_id);
                }
            }
        }

        Ok(())
    }

    #[expect(clippy::collapsible_if)]
    async fn unsubscribe(&self, args: Vec<OKXSubscriptionArg>) -> Result<(), OKXWsError> {
        // Send the command first; only update local state on success
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Unsubscribe { args: args.clone() })
            .map_err(|e| {
                OKXWsError::ClientError(format!("Failed to send unsubscribe command: {e}"))
            })?;

        for arg in &args {
            let topic = topic_from_subscription_arg(arg);
            self.subscriptions_state.mark_unsubscribe(&topic);

            if arg.inst_type.is_none() && arg.inst_family.is_none() && arg.inst_id.is_none() {
                self.subscriptions_bare.remove(&arg.channel);
            } else {
                if let Some(inst_type) = &arg.inst_type {
                    if let Some(mut entry) = self.subscriptions_inst_type.get_mut(&arg.channel) {
                        entry.remove(inst_type);
                        if entry.is_empty() {
                            drop(entry);
                            self.subscriptions_inst_type.remove(&arg.channel);
                        }
                    }
                }

                if let Some(inst_family) = &arg.inst_family {
                    if let Some(mut entry) = self.subscriptions_inst_family.get_mut(&arg.channel) {
                        entry.remove(inst_family);
                        if entry.is_empty() {
                            drop(entry);
                            self.subscriptions_inst_family.remove(&arg.channel);
                        }
                    }
                }

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

        Ok(())
    }

    async fn subscribe_inst_id(
        &self,
        channel: OKXWsChannel,
        inst_id: Ustr,
    ) -> Result<(), OKXWsError> {
        self.subscribe(vec![OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(inst_id),
        }])
        .await
    }

    async fn unsubscribe_inst_id(
        &self,
        channel: OKXWsChannel,
        inst_id: Ustr,
    ) -> Result<(), OKXWsError> {
        self.unsubscribe(vec![OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(inst_id),
        }])
        .await
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
        const BATCH_SIZE: usize = 256;

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
            log::debug!("No active subscriptions to unsubscribe from");
            return Ok(());
        }

        log::debug!("Batched unsubscribe from {} channels", all_args.len());

        for chunk in all_args.chunks(BATCH_SIZE) {
            self.unsubscribe(chunk.to_vec()).await?;
        }

        // The index-pair refcount mirrors live subscriptions; after a bulk
        // unsubscribe the venue knows nothing, so any retained count would
        // wedge the next `subscribe_index_prices`.
        self.index_pair_subscribers.clear();

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
    /// this method subscribes to the entire instrument type. OKX handles duplicate
    /// subscriptions gracefully and pushes a fresh snapshot on each subscribe.
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
        log::debug!("Subscribing to instrument type {inst_type:?} for {instrument_id}");
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
        self.subscribe_inst_id(OKXWsChannel::Books, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(OKXWsChannel::Books5, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(OKXWsChannel::Books50Tbt, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(OKXWsChannel::BooksTbt, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(OKXWsChannel::BboTbt, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(channel, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(OKXWsChannel::Tickers, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(OKXWsChannel::MarkPrice, instrument_id.symbol.inner())
            .await
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
        // Index-tickers channel requires base pair format (e.g., BTC-USDT)
        let symbol = instrument_id.symbol.inner();
        let (base, quote) = parse_base_quote_from_symbol(symbol.as_str())
            .map_err(|e| OKXWsError::ClientError(e.to_string()))?;
        let base_pair = Ustr::from(&format!("{base}-{quote}"));

        // Hold the transition lock across both the refcount update and the
        // venue send so a concurrent `unsubscribe_index_prices` cannot
        // observe a transient 0 state between our decrement and the venue
        // unsubscribe, or vice versa. Without this, contract rolls can
        // leave the venue unsubscribed while the local count says active.
        let _guard = self.index_pair_transition.lock().await;

        // Bump the per-base-pair refcount so a later unsubscribe can decide
        // whether it is the last subscriber. Only the 0→1 transition fires
        // a venue subscribe; subsequent callers piggy-back on the existing
        // channel.
        let is_first = {
            let mut count = self.index_pair_subscribers.entry(base_pair).or_insert(0);
            *count += 1;
            *count == 1
        };

        if !is_first {
            return Ok(());
        }

        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::IndexTickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(base_pair),
        };

        match self.subscribe(vec![arg]).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // When the venue subscribe fails there is no live channel,
                // even though other local callers may have piggy-backed on
                // the in-flight attempt (they saw `!is_first` and returned
                // `Ok`). Removing the entry entirely ensures the next
                // caller re-enters the 0→1 branch and re-arms the venue
                // subscription; a mere decrement would leave the map at 1+
                // without a matching feed and every later subscribe would
                // short-circuit into a silent no-op.
                self.index_pair_subscribers.remove(&base_pair);
                Err(e)
            }
        }
    }

    /// Subscribes to option summary data for an instrument family.
    ///
    /// Streams greeks (delta, gamma, vega, theta), implied volatility, and other
    /// option metrics for all instruments in the specified family.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-websocket-option-summary-channel>.
    pub async fn subscribe_option_summary(&self, inst_family: Ustr) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::OptionSummary,
            inst_type: None,
            inst_family: Some(inst_family),
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Returns a reference to the option greeks subscription map.
    ///
    /// The map stores the set of greeks conventions to emit for each subscribed instrument.
    pub fn option_greeks_subs(&self) -> &Arc<AtomicMap<InstrumentId, AHashSet<OKXGreeksType>>> {
        &self.option_greeks_subs
    }

    /// Adds an instrument to the option greeks subscription filter, emitting both
    /// Black-Scholes and price-adjusted greeks.
    pub fn add_option_greeks_sub(&self, instrument_id: InstrumentId) {
        let both: AHashSet<OKXGreeksType> =
            [OKXGreeksType::Bs, OKXGreeksType::Pa].into_iter().collect();
        self.option_greeks_subs.insert(instrument_id, both);
    }

    /// Adds an instrument to the option greeks subscription filter with an explicit
    /// set of greeks conventions to emit. An empty set is treated as "emit both".
    pub fn add_option_greeks_sub_with_conventions(
        &self,
        instrument_id: InstrumentId,
        conventions: AHashSet<OKXGreeksType>,
    ) {
        let set = if conventions.is_empty() {
            [OKXGreeksType::Bs, OKXGreeksType::Pa].into_iter().collect()
        } else {
            conventions
        };
        self.option_greeks_subs.insert(instrument_id, set);
    }

    /// Removes an instrument from the option greeks subscription filter.
    pub fn remove_option_greeks_sub(&self, instrument_id: &InstrumentId) {
        self.option_greeks_subs.remove(instrument_id);
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
        self.subscribe_inst_id(OKXWsChannel::FundingRate, instrument_id.symbol.inner())
            .await
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
        self.subscribe_inst_id(channel, bar_type.instrument_id().symbol.inner())
            .await
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
    /// No-op: the instruments channel is per-type (SWAP, FUTURES, etc.) and
    /// other instruments of the same type may still need it. The channel
    /// stays subscribed; overhead is negligible.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        log::debug!("Instrument unsubscribe is a no-op (shared per-type channel): {instrument_id}");
        Ok(())
    }

    /// Unsubscribe from full order book data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        self.unsubscribe_inst_id(OKXWsChannel::Books, instrument_id.symbol.inner())
            .await
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
        self.unsubscribe_inst_id(OKXWsChannel::Books5, instrument_id.symbol.inner())
            .await
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
        self.unsubscribe_inst_id(OKXWsChannel::Books50Tbt, instrument_id.symbol.inner())
            .await
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
        self.unsubscribe_inst_id(OKXWsChannel::BooksTbt, instrument_id.symbol.inner())
            .await
    }

    /// Unsubscribe from best bid/ask quote data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        self.unsubscribe_inst_id(OKXWsChannel::BboTbt, instrument_id.symbol.inner())
            .await
    }

    /// Unsubscribe from 24hr rolling ticker data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_ticker(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        self.unsubscribe_inst_id(OKXWsChannel::Tickers, instrument_id.symbol.inner())
            .await
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
        self.unsubscribe_inst_id(OKXWsChannel::MarkPrice, instrument_id.symbol.inner())
            .await
    }

    /// Unsubscribe from index price data for the base pair derived from
    /// `instrument_id`.
    ///
    /// Refcounting is handled internally so any caller (Rust data client,
    /// Python wrapper, etc.) can pair every `subscribe_index_prices` with
    /// exactly one `unsubscribe_index_prices`. The OKX `index-tickers`
    /// channel is keyed by base pair (e.g. `BTC-USDT`), so the venue
    /// unsubscribe only fires when the last subscriber for that pair drops.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let symbol = instrument_id.symbol.inner();
        let (base, quote) = parse_base_quote_from_symbol(symbol.as_str())
            .map_err(|e| OKXWsError::ClientError(e.to_string()))?;
        let base_pair = Ustr::from(&format!("{base}-{quote}"));

        // Serialize with any concurrent `subscribe_index_prices` on the same
        // base pair. See the subscribe path for the race this prevents.
        let _guard = self.index_pair_transition.lock().await;

        let is_last = {
            let Some(mut count) = self.index_pair_subscribers.get_mut(&base_pair) else {
                // No matching subscriber recorded; nothing to do.
                return Ok(());
            };
            *count = count.saturating_sub(1);
            *count == 0
        };

        if !is_last {
            return Ok(());
        }

        self.index_pair_subscribers
            .remove_if(&base_pair, |_, count| *count == 0);

        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::IndexTickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(base_pair),
        };
        self.unsubscribe(vec![arg]).await
    }

    /// Unsubscribe from option summary data for an instrument family.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_option_summary(&self, inst_family: Ustr) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::OptionSummary,
            inst_type: None,
            inst_family: Some(inst_family),
            inst_id: None,
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
        self.unsubscribe_inst_id(OKXWsChannel::FundingRate, instrument_id.symbol.inner())
            .await
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
        self.unsubscribe_inst_id(channel, instrument_id.symbol.inner())
            .await
    }

    /// Unsubscribe from candlestick/bar data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), OKXWsError> {
        let channel = bar_spec_as_okx_channel(bar_type.spec())
            .map_err(|e| OKXWsError::ClientError(e.to_string()))?;
        self.unsubscribe_inst_id(channel, bar_type.instrument_id().symbol.inner())
            .await
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

    /// Subscribes to advance algo order updates (trailing stops, iceberg, twap).
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn subscribe_algo_advance(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::AlgoAdvance,
            inst_type: Some(instrument_type),
            inst_family: None,
            inst_id: None,
        };
        self.subscribe(vec![arg]).await
    }

    /// Unsubscribes from advance algo order updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    pub async fn unsubscribe_algo_advance(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::AlgoAdvance,
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
        let request = OKXWsRequest::<Value> {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::BatchOrders,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize batch orders: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_ORDER.to_vec()),
            request_id: Some(request_id),
            client_order_id: None,
            op: Some(super::enums::OKXWsOperation::BatchOrders),
        };

        self.send_cmd(cmd).await
    }

    /// Cancel multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-cancel-orders>
    async fn ws_batch_cancel_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest::<Value> {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::BatchCancelOrders,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize batch cancel: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_CANCEL.to_vec()),
            request_id: Some(request_id),
            client_order_id: None,
            op: Some(super::enums::OKXWsOperation::BatchCancelOrders),
        };

        self.send_cmd(cmd).await
    }

    /// Amend multiple orders in a single batch via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-batch-amend-orders>
    async fn ws_batch_amend_orders(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest::<Value> {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::BatchAmendOrders,
            exp_time: None,
            args,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize batch amend: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_AMEND.to_vec()),
            request_id: Some(request_id),
            client_order_id: None,
            op: Some(super::enums::OKXWsOperation::BatchAmendOrders),
        };

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
    #[expect(clippy::too_many_arguments)]
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
        attach_algo_ords: Option<Vec<WsAttachAlgoOrdParams>>,
        px_usd: Option<String>,
        px_vol: Option<String>,
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

        let inst_id_code = self
            .get_inst_id_code(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!(
                    "No instIdCode cached for {instrument_id}, cannot submit order"
                ))
            })?;
        builder.inst_id_code(inst_id_code);

        builder.td_mode(td_mode);
        builder.cl_ord_id(client_order_id.as_str());

        let instrument = self
            .instruments_cache
            .get_cloned(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!("Unknown instrument {instrument_id}"))
            })?;

        let instrument_type =
            okx_instrument_type(&instrument).map_err(|e| OKXWsError::ClientError(e.to_string()))?;
        let quote_currency = instrument.quote_currency();

        // OKX options only support limit-style orders
        if instrument_type == OKXInstrumentType::Option
            && matches!(order_type, OrderType::Market | OrderType::MarketToLimit)
        {
            return Err(OKXWsError::ClientError(
                "Market orders are not supported for OKX options, use Limit orders instead"
                    .to_string(),
            ));
        }

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
            OKXInstrumentType::Option => {
                builder.ccy(quote_currency.to_string());

                if position_side.is_none() {
                    builder.pos_side(OKXPositionSide::Net);
                }
                // reduceOnly is not applicable to options per OKX docs
            }
            _ => {
                builder.ccy(quote_currency.to_string());

                if position_side.is_none() {
                    builder.pos_side(OKXPositionSide::Net);
                }

                if let Some(ro) = reduce_only
                    && ro
                {
                    builder.reduce_only(ro);
                }
            }
        }

        if let Some(attach_algo_ords) = attach_algo_ords {
            builder.attach_algo_ords(attach_algo_ords);
        }

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
                    builder.tgt_ccy(OKXTargetCurrency::QuoteCcy);
                }
                // For BUY orders, must explicitly set to base_ccy to override OKX default
                Some(false) if order_side == OrderSide::Buy => {
                    builder.tgt_ccy(OKXTargetCurrency::BaseCcy);
                }
                // For SELL orders with quote_quantity=false, omit tgtCcy (OKX defaults to base_ccy correctly)
                Some(false) | None => {}
            }
        }

        builder.side(order_side.as_specified());

        if let Some(pos_side) = position_side {
            builder.pos_side(pos_side);
        }

        // OKX implements FOK/IOC as order types rather than separate time-in-force
        // Market + FOK is unsupported (FOK requires a limit price)
        // optimal_limit_ioc is only supported for SWAP/FUTURES, not SPOT or OPTION
        let (okx_ord_type, price) = if post_only.unwrap_or(false) {
            (OKXOrderType::PostOnly, price)
        } else if let Some(tif) = time_in_force {
            match (order_type, tif) {
                (OrderType::Market, TimeInForce::Fok) => {
                    return Err(OKXWsError::ClientError(
                        "Market orders with FOK time-in-force are not supported by OKX. Use Limit order with FOK instead.".to_string()
                    ));
                }
                (OrderType::Market, TimeInForce::Ioc) => {
                    // optimal_limit_ioc only works for SWAP/FUTURES
                    if matches!(
                        instrument_type,
                        OKXInstrumentType::Spot | OKXInstrumentType::Option
                    ) {
                        (OKXOrderType::Market, price)
                    } else {
                        (OKXOrderType::OptimalLimitIoc, price)
                    }
                }
                (OrderType::Limit, TimeInForce::Fok) => {
                    // OKX uses op_fok for options FOK orders
                    if instrument_type == OKXInstrumentType::Option {
                        (OKXOrderType::OpFok, price)
                    } else {
                        (OKXOrderType::Fok, price)
                    }
                }
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

        // For options: pxUsd/pxVol are mutually exclusive with px
        if let Some(usd) = px_usd {
            builder.px_usd(usd);
        } else if let Some(vol) = px_vol {
            builder.px_vol(vol);
        } else if let Some(tp) = trigger_price {
            builder.px(tp.to_string());
        } else if let Some(p) = price {
            builder.px(p.to_string());
        }

        builder.tag(OKX_NAUTILUS_BROKER_ID);

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build order params error: {e}")))?;

        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::Order,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize order: {e}")))?;

        let cl_ord_key = client_order_id.to_string();
        self.pending_orders.insert(
            cl_ord_key.clone(),
            PendingOrderInfo {
                trader_id,
                strategy_id,
                instrument_id,
            },
        );

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_ORDER.to_vec()),
            request_id: Some(request_id),
            client_order_id: Some(client_order_id),
            op: Some(super::enums::OKXWsOperation::Order),
        };

        let result = self.send_cmd(cmd).await;

        if result.is_err() {
            self.pending_orders.remove(&cl_ord_key);
        }

        result
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
    #[expect(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        price: Option<Price>,
        quantity: Option<Quantity>,
        venue_order_id: Option<VenueOrderId>,
        new_px_usd: Option<String>,
        new_px_vol: Option<String>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsAmendOrderParamsBuilder::default();

        let inst_id_code = self
            .get_inst_id_code(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!(
                    "No instIdCode cached for {instrument_id}, cannot amend order"
                ))
            })?;
        builder.inst_id_code(inst_id_code);

        if let Some(venue_order_id) = venue_order_id {
            builder.ord_id(venue_order_id.as_str());
        }

        let cl_ord_key = client_order_id.map(|id| id.to_string());

        if let Some(client_order_id) = client_order_id {
            builder.cl_ord_id(client_order_id.as_str());
            self.pending_amends.insert(
                client_order_id.to_string(),
                PendingOrderInfo {
                    trader_id,
                    strategy_id,
                    instrument_id,
                },
            );
        }

        // For options: newPxUsd/newPxVol are mutually exclusive with newPx
        if let Some(usd) = new_px_usd {
            builder.new_px_usd(usd);
        } else if let Some(vol) = new_px_vol {
            builder.new_px_vol(vol);
        } else if let Some(price) = price {
            builder.new_px(price.to_string());
        }

        if let Some(quantity) = quantity {
            builder.new_sz(quantity.to_string());
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build amend params error: {e}")))?;

        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::AmendOrder,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize amend: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_AMEND.to_vec()),
            request_id: Some(request_id),
            client_order_id,
            op: Some(super::enums::OKXWsOperation::AmendOrder),
        };

        let result = self.send_cmd(cmd).await;

        if let (Err(_), Some(key)) = (&result, &cl_ord_key) {
            self.pending_amends.remove(key);
        }

        result
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
    pub async fn cancel_order(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsCancelOrderParamsBuilder::default();

        let inst_id_code = self
            .get_inst_id_code(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!(
                    "No instIdCode cached for {instrument_id}, cannot cancel order"
                ))
            })?;
        builder.inst_id_code(inst_id_code);

        if let Some(venue_order_id) = venue_order_id {
            builder.ord_id(venue_order_id.as_str());
        }

        let cl_ord_key = client_order_id.map(|id| id.to_string());

        if let Some(client_order_id) = client_order_id {
            builder.cl_ord_id(client_order_id.as_str());
            self.pending_cancels.insert(
                client_order_id.to_string(),
                PendingOrderInfo {
                    trader_id,
                    strategy_id,
                    instrument_id,
                },
            );
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build cancel params error: {e}")))?;

        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::CancelOrder,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize cancel: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_CANCEL.to_vec()),
            request_id: Some(request_id),
            client_order_id,
            op: Some(super::enums::OKXWsOperation::CancelOrder),
        };

        let result = self.send_cmd(cmd).await;

        if let (Err(_), Some(key)) = (&result, &cl_ord_key) {
            self.pending_cancels.remove(key);
        }

        result
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
        let instrument = self
            .instruments_cache
            .get_cloned(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!("Unknown instrument {instrument_id}"))
            })?;

        let inst_type =
            okx_instrument_type(&instrument).map_err(|e| OKXWsError::ClientError(e.to_string()))?;

        let symbol = instrument.symbol().inner();
        let inst_family = match &instrument {
            InstrumentAny::CurrencyPair(_) => symbol.as_str().to_string(),
            InstrumentAny::CryptoPerpetual(_) => symbol
                .as_str()
                .strip_suffix("-SWAP")
                .unwrap_or(symbol.as_str())
                .to_string(),
            InstrumentAny::CryptoFuture(_) => {
                let s = symbol.as_str();
                if let Some(idx) = s.rfind('-') {
                    s[..idx].to_string()
                } else {
                    s.to_string()
                }
            }
            _ => {
                return Err(OKXWsError::ClientError(
                    "Unsupported instrument type for mass cancel".to_string(),
                ));
            }
        };
        drop(instrument);

        let params = WsMassCancelParams {
            inst_type,
            inst_family: Ustr::from(&inst_family),
        };

        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::MassCancel,
            exp_time: None,
            args: vec![
                serde_json::to_value(params).map_err(|e| OKXWsError::JsonError(e.to_string()))?,
            ],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize mass cancel: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_CANCEL.to_vec()),
            request_id: Some(request_id),
            client_order_id: None,
            op: Some(super::enums::OKXWsOperation::MassCancel),
        };

        self.send_cmd(cmd).await
    }

    /// Submits multiple orders.
    ///
    /// # Errors
    ///
    /// Returns an error if any batch order parameters are invalid or if the
    /// batch request fails to send.
    #[expect(clippy::type_complexity)]
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

            let inst_id_code = self
                .get_inst_id_code(&inst_id.symbol.inner())
                .ok_or_else(|| {
                    OKXWsError::ClientError(format!(
                        "No instIdCode cached for {inst_id}, cannot submit order"
                    ))
                })?;
            builder.inst_id_code(inst_id_code);

            builder.td_mode(td_mode);
            builder.cl_ord_id(cl_ord_id.as_str());
            builder.side(ord_side.as_specified());

            if let Some(instrument) = self.instruments_cache.get_cloned(&inst_id.symbol.inner()) {
                builder.ccy(instrument.quote_currency().to_string());
            }

            if let Some(ps) = pos_side {
                builder.pos_side(OKXPositionSide::from(ps));
            } else if !matches!(inst_type, OKXInstrumentType::Spot) {
                builder.pos_side(OKXPositionSide::Net);
            }

            let okx_ord_type = if post_only.unwrap_or(false) {
                OKXOrderType::PostOnly
            } else {
                match ord_type {
                    OrderType::Market => OKXOrderType::Market,
                    OrderType::Limit => OKXOrderType::Limit,
                    OrderType::MarketToLimit => OKXOrderType::Ioc,
                    _ => {
                        return Err(OKXWsError::ClientError(format!(
                            "Unsupported order type for batch submit: {ord_type:?}"
                        )));
                    }
                }
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
    #[expect(clippy::type_complexity)]
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

            let inst_id_code = self
                .get_inst_id_code(&inst_id.symbol.inner())
                .ok_or_else(|| {
                    OKXWsError::ClientError(format!(
                        "No instIdCode cached for {inst_id}, cannot amend order"
                    ))
                })?;
            builder.inst_id_code(inst_id_code);

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
    pub async fn batch_cancel_orders(
        &self,
        orders: Vec<(InstrumentId, Option<ClientOrderId>, Option<VenueOrderId>)>,
    ) -> Result<(), OKXWsError> {
        let mut args: Vec<Value> = Vec::with_capacity(orders.len());
        for (inst_id, cl_ord_id, ord_id) in orders {
            let mut builder = WsCancelOrderParamsBuilder::default();

            let inst_id_code = self
                .get_inst_id_code(&inst_id.symbol.inner())
                .ok_or_else(|| {
                    OKXWsError::ClientError(format!(
                        "No instIdCode cached for {inst_id}, cannot cancel order"
                    ))
                })?;
            builder.inst_id_code(inst_id_code);

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
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_algo_order(
        &self,
        _trader_id: TraderId,
        _strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        limit_price: Option<Price>,
        reduce_only: Option<bool>,
        callback_ratio: Option<String>,
        callback_spread: Option<String>,
        activation_price: Option<Price>,
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

        let inst_id_code = self
            .get_inst_id_code(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!(
                    "No instIdCode cached for {instrument_id}, cannot submit algo order"
                ))
            })?;
        builder.inst_id_code(inst_id_code);

        builder.td_mode(td_mode);
        builder.cl_ord_id(client_order_id.as_str());
        builder.side(order_side.as_specified());
        builder.ord_type(
            conditional_order_to_algo_type(order_type)
                .map_err(|e| OKXWsError::ClientError(e.to_string()))?,
        );
        builder.sz(quantity.to_string());

        if let Some(tp) = trigger_price {
            builder.trigger_px(tp.to_string());
        }

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

        if let Some(ratio) = callback_ratio {
            builder.callback_ratio(ratio);
        }

        if let Some(spread) = callback_spread {
            builder.callback_spread(spread);
        }

        if let Some(active) = activation_price {
            builder.active_px(active.to_string());
        }

        builder.tag(OKX_NAUTILUS_BROKER_ID);

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build algo order params error: {e}")))?;

        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::OrderAlgo,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize algo order: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_ORDER.to_vec()),
            request_id: Some(request_id),
            client_order_id: Some(client_order_id),
            op: Some(super::enums::OKXWsOperation::OrderAlgo),
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
        _trader_id: TraderId,
        _strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        algo_order_id: Option<String>,
    ) -> Result<(), OKXWsError> {
        let mut builder = super::messages::WsCancelAlgoOrderParamsBuilder::default();

        let inst_id_code = self
            .get_inst_id_code(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                OKXWsError::ClientError(format!(
                    "No instIdCode cached for {instrument_id}, cannot cancel algo order"
                ))
            })?;
        builder.inst_id_code(inst_id_code);

        if let Some(algo_id) = algo_order_id {
            builder.algo_id(algo_id);
        }

        if let Some(cl_ord_id) = client_order_id {
            builder.algo_cl_ord_id(cl_ord_id.to_string());
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build cancel algo params error: {e}")))?;

        let request_id = self.generate_unique_request_id();
        let request = OKXWsRequest {
            id: Some(request_id.clone()),
            op: super::enums::OKXWsOperation::CancelAlgos,
            exp_time: None,
            args: vec![params],
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| OKXWsError::JsonError(format!("Failed to serialize cancel algo: {e}")))?;

        let cmd = HandlerCommand::Send {
            payload,
            rate_limit_keys: Some(OKX_RATE_LIMIT_KEY_CANCEL.to_vec()),
            request_id: Some(request_id),
            client_order_id,
            op: Some(super::enums::OKXWsOperation::CancelAlgos),
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
            enums::{
                OKXExecType, OKXOrderCategory, OKXOrderStatus, OKXPriceType, OKXQuickMarginType,
                OKXSelfTradePreventionMode, OKXSide,
            },
        },
        websocket::{
            handler::is_post_only_auto_cancel,
            messages::{OKXOrderMsg, OKXWebSocketError, OKXWsFrame},
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
    fn test_add_option_greeks_sub_defaults_to_both_conventions() {
        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("BTC-USD-250328-92000-C.OKX");

        client.add_option_greeks_sub(instrument_id);

        let subs = client.option_greeks_subs().load();
        let stored = subs.get(&instrument_id).expect("instrument not registered");
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&OKXGreeksType::Bs));
        assert!(stored.contains(&OKXGreeksType::Pa));
    }

    #[rstest]
    #[case::bs_only(vec![OKXGreeksType::Bs])]
    #[case::pa_only(vec![OKXGreeksType::Pa])]
    #[case::both(vec![OKXGreeksType::Bs, OKXGreeksType::Pa])]
    fn test_add_option_greeks_sub_with_conventions_stores_requested_set(
        #[case] conventions: Vec<OKXGreeksType>,
    ) {
        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("BTC-USD-250328-92000-C.OKX");
        let set: AHashSet<OKXGreeksType> = conventions.iter().copied().collect();

        client.add_option_greeks_sub_with_conventions(instrument_id, set.clone());

        let subs = client.option_greeks_subs().load();
        let stored = subs.get(&instrument_id).expect("instrument not registered");
        assert_eq!(stored, &set);
    }

    #[rstest]
    fn test_add_option_greeks_sub_with_empty_conventions_falls_back_to_both() {
        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("BTC-USD-250328-92000-C.OKX");

        client.add_option_greeks_sub_with_conventions(instrument_id, AHashSet::new());

        let subs = client.option_greeks_subs().load();
        let stored = subs.get(&instrument_id).expect("instrument not registered");
        assert_eq!(stored.len(), 2);
    }

    #[rstest]
    fn test_remove_option_greeks_sub_clears_entry() {
        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("BTC-USD-250328-92000-C.OKX");

        client.add_option_greeks_sub(instrument_id);
        client.remove_option_greeks_sub(&instrument_id);

        let subs = client.option_greeks_subs().load();
        assert!(!subs.contains_key(&instrument_id));
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
            None,
            TransportBackend::default(),
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
            None,
            TransportBackend::default(),
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

        let client_with_heartbeat = OKXWebSocketClient::new(
            None,
            None,
            None,
            None,
            None,
            Some(30),
            None,
            TransportBackend::default(),
            None,
        )
        .unwrap();

        assert!(client_with_heartbeat.heartbeat.is_some());
        assert_eq!(client_with_heartbeat.heartbeat.unwrap(), 30);
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

        let nautilus_msg = OKXWsMessage::Error(error);
        match nautilus_msg {
            OKXWsMessage::Error(e) => {
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
            None,
            TransportBackend::default(),
            None,
        )
        .unwrap();

        assert!(client_with_heartbeat.heartbeat.is_some());
        assert_eq!(client_with_heartbeat.heartbeat.unwrap(), 30);

        let account_id = AccountId::from("test-account-123");
        let client_with_account = OKXWebSocketClient::new(
            None,
            None,
            None,
            None,
            Some(account_id),
            None,
            None,
            TransportBackend::default(),
            None,
        )
        .unwrap();

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

            let nautilus_msg = OKXWsMessage::Error(error);
            match nautilus_msg {
                OKXWsMessage::Error(e) => {
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
        assert!(matches!(result, Some(OKXWsFrame::Reconnected)));
    }

    #[rstest]
    fn test_feed_handler_normal_message_processing() {
        let ping_msg = Message::Text(TEXT_PING.to_string().into());
        let result = OKXWsFeedHandler::parse_raw_message(ping_msg);
        assert!(matches!(result, Some(OKXWsFrame::Ping)));

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
        assert!(matches!(sub_result, Some(OKXWsFrame::Subscription { .. })));
    }

    #[rstest]
    fn test_feed_handler_close_message() {
        let result = OKXWsFeedHandler::parse_raw_message(Message::Close(None));
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconnection_message_constant() {
        assert_eq!(RECONNECTED, "__RECONNECTED__");
    }

    #[rstest]
    fn test_multiple_reconnection_signals() {
        for _ in 0..3 {
            let msg = Message::Text(RECONNECTED.to_string().into());
            let result = OKXWsFeedHandler::parse_raw_message(msg);
            assert!(matches!(result, Some(OKXWsFrame::Reconnected)));
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
            None,
            TransportBackend::default(),
            None,
        )
        .unwrap();

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
            ccy: Ustr::from("USDT"),
            cl_ord_id: "order-1".to_string(),
            algo_cl_ord_id: None,
            attach_algo_cl_ord_id: None,
            attach_algo_ords: Vec::new(),
            fee: None,
            fee_ccy: Ustr::from("USDT"),
            fill_px: "0".to_string(),
            fill_sz: "0".to_string(),
            fill_time: 0,
            inst_id: Ustr::from("ETH-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            lever: "1".to_string(),
            ord_id: Ustr::from("123456"),
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
            algo_id: None,
            fill_fee: None,
            fill_fee_ccy: None,
            fill_mark_px: None,
            fill_mark_vol: None,
            fill_px_vol: None,
            fill_px_usd: None,
            fill_fwd_px: None,
            fill_notional_usd: None,
            fill_pnl: None,
            is_tp_limit: None,
            linked_algo_ord: None,
            notional_usd: None,
            px_type: OKXPriceType::None,
            px_usd: None,
            px_vol: None,
            quick_mgn_type: OKXQuickMarginType::None,
            rebate: None,
            rebate_ccy: None,
            sl_ord_px: None,
            sl_trigger_px: None,
            sl_trigger_px_type: None,
            source: None,
            stp_id: None,
            stp_mode: OKXSelfTradePreventionMode::None,
            tag: None,
            tp_ord_px: None,
            tp_trigger_px: None,
            tp_trigger_px_type: None,
            amend_result: None,
            req_id: None,
            code: None,
            msg: None,
            u_time: 0,
        }
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_detects_cancel_source() {
        let mut msg = sample_canceled_order_msg();
        msg.cancel_source = Some(OKX_POST_ONLY_CANCEL_SOURCE.to_string());

        assert!(is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_detects_reason() {
        let mut msg = sample_canceled_order_msg();
        msg.cancel_source_reason = Some("POST_ONLY would take liquidity".to_string());

        assert!(is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_false_without_markers() {
        let msg = sample_canceled_order_msg();

        assert!(!is_post_only_auto_cancel(&msg));
    }

    #[rstest]
    fn test_is_post_only_auto_cancel_false_for_order_type_only() {
        let mut msg = sample_canceled_order_msg();
        msg.ord_type = OKXOrderType::PostOnly;

        assert!(!is_post_only_auto_cancel(&msg));
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
            None,
            TransportBackend::default(),
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

        let result = client.batch_cancel_orders(orders).await;
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
            None,
            TransportBackend::default(),
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let client_order_id = ClientOrderId::new("order1");

        let orders = vec![(instrument_id, Some(client_order_id), None)];

        let result = client.batch_cancel_orders(orders).await;

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
            None,
            TransportBackend::default(),
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let venue_order_id = VenueOrderId::new("venue1");

        let orders = vec![(instrument_id, None, Some(venue_order_id))];

        let result = client.batch_cancel_orders(orders).await;

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
            None,
            TransportBackend::default(),
            None,
        )
        .expect("Failed to create client");

        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let client_order_id = ClientOrderId::new("order1");
        let venue_order_id = VenueOrderId::new("venue1");

        let orders = vec![(instrument_id, Some(client_order_id), Some(venue_order_id))];

        let result = client.batch_cancel_orders(orders).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cancel_order_fails_without_inst_id_code() {
        use nautilus_model::identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId};

        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");

        let result = client
            .cancel_order(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                instrument_id,
                Some(ClientOrderId::new("O-001")),
                None,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No instIdCode cached for BTC-USDT-SWAP.OKX"),
            "Expected instIdCode error, found: {err}"
        );
    }

    #[tokio::test]
    async fn test_submit_order_fails_without_inst_id_code() {
        use nautilus_model::{
            enums::{OrderSide, OrderType},
            identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
            types::Quantity,
        };

        use crate::common::enums::OKXTradeMode;

        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");

        let result = client
            .submit_order(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                instrument_id,
                OKXTradeMode::Cross,
                ClientOrderId::new("O-001"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.01"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No instIdCode cached for ETH-USDT-SWAP.OKX"),
            "Expected instIdCode error, found: {err}"
        );
    }

    #[tokio::test]
    async fn test_cancel_order_passes_inst_id_code_lookup_when_cached() {
        use nautilus_model::identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId};
        use ustr::Ustr;

        let client = OKXWebSocketClient::default();
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");

        // Populate the cache so the lookup succeeds
        client.cache_inst_id_code(Ustr::from("BTC-USDT-SWAP"), 10459);

        let result = client
            .cancel_order(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                instrument_id,
                Some(ClientOrderId::new("O-001")),
                None,
            )
            .await;

        // Fails later (not connected) rather than at instIdCode lookup
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            !err.contains("No instIdCode cached"),
            "Should pass instIdCode lookup, found: {err}"
        );
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
            None,
            TransportBackend::default(),
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
            None,
            TransportBackend::default(),
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
            None,
            TransportBackend::default(),
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
            None,
            TransportBackend::default(),
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
            None,
            TransportBackend::default(),
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
