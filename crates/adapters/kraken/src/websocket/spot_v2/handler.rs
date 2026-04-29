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

//! WebSocket message handler for Kraken Spot v2.

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
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::{
    enums::{KrakenWsChannel, KrakenWsMessageType},
    messages::{
        KrakenSpotWsMessage, KrakenWsBookData, KrakenWsExecutionData, KrakenWsMessage,
        KrakenWsOhlcData, KrakenWsResponse, KrakenWsTickerData, KrakenWsTradeData,
    },
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum SpotHandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Subscribe { payload: String },
    Unsubscribe { payload: String },
    Ping { payload: String },
}

/// WebSocket message handler for Kraken Spot v2.
pub(super) struct SpotFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    pending_messages: VecDeque<KrakenSpotWsMessage>,
}

impl SpotFeedHandler {
    /// Creates a new [`SpotFeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
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

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn is_subscribed(&self, topic: &str) -> bool {
        self.subscriptions.all_topics().iter().any(|t| t == topic)
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub(super) async fn next(&mut self) -> Option<KrakenSpotWsMessage> {
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        SpotHandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by handler");
                            self.inner = Some(client);
                        }
                        SpotHandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");

                            if let Some(client) = self.inner.take() {
                                client.disconnect().await;
                            }
                        }
                        SpotHandlerCommand::Subscribe { payload }
                        | SpotHandlerCommand::Unsubscribe { payload }
                        | SpotHandlerCommand::Ping { payload } => {
                            if let Some(client) = &self.inner
                                && let Err(e) = client.send_text(payload.clone(), None).await
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

                    if let Message::Ping(data) = &msg {
                        log::trace!("Received ping frame with {} bytes", data.len());

                        if let Some(client) = &self.inner
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: {e}");
                        }
                        continue;
                    }

                    if self.signal.load(Ordering::Relaxed) {
                        log::debug!("Stop signal received");
                        return None;
                    }

                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(data) => {
                            match String::from_utf8(data.to_vec()) {
                                Ok(text) => text,
                                Err(e) => {
                                    log::warn!("Failed to decode binary message: {e}");
                                    continue;
                                }
                            }
                        }
                        Message::Pong(_) => {
                            log::trace!("Received pong");
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
                        _ => continue,
                    };

                    if text == RECONNECTED {
                        log::info!("Received WebSocket reconnected signal");
                        return Some(KrakenSpotWsMessage::Reconnected);
                    }

