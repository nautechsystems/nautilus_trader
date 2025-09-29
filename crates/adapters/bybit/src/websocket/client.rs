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

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use dashmap::DashMap;
use nautilus_common::runtime::get_runtime;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    RECONNECTED,
    websocket::{PingHandler, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    common::consts::{BYBIT_NAUTILUS_BROKER_ID, BYBIT_PONG, BYBIT_WS_PUBLIC_URL},
    websocket::{
        error::{BybitWsError, BybitWsResult},
        messages::{
            BybitWebSocketError, BybitWebSocketMessage, BybitWsAuthResponse, BybitWsKlineMsg,
            BybitWsOrderbookDepthMsg, BybitWsResponse, BybitWsSubscriptionMsg,
            BybitWsTickerLinearMsg, BybitWsTickerOptionMsg, BybitWsTradeMsg,
        },
    },
};

const MAX_ARGS_PER_SUBSCRIPTION_REQUEST: usize = 10;
const DEFAULT_HEARTBEAT_SECS: u64 = 20;
const PING_MESSAGE: &str = r#"{"op":"ping"}"#;
const PONG_MESSAGE: &str = r#"{"op":"pong"}"#;

/// Public/market data WebSocket client for Bybit.
pub struct BybitWebSocketClient {
    url: String,
    heartbeat: Option<u64>,
    inner: Arc<RwLock<Option<WebSocketClient>>>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<BybitWebSocketMessage>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    subscriptions: Arc<DashMap<String, ()>>,
}

impl fmt::Debug for BybitWebSocketClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BybitWebSocketClient")
            .field("url", &self.url)
            .field("heartbeat", &self.heartbeat)
            .field("active_subscriptions", &self.subscriptions.len())
            .finish()
    }
}

impl BybitWebSocketClient {
    /// Creates a new Bybit public WebSocket client.
    #[must_use]
    pub fn new_public(url: Option<String>, heartbeat: Option<u64>) -> Self {
        Self {
            url: url.unwrap_or_else(|| BYBIT_WS_PUBLIC_URL.to_string()),
            heartbeat: heartbeat.or(Some(DEFAULT_HEARTBEAT_SECS)),
            inner: Arc::new(RwLock::new(None)),
            rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            subscriptions: Arc::new(DashMap::new()),
        }
    }

