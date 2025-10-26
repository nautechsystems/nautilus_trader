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

use std::{collections::HashSet, sync::Arc, time::Duration};

use dashmap::DashMap;
use futures_util::{Stream, future::BoxFuture};
#[cfg(feature = "python")]
use nautilus_core::python::to_pyruntime_err;
use nautilus_core::time::get_atomic_clock_realtime;
#[cfg(feature = "python")]
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
#[cfg(feature = "python")]
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

#[cfg(feature = "python")]
use crate::websocket::parse::{
    parse_ws_candle, parse_ws_order_book_deltas, parse_ws_quote_tick, parse_ws_trade_tick,
};
use crate::{
    http::error::{Error, Result as HyperliquidResult},
    websocket::{
        messages::{
            ActionPayload, ExecutionReport, HyperliquidWsMessage, HyperliquidWsRequest,
            NautilusWsMessage, PostRequest, PostResponsePayload, SubscriptionRequest,
        },
        parse::{parse_ws_fill_report, parse_ws_order_status_report},
        post::{
            PostBatcher, PostIds, PostLane, PostRouter, ScheduledPost, WsSender, lane_for_action,
        },
    },
};

/// Errors that can occur during Hyperliquid WebSocket operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HyperliquidError {
    #[error("URL parsing failed: {0}")]
    UrlParsing(String),

    #[error("Message serialization failed: {0}")]
    MessageSerialization(String),

    #[error("Message deserialization failed: {0}")]
    MessageDeserialization(String),

    #[error("WebSocket connection failed: {0}")]
    Connection(String),

    #[error("Channel send failed: {0}")]
    ChannelSend(String),
}

/// Codec for encoding and decoding Hyperliquid WebSocket messages.
///
/// This struct provides methods to validate URLs and serialize/deserialize messages
/// according to the Hyperliquid WebSocket protocol.
#[derive(Debug, Default)]
pub struct HyperliquidCodec;

impl HyperliquidCodec {
    /// Creates a new Hyperliquid codec instance.
    pub fn new() -> Self {
        Self
    }

    /// Validates that a URL is a proper WebSocket URL.
    pub fn validate_url(url: &str) -> Result<(), HyperliquidError> {
        if url.starts_with("ws://") || url.starts_with("wss://") {
            Ok(())
        } else {
            Err(HyperliquidError::UrlParsing(format!(
                "URL must start with ws:// or wss://, was: {}",
                url
            )))
        }
    }

    /// Encodes a WebSocket request to JSON bytes.
    pub fn encode(&self, request: &HyperliquidWsRequest) -> Result<Vec<u8>, HyperliquidError> {
        serde_json::to_vec(request).map_err(|e| {
            HyperliquidError::MessageSerialization(format!("Failed to serialize request: {}", e))
        })
    }

    /// Decodes JSON bytes to a WebSocket message.
    pub fn decode(&self, data: &[u8]) -> Result<HyperliquidWsMessage, HyperliquidError> {
        serde_json::from_slice(data).map_err(|e| {
            HyperliquidError::MessageDeserialization(format!(
                "Failed to deserialize message: {}",
                e
            ))
        })
    }
}

/// Low-level Hyperliquid WebSocket client that wraps Nautilus WebSocketClient.
///
/// This is the inner client that handles the transport layer and provides low-level
/// WebSocket methods with `ws_*` prefixes.
#[derive(Debug)]
pub struct HyperliquidWebSocketInnerClient {
    inner: Arc<WebSocketClient>,
    rx_inbound: mpsc::Receiver<HyperliquidWsMessage>,
    sent_subscriptions: HashSet<String>,
    _reader_task: tokio::task::JoinHandle<()>,
    post_router: Arc<PostRouter>,
    post_ids: PostIds,
    #[allow(dead_code, reason = "Reserved for future direct WebSocket operations")]
    ws_sender: WsSender,
    post_batcher: PostBatcher,
}

impl HyperliquidWebSocketInnerClient {
    /// Creates a new Hyperliquid WebSocket inner client with reconnection/backoff/heartbeat.
    /// Returns a client that owns the inbound message receiver.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        // Create message handler for receiving raw WebSocket messages
        let (message_handler, mut raw_rx) = channel_message_handler();

        let cfg = WebSocketConfig {
            url: url.to_string(),
            headers: vec![],
            message_handler: Some(message_handler),
            heartbeat: Some(20), // seconds; set lower than server idle timeout
            heartbeat_msg: None, // use WS Ping frames by default
            ping_handler: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
        };