                    if let Some(msg) = self.parse_message(&text) {
                        return Some(msg);
                    }
                }
            }
        }
    }

    fn parse_message(&self, text: &str) -> Option<KrakenSpotWsMessage> {
        // Fast pre-filter for high-frequency control messages (no JSON parsing)
        if text.len() < 50 && text.starts_with("{\"channel\":\"") {
            if text.contains("heartbeat") {
                log::trace!("Received heartbeat");
                return None;
            }

            if text.contains("status") {
                log::debug!("Received status message");
                return None;
            }
        }

        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to parse message: {e}");
                return None;
            }
        };

        // Control messages have "method" field
        if value.get("method").is_some() {
            self.handle_control_message(value);
            return None;
        }

        // Data messages have "channel" and "data" fields
        if value.get("channel").is_some() && value.get("data").is_some() {
            match serde_json::from_value::<KrakenWsMessage>(value) {
                Ok(msg) => return self.handle_data_message(msg),
                Err(e) => {
                    log::debug!("Failed to parse data message: {e}");
                    return None;
                }
            }
        }

        log::debug!("Unhandled message structure: {text}");
        None
    }

    fn handle_control_message(&self, value: Value) {
        match serde_json::from_value::<KrakenWsResponse>(value) {
            Ok(response) => match response {
                KrakenWsResponse::Subscribe(sub) => {
                    if sub.success {
                        if let Some(result) = &sub.result {
                            log::debug!(
                                "Subscription confirmed: channel={:?}, req_id={:?}",
                                result.channel,
                                sub.req_id
                            );
                        } else {
                            log::debug!("Subscription confirmed: req_id={:?}", sub.req_id);
                        }
                    } else {
                        log::warn!(
                            "Subscription failed: error={:?}, req_id={:?}",
                            sub.error,
                            sub.req_id
                        );
                    }
                }
                KrakenWsResponse::Unsubscribe(unsub) => {
                    if unsub.success {
                        log::debug!("Unsubscription confirmed: req_id={:?}", unsub.req_id);
                    } else {
                        log::warn!(
                            "Unsubscription failed: error={:?}, req_id={:?}",
                            unsub.error,
                            unsub.req_id
                        );
                    }
                }
                KrakenWsResponse::Pong(pong) => {
                    log::trace!("Received pong: req_id={:?}", pong.req_id);
                }
                KrakenWsResponse::Other => {
                    log::debug!("Received unknown control response");
                }
            },
            Err(_) => {
                log::debug!("Received control message (failed to parse details)");
            }
        }
    }

    fn handle_data_message(&self, msg: KrakenWsMessage) -> Option<KrakenSpotWsMessage> {
        match msg.channel {
            KrakenWsChannel::Book => self.handle_book_message(msg),
            KrakenWsChannel::Ticker => self.handle_ticker_message(msg),
            KrakenWsChannel::Trade => self.handle_trade_message(msg),
            KrakenWsChannel::Ohlc => self.handle_ohlc_message(msg),
            KrakenWsChannel::Executions => self.handle_executions_message(msg),
            _ => {
                log::warn!("Unhandled channel: {:?}", msg.channel);
                None
            }
        }
    }

    fn handle_book_message(&self, msg: KrakenWsMessage) -> Option<KrakenSpotWsMessage> {
        let is_snapshot = msg.event_type == KrakenWsMessageType::Snapshot;
        let mut book_data = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsBookData>(data) {
                Ok(bd) => {
                    if !self.is_subscribed(&format!("book:{}", bd.symbol)) {
                        continue;
                    }
                    book_data.push(bd);
                }
                Err(e) => log::error!("Failed to deserialize book data: {e}"),
            }
        }

        if book_data.is_empty() {
            None
        } else {
            Some(KrakenSpotWsMessage::Book {
                data: book_data,
                is_snapshot,
            })
        }
    }

    fn handle_ticker_message(&self, msg: KrakenWsMessage) -> Option<KrakenSpotWsMessage> {
        let mut tickers = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsTickerData>(data) {
                Ok(td) => {
                    let symbol = &td.symbol;
                    let quotes_key = format!("quotes:{symbol}");
                    let ticker_key = format!("ticker:{symbol}");
                    if !self.is_subscribed(&quotes_key) && !self.is_subscribed(&ticker_key) {
                        continue;
                    }
                    tickers.push(td);
                }
                Err(e) => log::error!("Failed to deserialize ticker data: {e}"),
            }
        }

        if tickers.is_empty() {
            None
        } else {
            Some(KrakenSpotWsMessage::Ticker(tickers))
        }
    }

    fn handle_trade_message(&self, msg: KrakenWsMessage) -> Option<KrakenSpotWsMessage> {
        let mut trades = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsTradeData>(data) {
                Ok(td) => trades.push(td),
                Err(e) => log::error!("Failed to deserialize trade data: {e}"),
            }
        }

        if trades.is_empty() {
            None
        } else {
            Some(KrakenSpotWsMessage::Trade(trades))
        }
    }

    fn handle_ohlc_message(&self, msg: KrakenWsMessage) -> Option<KrakenSpotWsMessage> {
        let mut ohlc_data = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsOhlcData>(data) {
                Ok(od) => ohlc_data.push(od),
                Err(e) => log::error!("Failed to deserialize OHLC data: {e}"),
            }
        }

        if ohlc_data.is_empty() {
            None
        } else {
            Some(KrakenSpotWsMessage::Ohlc(ohlc_data))
        }
    }

    fn handle_executions_message(&self, msg: KrakenWsMessage) -> Option<KrakenSpotWsMessage> {
        let mut executions = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsExecutionData>(data) {
                Ok(ed) => executions.push(ed),
                Err(e) => log::error!("Failed to deserialize execution data: {e}"),
            }
        }

        if executions.is_empty() {
            None
        } else {
            Some(KrakenSpotWsMessage::Execution(executions))
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_test_handler() -> SpotFeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');

        SpotFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions)
    }

    #[rstest]
    fn test_ticker_message_filtered_without_quotes_subscription() {
        let handler = create_test_handler();

        let json = r#"{
            "channel": "ticker",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bid": 105944.20,
                "bid_qty": 2.5,
                "ask": 105944.30,
                "ask_qty": 3.2,
                "last": 105899.40,
                "volume": 163.28908096,
                "vwap": 105904.39279,
                "low": 104711.00,
                "high": 106613.10,
                "change": 250.00,
                "change_pct": 0.24,
                "timestamp": "2022-12-25T09:30:59.123456Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(
            result.is_none(),
            "Ticker message should be filtered when no quotes subscription exists"
        );
    }

    #[rstest]
    fn test_ticker_message_passes_with_quotes_subscription() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("quotes:BTC/USD");
        handler.subscriptions.confirm_subscribe("quotes:BTC/USD");

        let json = r#"{
            "channel": "ticker",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bid": 105944.20,
                "bid_qty": 2.5,
                "ask": 105944.30,
                "ask_qty": 3.2,
                "last": 105899.40,
                "volume": 163.28908096,
                "vwap": 105904.39279,
                "low": 104711.00,
                "high": 106613.10,
                "change": 250.00,
                "change_pct": 0.24,
                "timestamp": "2022-12-25T09:30:59.123456Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(
            result.is_some(),
            "Ticker message should pass with quotes subscription"
        );

        match result.unwrap() {
            KrakenSpotWsMessage::Ticker(data) => {
                assert!(!data.is_empty(), "Should have ticker data");
            }
            _ => panic!("Expected Ticker message"),
        }
    }

    #[rstest]
    fn test_ticker_message_passes_with_ticker_subscription() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("ticker:BTC/USD");
        handler.subscriptions.confirm_subscribe("ticker:BTC/USD");

        let json = r#"{
            "channel": "ticker",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bid": 105944.20,
                "bid_qty": 2.5,
                "ask": 105944.30,
                "ask_qty": 3.2,
                "last": 105899.40,
                "volume": 163.28908096,
                "vwap": 105904.39279,
                "low": 104711.00,
                "high": 106613.10,
                "change": 250.00,
                "change_pct": 0.24,
                "timestamp": "2022-12-25T09:30:59.123456Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(
            result.is_some(),
            "Ticker message should pass with ticker: subscription"
        );

        match result.unwrap() {
            KrakenSpotWsMessage::Ticker(data) => {
                assert!(!data.is_empty(), "Should have ticker data");
            }
            _ => panic!("Expected Ticker message"),
        }
    }

    #[rstest]
    fn test_book_message_filtered_without_book_subscription() {
        let handler = create_test_handler();

        let json = r#"{
            "channel": "book",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [{"price": 105944.20, "qty": 2.5}],
                "asks": [{"price": 105944.30, "qty": 3.2}],
                "checksum": 12345,
                "timestamp": "2023-10-06T17:35:55.440295Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(
            result.is_none(),
            "Book message should be filtered when no book subscription exists"
        );
    }

    #[rstest]
    fn test_book_message_passes_with_book_subscription() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("book:BTC/USD");
        handler.subscriptions.confirm_subscribe("book:BTC/USD");

        let json = r#"{
            "channel": "book",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [{"price": 105944.20, "qty": 2.5}],
                "asks": [{"price": 105944.30, "qty": 3.2}],
                "checksum": 12345,
                "timestamp": "2023-10-06T17:35:55.440295Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(
            result.is_some(),
            "Book message should pass with book subscription"
        );

        match result.unwrap() {
            KrakenSpotWsMessage::Book { data, is_snapshot } => {
                assert!(!data.is_empty());
                assert!(is_snapshot);
            }
            _ => panic!("Expected Book message"),
        }
    }

    #[rstest]
    fn test_quotes_and_book_subscriptions_independent() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("quotes:BTC/USD");
        handler.subscriptions.confirm_subscribe("quotes:BTC/USD");

        let book_json = r#"{
            "channel": "book",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [{"price": 105944.20, "qty": 2.5}],
                "asks": [{"price": 105944.30, "qty": 3.2}],
                "checksum": 12345,
                "timestamp": "2023-10-06T17:35:55.440295Z"
            }]
        }"#;

        let book_result = handler.parse_message(book_json);
        assert!(
            book_result.is_none(),
            "Book message should be filtered without book: subscription"
        );

        let ticker_json = r#"{
            "channel": "ticker",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bid": 105944.20,
                "bid_qty": 2.5,
                "ask": 105944.30,
                "ask_qty": 3.2,
                "last": 105899.40,
                "volume": 163.28908096,
                "vwap": 105904.39279,
                "low": 104711.00,
                "high": 106613.10,
                "change": 250.00,
                "change_pct": 0.24,
                "timestamp": "2022-12-25T09:30:59.123456Z"
            }]
        }"#;

        let ticker_result = handler.parse_message(ticker_json);
        assert!(
            ticker_result.is_some(),
            "Ticker should pass with quotes subscription"
        );
    }
}
