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

use anyhow::Result;
use futures_util::future::BoxFuture;
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};
use ustr::Ustr;

use crate::{
    http::error::{Error, Result as HyperliquidResult},
    websocket::{
        messages::{
            ActionPayload, HyperliquidWsMessage, HyperliquidWsRequest, PostRequest,
            PostResponsePayload, SubscriptionRequest,
        },
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
    pub async fn connect(url: &str) -> Result<Self> {
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
        info!("Hyperliquid WebSocket connected: {}", url);

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
                        debug!("Received WS text: {}", txt);
                        match serde_json::from_str::<HyperliquidWsMessage>(&txt) {
                            Ok(hl_msg) => {
                                if let HyperliquidWsMessage::Post { data } = &hl_msg {
                                    // Route the correlated response
                                    post_router_for_reader.complete(data.clone()).await;
                                }
                                if let Err(e) = tx_inbound.send(hl_msg).await {
                                    error!("Failed to send decoded message: {}", e);
                                    break;
                                }
                            }
                            Err(err) => {
                                error!(
                                    "Failed to decode Hyperliquid message: {} | text: {}",
                                    err, txt
                                );
                            }
                        }
                    }
                    Message::Binary(data) => {
                        debug!("Received binary message ({} bytes), ignoring", data.len())
                    }
                    Message::Ping(data) => debug!("Received ping frame ({} bytes)", data.len()),
                    Message::Pong(data) => debug!("Received pong frame ({} bytes)", data.len()),
                    Message::Close(close_frame) => {
                        info!("Received close frame: {:?}", close_frame);
                        break;
                    }
                    Message::Frame(_) => warn!("Received raw frame (unexpected)"),
                }
            }
            info!("Hyperliquid WebSocket reader finished");
        });

        // Spawn task to handle outbound messages
        let client_for_sender = Arc::clone(&client);
        tokio::spawn(async move {
            while let Some(req) = rx_outbound.recv().await {
                let json = match serde_json::to_string(&req) {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Failed to serialize WS request: {}", e);
                        continue;
                    }
                };
                debug!("Sending WS message: {}", json);
                if let Err(e) = client_for_sender.send_text(json, None).await {
                    error!("Failed to send WS message: {}", e);
                    break;
                }
            }
            info!("WebSocket sender task finished");
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
    pub async fn ws_send(&self, request: &HyperliquidWsRequest) -> Result<()> {
        let json = serde_json::to_string(request)?;
        debug!("Sending WS message: {}", json);
        self.inner
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Low-level method to send a request only once (dedup by JSON serialization).
    pub async fn ws_send_once(&mut self, request: &HyperliquidWsRequest) -> Result<()> {
        let json = serde_json::to_string(request)?;
        if self.sent_subscriptions.contains(&json) {
            debug!("Skipping duplicate request: {}", json);
            return Ok(());
        }

        debug!("Sending WS message: {}", json);
        self.inner
            .send_text(json.clone(), None)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        self.sent_subscriptions.insert(json);
        Ok(())
    }

    /// Low-level method to subscribe to a specific channel.
    pub async fn ws_subscribe(&mut self, subscription: SubscriptionRequest) -> Result<()> {
        let request = HyperliquidWsRequest::Subscribe { subscription };
        self.ws_send_once(&request).await
    }

    /// Low-level method to unsubscribe from a specific channel.
    pub async fn ws_unsubscribe(&mut self, subscription: SubscriptionRequest) -> Result<()> {
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
    pub async fn ws_disconnect(&mut self) -> Result<()> {
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
/// This is the outer client that wraps the inner client and provides Nautilus-specific
/// functionality for WebSocket operations using standard domain methods.
#[derive(Debug)]
pub struct HyperliquidWebSocketClient {
    inner: Option<HyperliquidWebSocketInnerClient>,
    url: String,
}

impl HyperliquidWebSocketClient {
    /// Creates a new Hyperliquid WebSocket client without connecting.
    /// The connection will be established when start() is called.
    pub fn new(url: String) -> Self {
        Self { inner: None, url }
    }

    /// Creates a new Hyperliquid WebSocket client and establishes connection.
    pub async fn connect(url: &str) -> Result<Self> {
        let inner = HyperliquidWebSocketInnerClient::connect(url).await?;
        Ok(Self {
            inner: Some(inner),
            url: url.to_string(),
        })
    }

    /// Establishes the WebSocket connection if not already connected.
    pub async fn ensure_connected(&mut self) -> Result<()> {
        if self.inner.is_none() {
            let inner = HyperliquidWebSocketInnerClient::connect(&self.url).await?;
            self.inner = Some(inner);
        }
        Ok(())
    }

    /// Returns true if the WebSocket is connected.
    pub fn is_connected(&self) -> bool {
        self.inner.is_some()
    }

    /// Subscribe to order updates for a specific user address.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn subscribe_order_updates(&mut self, user: &str) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::OrderUpdates {
            user: user.to_string(),
        };
        self.inner
            .as_mut()
            .unwrap()
            .ws_subscribe(subscription)
            .await
    }

    /// Subscribe to user events (fills, funding, liquidations) for a specific user address.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn subscribe_user_events(&mut self, user: &str) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::UserEvents {
            user: user.to_string(),
        };
        self.inner
            .as_mut()
            .unwrap()
            .ws_subscribe(subscription)
            .await
    }

    /// Subscribe to all user channels (order updates + user events) for convenience.
    pub async fn subscribe_all_user_channels(&mut self, user: &str) -> Result<()> {
        self.subscribe_order_updates(user).await?;
        self.subscribe_user_events(user).await?;
        Ok(())
    }

    /// Subscribe to trades for a specific coin.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn subscribe_trades(&mut self, coin: Ustr) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Trades { coin };
        self.inner
            .as_mut()
            .unwrap()
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from trades for a specific coin.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn unsubscribe_trades(&mut self, coin: Ustr) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Trades { coin };
        self.inner
            .as_mut()
            .unwrap()
            .ws_unsubscribe(subscription)
            .await
    }

    /// Subscribe to L2 order book for a specific coin.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn subscribe_book(&mut self, coin: Ustr) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::L2Book {
            coin,
            n_sig_figs: None,
            mantissa: None,
        };
        self.inner
            .as_mut()
            .unwrap()
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from L2 order book for a specific coin.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn unsubscribe_book(&mut self, coin: Ustr) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::L2Book {
            coin,
            n_sig_figs: None,
            mantissa: None,
        };
        self.inner
            .as_mut()
            .unwrap()
            .ws_unsubscribe(subscription)
            .await
    }

    /// Subscribe to BBO (best bid/offer) for a specific coin.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn subscribe_bbo(&mut self, coin: Ustr) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Bbo { coin };
        self.inner
            .as_mut()
            .unwrap()
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from BBO (best bid/offer) for a specific coin.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn unsubscribe_bbo(&mut self, coin: Ustr) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Bbo { coin };
        self.inner
            .as_mut()
            .unwrap()
            .ws_unsubscribe(subscription)
            .await
    }

    /// Subscribe to candlestick data for a specific coin and interval.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn subscribe_candle(&mut self, coin: Ustr, interval: String) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Candle { coin, interval };
        self.inner
            .as_mut()
            .unwrap()
            .ws_subscribe(subscription)
            .await
    }

    /// Unsubscribe from candlestick data for a specific coin and interval.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn unsubscribe_candle(&mut self, coin: Ustr, interval: String) -> Result<()> {
        self.ensure_connected().await?;
        let subscription = SubscriptionRequest::Candle { coin, interval };
        self.inner
            .as_mut()
            .unwrap()
            .ws_unsubscribe(subscription)
            .await
    }

    /// Get the next event from the WebSocket stream.
    /// Returns None when the connection is closed or the receiver is exhausted.
    pub async fn next_event(&mut self) -> Option<HyperliquidWsMessage> {
        if let Some(ref mut inner) = self.inner {
            inner.ws_next_event().await
        } else {
            None
        }
    }

    /// Returns true if the WebSocket connection is active.
    pub fn is_active(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.is_active())
    }

    /// Returns true if the WebSocket is reconnecting.
    pub fn is_reconnecting(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.is_reconnecting())
    }

    /// Returns true if the WebSocket is disconnecting.
    pub fn is_disconnecting(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.is_disconnecting())
    }

    /// Returns true if the WebSocket is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.as_ref().is_none_or(|inner| inner.is_closed())
    }

    /// Disconnect the WebSocket client.
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(ref mut inner) = self.inner {
            inner.ws_disconnect().await
        } else {
            Ok(())
        }
    }

    /// Escape hatch: send raw requests for tests/power users.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn send_raw(&mut self, request: &HyperliquidWsRequest) -> Result<()> {
        self.ensure_connected().await?;
        self.inner.as_mut().unwrap().ws_send(request).await
    }

    /// High-level: call info l2Book (WS post)
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn info_l2_book(
        &mut self,
        coin: &str,
        timeout: Duration,
    ) -> HyperliquidResult<crate::http::models::HyperliquidL2Book> {
        self.ensure_connected().await.map_err(|e| Error::Http {
            status: 500,
            message: e.to_string(),
        })?;
        self.inner
            .as_mut()
            .unwrap()
            .info_l2_book(coin, timeout)
            .await
    }

    /// High-level: fire arbitrary info (WS post) returning raw payload.
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn post_info_raw(
        &mut self,
        payload: serde_json::Value,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponsePayload> {
        self.ensure_connected().await.map_err(|e| Error::Http {
            status: 500,
            message: e.to_string(),
        })?;
        self.inner
            .as_mut()
            .unwrap()
            .post_info_raw(payload, timeout)
            .await
    }

    /// High-level: fire action (already signed ActionPayload)
    ///
    /// # Panics
    ///
    /// Panics if the WebSocket client is not connected. Call `ensure_connected()` first.
    pub async fn post_action_raw(
        &mut self,
        action: ActionPayload,
        timeout: Duration,
    ) -> HyperliquidResult<PostResponsePayload> {
        self.ensure_connected().await.map_err(|e| Error::Http {
            status: 500,
            message: e.to_string(),
        })?;
        self.inner
            .as_mut()
            .unwrap()
            .post_action_raw(action, timeout)
            .await
    }
}