        let client = Arc::new(WebSocketClient::connect(cfg, None, vec![], None).await?);
        tracing::info!("Hyperliquid WebSocket connected: {}", url);

        let post_router = PostRouter::new();
        let post_ids = PostIds::new(1);
        let (tx_inbound, rx_inbound) = mpsc::channel::<HyperliquidWsMessage>(1024);
        let (tx_outbound, mut rx_outbound) = mpsc::channel::<HyperliquidWsRequest>(1024);

        let ws_sender = WsSender::new(tx_outbound);

        // Reader task: decode messages and route post replies *before* handing to general pipeline.
        let post_router_for_reader = Arc::clone(&post_router);
        let reader_task = tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                match msg {
                    Message::Text(txt) => {
                        tracing::debug!("Received WS text: {}", txt);
                        match serde_json::from_str::<HyperliquidWsMessage>(&txt) {
                            Ok(hl_msg) => {
                                if let HyperliquidWsMessage::Post { data } = &hl_msg {
                                    // Route the correlated response
                                    post_router_for_reader.complete(data.clone()).await;
                                }
                                if let Err(e) = tx_inbound.send(hl_msg).await {
                                    tracing::error!("Failed to send decoded message: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to decode Hyperliquid message: {} | text: {}",
                                    e,
                                    txt
                                );
                            }
                        }
                    }
                    Message::Binary(data) => {
                        tracing::debug!("Received binary message ({} bytes), ignoring", data.len());
                    }
                    Message::Ping(data) => {
                        tracing::debug!("Received ping frame ({} bytes)", data.len());
                    }
                    Message::Pong(data) => {
                        tracing::debug!("Received pong frame ({} bytes)", data.len());
                    }
                    Message::Close(close_frame) => {
                        tracing::info!("Received close frame: {:?}", close_frame);
                        break;
                    }
                    Message::Frame(_) => tracing::warn!("Received raw frame (unexpected)"),
                }
            }
            tracing::info!("Hyperliquid WebSocket reader finished");
        });

        // Spawn task to handle outbound messages
        let client_for_sender = Arc::clone(&client);
        tokio::spawn(async move {
            while let Some(req) = rx_outbound.recv().await {
                let json = match serde_json::to_string(&req) {
                    Ok(json) => json,
                    Err(e) => {
                        tracing::error!("Failed to serialize WS request: {}", e);
                        continue;
                    }
                };
                tracing::debug!("Sending WS message: {}", json);
                if let Err(e) = client_for_sender.send_text(json, None).await {
                    tracing::error!("Failed to send WS message: {}", e);
                    break;
                }
            }
            tracing::info!("WebSocket sender task finished");
        });

        // Create send function for batcher using a proper async closure
        let ws_sender_for_batcher = ws_sender.clone();

        let send_fn =
            move |req: HyperliquidWsRequest| -> BoxFuture<'static, HyperliquidResult<()>> {
                let sender = ws_sender_for_batcher.clone();
                Box::pin(async move { sender.send(req).await })
            };

        let post_batcher = PostBatcher::new(send_fn);

        let hl_client = Self {
            inner: client,
            rx_inbound,
            sent_subscriptions: HashSet::new(),
            _reader_task: reader_task,
            post_router,
            post_ids,
            ws_sender,
            post_batcher,
        };

        Ok(hl_client)
    }

    /// Low-level method to send a Hyperliquid WebSocket request.
    pub async fn ws_send(&self, request: &HyperliquidWsRequest) -> anyhow::Result<()> {
        let json = serde_json::to_string(request)?;
        tracing::debug!("Sending WS message: {}", json);
        self.inner
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Low-level method to send a request only once (dedup by JSON serialization).
    pub async fn ws_send_once(&mut self, request: &HyperliquidWsRequest) -> anyhow::Result<()> {
        let json = serde_json::to_string(request)?;
        if self.sent_subscriptions.contains(&json) {
            tracing::debug!("Skipping duplicate request: {}", json);
            return Ok(());
        }

        tracing::debug!("Sending WS message: {}", json);
        self.inner
            .send_text(json.clone(), None)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        self.sent_subscriptions.insert(json);
        Ok(())
    }

    /// Low-level method to subscribe to a specific channel.
    pub async fn ws_subscribe(&mut self, subscription: SubscriptionRequest) -> anyhow::Result<()> {
        let request = HyperliquidWsRequest::Subscribe { subscription };
        self.ws_send_once(&request).await
    }

    /// Low-level method to unsubscribe from a specific channel.
    pub async fn ws_unsubscribe(
        &mut self,
        subscription: SubscriptionRequest,
    ) -> anyhow::Result<()> {
        let request = HyperliquidWsRequest::Unsubscribe { subscription };
        self.ws_send(&request).await
    }

    /// Get the next event from the WebSocket stream.
    /// Returns None when the connection is closed or the receiver is exhausted.
    pub async fn ws_next_event(&mut self) -> Option<HyperliquidWsMessage> {
        self.rx_inbound.recv().await
    }

    /// Returns true if the WebSocket connection is active.
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Returns true if the WebSocket is reconnecting.
    pub fn is_reconnecting(&self) -> bool {
        self.inner.is_reconnecting()
    }

    /// Returns true if the WebSocket is disconnecting.
    pub fn is_disconnecting(&self) -> bool {
        self.inner.is_disconnecting()
    }

    /// Returns true if the WebSocket is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Disconnect the WebSocket client.
    pub async fn ws_disconnect(&mut self) -> anyhow::Result<()> {
        self.inner.disconnect().await;
        Ok(())
    }

    /// Convenience: enqueue a post on a specific lane.
    async fn enqueue_post(
        &self,
        id: u64,
        request: PostRequest,
        lane: PostLane,
    ) -> HyperliquidResult<()> {
        self.post_batcher
            .enqueue(ScheduledPost { id, request, lane })
            .await
    }

    /// Core: send an Info post and await response with timeout.
    pub async fn post_info_raw(
        &self,
        payload: serde_json::Value,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponsePayload> {
        let id = self.post_ids.next();
        let rx = self.post_router.register(id).await?;
        self.enqueue_post(id, PostRequest::Info { payload }, PostLane::Normal)
            .await?;
        let resp = self.post_router.await_with_timeout(id, rx, timeout).await?;
        Ok(resp.response)
    }

    /// Core: send an Action post and await response with timeout.
    pub async fn post_action_raw(
        &self,
        action: ActionPayload,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponsePayload> {
        let id = self.post_ids.next();
        let rx = self.post_router.register(id).await?;
        let lane = lane_for_action(&action.action);
        self.enqueue_post(id, PostRequest::Action { payload: action }, lane)
            .await?;
        let resp = self.post_router.await_with_timeout(id, rx, timeout).await?;
        Ok(resp.response)
    }

    /// Get l2Book via WS post and parse using shared REST model.
    pub async fn info_l2_book(
        &self,
        coin: &str,
        timeout: Duration,
    ) -> HyperliquidResult<crate::http::models::HyperliquidL2Book> {
        let payload = match self
            .post_info_raw(serde_json::json!({"type":"l2Book","coin":coin}), timeout)
            .await?
        {
            PostResponsePayload::Info { payload } => payload,
            PostResponsePayload::Error { payload } => return Err(Error::exchange(payload)),
            PostResponsePayload::Action { .. } => {
                return Err(Error::decode("expected info payload, was action"));
            }
        };
        serde_json::from_value(payload).map_err(Error::Serde)
    }
}

/// High-level Hyperliquid WebSocket client that provides standardized domain methods.
///
/// This client uses Arc<RwLock<>> for internal state to support Clone and safe sharing
/// across async tasks, following the same pattern as other exchange adapters (OKX, Bitmex, Bybit).
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct HyperliquidWebSocketClient {
    inner: Arc<RwLock<Option<HyperliquidWebSocketInnerClient>>>,
    url: String,
    instruments: Arc<DashMap<InstrumentId, InstrumentAny>>,
    instruments_by_symbol: Arc<DashMap<Ustr, InstrumentId>>,
}

impl HyperliquidWebSocketClient {
    /// Creates a new Hyperliquid WebSocket client without connecting.
    /// The connection will be established when `ensure_connected()` is called.
    pub fn new(url: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
            url,
            instruments: Arc::new(DashMap::new()),
            instruments_by_symbol: Arc::new(DashMap::new()),
        }
    }

    /// Adds an instrument to the cache for parsing WebSocket messages.
    pub fn add_instrument(&self, instrument: InstrumentAny) {
        // Insert instrument into primary cache
        let instrument_id = instrument.id();
        self.instruments.insert(instrument_id, instrument);

        // Extract coin prefix (e.g., "BTC" from "BTC-PERP") and index for fast lookup
        let symbol = instrument_id.symbol.as_str();
        if let Some(coin) = symbol.split('-').next() {
            self.instruments_by_symbol
                .insert(Ustr::from(coin), instrument_id);
        }
    }

    /// Gets an instrument from the cache by ID.
    pub fn get_instrument(&self, id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments.get(id).map(|e| e.value().clone())
    }

    /// Gets an instrument from the cache by symbol.
    pub fn get_instrument_by_symbol(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        // Fast path: lookup instrument id by coin prefix, then fetch instrument by id.
        if let Some(id_entry) = self.instruments_by_symbol.get(symbol) {
            let instrument_id = *id_entry.value();
            if let Some(inst_entry) = self.instruments.get(&instrument_id) {
                return Some(inst_entry.value().clone());
            }
        }

        // Fallback: (should be rare) scan full instruments map to find exact symbol match
        self.instruments
            .iter()
            .find(|e| e.key().symbol == (*symbol).into())
            .map(|e| e.value().clone())
    }

    /// Creates a new Hyperliquid WebSocket client and establishes connection.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let inner_client = HyperliquidWebSocketInnerClient::connect(url).await?;
        Ok(Self {
            inner: Arc::new(RwLock::new(Some(inner_client))),
            url: url.to_string(),
            instruments: Arc::new(DashMap::new()),
            instruments_by_symbol: Arc::new(DashMap::new()),
        })
    }

    /// Establishes the WebSocket connection if not already connected.
    pub async fn ensure_connected(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        if inner.is_none() {
            let inner_client = HyperliquidWebSocketInnerClient::connect(&self.url).await?;
            *inner = Some(inner_client);
        }
        Ok(())
    }

    /// Returns true if the WebSocket is connected.
    pub async fn is_connected(&self) -> bool {
        let inner = self.inner.read().await;
        inner.is_some()
    }

    /// Returns the URL of this WebSocket client.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Subscribe to order updates for a specific user address.
    ///
    /// Ensures connection is established before subscribing.
    pub async fn subscribe_order_updates(&self, user: &str) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::OrderUpdates {
            user: user.to_string(),
        };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_subscribe(subscription)
            .await
    }

    /// Subscribe to user events (fills, funding, liquidations) for a specific user address.
    ///
    /// Ensures connection is established before subscribing.
    pub async fn subscribe_user_events(&self, user: &str) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::UserEvents {
            user: user.to_string(),
        };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_subscribe(subscription)
            .await
    }

    /// Subscribe to all user channels (order updates + user events) for convenience.
    pub async fn subscribe_all_user_channels(&self, user: &str) -> anyhow::Result<()> {
        self.subscribe_order_updates(user).await?;
        self.subscribe_user_events(user).await?;
        Ok(())
    }

    /// Subscribe to trades for a specific coin.
    ///
    /// Ensures connection is established before subscribing.
    pub async fn subscribe_trades(&self, coin: Ustr) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Trades { coin };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from trades for a specific coin.
    ///
    /// Ensures connection is established before unsubscribing.
    pub async fn unsubscribe_trades(&self, coin: Ustr) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Trades { coin };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_unsubscribe(subscription)
            .await
    }

    /// Subscribe to L2 order book for a specific coin.
    ///
    /// Ensures connection is established before subscribing.
    pub async fn subscribe_book(&self, coin: Ustr) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::L2Book {
            coin,
            n_sig_figs: None,
            mantissa: None,
        };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from L2 order book for a specific coin.
    ///
    /// Ensures connection is established before unsubscribing.
    pub async fn unsubscribe_book(&self, coin: Ustr) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::L2Book {
            coin,
            n_sig_figs: None,
            mantissa: None,
        };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_unsubscribe(subscription)
            .await
    }

    /// Subscribe to BBO (best bid/offer) for a specific coin.
    ///
    /// Ensures connection is established before subscribing.
    pub async fn subscribe_bbo(&self, coin: Ustr) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        tracing::info!("Subscribing to BBO for coin: {}", coin);
        let subscription = SubscriptionRequest::Bbo { coin };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from BBO (best bid/offer) for a specific coin.
    ///
    /// Ensures connection is established before unsubscribing.
    pub async fn unsubscribe_bbo(&self, coin: Ustr) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Bbo { coin };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_unsubscribe(subscription)
            .await
    }

    /// Subscribe to candlestick data for a specific coin and interval.
    ///
    /// Ensures connection is established before subscribing.
    pub async fn subscribe_candle(&self, coin: Ustr, interval: String) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Candle { coin, interval };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from candlestick data for a specific coin and interval.
    ///
    /// Ensures connection is established before unsubscribing.
    pub async fn unsubscribe_candle(&self, coin: Ustr, interval: String) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Candle { coin, interval };
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_unsubscribe(subscription)
            .await
    }

    /// Get the next event from the WebSocket stream.
    /// Returns None when the connection is closed or the receiver is exhausted.
    pub async fn next_event(&self) -> Option<HyperliquidWsMessage> {
        let mut inner = self.inner.write().await;
        if let Some(ref mut client) = *inner {
            client.ws_next_event().await
        } else {
            None
        }
    }

    /// Returns true if the WebSocket connection is active.
    pub async fn is_active(&self) -> bool {
        let inner = self.inner.read().await;
        inner.as_ref().is_some_and(|client| client.is_active())
    }

    /// Returns true if the WebSocket is reconnecting.
    pub async fn is_reconnecting(&self) -> bool {
        let inner = self.inner.read().await;
        inner
            .as_ref()
            .is_some_and(|client| client.is_reconnecting())
    }

    /// Returns true if the WebSocket is disconnecting.
    pub async fn is_disconnecting(&self) -> bool {
        let inner = self.inner.read().await;
        inner
            .as_ref()
            .is_some_and(|client| client.is_disconnecting())
    }

    /// Returns true if the WebSocket is closed.
    pub async fn is_closed(&self) -> bool {
        let inner = self.inner.read().await;
        inner.as_ref().is_none_or(|client| client.is_closed())
    }

    /// Disconnect the WebSocket client.
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        if let Some(ref mut client) = *inner {
            client.ws_disconnect().await
        } else {
            Ok(())
        }
    }

    /// Escape hatch: send raw requests for tests/power users.
    ///
    /// Ensures connection is established before sending.
    pub async fn send_raw(&self, request: &HyperliquidWsRequest) -> anyhow::Result<()> {
        self.ensure_connected().await?;
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Client not connected"))?
            .ws_send(request)
            .await
    }

    /// High-level: call info l2Book (WS post)
    ///
    /// Ensures connection is established before making the request.
    pub async fn info_l2_book(
        &self,
        coin: &str,
        timeout: Duration,
    ) -> HyperliquidResult<crate::http::models::HyperliquidL2Book> {
        self.ensure_connected().await.map_err(|e| Error::Http {
            status: 500,
            message: e.to_string(),
        })?;
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| Error::Http {
                status: 500,
                message: "Client not connected".to_string(),
            })?
            .info_l2_book(coin, timeout)
            .await
    }

    /// High-level: fire arbitrary info (WS post) returning raw payload.
    ///
    /// Ensures connection is established before making the request.
    pub async fn post_info_raw(
        &self,
        payload: serde_json::Value,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponsePayload> {
        self.ensure_connected().await.map_err(|e| Error::Http {
            status: 500,
            message: e.to_string(),
        })?;
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| Error::Http {
                status: 500,
                message: "Client not connected".to_string(),
            })?
            .post_info_raw(payload, timeout)
            .await
    }

    /// High-level: fire action (already signed ActionPayload)
    ///
    /// Ensures connection is established before making the request.
    pub async fn post_action_raw(
        &self,
        action: ActionPayload,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponsePayload> {
        self.ensure_connected().await.map_err(|e| Error::Http {
            status: 500,
            message: e.to_string(),
        })?;
        let mut inner = self.inner.write().await;
        inner
            .as_mut()
            .ok_or_else(|| Error::Http {
                status: 500,
                message: "Client not connected".to_string(),
            })?
            .post_action_raw(action, timeout)
            .await
    }

    /// Creates a stream of execution messages (order updates and fills).
    ///
    /// This method spawns a background task that listens for WebSocket messages
    /// and processes OrderUpdates and UserEvents (fills) into ExecutionReports.
    /// The execution reports are sent through the returned stream for processing
    /// by the execution client.
    ///
    /// # Arguments
    ///
    /// * `account_id` - Account ID for report generation
    /// * `user_address` - User address to subscribe to order updates and user events
    ///
    /// # Returns
    ///
    /// A stream of `NautilusWsMessage` containing execution reports
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails or connection cannot be established
    pub async fn stream_execution_messages(
        &self,
        account_id: AccountId,
        user_address: String,
    ) -> anyhow::Result<impl Stream<Item = NautilusWsMessage>> {
        // Ensure connection
        self.ensure_connected().await?;

        // Subscribe to order updates and user events
        self.subscribe_order_updates(&user_address).await?;
        self.subscribe_user_events(&user_address).await?;

        let client = self.clone();
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn background task to process WebSocket messages
        tokio::spawn(async move {
            let clock = get_atomic_clock_realtime();

            loop {
                let event = client.next_event().await;

                match event {
                    Some(msg) => {
                        match &msg {
                            HyperliquidWsMessage::OrderUpdates { data } => {
                                let mut exec_reports = Vec::new();

                                // Process each order update in the array
                                for order_update in data {
                                    if let Some(instrument) =
                                        client.get_instrument_by_symbol(&order_update.order.coin)
                                    {
                                        let ts_init = clock.get_time_ns();

                                        match parse_ws_order_status_report(
                                            order_update,
                                            &instrument,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                exec_reports.push(ExecutionReport::Order(report));
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Error parsing order update: {}",
                                                    e
                                                );
                                            }
                                        }
                                    } else {
                                        tracing::warn!(
                                            "No instrument found for symbol: {}",
                                            order_update.order.coin
                                        );
                                    }
                                }

                                // Send reports if any
                                if !exec_reports.is_empty()
                                    && let Err(e) =
                                        tx.send(NautilusWsMessage::ExecutionReports(exec_reports))
                                {
                                    tracing::error!("Failed to send execution reports: {}", e);
                                    break;
                                }
                            }
                            HyperliquidWsMessage::UserEvents { data } => {
                                use crate::websocket::messages::WsUserEventData;

                                let ts_init = clock.get_time_ns();

                                match data {
                                    WsUserEventData::Fills { fills } => {
                                        let mut exec_reports = Vec::new();

                                        // Process each fill
                                        for fill in fills {
                                            if let Some(instrument) =
                                                client.get_instrument_by_symbol(&fill.coin)
                                            {
                                                match parse_ws_fill_report(
                                                    fill,
                                                    &instrument,
                                                    account_id,
                                                    ts_init,
                                                ) {
                                                    Ok(report) => {
                                                        exec_reports
                                                            .push(ExecutionReport::Fill(report));
                                                    }
                                                    Err(e) => {
                                                        tracing::error!(
                                                            "Error parsing fill: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                            } else {
                                                tracing::warn!(
                                                    "No instrument found for symbol: {}",
                                                    fill.coin
                                                );
                                            }
                                        }

                                        // Send reports if any
                                        if !exec_reports.is_empty()
                                            && let Err(e) = tx.send(
                                                NautilusWsMessage::ExecutionReports(exec_reports),
                                            )
                                        {
                                            tracing::error!("Failed to send fill reports: {}", e);
                                            break;
                                        }
                                    }
                                    _ => {
                                        // Other user events (funding, liquidation, etc.) not handled yet
                                    }
                                }
                            }
                            _ => {
                                // Ignore other message types in execution stream
                            }
                        }
                    }
                    None => {
                        // Connection closed
                        break;
                    }
                }
            }
        });

        // Return the stream
        Ok(async_stream::stream! {
            let mut rx = rx;
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        })
    }
}

// Python bindings
#[cfg(feature = "python")]
#[pyo3::pymethods]
impl HyperliquidWebSocketClient {
    #[new]
    #[pyo3(signature = (url))]
    fn py_new(url: String) -> PyResult<Self> {
        Ok(Self::new(url))
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> String {
        self.url().to_string()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(client.is_active().await) })
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(client.is_closed().await) })
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &self,
        py: Python<'py>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Parse instruments from Python objects and store in cache
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            self.add_instrument(inst_any);
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.ensure_connected().await.map_err(to_pyruntime_err)?;

            // Spawn background task to handle incoming messages
            tokio::spawn(async move {
                let clock = get_atomic_clock_realtime();

                loop {
                    let event = client.next_event().await;

                    match event {
                        Some(msg) => {
                            tracing::debug!("Received WebSocket message: {:?}", msg);

                            // Parse and send to Python callback
                            match msg {
                                HyperliquidWsMessage::Trades { data } => {
                                    for trade in data {
                                        if let Some(instrument) =
                                            client.get_instrument_by_symbol(&trade.coin)
                                        {
                                            let ts_init = clock.get_time_ns();
                                            match parse_ws_trade_tick(&trade, &instrument, ts_init)
                                            {
                                                Ok(tick) => {
                                                    Python::attach(|py| {
                                                        let py_obj = data_to_pycapsule(
                                                            py,
                                                            Data::Trade(tick),
                                                        );
                                                        if let Err(e) =
                                                            callback.bind(py).call1((py_obj,))
                                                        {
                                                            tracing::error!(
                                                                "Error calling Python callback: {}",
                                                                e
                                                            );
                                                        }
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Error parsing trade tick: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else {
                                            tracing::warn!(
                                                "No instrument found for symbol: {}",
                                                trade.coin
                                            );
                                        }
                                    }
                                }
                                HyperliquidWsMessage::L2Book { data } => {
                                    if let Some(instrument) =
                                        client.get_instrument_by_symbol(&data.coin)
                                    {
                                        let ts_init = clock.get_time_ns();
                                        match parse_ws_order_book_deltas(
                                            &data,
                                            &instrument,
                                            ts_init,
                                        ) {
                                            Ok(deltas) => {
                                                Python::attach(|py| {
                                                    let py_obj = data_to_pycapsule(
                                                        py,
                                                        Data::Deltas(OrderBookDeltas_API::new(
                                                            deltas,
                                                        )),
                                                    );
                                                    if let Err(e) =
                                                        callback.bind(py).call1((py_obj,))
                                                    {
                                                        tracing::error!(
                                                            "Error calling Python callback: {}",
                                                            e
                                                        );
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Error parsing order book deltas: {}",
                                                    e
                                                );
                                            }
                                        }
                                    } else {
                                        tracing::warn!(
                                            "No instrument found for symbol: {}",
                                            data.coin
                                        );
                                    }
                                }
                                HyperliquidWsMessage::Bbo { data } => {
                                    if let Some(instrument) =
                                        client.get_instrument_by_symbol(&data.coin)
                                    {
                                        let ts_init = clock.get_time_ns();
                                        match parse_ws_quote_tick(&data, &instrument, ts_init) {
                                            Ok(quote) => {
                                                Python::attach(|py| {
                                                    let py_obj =
                                                        data_to_pycapsule(py, Data::Quote(quote));
                                                    if let Err(e) =
                                                        callback.bind(py).call1((py_obj,))
                                                    {
                                                        tracing::error!(
                                                            "Error calling Python callback: {}",
                                                            e
                                                        );
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                tracing::error!("Error parsing quote tick: {}", e);
                                            }
                                        }
                                    } else {
                                        tracing::warn!(
                                            "No instrument found for symbol: {}",
                                            data.coin
                                        );
                                    }
                                }
                                HyperliquidWsMessage::Candle { data } => {
                                    if let Some(instrument) =
                                        client.get_instrument_by_symbol(&data.s)
                                    {
                                        let ts_init = clock.get_time_ns();
                                        // Create a bar type from the instrument and interval
                                        // The actual bar type construction should be done based on the interval
                                        let bar_type_str =
                                            format!("{}-{}-LAST-EXTERNAL", instrument.id(), data.i);
                                        match bar_type_str.parse::<BarType>() {
                                            Ok(bar_type) => {
                                                match parse_ws_candle(
                                                    &data,
                                                    &instrument,
                                                    &bar_type,
                                                    ts_init,
                                                ) {
                                                    Ok(bar) => {
                                                        Python::attach(|py| {
                                                            let py_obj = data_to_pycapsule(
                                                                py,
                                                                Data::Bar(bar),
                                                            );
                                                            if let Err(e) =
                                                                callback.bind(py).call1((py_obj,))
                                                            {
                                                                tracing::error!(
                                                                    "Error calling Python callback: {}",
                                                                    e
                                                                );
                                                            }
                                                        });
                                                    }
                                                    Err(e) => {
                                                        tracing::error!(
                                                            "Error parsing candle: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!("Error creating bar type: {}", e);
                                            }
                                        }
                                    } else {
                                        tracing::warn!(
                                            "No instrument found for symbol: {}",
                                            data.s
                                        );
                                    }
                                }
                                HyperliquidWsMessage::OrderUpdates { data } => {
                                    // Process each order update in the array
                                    for order_update in data {
                                        if let Some(instrument) = client
                                            .get_instrument_by_symbol(&order_update.order.coin)
                                        {
                                            let ts_init = clock.get_time_ns();
                                            // We need an account_id - this should come from the client config
                                            // For now, use a default account ID
                                            let account_id =
                                                nautilus_model::identifiers::AccountId::new(
                                                    "HYPERLIQUID-001",
                                                );

                                            match parse_ws_order_status_report(
                                                &order_update,
                                                &instrument,
                                                account_id,
                                                ts_init,
                                            ) {
                                                Ok(report) => {
                                                    // Note: Execution reports should be handled via
                                                    // stream_execution_messages() for execution clients,
                                                    // not through data callbacks
                                                    tracing::info!(
                                                        "Parsed order status report: order_id={}, status={:?}",
                                                        report.venue_order_id,
                                                        report.order_status
                                                    );
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Error parsing order update: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else {
                                            tracing::warn!(
                                                "No instrument found for symbol: {}",
                                                order_update.order.coin
                                            );
                                        }
                                    }
                                }
                                HyperliquidWsMessage::UserEvents { data } => {
                                    use crate::websocket::messages::WsUserEventData;

                                    // We need an account_id - this should come from the client config
                                    let account_id = nautilus_model::identifiers::AccountId::new(
                                        "HYPERLIQUID-001",
                                    );
                                    let ts_init = clock.get_time_ns();

                                    match data {
                                        WsUserEventData::Fills { fills } => {
                                            // Process each fill
                                            for fill in fills {
                                                if let Some(instrument) =
                                                    client.get_instrument_by_symbol(&fill.coin)
                                                {
                                                    match parse_ws_fill_report(
                                                        &fill,
                                                        &instrument,
                                                        account_id,
                                                        ts_init,
                                                    ) {
                                                        Ok(report) => {
                                                            // Note: Execution reports should be handled via
                                                            // stream_execution_messages() for execution clients,
                                                            // not through data callbacks
                                                            tracing::info!(
                                                                "Parsed fill report: trade_id={}, side={:?}, qty={}, price={}",
                                                                report.trade_id,
                                                                report.order_side,
                                                                report.last_qty,
                                                                report.last_px
                                                            );
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                "Error parsing fill: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!(
                                                        "No instrument found for symbol: {}",
                                                        fill.coin
                                                    );
                                                }
                                            }
                                        }
                                        WsUserEventData::Funding { funding } => {
                                            tracing::debug!(
                                                "Received funding update: {:?}",
                                                funding
                                            );
                                            // Funding updates would need to be converted to appropriate Nautilus events
                                            // This could be implemented if funding rate updates are needed
                                        }
                                        WsUserEventData::Liquidation { liquidation } => {
                                            tracing::warn!(
                                                "Received liquidation event: {:?}",
                                                liquidation
                                            );
                                            // Liquidation events would need special handling
                                            // This could be implemented based on requirements
                                        }
                                        WsUserEventData::NonUserCancel { non_user_cancel } => {
                                            tracing::info!(
                                                "Received non-user cancel events: {:?}",
                                                non_user_cancel
                                            );
                                            // These are system-initiated cancels (e.g., post-only rejected)
                                            // Could be converted to order status updates if needed
                                        }
                                        WsUserEventData::TriggerActivated { trigger_activated } => {
                                            tracing::debug!(
                                                "Trigger order activated: {:?}",
                                                trigger_activated
                                            );
                                            // Trigger activation events indicate a conditional order moved to active
                                            // Could be converted to order status updates if needed
                                        }
                                        WsUserEventData::TriggerTriggered { trigger_triggered } => {
                                            tracing::debug!(
                                                "Trigger order triggered: {:?}",
                                                trigger_triggered
                                            );
                                            // Trigger execution events indicate a conditional order was triggered
                                            // Could be converted to order status updates if needed
                                        }
                                    }
                                }
                                _ => {
                                    tracing::debug!("Unhandled message type: {:?}", msg);
                                }
                            }
                        }
                        None => {
                            tracing::info!("WebSocket connection closed");
                            break;
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let start = std::time::Instant::now();
            loop {
                if client.is_active().await {
                    return Ok(());
                }

                if start.elapsed().as_secs_f64() >= timeout_secs {
                    return Err(PyRuntimeError::new_err(format!(
                        "WebSocket connection did not become active within {} seconds",
                        timeout_secs
                    )));
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.disconnect().await {
                tracing::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_book_deltas")]
    fn py_subscribe_order_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_order_book_deltas")]
    fn py_unsubscribe_order_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_book_snapshots")]
    fn py_subscribe_order_book_snapshots<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        _book_type: u8,
        _depth: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(instrument_id.symbol.as_str());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        // Extract coin from symbol (e.g., "BTC-USD-PERP" -> "BTC")
        let coin_str = instrument_id
            .symbol
            .as_str()
            .split('-')
            .next()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid instrument symbol")
            })?;
        let coin = Ustr::from(coin_str);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_bbo(coin).await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        // Extract coin from symbol (e.g., "BTC-USD-PERP" -> "BTC")
        let coin_str = instrument_id
            .symbol
            .as_str()
            .split('-')
            .next()
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid instrument symbol")
            })?;
        let coin = Ustr::from(coin_str);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_bbo(coin)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(bar_type.instrument_id().symbol.as_str());
        let interval = "1m".to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_candle(coin, interval)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let coin = Ustr::from(bar_type.instrument_id().symbol.as_str());
        let interval = "1m".to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candle(coin, interval)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_order_updates")]
    fn py_subscribe_order_updates<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_order_updates(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_user_events")]
    fn py_subscribe_user_events<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_user_events(&user)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}
