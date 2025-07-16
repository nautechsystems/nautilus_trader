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
//! This module defines and implements a strongly-typed [`OKXWebSocketClient`] for
//! connecting to OKX WebSocket streams. It handles authentication (when credentials
//! are provided), manages subscriptions to market data and account update channels,
//! and parses incoming messages into structured Nautilus domain objects.

use std::{
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use futures_util::{Stream, StreamExt};
use nautilus_common::runtime::get_runtime;
use nautilus_core::{
    UUID4, consts::NAUTILUS_USER_AGENT, env::get_env_var, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderStatus, OrderType, PositionSide},
    events::{AccountState, OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Money, Price, Quantity},
};
use nautilus_network::{
    ratelimiter::quota::Quota,
    websocket::{Consumer, MessageReader, WebSocketClient, WebSocketConfig},
};
use reqwest::header::USER_AGENT;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{Error, Message};
use ustr::Ustr;

use super::{
    enums::{OKXWsChannel, OKXWsOperation},
    error::OKXWsError,
    messages::{
        ExecutionReport, NautilusWsMessage, OKXSubscription, OKXSubscriptionArg, OKXWebSocketError,
        OKXWebSocketEvent, OKXWsRequest, WsAmendOrderParams, WsAmendOrderParamsBuilder,
        WsCancelOrderParams, WsCancelOrderParamsBuilder, WsPostOrderParams,
        WsPostOrderParamsBuilder,
    },
    parse::{parse_book_msg_vec, parse_ws_message_data},
};
use crate::{
    common::{
        consts::OKX_WS_PUBLIC_URL,
        credential::Credential,
        enums::{OKXInstrumentType, OKXOrderType, OKXPositionSide, OKXSide, OKXTradeMode},
        parse::{bar_spec_as_okx_channel, okx_instrument_type, parse_account_state},
    },
    http::models::OKXAccount,
    websocket::{messages::OKXOrderMsg, parse::parse_order_msg_vec},
};

type PlaceRequestData = (ClientOrderId, TraderId, StrategyId, InstrumentId);
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

/// Default OKX WebSocket rate limit: 3 requests per second.
///
/// - Connection limit: 3 requests per second (per IP).
/// - Subscription requests: 480 'subscribe/unsubscribe/login' requests per connection per hour.
/// - 30 WebSocket connections max per specific channel per sub-account.
///
/// We use 3 requests per second as the base limit to respect the connection rate limit.
pub static OKX_WS_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(3).unwrap()));

/// Rate limit for order-related WebSocket operations: 250 requests per second.
///
/// Based on OKX documentation for sub-account order limits (1000 per 2 seconds,
/// so we use half for conservative rate limiting).
pub static OKX_WS_ORDER_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(250).unwrap()));

