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

//! Market data WebSocket message handler for Ax.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_network::websocket::{SubscriptionState, WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::AxMdSubscriptionSpec;
use crate::{
    common::enums::{AxCandleWidth, AxMdRequestType},
    websocket::{
        messages::{
            AxDataWsMessage, AxMdMessage, AxMdSubscribe, AxMdSubscribeCandles, AxMdUnsubscribe,
            AxMdUnsubscribeCandles,
        },
        parse::parse_md_message,
    },
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocket client for this handler.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Replay all subscriptions after a reconnection.
    ReplaySubscriptions,
    /// Subscribe to market data for a symbol.
    Subscribe {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Market data subscription options.
        spec: AxMdSubscriptionSpec,
    },
    /// Unsubscribe from market data for a symbol.
    Unsubscribe {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Subscription topic for state tracking.
        topic: String,
    },
    /// Subscribe to candle data for a symbol.
    SubscribeCandles {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Candle width/interval.
        width: AxCandleWidth,
    },
    /// Unsubscribe from candle data for a symbol.
    UnsubscribeCandles {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
        /// Candle width/interval.
        width: AxCandleWidth,
        /// Subscription topic for state tracking.
        topic: String,
    },
}

/// Market data feed handler that processes WebSocket messages.
///
/// Runs in a dedicated Tokio task and owns the WebSocket client exclusively.
/// Emits raw venue types for downstream consumers to parse.
pub(crate) struct AxMdWsFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    message_queue: VecDeque<AxDataWsMessage>,
    replay_request_id: i64,
    needs_subscription_replay: bool,
    pending_subscription_requests: AHashMap<i64, PendingSubscriptionRequest>,
}

