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

use crate::{
    common::enums::{AxCandleWidth, AxMarketDataLevel, AxMdRequestType},
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
        /// Market data level.
        level: AxMarketDataLevel,
    },
    /// Unsubscribe from market data for a symbol.
    Unsubscribe {
        /// Request ID for correlation.
        request_id: i64,
        /// Instrument symbol.
        symbol: Ustr,
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
    },
}

/// Market data feed handler that processes WebSocket messages.
///
/// Runs in a dedicated Tokio task and owns the WebSocket client exclusively.
/// Emits raw venue types for downstream consumers to parse.
pub(crate) struct FeedHandler {
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    message_queue: VecDeque<AxDataWsMessage>,
    replay_request_id: i64,
    needs_subscription_replay: bool,
    pending_subscribe_requests: AHashMap<i64, String>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[must_use]
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            message_queue: VecDeque::new(),
            replay_request_id: -1,
            needs_subscription_replay: false,
            pending_subscribe_requests: AHashMap::new(),
        }
    }

    fn next_replay_request_id(&mut self) -> i64 {
        self.replay_request_id -= 1;
        self.replay_request_id
    }

    async fn replay_subscriptions(&mut self) {
        let topics = self.subscriptions.all_topics();
        if topics.is_empty() {
            log::debug!("No subscriptions to replay after reconnect");
            return;
        }

        log::info!("Replaying {} subscriptions after reconnect", topics.len());

        for topic in topics {
            self.subscriptions.mark_subscribe(&topic);

            // Topic format: "symbol:Level" or "candles:symbol:Width"
            if let Some(rest) = topic.strip_prefix("candles:") {
                if let Some((symbol, width_str)) = rest.rsplit_once(':') {
                    if let Some(width) = Self::parse_candle_width(width_str) {
                        let request_id = self.next_replay_request_id();
                        log::debug!(
                            "Replaying candle subscription: symbol={symbol}, width={width:?}"
                        );
                        self.send_subscribe_candles(request_id, Ustr::from(symbol), width)
                            .await;
                    } else {
                        log::warn!("Failed to parse candle width from topic: {topic}");
                    }
                } else {
                    log::warn!("Invalid candle topic format: {topic}");
                }
            } else if let Some((symbol, level_str)) = topic.rsplit_once(':') {
                if let Some(level) = Self::parse_market_data_level(level_str) {
                    let request_id = self.next_replay_request_id();
                    log::debug!(
                        "Replaying market data subscription: symbol={symbol}, level={level:?}"
                    );
                    self.send_subscribe(request_id, Ustr::from(symbol), level)
                        .await;
                } else {
                    log::warn!("Failed to parse market data level from topic: {topic}");
                }
            } else {
                log::warn!("Unknown topic format: {topic}");
            }
        }

        log::info!("Subscription replay completed");
    }

    fn parse_market_data_level(s: &str) -> Option<AxMarketDataLevel> {
        match s {
            "Level1" => Some(AxMarketDataLevel::Level1),
            "Level2" => Some(AxMarketDataLevel::Level2),
            "Level3" => Some(AxMarketDataLevel::Level3),
            _ => None,
        }
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
    pub async fn next(&mut self) -> Option<AxDataWsMessage> {
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

                        if let Some(client) = &self.client
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
                self.client = Some(client);
            }
            HandlerCommand::Disconnect => {
                log::debug!("Disconnect command received");

                if let Some(client) = self.client.take() {
                    client.disconnect().await;
                }
            }
            HandlerCommand::ReplaySubscriptions => {
                log::debug!("ReplaySubscriptions command received");
                self.replay_subscriptions().await;
            }
            HandlerCommand::Subscribe {
                request_id,
                symbol,
                level,
            } => {
                log::debug!(
                    "Subscribe command received: request_id={request_id}, symbol={symbol}, level={level:?}"
                );
                let topic = format!("{symbol}:{level:?}");
                self.pending_subscribe_requests.insert(request_id, topic);
                self.send_subscribe(request_id, symbol, level).await;
            }
            HandlerCommand::Unsubscribe { request_id, symbol } => {
                log::debug!(
                    "Unsubscribe command received: request_id={request_id}, symbol={symbol}"
                );
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
                self.pending_subscribe_requests.insert(request_id, topic);
                self.send_subscribe_candles(request_id, symbol, width).await;
            }
            HandlerCommand::UnsubscribeCandles {
                request_id,
                symbol,
                width,
            } => {
                log::debug!(
                    "UnsubscribeCandles command received: request_id={request_id}, symbol={symbol}, width={width:?}"
                );
                self.message_queue
                    .push_back(AxDataWsMessage::CandleUnsubscribed { symbol, width });
                self.send_unsubscribe_candles(request_id, symbol, width)
                    .await;
            }
        }
    }

    async fn send_subscribe(&mut self, request_id: i64, symbol: Ustr, level: AxMarketDataLevel) {
        let msg = AxMdSubscribe {
            rid: request_id,
            msg_type: AxMdRequestType::Subscribe,
            symbol,
            level,
        };

        if let Err(e) = self.send_json(&msg).await {
            self.pending_subscribe_requests.remove(&request_id);
            log::error!("Failed to send subscribe message: {e}");
        }
    }

    async fn send_unsubscribe(&self, request_id: i64, symbol: Ustr) {
        let msg = AxMdUnsubscribe {
            rid: request_id,
            msg_type: AxMdRequestType::Unsubscribe,
            symbol,
        };

        if let Err(e) = self.send_json(&msg).await {
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
            self.pending_subscribe_requests.remove(&request_id);
            log::error!("Failed to send subscribe_candles message: {e}");
        }
    }

    async fn send_unsubscribe_candles(&self, request_id: i64, symbol: Ustr, width: AxCandleWidth) {
        let msg = AxMdUnsubscribeCandles {
            rid: request_id,
            msg_type: AxMdRequestType::UnsubscribeCandles,
            symbol,
            width,
        };

        if let Err(e) = self.send_json(&msg).await {
            log::error!("Failed to send unsubscribe_candles message: {e}");
        }
    }

    async fn send_json<T: serde::Serialize>(&self, msg: &T) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Err("No WebSocket client available".to_string());
        };

        let payload = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        log::trace!("Sending: {payload}");

        client
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

                if is_benign {
                    if let Some(rid) = error.request_id {
                        self.pending_subscribe_requests.remove(&rid);
                    }
                    log::warn!("Subscription state: {}", error.message);
                } else {
                    if let Some(rid) = error.request_id
                        && let Some(topic) = self.pending_subscribe_requests.remove(&rid)
                    {
                        log::warn!(
                            "Rolling back subscription for topic '{topic}' \
                             due to error: {}",
                            error.message
                        );
                        self.subscriptions.mark_unsubscribe(&topic);
                    }
                    log::error!("Received error from exchange: {}", error.message);
                }
            }
            AxMdMessage::SubscriptionResponse(response) => {
                self.pending_subscribe_requests.remove(&response.rid);

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