/// Provides a WebSocket client for connecting to [OKX](https://okx.com).
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct OKXWebSocketClient {
    url: String,
    account_id: AccountId,
    credential: Option<Credential>,
    heartbeat: Option<u64>,
    inner: Option<Arc<WebSocketClient>>,
    rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions_inst_type: Arc<Mutex<AHashMap<OKXWsChannel, AHashSet<OKXInstrumentType>>>>,
    subscriptions_inst_family: Arc<Mutex<AHashMap<OKXWsChannel, AHashSet<Ustr>>>>,
    subscriptions_inst_id: Arc<Mutex<AHashMap<OKXWsChannel, AHashSet<Ustr>>>>,
    request_id_counter: Arc<AtomicU64>,
    pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
    pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
    pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
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
        let subscriptions_inst_type = Arc::new(Mutex::new(AHashMap::new()));
        let subscriptions_inst_family = Arc::new(Mutex::new(AHashMap::new()));
        let subscriptions_inst_id = Arc::new(Mutex::new(AHashMap::new()));

        Ok(Self {
            url,
            account_id,
            credential,
            heartbeat,
            inner: None,
            rx: None,
            signal,
            task_handle: None,
            subscriptions_inst_type,
            subscriptions_inst_family,
            subscriptions_inst_id,
            request_id_counter: Arc::new(AtomicU64::new(1)),
            pending_place_requests: Arc::new(DashMap::new()),
            pending_cancel_requests: Arc::new(DashMap::new()),
            pending_amend_requests: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(AHashMap::new()),
        })
    }

    /// Creates a new [`OKXWebSocketClient`] instance.
    pub fn with_credentials(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let url = url.unwrap_or(OKX_WS_PUBLIC_URL.to_string());
        let api_key = api_key.unwrap_or(get_env_var("OKX_API_KEY")?);
        let api_secret = api_secret.unwrap_or(get_env_var("OKX_API_SECRET")?);
        let api_passphrase = api_passphrase.unwrap_or(get_env_var("OKX_API_PASSPHRASE")?);

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

    /// Returns the websocket url being used by the client.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the public API key being used by the client.
    pub fn api_key(&self) -> Option<&str> {
        self.credential.clone().map(|c| c.api_key.as_str())
    }

    /// Returns a value indicating whether the client is active.
    pub fn is_active(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_active(),
            None => false,
        }
    }

    /// Returns a value indicating whether the client is closed.
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_closed(),
            None => true,
        }
    }

    pub async fn connect(&mut self, instruments: Vec<InstrumentAny>) -> anyhow::Result<()> {
        let client = self.clone();
        let post_reconnect = Arc::new(move || {
            let client = client.clone();
            tokio::spawn(async move { client.resubscribe_all().await });
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            #[cfg(feature = "python")]
            handler: Consumer::Python(None),
            #[cfg(not(feature = "python"))]
            handler: {
                let (consumer, _rx) = Consumer::rust_consumer();
                consumer
            },
            #[cfg(feature = "python")]
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };
        // Configure rate limits for different operation types
        let keyed_quotas = vec![
            ("subscription".to_string(), *OKX_WS_QUOTA),
            ("order".to_string(), *OKX_WS_ORDER_QUOTA),
            ("cancel".to_string(), *OKX_WS_ORDER_QUOTA),
            ("amend".to_string(), *OKX_WS_ORDER_QUOTA),
        ];

        let (reader, client) = WebSocketClient::connect_stream(
            config,
            keyed_quotas,
            Some(*OKX_WS_QUOTA), // Default quota for general operations
            Some(post_reconnect),
        )
        .await?;

        self.inner = Some(Arc::new(client));

        if self.credential.is_some() {
            self.authenticate().await?;
        }

        let account_id = self.account_id;
        let mut instruments_map: AHashMap<Ustr, InstrumentAny> = AHashMap::new();
        for inst in instruments {
            instruments_map.insert(inst.symbol().inner(), inst.clone());
        }

        self.instruments_cache = Arc::new(instruments_map.clone());

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        self.rx = Some(Arc::new(rx));
        let signal = self.signal.clone();
        let pending_place_requests = self.pending_place_requests.clone();
        let pending_cancel_requests = self.pending_cancel_requests.clone();
        let pending_amend_requests = self.pending_amend_requests.clone();

        let stream_handle = get_runtime().spawn(async move {
            OKXWsMessageHandler::new(
                account_id,
                instruments_map,
                reader,
                signal,
                tx,
                pending_place_requests,
                pending_cancel_requests,
                pending_amend_requests,
            )
            .run()
            .await;
        });

        self.task_handle = Some(Arc::new(stream_handle));

        Ok(())
    }

    /// Authenticates the WebSocket session with OKX.
    async fn authenticate(&mut self) -> Result<(), Error> {
        let credential = match &self.credential {
            Some(credential) => credential,
            None => {
                panic!("API credentials not available to authenticate");
            }
        };

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time should be after UNIX epoch")
            .as_secs()
            .to_string();
        let signature = credential.sign(&timestamp, "GET", "/users/self/verify", "");

        let auth_message = serde_json::json!({
            "op": "login",
            "args": [{
                "apiKey": credential.api_key,
                "passphrase": credential.api_passphrase,
                "timestamp": timestamp,
                "sign": signature,
            }]
        });

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(auth_message.to_string(), None).await {
                tracing::error!("Error sending message: {e:?}");
            }
        } else {
            log::error!("Cannot authenticate: not connected");
        }

        Ok(())
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

    /// Closes the client.
    pub async fn close(&mut self) -> Result<(), Error> {
        log::debug!("Starting close process");

        self.signal.store(true, Ordering::Relaxed);

        if let Some(inner) = &self.inner {
            log::debug!("Disconnecting websocket");

            match tokio::time::timeout(Duration::from_secs(3), inner.disconnect()).await {
                Ok(()) => log::debug!("Websocket disconnected successfully"),
                Err(_) => {
                    log::warn!("Timeout waiting for websocket disconnect, continuing with cleanup")
                }
            }
        } else {
            log::debug!("No active connection to disconnect");
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
                    log::warn!(
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

    fn generate_unique_request_id(&self) -> String {
        self.request_id_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string()
    }

    async fn subscribe(&self, args: Vec<OKXSubscriptionArg>) -> Result<(), OKXWsError> {
        for arg in &args {
            // Update instrument type subscriptions
            if let Some(inst_type) = &arg.inst_type {
                let mut active_subs = self.subscriptions_inst_type.lock().await;
                active_subs
                    .entry(arg.channel.clone())
                    .or_insert_with(AHashSet::new)
                    .insert(*inst_type);
            }

            // Update instrument family subscriptions
            if let Some(inst_family) = &arg.inst_family {
                let mut active_subs = self.subscriptions_inst_family.lock().await;
                active_subs
                    .entry(arg.channel.clone())
                    .or_insert_with(AHashSet::new)
                    .insert(*inst_family);
            }

            // Update instrument ID subscriptions
            if let Some(inst_id) = &arg.inst_id {
                let mut active_subs = self.subscriptions_inst_family.lock().await;
                active_subs
                    .entry(arg.channel.clone())
                    .or_insert_with(AHashSet::new)
                    .insert(*inst_id);
            }
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Subscribe,
            args,
        };

        let json_txt =
            serde_json::to_string(&message).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner
                .send_text(json_txt, Some(vec!["subscription".to_string()]))
                .await
            {
                tracing::error!("Error sending message: {e:?}")
            }
        } else {
            return Err(OKXWsError::ClientError(
                "Cannot send message: not connected".to_string(),
            ));
        }

        Ok(())
    }

    async fn unsubscribe(&self, args: Vec<OKXSubscriptionArg>) -> Result<(), OKXWsError> {
        for arg in &args {
            // Update instrument type subscriptions
            if let Some(inst_type) = &arg.inst_type {
                let mut active_subs = self.subscriptions_inst_type.lock().await;
                active_subs
                    .entry(arg.channel.clone())
                    .or_insert_with(AHashSet::new)
                    .remove(inst_type);
            }

            // Update instrument family subscriptions
            if let Some(inst_family) = &arg.inst_family {
                let mut active_subs = self.subscriptions_inst_family.lock().await;
                active_subs
                    .entry(arg.channel.clone())
                    .or_insert_with(AHashSet::new)
                    .remove(inst_family);
            }

            // Update instrument ID subscriptions
            if let Some(inst_id) = &arg.inst_id {
                let mut active_subs = self.subscriptions_inst_family.lock().await;
                active_subs
                    .entry(arg.channel.clone())
                    .or_insert_with(AHashSet::new)
                    .remove(inst_id);
            }
        }

        let message = OKXSubscription {
            op: OKXWsOperation::Unsubscribe,
            args,
        };

        let json_txt = serde_json::to_string(&message).expect("Must be valid JSON");

        if let Some(inner) = &self.inner {
            if let Err(e) = inner
                .send_text(json_txt, Some(vec!["subscription".to_string()]))
                .await
            {
                tracing::error!("Error sending message: {e:?}")
            }
        } else {
            log::error!("Cannot send message: not connected");
        }

        Ok(())
    }

    async fn resubscribe_all(&self) {
        let subs_inst_type = self.subscriptions_inst_type.lock().await.clone();
        let subs_inst_family = self.subscriptions_inst_family.lock().await.clone();
        let subs_inst_id = self.subscriptions_inst_id.lock().await.clone();

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
    }

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

    pub async fn subscribe_order_book(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        // let (symbol, inst_type) = extract_okx_symbol_and_inst_type(&instrument_id);
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribe to trade data for an instrument.
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-order-book-channel>.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        // let (_, inst_type) = extract_okx_symbol_and_inst_type(&instrument_id);
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::BboTbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    /// Subscribe to trade data for an instrument.
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-ws-trades-channel>.
    pub async fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
        _aggregated: bool, // TODO: TBD?
    ) -> Result<(), OKXWsError> {
        // TODO: aggregated parameter is ignored, always uses 'trades' channel.
        // let (symbol, _) = extract_okx_symbol_and_inst_type(&instrument_id);

        // Use trades channel for all instruments (trades-all not available?)
        let channel = OKXWsChannel::Trades;

        let arg = OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

    pub async fn subscribe_ticker(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Tickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.subscribe(vec![arg]).await
    }

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

    pub async fn unsubscribe_order_book(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Books,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    pub async fn unsubscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::BboTbt,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

    pub async fn unsubscribe_ticker(&self, instrument_id: InstrumentId) -> Result<(), OKXWsError> {
        let arg = OKXSubscriptionArg {
            channel: OKXWsChannel::Tickers,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

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

    /// Unsubscribe from trade data for an instrument.
    ///
    /// # Parameters
    /// - `instrument_id`: The instrument to unsubscribe from
    /// - `aggregated`: Parameter is ignored, always uses 'trades' channel
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
        _aggregated: bool,
    ) -> Result<(), OKXWsError> {
        // Use trades channel for all instruments (trades-all not available?)
        let channel = OKXWsChannel::Trades;

        let arg = OKXSubscriptionArg {
            channel,
            inst_type: None,
            inst_family: None,
            inst_id: Some(instrument_id.symbol.inner()),
        };
        self.unsubscribe(vec![arg]).await
    }

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

    /// Subscribes to fill updates for the given instrument type.
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
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["cancel".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
        }
    }

    #[allow(dead_code)] // TODO: Implement for MM pending orders
    /// Cancel multiple orders at once via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-websocket-mass-cancel-order>
    async fn ws_mass_cancel(&self, args: Vec<Value>) -> Result<(), OKXWsError> {
        // Generate unique request ID for WebSocket message
        let request_id = self
            .request_id_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string();

        let req = OKXWsRequest {
            id: Some(request_id),
            op: OKXWsOperation::MassCancel,
            args,
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["cancel".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
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
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["amend".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
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
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["order".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
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
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["cancel".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
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
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["amend".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
        }
    }

    /// Submits a new order using Nautilus domain types via WebSocket.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-place-order>.
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
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        position_side: Option<PositionSide>,
    ) -> Result<(), OKXWsError> {
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
                // Defaults
            }
            OKXInstrumentType::Margin => {
                // MARGIN: use quote currency for margin
                builder.ccy(quote_currency.to_string());

                // TODO: Consider position mode (only applicable for NET)
                if let Some(ro) = reduce_only {
                    if ro {
                        builder.reduce_only(ro);
                    }
                }
            }
            OKXInstrumentType::Swap | OKXInstrumentType::Futures => {
                // SWAP/FUTURES: use quote currency for margin (required by OKX)
                builder.ccy(quote_currency.to_string());
            }
            _ => {
                // For other instrument types (OPTIONS, etc.), use quote currency as fallback
                builder.ccy(quote_currency.to_string());
                builder.tgt_ccy(quote_currency.to_string());

                // TODO: Consider position mode (only applicable for NET)
                if let Some(ro) = reduce_only {
                    if ro {
                        builder.reduce_only(ro);
                    }
                }
            }
        };

        builder.side(OKXSide::from(order_side));

        if let Some(pos_side) = position_side {
            builder.pos_side(pos_side);
        };

        let okx_ord_type = if post_only.unwrap_or(false) {
            OKXOrderType::PostOnly
        } else {
            OKXOrderType::from(order_type)
        };

        builder.ord_type(okx_ord_type);
        builder.sz(quantity.to_string());

        if let Some(tp) = trigger_price {
            builder.px(tp.to_string());
        } else if let Some(p) = price {
            builder.px(p.to_string());
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build order params error: {e}")))?;

        let request_id = self.generate_unique_request_id();

        self.pending_place_requests.insert(
            request_id.clone(),
            (client_order_id, trader_id, strategy_id, instrument_id),
        );

        self.ws_place_order(params, Some(request_id)).await
    }

    /// Cancels an existing order via WebSocket using Nautilus domain types.
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
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        position_side: Option<PositionSide>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsCancelOrderParamsBuilder::default();
        // Note: instType should NOT be included in cancel order requests
        // For WebSocket orders, use the full symbol (including SWAP/FUTURES suffix if present)
        builder.inst_id(instrument_id.symbol.as_str());
        builder.cl_ord_id(client_order_id.as_str());
        if let Some(ps) = position_side {
            builder.pos_side(OKXPositionSide::from(ps));
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build cancel params error: {e}")))?;

        let request_id = self.generate_unique_request_id();

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

        self.ws_cancel_order(params, Some(request_id)).await
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
            args: vec![params],
        };

        let txt = serde_json::to_string(&req).map_err(|e| OKXWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            if let Err(e) = inner.send_text(txt, Some(vec!["order".to_string()])).await {
                tracing::error!("Error sending message: {e:?}");
            }
            Ok(())
        } else {
            Err(OKXWsError::ClientError("Not connected".to_string()))
        }
    }

    /// Modifies an existing order via WebSocket using Nautilus domain types.
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
        client_order_id: ClientOrderId,
        new_client_order_id: ClientOrderId,
        price: Option<Price>,
        quantity: Option<Quantity>,
        venue_order_id: Option<VenueOrderId>,
        position_side: Option<PositionSide>,
    ) -> Result<(), OKXWsError> {
        let mut builder = WsAmendOrderParamsBuilder::default();

        builder.inst_id(instrument_id.symbol.as_str());
        builder.cl_ord_id(client_order_id.as_str());
        builder.new_cl_ord_id(new_client_order_id.as_str());
        if let Some(p) = price {
            builder.px(p.to_string());
        }
        if let Some(q) = quantity {
            builder.sz(q.to_string());
        }
        if let Some(ps) = position_side {
            builder.pos_side(OKXPositionSide::from(ps));
        }

        let params = builder
            .build()
            .map_err(|e| OKXWsError::ClientError(format!("Build amend params error: {e}")))?;

        // Generate unique request ID for WebSocket message
        let request_id = self
            .request_id_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string();

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

        self.ws_amend_order(params, Some(request_id)).await
    }

    /// Submits multiple orders via WebSocket using Nautilus domain types.
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
            builder.side(OKXSide::from(ord_side));
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

            let params = builder
                .build()
                .map_err(|e| OKXWsError::ClientError(format!("Build order params error: {e}")))?;
            let val =
                serde_json::to_value(params).map_err(|e| OKXWsError::JsonError(e.to_string()))?;
            args.push(val);
        }

        self.ws_batch_place_orders(args).await
    }

    /// Cancels multiple orders via WebSocket using Nautilus domain types.
    #[allow(clippy::type_complexity)]
    pub async fn batch_cancel_orders(
        &self,
        orders: Vec<(
            OKXInstrumentType,
            InstrumentId,
            Option<ClientOrderId>,
            Option<String>,
            Option<PositionSide>,
        )>,
    ) -> Result<(), OKXWsError> {
        let mut args: Vec<Value> = Vec::with_capacity(orders.len());
        for (_inst_type, inst_id, cl_ord_id, ord_id, pos_side) in orders {
            let mut builder = WsCancelOrderParamsBuilder::default();
            // Note: instType should NOT be included in cancel order requests
            builder.inst_id(inst_id.symbol.inner());
            if let Some(c) = cl_ord_id {
                builder.cl_ord_id(c.as_str());
            }
            if let Some(o) = ord_id {
                builder.ord_id(o);
            }
            if let Some(ps) = pos_side {
                builder.pos_side(OKXPositionSide::from(ps));
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

    /// Modifies multiple orders via WebSocket using Nautilus domain types.
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
            Option<PositionSide>,
        )>,
    ) -> Result<(), OKXWsError> {
        let mut args: Vec<Value> = Vec::with_capacity(orders.len());
        for (_inst_type, inst_id, cl_ord_id, new_cl_ord_id, pr, sz, ps) in orders {
            let mut builder = WsAmendOrderParamsBuilder::default();
            // Note: instType should NOT be included in amend order requests
            builder.inst_id(inst_id.symbol.inner());
            builder.cl_ord_id(cl_ord_id.as_str());
            builder.new_cl_ord_id(new_cl_ord_id.as_str());
            if let Some(p) = pr {
                builder.px(p.to_string());
            }
            if let Some(q) = sz {
                builder.sz(q.to_string());
            }
            if let Some(side) = ps {
                let okx_ps = match side {
                    PositionSide::Long => OKXPositionSide::Long,
                    PositionSide::Short => OKXPositionSide::Short,
                    _ => OKXPositionSide::None,
                };
                builder.pos_side(okx_ps);
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
}

struct OKXFeedHandler {
    reader: MessageReader,
    signal: Arc<AtomicBool>,
}

impl OKXFeedHandler {
    /// Creates a new [`OKXFeedHandler`] instance.
    pub const fn new(reader: MessageReader, signal: Arc<AtomicBool>) -> Self {
        Self { reader, signal }
    }

    /// Get the next message from the WebSocket stream.
    async fn next(&mut self) -> Option<OKXWebSocketEvent> {
        // Timeout awaiting the next message before checking signal
        let timeout = Duration::from_millis(10);

        loop {
            if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::debug!("Stop signal received");
                break;
            }

            match tokio::time::timeout(timeout, self.reader.next()).await {
                Ok(Some(msg)) => match msg {
                    Ok(Message::Text(text)) => {
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
                                        continue;
                                    } else {
                                        tracing::error!(
                                            "Authentication failed: {event} {code} - {msg}"
                                        );
                                        return Some(ws_event);
                                    }
                                }
                                OKXWebSocketEvent::Subscription {
                                    event,
                                    arg,
                                    conn_id,
                                } => {
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
                                OKXWebSocketEvent::Data { .. } => return Some(ws_event),
                                OKXWebSocketEvent::BookData { .. } => return Some(ws_event),
                                OKXWebSocketEvent::OrderResponse {
                                    id,
                                    op,
                                    code,
                                    msg,
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
                                    } else {
                                        // Extract error message
                                        let error_msg = data
                                            .first()
                                            .and_then(|d| d.get("sMsg"))
                                            .and_then(|s| s.as_str())
                                            .unwrap_or(msg.as_str());
                                        tracing::error!(
                                            "Order operation failed: id={id:?}, op={op}, code={code}, error={error_msg}",
                                        );
                                    }
                                    return Some(ws_event);
                                }
                            },
                            Err(e) => {
                                tracing::error!("Failed to parse message: {e}: {text}");
                                break;
                            }
                        }
                    }
                    Ok(Message::Binary(msg)) => {
                        tracing::debug!("Raw binary: {msg:?}");
                    }
                    Ok(Message::Close(_)) => {
                        tracing::debug!("Received close message");
                        return None;
                    }
                    Ok(msg) => {
                        tracing::warn!("Unexpected message: {msg}");
                    }
                    Err(e) => {
                        tracing::error!("{e}");
                        break; // Break as indicates a bug in the code
                    }
                },
                Ok(None) => {
                    tracing::info!("WebSocket stream closed");
                    break;
                }
                Err(_) => {} // Timeout occurred awaiting a message, continue loop to check signal
            }
        }

        tracing::debug!("Stopped message streaming");
        None
    }
}

struct OKXWsMessageHandler {
    account_id: AccountId,
    handler: OKXFeedHandler,
    tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
    pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
    pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
    instruments: AHashMap<Ustr, InstrumentAny>,
    last_account_state: Option<AccountState>,
    fee_cache: AHashMap<Ustr, Money>, // Key is order ID
}

impl OKXWsMessageHandler {
    /// Creates a new [`OKXFeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: AccountId,
        instruments: AHashMap<Ustr, InstrumentAny>,
        reader: MessageReader,
        signal: Arc<AtomicBool>,
        tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
        pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
        pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
    ) -> Self {
        Self {
            account_id,
            handler: OKXFeedHandler::new(reader, signal),
            tx,
            pending_place_requests,
            pending_cancel_requests,
            pending_amend_requests,
            instruments,
            last_account_state: None,
            fee_cache: AHashMap::new(),
        }
    }

    async fn run(&mut self) {
        while let Some(data) = self.next().await {
            if let Err(e) = self.tx.send(data) {
                tracing::error!("Error sending data: {e}");
                break; // Stop processing on channel error for now
            }
        }
    }

    async fn next(&mut self) -> Option<NautilusWsMessage> {
        let clock = get_atomic_clock_realtime();

        while let Some(event) = self.handler.next().await {
            let ts_init = clock.get_time_ns();

            if let OKXWebSocketEvent::BookData { arg, action, data } = event {
                let inst = match arg.inst_id {
                    Some(inst_id) => self.instruments.get(&inst_id),
                    None => {
                        tracing::error!("Instrument ID missing for book data event");
                        continue;
                    }
                };

                let instrument_id = inst?.id();
                let price_precision = inst?.price_precision();
                let size_precision = inst?.size_precision();

                match parse_book_msg_vec(
                    data,
                    &instrument_id,
                    price_precision,
                    size_precision,
                    action,
                    ts_init,
                ) {
                    Ok(data) => return Some(NautilusWsMessage::Data(data)),
                    Err(e) => {
                        tracing::error!("Failed to parse book message: {e}");
                        continue;
                    }
                }
            }

            if let OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            } = event
            {
                if code == "0" {
                    tracing::info!(
                        "Order operation successful: id={:?} op={op} code={code}",
                        id
                    );

                    if let Some(data) = data.first() {
                        let success_msg = data
                            .get("sMsg")
                            .and_then(|s| s.as_str())
                            .unwrap_or("Order operation successful");
                        tracing::debug!("Order success details: {success_msg}");

                        // Note: We rely on the orders channel subscription to provide the proper
                        // OrderStatusReport with correct instrument ID and full order details.
                        // The placement response has limited information.
                    }
                } else {
                    // Extract actual error message from data array, same as in the handler
                    let error_msg = data
                        .first()
                        .and_then(|d| d.get("sMsg"))
                        .and_then(|s| s.as_str())
                        .unwrap_or(&msg);

                    // Debug: Check what fields are available in error data
                    if let Some(data_obj) = data.first() {
                        tracing::debug!(
                            "Error data fields: {}",
                            serde_json::to_string_pretty(data_obj)
                                .unwrap_or_else(|_| "unable to serialize".to_string())
                        );
                    }

                    tracing::error!(
                        "Order operation failed: id={:?} op={op} code={code} msg={msg}",
                        id
                    );

                    // Fetch pending request mapping for rejection based on operation type
                    if let Some(id) = &id {
                        match op {
                            OKXWsOperation::Order => {
                                if let Some((
                                    _,
                                    (client_order_id, trader_id, strategy_id, instrument_id),
                                )) = self.pending_place_requests.remove(id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        self.account_id,
                                        Ustr::from(error_msg), // Rejection reason from OKX
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
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
                                )) = self.pending_cancel_requests.remove(id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderCancelRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        Ustr::from(error_msg), // Rejection reason from OKX
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
                                )) = self.pending_amend_requests.remove(id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderModifyRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        Ustr::from(error_msg), // Rejection reason from OKX
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
                            _ => {
                                tracing::warn!("Unhandled operation type for rejection: {op}");
                            }
                        }
                    }

                    // Fallback to error if no mapping found
                    let error = OKXWebSocketError {
                        code: code.clone(),
                        message: error_msg.to_string(),
                        conn_id: None, // Order responses don't have connection IDs
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                continue;
            }

            if let OKXWebSocketEvent::Data { ref arg, ref data } = event {
                if arg.channel == OKXWsChannel::Account {
                    match serde_json::from_value::<Vec<OKXAccount>>(data.clone()) {
                        Ok(accounts) => {
                            if let Some(account) = accounts.first() {
                                // TODO: Parse account ID from somewhere (could be from credentials or config)
                                match parse_account_state(account, self.account_id, ts_init) {
                                    Ok(account_state) => {
                                        // TODO: Optimize this account state comparison
                                        if let Some(last_account_state) = &self.last_account_state {
                                            if account_state
                                                .has_same_balances_and_margins(last_account_state)
                                            {
                                                continue; // Nothing to update
                                            }
                                        }
                                        self.last_account_state = Some(account_state.clone());
                                        return Some(NautilusWsMessage::AccountUpdate(
                                            account_state,
                                        ));
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to parse account state: {e}");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse account data: {e}, raw data: {}",
                                data
                            );
                        }
                    }
                    continue;
                }

                if arg.channel == OKXWsChannel::Orders {
                    tracing::debug!("Received orders channel message: {data}");

                    let data: Vec<OKXOrderMsg> = serde_json::from_value(data.clone()).unwrap();

                    let mut exec_reports = Vec::with_capacity(data.len());

                    for msg in data {
                        match parse_order_msg_vec(
                            vec![msg],
                            self.account_id,
                            &self.instruments,
                            &self.fee_cache,
                            ts_init,
                        ) {
                            Ok(mut reports) => {
                                // Update fee cache based on the new reports
                                for report in &reports {
                                    match report {
                                        ExecutionReport::Fill(fill_report) => {
                                            let order_id = fill_report.venue_order_id.inner();
                                            let current_fee = self
                                                .fee_cache
                                                .get(&order_id)
                                                .copied()
                                                .unwrap_or_else(|| {
                                                    Money::new(0.0, fill_report.commission.currency)
                                                });
                                            let total_fee = current_fee + fill_report.commission;
                                            self.fee_cache.insert(order_id, total_fee);
                                        }
                                        ExecutionReport::Order(status_report) => {
                                            if matches!(
                                                status_report.order_status,
                                                OrderStatus::Filled,
                                            ) {
                                                self.fee_cache
                                                    .remove(&status_report.venue_order_id.inner());
                                            }
                                        }
                                    }
                                }
                                exec_reports.append(&mut reports);
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse order message: {e}");
                                continue;
                            }
                        }
                    }

                    if !exec_reports.is_empty() {
                        return Some(NautilusWsMessage::ExecutionReports(exec_reports));
                    }
                }

                let inst = match arg.inst_id.and_then(|id| self.instruments.get(&id)) {
                    Some(inst) => inst,
                    None => {
                        tracing::error!(
                            "No instrument for channel {:?}, inst_id {:?}",
                            arg.channel,
                            arg.inst_id
                        );
                        continue;
                    }
                };
                let instrument_id = inst.id();
                let price_precision = inst.price_precision();
                let size_precision = inst.size_precision();

                match parse_ws_message_data(
                    &arg.channel,
                    data.clone(),
                    &instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                ) {
                    Ok(Some(msg)) => return Some(msg),
                    Ok(None) => {
                        // No message to return (e.g., empty instrument payload)
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("Error parsing message for channel {:?}: {e}", arg.channel)
                    }
                }
            }

            // Handle login events (authentication failures)
            if let OKXWebSocketEvent::Login {
                code, msg, conn_id, ..
            } = &event
            {
                if code != "0" {
                    let error = OKXWebSocketError {
                        code: code.clone(),
                        message: msg.clone(),
                        conn_id: Some(conn_id.clone()),
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
            }

            // Handle general error events
            if let OKXWebSocketEvent::Error { code, msg } = &event {
                let error = OKXWebSocketError {
                    code: code.clone(),
                    message: msg.clone(),
                    conn_id: None,
                    timestamp: clock.get_time_ns().as_u64(),
                };
                return Some(NautilusWsMessage::Error(error));
            }
        }
        None // Connection closed
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use futures_util;
    use rstest::rstest;

    use super::*;

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

        client.pending_place_requests.insert(
            "place-123".to_string(),
            (client_order_id, trader_id, strategy_id, instrument_id),
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
            NautilusWsMessage::Error(err) => {
                assert_eq!(err.code, "60012");
                assert_eq!(err.message, "Invalid request");
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

                client_clone.pending_place_requests.insert(
                    request_id_str.clone(),
                    (client_order_id, trader_id, strategy_id, instrument_id),
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
                NautilusWsMessage::Error(err) => {
                    assert_eq!(err.code, code);
                    assert_eq!(err.message, message);
                    assert_eq!(err.conn_id, conn_id);
                }
                _ => panic!("Expected Error variant"),
            }
        }
    }
}