impl AxMdWsFeedHandler {
    /// Creates a new [`AxMdWsFeedHandler`] instance.
    #[must_use]
    pub(crate) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            message_queue: VecDeque::new(),
            replay_request_id: -1,
            needs_subscription_replay: false,
            pending_subscription_requests: AHashMap::new(),
        }
    }

    fn next_replay_request_id(&mut self) -> i64 {
        self.replay_request_id -= 1;
        self.replay_request_id
    }

    async fn replay_subscriptions(&mut self) {
        self.pending_subscription_requests.clear();
        let topics = self.subscriptions.reset_after_reconnect();
        if topics.is_empty() {
            log::debug!("No subscriptions to replay after reconnect");
            return;
        }

        log::debug!("Replaying {} subscriptions after reconnect", topics.len());

        for topic in topics {
            // Topic format: "symbol:Level:trades:ticker" or "candles:symbol:Width"
            if let Some(rest) = topic.strip_prefix("candles:") {
                if let Some((symbol, width_str)) = rest.rsplit_once(':') {
                    if let Some(width) = Self::parse_candle_width(width_str) {
                        let request_id = self.next_replay_request_id();
                        log::debug!(
                            "Replaying candle subscription: symbol={symbol}, width={width:?}"
                        );
                        self.pending_subscription_requests.insert(
                            request_id,
                            PendingSubscriptionRequest::Subscribe(topic.clone()),
                        );
                        self.send_subscribe_candles(request_id, Ustr::from(symbol), width)
                            .await;
                    } else {
                        log::warn!("Failed to parse candle width from topic: {topic}");
                    }
                } else {
                    log::warn!("Invalid candle topic format: {topic}");
                }
            } else if let Some((symbol, spec)) = AxMdSubscriptionSpec::parse_topic(&topic) {
                let request_id = self.next_replay_request_id();
                log::debug!("Replaying market data subscription: symbol={symbol}, spec={spec:?}");
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));
                self.send_subscribe(request_id, symbol, spec).await;
            } else {
                log::warn!("Failed to parse market data subscription topic: {topic}");
            }
        }

        log::debug!("Subscription replay completed");
    }

    fn parse_candle_width(s: &str) -> Option<AxCandleWidth> {
        match s {
            "Seconds1" => Some(AxCandleWidth::Seconds1),
            "Seconds5" => Some(AxCandleWidth::Seconds5),
            "Minutes1" => Some(AxCandleWidth::Minutes1),
            "Minutes5" => Some(AxCandleWidth::Minutes5),
            "Minutes15" => Some(AxCandleWidth::Minutes15),
            "Hours1" => Some(AxCandleWidth::Hours1),
            "Days1" => Some(AxCandleWidth::Days1),
            _ => None,
        }
    }

    /// Returns the next message from the handler.
    ///
    /// This method blocks until a message is available or the handler is stopped.
    pub(crate) async fn next(&mut self) -> Option<AxDataWsMessage> {
        loop {
            if self.needs_subscription_replay && self.message_queue.is_empty() {
                self.needs_subscription_replay = false;
                self.replay_subscriptions().await;
            }

            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Acquire) {
                        log::debug!("Stop signal received during idle period");
                        return None;
                    }
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            log::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    if let Message::Ping(data) = &msg {
                        log::trace!("Received ping frame with {} bytes", data.len());

                        if let Some(client) = &self.inner
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: {e}");
                        }
                        continue;
                    }

                    if let Some(message) = self.parse_raw_message(msg) {
                        self.message_queue.push_back(message);
                    }

                    if self.signal.load(Ordering::Acquire) {
                        log::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: HandlerCommand) {
        match cmd {
            HandlerCommand::SetClient(client) => {
                log::debug!("WebSocketClient received by handler");
                self.inner = Some(client);
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnect command received");

                if let Some(inner) = self.inner.take() {
                    inner.disconnect().await;
                }
            }
            HandlerCommand::ReplaySubscriptions => {
                log::debug!("ReplaySubscriptions command received");
                self.replay_subscriptions().await;
            }
            HandlerCommand::Subscribe {
                request_id,
                symbol,
                spec,
            } => {
                log::debug!(
                    "Subscribe command received: request_id={request_id}, symbol={symbol}, spec={spec:?}"
                );
                let topic = spec.topic(symbol.as_str());
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));
                self.send_subscribe(request_id, symbol, spec).await;
            }
            HandlerCommand::Unsubscribe {
                request_id,
                symbol,
                topic,
            } => {
                log::debug!(
                    "Unsubscribe command received: request_id={request_id}, symbol={symbol}"
                );
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Unsubscribe(topic));
                self.send_unsubscribe(request_id, symbol).await;
            }
            HandlerCommand::SubscribeCandles {
                request_id,
                symbol,
                width,
            } => {
                log::debug!(
                    "SubscribeCandles command received: request_id={request_id}, symbol={symbol}, width={width:?}"
                );
                let topic = format!("candles:{symbol}:{width:?}");
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Subscribe(topic));
                self.send_subscribe_candles(request_id, symbol, width).await;
            }
            HandlerCommand::UnsubscribeCandles {
                request_id,
                symbol,
                width,
                topic,
            } => {
                log::debug!(
                    "UnsubscribeCandles command received: request_id={request_id}, symbol={symbol}, width={width:?}"
                );
                self.pending_subscription_requests
                    .insert(request_id, PendingSubscriptionRequest::Unsubscribe(topic));
                self.message_queue
                    .push_back(AxDataWsMessage::CandleUnsubscribed { symbol, width });
                self.send_unsubscribe_candles(request_id, symbol, width)
                    .await;
            }
        }
    }

    async fn send_subscribe(&mut self, request_id: i64, symbol: Ustr, spec: AxMdSubscriptionSpec) {
        let msg = AxMdSubscribe {
            rid: request_id,
            msg_type: AxMdRequestType::Subscribe,
            symbol,
            level: spec.level,
            trades: spec.trades,
            ticker: spec.ticker,
        };

        if let Err(e) = self.send_json(&msg).await {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send subscribe message: {e}");
        }
    }

    async fn send_unsubscribe(&mut self, request_id: i64, symbol: Ustr) {
        let msg = AxMdUnsubscribe {
            rid: request_id,
            msg_type: AxMdRequestType::Unsubscribe,
            symbol,
        };

        if let Err(e) = self.send_json(&msg).await {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send unsubscribe message: {e}");
        }
    }

    async fn send_subscribe_candles(
        &mut self,
        request_id: i64,
        symbol: Ustr,
        width: AxCandleWidth,
    ) {
        let msg = AxMdSubscribeCandles {
            rid: request_id,
            msg_type: AxMdRequestType::SubscribeCandles,
            symbol,
            width,
        };

        if let Err(e) = self.send_json(&msg).await {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send subscribe_candles message: {e}");
        }
    }

    async fn send_unsubscribe_candles(
        &mut self,
        request_id: i64,
        symbol: Ustr,
        width: AxCandleWidth,
    ) {
        let msg = AxMdUnsubscribeCandles {
            rid: request_id,
            msg_type: AxMdRequestType::UnsubscribeCandles,
            symbol,
            width,
        };

        if let Err(e) = self.send_json(&msg).await {
            self.pending_subscription_requests.remove(&request_id);
            log::error!("Failed to send unsubscribe_candles message: {e}");
        }
    }

    async fn send_json<T: serde::Serialize>(&self, msg: &T) -> Result<(), String> {
        let Some(inner) = &self.inner else {
            return Err("No WebSocket client available".to_string());
        };

        let payload = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        log::trace!("Sending WebSocket payload ({} bytes)", payload.len());

        inner
            .send_text(payload, None)
            .await
            .map_err(|e| e.to_string())
    }

    fn parse_raw_message(&mut self, msg: Message) -> Option<AxDataWsMessage> {
        match msg {
            Message::Text(text) => {
                if text == nautilus_network::RECONNECTED {
                    log::info!("Received WebSocket reconnected signal");
                    self.needs_subscription_replay = true;
                    return Some(AxDataWsMessage::Reconnected);
                }

                log::trace!("Raw websocket message: {text}");

                match parse_md_message(&text) {
                    Ok(message) => self.handle_message(message),
                    Err(e) => {
                        log::error!("Failed to parse WebSocket message: {e}: {text}");
                        None
                    }
                }
            }
            Message::Binary(data) => {
                log::debug!("Received binary message with {} bytes", data.len());
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message, waiting for reconnection");
                None
            }
            _ => None,
        }
    }

    fn handle_message(&mut self, message: AxMdMessage) -> Option<AxDataWsMessage> {
        match &message {
            AxMdMessage::Error(error) => {
                let is_benign = error.message.contains("already subscribed")
                    || error.message.contains("not subscribed");

                if let Some(rid) = error.request_id
                    && let Some(request) = self.pending_subscription_requests.remove(&rid)
                {
                    match request {
                        PendingSubscriptionRequest::Subscribe(topic) => {
                            self.subscriptions.mark_failure(&topic);
                        }
                        PendingSubscriptionRequest::Unsubscribe(topic) => {
                            self.subscriptions.confirm_unsubscribe(&topic);
                        }
                    }
                }

                if is_benign {
                    log::warn!("Subscription state: {}", error.message);
                } else {
                    log::error!("Received error from exchange: {}", error.message);
                }
            }
            AxMdMessage::SubscriptionResponse(response) => {
                let is_subscribe = response.result.subscribed.is_some()
                    || response.result.subscribed_candle.is_some();
                let is_unsubscribe = response.result.unsubscribed.is_some()
                    || response.result.unsubscribed_candle.is_some();

                if let Some(request) = self.pending_subscription_requests.remove(&response.rid) {
                    match request {
                        PendingSubscriptionRequest::Subscribe(topic) if is_subscribe => {
                            self.subscriptions.confirm_subscribe(&topic);
                        }
                        PendingSubscriptionRequest::Unsubscribe(topic) if is_unsubscribe => {
                            self.subscriptions.confirm_unsubscribe(&topic);
                        }
                        request => {
                            log::warn!(
                                "Unexpected subscription response for request: {request:?}, \
                                 response={response:?}"
                            );
                        }
                    }
                }

                if let Some(symbol) = &response.result.subscribed {
                    log::debug!("Subscription confirmed for symbol: {symbol}");
                } else if let Some(candle) = &response.result.subscribed_candle {
                    log::debug!("Candle subscription confirmed: {candle}");
                } else if let Some(symbol) = &response.result.unsubscribed {
                    log::debug!("Unsubscription confirmed for symbol: {symbol}");
                } else if let Some(candle) = &response.result.unsubscribed_candle {
                    log::debug!("Candle unsubscription confirmed: {candle}");
                }
                return None;
            }
            _ => {}
        }

        Some(AxDataWsMessage::MdMessage(message))
    }
}