    /// Establishes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying WebSocket connection cannot be established.
    pub async fn connect(&mut self) -> BybitWsResult<()> {
        let (message_handler, mut message_rx) = channel_message_handler();

        let inner_for_ping = Arc::clone(&self.inner);
        let ping_handler: PingHandler = Arc::new(move |payload: Vec<u8>| {
            let inner = Arc::clone(&inner_for_ping);
            get_runtime().spawn(async move {
                let len = payload.len();
                let guard = inner.read().await;
                if let Some(client) = guard.as_ref() {
                    if let Err(err) = client.send_pong(payload).await {
                        tracing::warn!(error = %err, "Failed to send pong frame");
                    } else {
                        tracing::trace!("Sent pong frame ({len} bytes)");
                    }
                }
            });
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: Self::default_headers(),
            message_handler: Some(message_handler),
            heartbeat: self.heartbeat,
            heartbeat_msg: Some(PING_MESSAGE.to_string()),
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
        let subscriptions = Arc::clone(&self.subscriptions);

        let task_handle = get_runtime().spawn(async move {
            while let Some(message) = message_rx.recv().await {
                if signal.load(Ordering::Relaxed) {
                    break;
                }

                match BybitWebSocketClient::handle_message(&inner, &subscriptions, message).await {
                    Ok(Some(BybitWebSocketMessage::Reconnected)) => {
                        if let Err(err) =
                            BybitWebSocketClient::resubscribe_all_inner(&inner, &subscriptions)
                                .await
                        {
                            let error = BybitWebSocketError::from_message(err.to_string());
                            if event_tx.send(BybitWebSocketMessage::Error(error)).is_err() {
                                break;
                            }
                        }
                        if event_tx.send(BybitWebSocketMessage::Reconnected).is_err() {
                            break;
                        }
                    }
                    Ok(Some(event)) => {
                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        let error = BybitWebSocketError::from_message(err.to_string());
                        if event_tx.send(BybitWebSocketMessage::Error(error)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        self.task_handle = Some(task_handle);

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
            && let Err(err) = handle.await
        {
            tracing::error!(error = %err, "Bybit websocket task terminated with error");
        }

        self.rx = None;

        Ok(())
    }

    /// Returns `true` when the underlying client is active.
    #[must_use]
    pub async fn is_active(&self) -> bool {
        let guard = self.inner.read().await;
        guard.as_ref().is_some_and(WebSocketClient::is_active)
    }

    /// Subscribe to the provided topic strings.
    pub async fn subscribe(&self, topics: Vec<String>) -> BybitWsResult<()> {
        if topics.is_empty() {
            return Ok(());
        }

        let mut new_topics = Vec::new();
        for topic in topics {
            if self.subscriptions.contains_key(&topic) {
                tracing::debug!("Bybit subscription already active: {topic}");
                continue;
            }
            self.subscriptions.insert(topic.clone(), ());
            new_topics.push(topic);
        }

        if new_topics.is_empty() {
            return Ok(());
        }

        Self::send_topics_inner(&self.inner, "subscribe", new_topics).await
    }

    /// Unsubscribe from the provided topics.
    pub async fn unsubscribe(&self, topics: Vec<String>) -> BybitWsResult<()> {
        if topics.is_empty() {
            return Ok(());
        }

        let mut removed_topics = Vec::new();
        for topic in topics {
            if self.subscriptions.remove(&topic).is_some() {
                removed_topics.push(topic);
            } else {
                tracing::debug!("Cannot unsubscribe '{topic}': not currently subscribed");
            }
        }

        if removed_topics.is_empty() {
            return Ok(());
        }

        Self::send_topics_inner(&self.inner, "unsubscribe", removed_topics).await
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

    fn default_headers() -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Referer".to_string(), BYBIT_NAUTILUS_BROKER_ID.to_string()),
        ]
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
        op: &str,
        topics: Vec<String>,
    ) -> BybitWsResult<()> {
        if topics.is_empty() {
            return Ok(());
        }

        for chunk in topics.chunks(MAX_ARGS_PER_SUBSCRIPTION_REQUEST) {
            let payload = json!({
                "op": op,
                "args": chunk,
            });
            Self::send_text_inner(inner, &payload.to_string()).await?;
        }

        Ok(())
    }

    async fn resubscribe_all_inner(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        subscriptions: &Arc<DashMap<String, ()>>,
    ) -> BybitWsResult<()> {
        let topics: Vec<String> = subscriptions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        Self::send_topics_inner(inner, "subscribe", topics).await
    }

    async fn handle_message(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        _subscriptions: &Arc<DashMap<String, ()>>,
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

                if let Some(op) = value.get("op").and_then(Value::as_str) {
                    if op.eq_ignore_ascii_case("ping") {
                        Self::send_text_inner(inner, PONG_MESSAGE).await?;
                        return Ok(None);
                    }
                    if op.eq_ignore_ascii_case("pong") {
                        return Ok(Some(BybitWebSocketMessage::Pong));
                    }
                }

                if let Some(event) = Self::classify_message(&value) {
                    if let BybitWebSocketMessage::Error(err) = &event {
                        tracing::debug!(code = err.code, message = %err.message, "Bybit websocket error frame");
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
            && auth.op.eq_ignore_ascii_case("auth")
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
                ret_msg: auth.ret_msg.clone(),
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
            }
        }

        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::load_test_json;

    #[test]
    fn classify_orderbook_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json"))
            .expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected orderbook message");
        assert!(matches!(message, BybitWebSocketMessage::Orderbook(_)));
    }

    #[test]
    fn classify_trade_snapshot() {
        let json: Value =
            serde_json::from_str(&load_test_json("ws_public_trade.json")).expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected trade message");
        assert!(matches!(message, BybitWebSocketMessage::Trade(_)));
    }

    #[test]
    fn classify_ticker_linear_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_linear.json"))
            .expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected ticker message");
        assert!(matches!(message, BybitWebSocketMessage::TickerLinear(_)));
    }

    #[test]
    fn classify_ticker_option_snapshot() {
        let json: Value = serde_json::from_str(&load_test_json("ws_ticker_option.json"))
            .expect("invalid fixture");
        let message =
            BybitWebSocketClient::classify_message(&json).expect("expected ticker message");
        assert!(matches!(message, BybitWebSocketMessage::TickerOption(_)));
    }
}
