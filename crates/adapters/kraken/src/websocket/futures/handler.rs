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

//! WebSocket message handler for Kraken Futures.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use serde::Deserialize;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::messages::{
    KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesChannel,
    KrakenFuturesFillsDelta, KrakenFuturesMessageType, KrakenFuturesOpenOrdersCancel,
    KrakenFuturesOpenOrdersDelta, KrakenFuturesTickerData, KrakenFuturesTradeData,
    KrakenFuturesWsMessage, classify_futures_message,
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum FuturesHandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Subscribe { payload: String },
    Unsubscribe { payload: String },
    RequestChallenge { payload: String },
}

/// WebSocket message handler for Kraken Futures.
pub struct FuturesFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<FuturesHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    pending_messages: VecDeque<KrakenFuturesWsMessage>,
}

impl FuturesFeedHandler {
    /// Creates a new [`FuturesFeedHandler`] instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<FuturesHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            pending_messages: VecDeque::new(),
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn is_subscribed(&self, channel: KrakenFuturesChannel, symbol: &Ustr) -> bool {
        let channel_ustr = Ustr::from(channel.as_ref());
        self.subscriptions.is_subscribed(&channel_ustr, symbol)
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub async fn next(&mut self) -> Option<KrakenFuturesWsMessage> {
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        FuturesHandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by futures handler");
                            self.inner = Some(client);
                        }
                        FuturesHandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");

                            if let Some(client) = self.inner.take() {
                                client.disconnect().await;
                            }
                            return None;
                        }
                        FuturesHandlerCommand::Subscribe { payload }
                        | FuturesHandlerCommand::Unsubscribe { payload }
                        | FuturesHandlerCommand::RequestChallenge { payload } => {
                            if let Some(ref client) = self.inner
                                && let Err(e) = client.send_text(payload, None).await
                            {
                                log::error!("Failed to send text: {e}");
                            }
                        }
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

                    if self.signal.load(Ordering::Relaxed) {
                        log::debug!("Stop signal received");
                        return None;
                    }