#[derive(Debug)]
enum PendingSubscriptionRequest {
    Subscribe(String),
    Unsubscribe(String),
}

#[cfg(test)]
mod tests {
    use nautilus_network::websocket::SubscriptionState;
    use rstest::rstest;

    use super::*;
    use crate::websocket::messages::{AxMdSubscriptionResponse, AxMdSubscriptionResult, AxWsError};

    const TOPIC: &str = "EURUSD-PERP:Level2:false:false";

    #[rstest]
    fn test_subscription_response_confirms_subscribe() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler
            .pending_subscription_requests
            .insert(1, PendingSubscriptionRequest::Subscribe(TOPIC.to_string()));

        handler.handle_message(AxMdMessage::SubscriptionResponse(
            AxMdSubscriptionResponse {
                rid: 1,
                result: AxMdSubscriptionResult {
                    subscribed: Some("EURUSD-PERP".to_string()),
                    subscribed_candle: None,
                    unsubscribed: None,
                    unsubscribed_candle: None,
                },
            },
        ));

        assert_eq!(subscriptions.len(), 1);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    fn test_subscription_error_keeps_topic_pending_for_replay() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler
            .pending_subscription_requests
            .insert(2, PendingSubscriptionRequest::Subscribe(TOPIC.to_string()));