                    match &msg {
                        Message::Ping(data) => {
                            let len = data.len();
                            log::trace!("Received ping frame with {len} bytes");

                            if let Some(client) = &self.inner
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                log::warn!("Failed to send pong frame: {e}");
                            }
                            continue;
                        }
                        Message::Pong(_) => {
                            log::debug!("Received pong from server");
                            continue;
                        }
                        Message::Close(_) => {
                            log::info!("WebSocket connection closed");
                            return None;
                        }
                        Message::Frame(_) => {
                            log::trace!("Received raw frame");
                            continue;
                        }
                        _ => {}
                    }

                    let text: &str = match &msg {
                        Message::Text(text) => text,
                        Message::Binary(data) => match std::str::from_utf8(data) {
                            Ok(s) => s,
                            Err(_) => continue,
                        },
                        _ => continue,
                    };

                    if text == RECONNECTED {
                        log::info!("Received WebSocket reconnected signal");
                        return Some(KrakenFuturesWsMessage::Reconnected);
                    }

                    self.parse_message(text);

                    if let Some(msg) = self.pending_messages.pop_front() {
                        return Some(msg);
                    }
                }
            }
        }
    }

    fn parse_message(&mut self, text: &str) {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("Failed to parse message as JSON: {e}");
                return;
            }
        };

        match classify_futures_message(&value) {
            KrakenFuturesMessageType::OpenOrdersSnapshot => {
                log::debug!(
                    "Skipping open_orders_snapshot (REST reconciliation handles initial state)"
                );
            }
            KrakenFuturesMessageType::OpenOrdersCancel => {
                self.handle_open_orders_cancel_value(value);
            }
            KrakenFuturesMessageType::OpenOrdersDelta => {
                self.handle_open_orders_delta_value(value);
            }
            KrakenFuturesMessageType::FillsSnapshot => {
                log::debug!("Skipping fills_snapshot (REST reconciliation handles initial state)");
            }
            KrakenFuturesMessageType::FillsDelta => {
                self.handle_fills_delta_value(value);
            }
            KrakenFuturesMessageType::Ticker => {
                self.handle_ticker_message_value(value);
            }
            KrakenFuturesMessageType::TradeSnapshot => {
                log::debug!("Skipping trade_snapshot (only streaming live trades)");
            }
            KrakenFuturesMessageType::Trade => {
                self.handle_trade_message_value(value);
            }
            KrakenFuturesMessageType::BookSnapshot => {
                self.handle_book_snapshot_value(value);
            }
            KrakenFuturesMessageType::BookDelta => {
                self.handle_book_delta_value(value);
            }
            KrakenFuturesMessageType::Info => {
                log::debug!("Received info message: {text}");
            }
            KrakenFuturesMessageType::Pong => {
                log::debug!("Received text pong response");
            }
            KrakenFuturesMessageType::Subscribed => {
                log::debug!("Subscription confirmed: {text}");
            }
            KrakenFuturesMessageType::Unsubscribed => {
                log::debug!("Unsubscription confirmed: {text}");
            }
            KrakenFuturesMessageType::Challenge => {
                self.handle_challenge_response_value(value);
            }
            KrakenFuturesMessageType::Heartbeat => {
                log::trace!("Heartbeat received");
            }
            KrakenFuturesMessageType::Error => {
                let message = value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                log::error!("Kraken Futures WebSocket error: {message}");
            }
            KrakenFuturesMessageType::Alert => {
                let message = value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown alert");
                log::warn!("Kraken Futures WebSocket alert: {message}");
            }
            KrakenFuturesMessageType::Unknown => {
                log::warn!("Unhandled futures message: {text}");
            }
        }
    }

    fn handle_challenge_response_value(&mut self, value: Value) {
        #[derive(Deserialize)]
        struct ChallengeResponse {
            message: String,
        }

        match serde_json::from_value::<ChallengeResponse>(value) {
            Ok(response) => {
                let len = response.message.len();
                log::debug!("Challenge received, length: {len}");

                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::Challenge(response.message));
            }
            Err(e) => {
                log::error!("Failed to parse challenge response: {e}");
            }
        }
    }

    fn handle_ticker_message_value(&mut self, value: Value) {
        let ticker = match serde_json::from_value::<KrakenFuturesTickerData>(value) {
            Ok(t) => t,
            Err(e) => {
                log::debug!("Failed to parse ticker: {e}");
                return;
            }
        };

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::Ticker(ticker));
    }

    fn handle_trade_message_value(&mut self, value: Value) {
        let trade = match serde_json::from_value::<KrakenFuturesTradeData>(value) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("Failed to parse trade: {e}");
                return;
            }
        };

        if !self.is_subscribed(KrakenFuturesChannel::Trades, &trade.product_id) {
            log::debug!(
                "Received trade for unsubscribed product: {}",
                trade.product_id
            );
            return;
        }

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::Trade(trade));
    }

    fn handle_book_snapshot_value(&mut self, value: Value) {
        let snapshot = match serde_json::from_value::<KrakenFuturesBookSnapshot>(value) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Failed to parse book snapshot: {e}");
                return;
            }
        };

        let has_book = self.is_subscribed(KrakenFuturesChannel::Book, &snapshot.product_id);
        let has_quotes = self.is_subscribed(KrakenFuturesChannel::Quotes, &snapshot.product_id);

        if !has_book && !has_quotes {
            log::debug!(
                "Received book snapshot for unsubscribed product: {}",
                snapshot.product_id
            );
            return;
        }

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::BookSnapshot(snapshot));
    }

    fn handle_book_delta_value(&mut self, value: Value) {
        let delta = match serde_json::from_value::<KrakenFuturesBookDelta>(value) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to parse book delta: {e}");
                return;
            }
        };

        let has_book = self.is_subscribed(KrakenFuturesChannel::Book, &delta.product_id);
        let has_quotes = self.is_subscribed(KrakenFuturesChannel::Quotes, &delta.product_id);

        if !has_book && !has_quotes {
            log::debug!(
                "Received book delta for unsubscribed product: {}",
                delta.product_id
            );
            return;
        }

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::BookDelta(delta));
    }

    fn handle_open_orders_delta_value(&mut self, value: Value) {
        let delta = match serde_json::from_value::<KrakenFuturesOpenOrdersDelta>(value) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to parse open_orders delta: {e}");
                return;
            }
        };

        log::debug!(
            "Received open_orders delta: order_id={}, is_cancel={}, reason={:?}",
            delta.order.order_id,
            delta.is_cancel,
            delta.reason
        );

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::OpenOrdersDelta(delta));
    }

    fn handle_open_orders_cancel_value(&mut self, value: Value) {
        let cancel = match serde_json::from_value::<KrakenFuturesOpenOrdersCancel>(value) {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to parse open_orders cancel: {e}");
                return;
            }
        };

        log::debug!(
            "Received open_orders cancel: order_id={}, cli_ord_id={:?}, reason={:?}",
            cancel.order_id,
            cancel.cli_ord_id,
            cancel.reason
        );

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::OpenOrdersCancel(cancel));
    }

    fn handle_fills_delta_value(&mut self, value: Value) {
        let delta = match serde_json::from_value::<KrakenFuturesFillsDelta>(value) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to parse fills delta: {e}");
                return;
            }
        };

        log::debug!("Received fills delta: fill_count={}", delta.fills.len());

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::FillsDelta(delta));
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_test_handler() -> FuturesFeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');

        FuturesFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions)
    }

    #[rstest]
    fn test_parse_ticker_emits_ticker_message() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_ticker.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::Ticker(ticker) = msg else {
            panic!("Expected Ticker message, was {msg:?}");
        };
        assert_eq!(ticker.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(ticker.bid, Some(21978.5));
        assert_eq!(ticker.ask, Some(21987.0));
    }

    #[rstest]
    fn test_parse_trade_emits_trade_message() {
        let mut handler = create_test_handler();
        handler.subscriptions.mark_subscribe("trades:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("trades:PI_XBTUSD");

        let json = include_str!("../../../test_data/ws_futures_trade.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::Trade(trade) = msg else {
            panic!("Expected Trade message, was {msg:?}");
        };
        assert_eq!(trade.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(trade.price, 34969.5);
        assert_eq!(trade.qty, 15000.0);
    }

    #[rstest]
    fn test_parse_trade_filters_unsubscribed() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_trade.json");

        handler.parse_message(json);

        assert!(
            handler.pending_messages.is_empty(),
            "Trade for unsubscribed product should be filtered"
        );
    }

    #[rstest]
    fn test_parse_book_snapshot_emits_book_snapshot() {
        let mut handler = create_test_handler();
        handler.subscriptions.mark_subscribe("book:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("book:PI_XBTUSD");

        let json = include_str!("../../../test_data/ws_futures_book_snapshot.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::BookSnapshot(snapshot) = msg else {
            panic!("Expected BookSnapshot message, was {msg:?}");
        };
        assert_eq!(snapshot.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(snapshot.bids.len(), 2);
        assert_eq!(snapshot.asks.len(), 2);
    }

    #[rstest]
    fn test_parse_book_snapshot_filters_unsubscribed() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_book_snapshot.json");

        handler.parse_message(json);

        assert!(
            handler.pending_messages.is_empty(),
            "Book snapshot for unsubscribed product should be filtered"
        );
    }

    #[rstest]
    fn test_parse_book_delta_emits_book_delta() {
        let mut handler = create_test_handler();
        handler.subscriptions.mark_subscribe("book:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("book:PI_XBTUSD");

        let json = include_str!("../../../test_data/ws_futures_book_delta.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::BookDelta(delta) = msg else {
            panic!("Expected BookDelta message, was {msg:?}");
        };
        assert_eq!(delta.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(delta.price, 34981.0);
    }

    #[rstest]
    fn test_parse_book_delta_filters_unsubscribed() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_book_delta.json");

        handler.parse_message(json);

        assert!(
            handler.pending_messages.is_empty(),
            "Book delta for unsubscribed product should be filtered"
        );
    }

    #[rstest]
    fn test_parse_open_orders_cancel_emits_cancel() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_open_orders_cancel.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::OpenOrdersCancel(cancel) = msg else {
            panic!("Expected OpenOrdersCancel message, was {msg:?}");
        };
        assert_eq!(cancel.order_id, "660c6b23-8007-48c1-a7c9-4893f4572e8c");
        assert!(cancel.is_cancel);
    }

    #[rstest]
    fn test_parse_open_orders_delta_emits_delta() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_open_orders_delta.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::OpenOrdersDelta(delta) = msg else {
            panic!("Expected OpenOrdersDelta message, was {msg:?}");
        };
        assert_eq!(delta.order.instrument, Ustr::from("PI_XBTUSD"));
        assert!(!delta.is_cancel);
    }

    #[rstest]
    fn test_parse_fills_delta_emits_fills() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_fills_delta.json");

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::FillsDelta(fills) = msg else {
            panic!("Expected FillsDelta message, was {msg:?}");
        };
        assert_eq!(fills.fills.len(), 1);
        assert_eq!(
            fills.fills[0].fill_id,
            "6a22a3fb-e18e-4e76-b841-8689735c9158"
        );
    }

    #[rstest]
    fn test_parse_challenge_emits_challenge_message() {
        let mut handler = create_test_handler();
        let json = r#"{"event":"challenge","message":"server-challenge-abc"}"#;

        handler.parse_message(json);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::Challenge(challenge) = msg else {
            panic!("Expected Challenge message, was {msg:?}");
        };
        assert_eq!(challenge, "server-challenge-abc");
    }

    #[rstest]
    fn test_heartbeat_produces_no_message() {
        let mut handler = create_test_handler();
        let json = r#"{"feed":"heartbeat","time":1700000000000}"#;

        handler.parse_message(json);

        assert!(handler.pending_messages.is_empty());
    }

    #[rstest]
    fn test_info_event_produces_no_message() {
        let mut handler = create_test_handler();
        let json = r#"{"event":"info","version":1}"#;

        handler.parse_message(json);

        assert!(handler.pending_messages.is_empty());
    }
}