        handler.handle_message(AxMdMessage::Error(AxWsError {
            code: Some("400".to_string()),
            message: "subscription failed".to_string(),
            request_id: Some(2),
        }));

        assert_eq!(subscriptions.len(), 0);
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![TOPIC.to_string()]
        );
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    fn test_already_subscribed_keeps_topic_pending_for_replay() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler
            .pending_subscription_requests
            .insert(3, PendingSubscriptionRequest::Subscribe(TOPIC.to_string()));

        handler.handle_message(AxMdMessage::Error(AxWsError {
            code: Some("400".to_string()),
            message: "already subscribed".to_string(),
            request_id: Some(3),
        }));

        assert_eq!(subscriptions.len(), 0);
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            vec![TOPIC.to_string()]
        );
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    #[case("not subscribed")]
    #[case("subscription failed")]
    fn test_unsubscribe_error_confirms_removal(#[case] message: &str) {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        subscriptions.mark_unsubscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler.pending_subscription_requests.insert(
            4,
            PendingSubscriptionRequest::Unsubscribe(TOPIC.to_string()),
        );

        handler.handle_message(AxMdMessage::Error(AxWsError {
            code: Some("400".to_string()),
            message: message.to_string(),
            request_id: Some(4),
        }));

        assert_eq!(subscriptions.len(), 0);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    fn test_subscription_response_confirms_unsubscribe() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        subscriptions.mark_unsubscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler.pending_subscription_requests.insert(
            5,
            PendingSubscriptionRequest::Unsubscribe(TOPIC.to_string()),
        );

        handler.handle_message(AxMdMessage::SubscriptionResponse(
            AxMdSubscriptionResponse {
                rid: 5,
                result: AxMdSubscriptionResult {
                    subscribed: None,
                    subscribed_candle: None,
                    unsubscribed: Some("EURUSD-PERP".to_string()),
                    unsubscribed_candle: None,
                },
            },
        ));

        assert_eq!(subscriptions.len(), 0);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_replay_clears_stale_requests_and_pending_unsubscribes() {
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe(TOPIC);
        subscriptions.confirm_subscribe(TOPIC);
        subscriptions.mark_unsubscribe(TOPIC);
        let mut handler = create_handler(subscriptions.clone());
        handler.pending_subscription_requests.insert(
            6,
            PendingSubscriptionRequest::Unsubscribe(TOPIC.to_string()),
        );

        handler.replay_subscriptions().await;

        assert_eq!(subscriptions.len(), 0);
        assert!(subscriptions.all_topics().is_empty());
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert!(handler.pending_subscription_requests.is_empty());
    }

    fn create_handler(subscriptions: SubscriptionState) -> AxMdWsFeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        AxMdWsFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            subscriptions,
        )
    }
}
