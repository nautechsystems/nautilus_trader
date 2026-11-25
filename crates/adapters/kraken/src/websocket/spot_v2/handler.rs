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

use futures_util::future::BoxFuture;
#[cfg(test)]
use nautilus_core::string::secret::REDACTED;
use nautilus_core::string::secret::SecretString;
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use serde::Deserialize;
use serde_json::{Value, value::RawValue};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::{
    enums::{KrakenWsChannel, KrakenWsMessageType},
    level_2::BookRequests,
    messages::{
        KrakenSpotWsMessage, KrakenWsBookData, KrakenWsExecutionData, KrakenWsOhlcData,
        KrakenWsRawMessage, KrakenWsResponse, KrakenWsTickerData, KrakenWsTradeData,
    },
    parse::parse_order_response,
};
use crate::{
    common::consts::{KRAKEN_RATE_LIMIT_KEY_ORDER, KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION},
    websocket::{
        error::KrakenWsError,
        spot_v2::level_3::messages::{KrakenL3Snapshot, KrakenL3UpdateData},
    },
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum SpotHandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Subscribe { payload: SecretString },
    Unsubscribe { payload: SecretString },
    Ping { payload: SecretString },
    SendOrderRequest { req_id: u64, payload: SecretString },
}

#[derive(Debug)]
pub(super) struct SpotBookWrite {
    pub(super) epoch: u64,
    pub(super) operation: SpotBookOperation,
}

#[derive(Debug)]
pub(super) enum SpotBookOperation {
    Send(SecretString, Option<CancellationToken>),
    Replace {
        req_id: u64,
        payloads: [SecretString; 2],
        cancel: CancellationToken,
        result: Option<tokio::sync::oneshot::Sender<Result<(), KrakenWsError>>>,
    },
}

/// WebSocket message handler for Kraken Spot v2.
pub(super) struct SpotFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<Arc<WebSocketClient>>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
    subscriptions: SubscriptionState,
    pending_messages: VecDeque<KrakenSpotWsMessage>,
    book_rx: Option<tokio::sync::mpsc::UnboundedReceiver<SpotBookWrite>>,
    book_requests: BookRequests,
    book_write: Option<BoxFuture<'static, ()>>,
    observed_epoch: u64,
}

impl SpotFeedHandler {
    /// Creates a new [`SpotFeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            pending_messages: VecDeque::new(),
            book_rx: None,
            book_requests: BookRequests::default(),
            book_write: None,
            observed_epoch: 0,
        }
    }

    pub(super) fn manage_book_writes(
        &mut self,
        book_rx: tokio::sync::mpsc::UnboundedReceiver<SpotBookWrite>,
        book_requests: BookRequests,
    ) {
        self.book_rx = Some(book_rx);
        self.book_requests = book_requests;
    }

    pub(super) fn book_epoch(&self) -> u64 {
        self.observed_epoch
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
                () = async { self.book_write.as_mut().expect("active book write").await },
                    if self.book_write.is_some() => {
                    self.book_write = None;
                }
                write = async { self.book_rx.as_mut().expect("book write channel").recv().await },
                    if self.book_write.is_none() && self.book_rx.is_some() && self.inner.is_some() => {
                    if let Some(write) = write {
                        self.start_book_write(write);
                    } else {
                        self.book_rx = None;
                    }
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        SpotHandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by handler");
                            self.inner = Some(Arc::new(client));
                        }
                        SpotHandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");
                            self.book_write = None;

                            if let Some(client) = self.inner.take() {
                                client.disconnect().await;
                            }
                        }
                        SpotHandlerCommand::Subscribe { payload }
                        | SpotHandlerCommand::Unsubscribe { payload } => {
                            if let Some(client) = &self.inner {
                                if let Err(e) = client
                                    .send_text(
                                        payload.expose_secret().to_owned(),
                                        Some(KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
                                    )
                                    .await
                                {
                                    log::error!("Failed to send text: {e}");
                                }
                            } else {
                                log::error!(
                                    "Kraken WS subscribe or unsubscribe dropped, no active client"
                                );
                            }
                        }
                        SpotHandlerCommand::Ping { payload } => {
                            if let Some(client) = &self.inner
                                && let Err(e) = client.send_text(payload.expose_secret().to_owned(), None).await
                            {
                                log::error!("Failed to send text: {e}");
                            }
                        }
                        SpotHandlerCommand::SendOrderRequest { req_id, payload } => {
                            if let Some(client) = &self.inner {
                                if let Err(e) = client.send_text(payload.expose_secret().to_owned(), Some(KRAKEN_RATE_LIMIT_KEY_ORDER.as_slice())).await {
                                    log::error!(
                                        "Kraken WS send_order_request failed req_id={req_id}: {e}"
                                    );
                                } else {
                                    log::debug!(
                                        "Kraken WS send_order_request enqueued req_id={req_id}"
                                    );
                                }
                            } else {
                                log::error!(
                                    "Kraken WS send_order_request without active client req_id={req_id}"
                                );
                            }
                        }
                    }
                }

                msg = self.raw_rx.recv() => {
                    let (epoch, msg) = match msg {
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
                            log::debug!("WebSocket connection closed");
                            return None;
                        }
                        Message::Frame(_) => {
                            log::trace!("Received raw frame");
                            continue;
                        }
                        _ => continue,
                    };

                    if text == RECONNECTED {
                        log::debug!("Received WebSocket reconnected signal");
                        self.book_write = None;
                        self.observed_epoch = epoch;
                        return Some(KrakenSpotWsMessage::Reconnected);
                    }

                    if let Some(msg) = self.parse_message(&text)
                        && (epoch == self.observed_epoch || !self.is_book_message(&msg))
                    {
                        return Some(msg);
                    }
                }
            }
        }
    }

    fn is_book_message(&self, msg: &KrakenSpotWsMessage) -> bool {
        match msg {
            KrakenSpotWsMessage::Book { .. }
            | KrakenSpotWsMessage::L3Snapshot(_)
            | KrakenSpotWsMessage::L3Update(_) => true,
            KrakenSpotWsMessage::SubscriptionAck {
                req_id: Some(req_id),
                ..
            } => self.book_requests.lock().contains_key(req_id),
            _ => false,
        }
    }

    fn start_book_write(&mut self, write: SpotBookWrite) {
        let client = Arc::clone(self.inner.as_ref().expect("active book writer"));
        let current_epoch = write.epoch == self.observed_epoch;
        match write.operation {
            SpotBookOperation::Send(payload, cancel) if current_epoch => {
                self.book_write = Some(Box::pin(async move {
                    if let Err(e) =
                        send_book_payload(&client, write.epoch, payload, cancel.as_ref()).await
                    {
                        log::debug!("Spot book command did not complete: {e}");
                    }
                }));
            }
            SpotBookOperation::Send(..) => {}
            SpotBookOperation::Replace {
                req_id,
                payloads,
                cancel,
                result,
            } => {
                if !current_epoch || !self.book_requests.lock().contains_key(&req_id) {
                    if let Some(result) = result {
                        let _ = result.send(Err(KrakenWsError::ConnectionError(
                            "Spot book write was superseded".into(),
                        )));
                    }

                    return;
                }

                self.book_write = Some(Box::pin(async move {
                    let sent = send_book_replacement(&client, write.epoch, payloads, &cancel).await;
                    if let Some(result) = result {
                        let _ = result.send(sent);
                    } else if let Err(e) = sent {
                        log::debug!("Spot book replacement did not complete: {e}");
                    }
                }));
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

        if text.contains("\"level3\"")
            && let Some(msg) = parse_level3_text(text)
        {
            return self.handle_l3_message(msg);
        }

        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to parse message: {e}");
                return None;
            }
        };

        if value.get("method").is_some() {
            match parse_order_response(text) {
                Ok(Some(msg)) => return Some(msg),
                Ok(None) => {}
                Err(e) => log::warn!("Failed to parse order response: {e:#}"),
            }
            return self.handle_control_message(value);
        }

        if value.get("channel").is_some() && value.get("data").is_some() {
            match serde_json::from_str::<KrakenWsRawMessage>(text) {
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

    /// Logs a control response and yields every `subscribe` answer to the stream, so a consumer
    /// holding a subscription can see the venue reject it rather than infer it from silence.
    fn handle_control_message(&self, value: Value) -> Option<KrakenSpotWsMessage> {
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
                            "Subscription failed: error={:?}, req_id={:?}, symbol={:?}",
                            sub.error,
                            sub.req_id,
                            sub.symbol
                        );
                    }

                    Some(KrakenSpotWsMessage::SubscriptionAck {
                        req_id: sub.req_id,
                        symbol: sub.symbol,
                        success: sub.success,
                        error: sub.error,
                    })
                }
                KrakenWsResponse::Unsubscribe(unsub) => {
                    if unsub.success {
                        log::debug!("Unsubscription confirmed: req_id={:?}", unsub.req_id);
                    } else {
                        log::warn!(
                            "Unsubscription failed: error={:?}, req_id={:?}, symbol={:?}",
                            unsub.error,
                            unsub.req_id,
                            unsub.symbol
                        );
                    }
                    None
                }
                KrakenWsResponse::Pong(pong) => {
                    log::trace!("Received pong: req_id={:?}", pong.req_id);
                    None
                }
                KrakenWsResponse::Other => {
                    log::debug!("Received unknown control response");
                    None
                }
            },
            Err(_) => {
                log::debug!("Received control message (failed to parse details)");
                None
            }
        }
    }

    fn handle_data_message(&self, msg: KrakenWsRawMessage) -> Option<KrakenSpotWsMessage> {
        match msg.channel {
            KrakenWsChannel::Book => self.handle_book_message(msg),
            KrakenWsChannel::Ticker => self.handle_ticker_message(msg),
            KrakenWsChannel::Trade => self.handle_trade_message(msg),
            KrakenWsChannel::Ohlc => self.handle_ohlc_message(msg),
            KrakenWsChannel::Executions => self.handle_executions_message(msg),
            KrakenWsChannel::Level3 => {
                // Well-formed level3 messages take the fast path in `parse_message`, so
                // reaching here means the venue sent one that failed to parse
                log::warn!("Dropping malformed level3 message");
                None
            }
            _ => {
                log::warn!("Unhandled channel: {:?}", msg.channel);
                None
            }
        }
    }

    fn handle_book_message(&self, msg: KrakenWsRawMessage) -> Option<KrakenSpotWsMessage> {
        let is_snapshot = msg.event_type == KrakenWsMessageType::Snapshot;
        let mut book_data = Vec::new();

        for data in msg.data {
            match serde_json::from_str::<KrakenWsBookData>(data.get()) {
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

    fn handle_ticker_message(&self, msg: KrakenWsRawMessage) -> Option<KrakenSpotWsMessage> {
        let mut tickers = Vec::new();

        for data in msg.data {
            match serde_json::from_str::<KrakenWsTickerData>(data.get()) {
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

    fn handle_trade_message(&self, msg: KrakenWsRawMessage) -> Option<KrakenSpotWsMessage> {
        let mut trades = Vec::new();

        for data in msg.data {
            match serde_json::from_str::<KrakenWsTradeData>(data.get()) {
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

    fn handle_ohlc_message(&self, msg: KrakenWsRawMessage) -> Option<KrakenSpotWsMessage> {
        let mut ohlc_data = Vec::new();

        for data in msg.data {
            match serde_json::from_str::<KrakenWsOhlcData>(data.get()) {
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

    fn handle_executions_message(&self, msg: KrakenWsRawMessage) -> Option<KrakenSpotWsMessage> {
        let mut executions = Vec::new();

        for data in msg.data {
            match serde_json::from_str::<KrakenWsExecutionData>(data.get()) {
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

async fn send_book_replacement(
    ws_client: &WebSocketClient,
    epoch: u64,
    payloads: [SecretString; 2],
    cancel: &CancellationToken,
) -> Result<(), KrakenWsError> {
    for payload in payloads {
        send_book_payload(ws_client, epoch, payload, Some(cancel)).await?;
    }

    Ok(())
}

async fn send_book_payload(
    ws_client: &WebSocketClient,
    epoch: u64,
    payload: SecretString,
    cancel: Option<&CancellationToken>,
) -> Result<(), KrakenWsError> {
    let send = ws_client.send_text_on_connection(
        payload.expose_secret().to_owned(),
        Some(KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
        epoch,
    );

    let sent = if let Some(cancel) = cancel {
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(KrakenWsError::Disconnected("Book write cancelled".into())),
            result = send => result,
        }
    } else {
        send.await
    };

    sent.map_err(|e| KrakenWsError::ConnectionError(e.to_string()))
}

#[derive(Deserialize)]
struct Level3RawMessage<'a> {
    channel: &'a str,
    #[serde(rename = "type")]
    msg_type: &'a str,
    #[serde(borrow)]
    data: Vec<&'a RawValue>,
}

fn parse_level3_text(text: &str) -> Option<KrakenSpotWsMessage> {
    let msg: Level3RawMessage<'_> = serde_json::from_str(text).ok()?;
    if msg.channel != "level3" {
        return None;
    }
    let first = msg.data.first()?.get();

    match msg.msg_type {
        "snapshot" => match serde_json::from_str::<KrakenL3Snapshot>(first) {
            Ok(snap) => Some(KrakenSpotWsMessage::L3Snapshot(snap)),
            Err(e) => {
                log::warn!("Failed to deserialize L3 snapshot: {e}");
                None
            }
        },
        "update" => match serde_json::from_str::<KrakenL3UpdateData>(first) {
            Ok(update) => Some(KrakenSpotWsMessage::L3Update(update)),
            Err(e) => {
                log::warn!("Failed to deserialize L3 update: {e}");
                None
            }
        },
        _ => None,
    }
}

impl SpotFeedHandler {
    fn handle_l3_message(&self, msg: KrakenSpotWsMessage) -> Option<KrakenSpotWsMessage> {
        let symbol = match &msg {
            KrakenSpotWsMessage::L3Snapshot(s) => &s.symbol,
            KrakenSpotWsMessage::L3Update(u) => &u.symbol,
            _ => return None,
        };

        if !self.is_subscribed(&format!("level3:{symbol}")) {
            return None;
        }
        Some(msg)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use parking_lot::Mutex;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::websocket::spot_v2::level_2::BookRequest;

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
    fn test_ticker_message_preserves_decimal_precision() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("ticker:BTC/USD");
        handler.subscriptions.confirm_subscribe("ticker:BTC/USD");
        let json = include_str!("../../../test_data/ws_ticker_precision.json");

        let message = handler.parse_message(json).unwrap();

        let KrakenSpotWsMessage::Ticker(data) = message else {
            panic!("Expected Ticker message, was {message:?}");
        };
        let ticker = &data[0];
        assert_eq!(ticker.symbol, "BTC/USD");
        assert_eq!(ticker.bid, dec!(123456789.123456789));
        assert_eq!(ticker.bid_qty, dec!(0.1234567890123456789012345678));
        assert_eq!(ticker.ask, dec!(123456789.223456789));
        assert_eq!(ticker.ask_qty, dec!(0.2234567890123456789012345678));
        assert_eq!(ticker.last, dec!(123456789.323456789));
        assert_eq!(ticker.volume, dec!(123456789.423456789));
        assert_eq!(ticker.vwap, dec!(123456789.523456789));
        assert_eq!(ticker.low, dec!(123456789.623456789));
        assert_eq!(ticker.high, dec!(123456789.723456789));
        assert_eq!(ticker.change, dec!(123456789.823456789));
        assert_eq!(ticker.change_pct, dec!(0.9234567890123456789012345678));
        assert_eq!(
            ticker.timestamp,
            "2022-12-25T09:30:59.123456Z"
                .parse::<jiff::Timestamp>()
                .unwrap()
        );
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
    fn test_send_order_request_variant_construction() {
        let cmd = SpotHandlerCommand::SendOrderRequest {
            req_id: 7,
            payload: SecretString::from(r#"{"method":"add_order","req_id":7}"#.to_string()),
        };

        match cmd {
            SpotHandlerCommand::SendOrderRequest { req_id, payload } => {
                assert_eq!(req_id, 7);
                assert!(payload.expose_secret().contains("add_order"));
            }
            _ => panic!("Expected SendOrderRequest, was a different variant"),
        }
    }

    #[rstest]
    fn test_command_debug_redacts_payload() {
        let cmd = SpotHandlerCommand::SendOrderRequest {
            req_id: 7,
            payload: SecretString::from("auth-token".to_string()),
        };

        let debug = format!("{cmd:?}");

        assert!(debug.contains(REDACTED));
        assert!(!debug.contains("auth-token"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_order_request_without_active_client_does_not_panic() {
        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let subscriptions = SubscriptionState::new(':');

        let mut handler = SpotFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions);

        cmd_tx
            .send(SpotHandlerCommand::SendOrderRequest {
                req_id: 42,
                payload: SecretString::from(r#"{"method":"add_order","req_id":42}"#.to_string()),
            })
            .unwrap();

        drop(cmd_tx);
        drop(raw_tx);

        let result = handler.next().await;
        assert!(
            result.is_none(),
            "Handler should return None when streams close"
        );
    }

    struct DroppedCommandLogCapture {
        messages: Mutex<Vec<String>>,
    }

    static DROPPED_COMMAND_LOG_CAPTURE: DroppedCommandLogCapture = DroppedCommandLogCapture {
        messages: Mutex::new(Vec::new()),
    };

    impl log::Log for DroppedCommandLogCapture {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() == log::Level::Error
                && metadata.target() == "nautilus_kraken::websocket::spot_v2::handler"
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                self.messages.lock().push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    /// A subscribe that reaches the handler before its `SetClient` is dropped with an error
    /// naming the lost command, rather than vanishing while the client already answered `Ok`.
    #[rstest]
    #[tokio::test]
    async fn test_subscribe_without_active_client_logs_dropped_command() {
        log::set_logger(&DROPPED_COMMAND_LOG_CAPTURE).expect("test logger already installed");
        log::set_max_level(log::LevelFilter::Error);

        let signal = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        // The raw stream stays open so `next` cannot return early: the only ready
        // branch is the command channel, so the queued subscribe is handled first.
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Message)>();
        let subscriptions = SubscriptionState::new(':');

        let mut handler = SpotFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions);

        cmd_tx
            .send(SpotHandlerCommand::Subscribe {
                payload: SecretString::from(
                    r#"{"method":"subscribe","token":"SUBSCRIBE_SECRET_MARKER"}"#.to_string(),
                ),
            })
            .unwrap();

        drop(cmd_tx);

        let mut next = std::pin::pin!(handler.next());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);

        while DROPPED_COMMAND_LOG_CAPTURE.messages.lock().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "handler never processed the queued subscribe"
            );
            let _ = tokio::time::timeout(Duration::from_millis(10), next.as_mut()).await;
        }

        let messages = DROPPED_COMMAND_LOG_CAPTURE.messages.lock().clone();
        assert_eq!(
            messages,
            vec!["Kraken WS subscribe or unsubscribe dropped, no active client".to_string()],
            "a subscribe without an active client must log the dropped command"
        );
        assert!(
            !messages.join("\n").contains("SUBSCRIBE_SECRET_MARKER"),
            "dropped-command log must not leak the payload"
        );
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

    #[rstest]
    fn test_parse_message_routes_add_order_response_to_order_response_variant() {
        use super::super::enums::KrakenWsMethod;

        let handler = create_test_handler();
        let json = r#"{"method":"add_order","req_id":42,"success":true,"time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z","result":{"order_id":"OABCDE-12345-FGHIJ","cl_ord_id":"O-20260505-000001","order_userref":0}}"#;

        let result = handler.parse_message(json);
        match result {
            Some(KrakenSpotWsMessage::OrderResponse(resp)) => {
                assert_eq!(resp.method, KrakenWsMethod::AddOrder);
                assert_eq!(resp.req_id, Some(42));
                assert!(resp.success);
            }
            other => panic!("expected OrderResponse, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_spot_order_reply_can_precede_the_reconnect_marker() {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = SpotFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            SubscriptionState::new(':'),
        );

        raw_tx.send((1, Message::text(r#"{"method":"add_order","req_id":42,"success":false,"error":"EOrder:Insufficient funds","time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z"}"#))).unwrap();
        raw_tx.send((1, Message::text(RECONNECTED))).unwrap();

        let Some(KrakenSpotWsMessage::OrderResponse(response)) = handler.next().await else {
            panic!("expected the definitive order rejection before reconnect");
        };

        assert_eq!(response.req_id, Some(42));
        assert!(!response.success);
        assert!(matches!(
            handler.next().await,
            Some(KrakenSpotWsMessage::Reconnected)
        ));
    }

    /// A rejected `subscribe` reaches the stream with the request id, the pair and the venue's
    /// reason, so the holder of the subscription can act on it.
    #[rstest]
    fn test_parse_message_yields_a_subscribe_rejection() {
        let handler = create_test_handler();
        let json = r#"{"error":"Currency pair not supported BOGUS/NOPE","method":"subscribe","req_id":7,"success":false,"symbol":"BOGUS/NOPE","time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z"}"#;

        let result = handler.parse_message(json);

        let Some(KrakenSpotWsMessage::SubscriptionAck {
            req_id,
            symbol,
            success,
            error,
        }) = result
        else {
            panic!("expected a SubscriptionAck, was {result:?}");
        };
        assert_eq!(req_id, Some(7));
        assert_eq!(symbol, Some(ustr::Ustr::from("BOGUS/NOPE")));
        assert!(!success);
        assert_eq!(
            error.as_deref(),
            Some("Currency pair not supported BOGUS/NOPE")
        );
    }

    /// A confirmed `subscribe` reaches the stream too, so the consumer can retire the request.
    #[rstest]
    fn test_parse_message_yields_a_subscribe_confirmation() {
        let handler = create_test_handler();
        let json = r#"{"method":"subscribe","result":{"channel":"book","symbol":"BTC/USD","depth":10,"snapshot":true},"success":true,"time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z","req_id":3}"#;

        let result = handler.parse_message(json);

        let Some(KrakenSpotWsMessage::SubscriptionAck {
            req_id,
            symbol,
            success,
            error,
        }) = result
        else {
            panic!("expected a SubscriptionAck, was {result:?}");
        };
        assert_eq!(req_id, Some(3));
        assert_eq!(symbol, None, "a confirmation names the pair in result only");
        assert!(success);
        assert!(error.is_none());

        let unsubscribe = r#"{"method":"unsubscribe","result":{"channel":"book","symbol":"BTC/USD"},"success":true,"time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z","req_id":4}"#;
        assert!(
            handler.parse_message(unsubscribe).is_none(),
            "only subscribe answers reach the stream"
        );
    }

    #[tokio::test]
    async fn test_reconnect_retires_spot_book_writes_without_blocking_feed() {
        use std::num::NonZeroU32;

        use futures_util::StreamExt;
        use nautilus_network::{ratelimiter::quota::Quota, websocket::WebSocketConfig};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = WebSocketConfig::builder()
            .url(format!("ws://{}", listener.local_addr().unwrap()))
            .build()
            .unwrap();

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            while socket.next().await.is_some() {}
        });

        let (raw_handler, _) = nautilus_network::websocket::channel_message_handler();
        let client = WebSocketClient::builder()
            .config(config)
            .message_handler(raw_handler)
            .keyed_quotas(vec![(
                KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION[0].to_string(),
                Quota::per_hour(NonZeroU32::MIN),
            )])
            .connect()
            .await
            .unwrap();
        client
            .send_text(
                "first".into(),
                Some(KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
            )
            .await
            .unwrap();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (book_tx, book_rx) = tokio::sync::mpsc::unbounded_channel();
        let requests = BookRequests::default();
        let symbol = ustr::Ustr::from("BTC/USD");
        requests.lock().insert(50, BookRequest::new(symbol));
        let epoch = client.connection_epoch();

        let mut handler = SpotFeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            SubscriptionState::new(':'),
        );

        let (result, mut response) = tokio::sync::oneshot::channel();
        handler.manage_book_writes(book_rx, Arc::clone(&requests));
        cmd_tx.send(SpotHandlerCommand::SetClient(client)).unwrap();

        let write = |epoch, req_id, result| SpotBookWrite {
            epoch,
            operation: SpotBookOperation::Replace {
                req_id,
                payloads: [
                    SecretString::from("unsubscribe"),
                    SecretString::from("subscribe"),
                ],
                cancel: CancellationToken::new(),
                result: Some(result),
            },
        };

        book_tx.send(write(epoch, 50, result)).unwrap();

        let mut next = Box::pin(handler.next());
        assert!(futures_util::poll!(&mut next).is_pending());
        raw_tx.send((epoch, Message::text(
            r#"{"method":"subscribe","result":{"channel":"book","symbol":"ETH/USD"},"success":true,"time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z","req_id":60}"#,
        ))).unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(1), next.as_mut())
            .await
            .unwrap();
        assert!(matches!(
            frame,
            Some(KrakenSpotWsMessage::SubscriptionAck {
                req_id: Some(60),
                success: true,
                ..
            })
        ));
        assert!(matches!(
            response.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        drop(next);
        raw_tx
            .send((epoch + 1, Message::text(RECONNECTED)))
            .unwrap();
        assert!(matches!(
            handler.next().await,
            Some(KrakenSpotWsMessage::Reconnected)
        ));
        assert!(response.await.is_err(), "reconnect drops the stalled write");

        requests.lock().remove(&50);
        requests.lock().insert(51, BookRequest::new(symbol));
        let (old_connection, old_connection_result) = tokio::sync::oneshot::channel();
        let (old_request, old_request_result) = tokio::sync::oneshot::channel();
        book_tx.send(write(epoch, 51, old_connection)).unwrap();
        book_tx.send(write(epoch + 1, 50, old_request)).unwrap();
        let mut next = std::pin::pin!(handler.next());
        assert!(futures_util::poll!(&mut next).is_pending());

        for result in [old_connection_result, old_request_result] {
            assert!(
                matches!(result.await.unwrap(), Err(KrakenWsError::ConnectionError(message)) if message == "Spot book write was superseded")
            );
        }

        server.abort();
    }

    /// The handler yields a subscribe answer and the book frames after it in wire order, and a
    /// failed unsubscribe, which the venue reports under the subscribe method with the
    /// unsubscribe's request id, reaches the stream as a subscribe answer.
    #[rstest]
    #[tokio::test]
    async fn test_next_yields_answers_and_book_frames_in_wire_order() {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe("book:BTC/USD");
        subscriptions.confirm_subscribe("book:BTC/USD");
        let mut handler = SpotFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions);

        for text in [
            r#"{"method":"subscribe","result":{"channel":"book","symbol":"BTC/USD","depth":10,"snapshot":true},"success":true,"time_in":"2026-05-05T10:00:00.123Z","time_out":"2026-05-05T10:00:00.125Z","req_id":3}"#,
            r#"{"channel":"book","type":"snapshot","data":[{"symbol":"BTC/USD","bids":[{"price":105944.20,"qty":2.5}],"asks":[{"price":105944.30,"qty":3.2}],"checksum":12345,"timestamp":"2023-10-06T17:35:55.440295Z"}]}"#,
            r#"{"channel":"book","type":"update","data":[{"symbol":"BTC/USD","bids":[{"price":105944.20,"qty":2.0}],"asks":[],"checksum":12346,"timestamp":"2023-10-06T17:35:55.540295Z"}]}"#,
            r#"{"error":"Subscription with depth 10 not Found BTC/USD","method":"subscribe","req_id":502,"success":false,"symbol":"BTC/USD","time_in":"2026-05-05T10:00:01.123Z","time_out":"2026-05-05T10:00:01.125Z"}"#,
        ] {
            raw_tx.send((0, Message::Text(text.into()))).unwrap();
        }

        let Some(KrakenSpotWsMessage::SubscriptionAck {
            req_id: Some(3),
            success: true,
            ..
        }) = handler.next().await
        else {
            panic!("expected the confirmation first");
        };
        let Some(KrakenSpotWsMessage::Book {
            is_snapshot: true, ..
        }) = handler.next().await
        else {
            panic!("expected the snapshot second");
        };
        let Some(KrakenSpotWsMessage::Book {
            is_snapshot: false, ..
        }) = handler.next().await
        else {
            panic!("expected the update third");
        };
        let failed_unsubscribe = handler.next().await;
        let Some(KrakenSpotWsMessage::SubscriptionAck {
            req_id: Some(502),
            symbol: Some(symbol),
            success: false,
            ..
        }) = failed_unsubscribe
        else {
            panic!(
                "expected the failed unsubscribe as a subscribe answer, was {failed_unsubscribe:?}"
            );
        };
        assert_eq!(symbol, ustr::Ustr::from("BTC/USD"));
    }

    #[rstest]
    fn test_parse_level3_snapshot_with_subscription_passes() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("level3:BTC/USD");
        handler.subscriptions.confirm_subscribe("level3:BTC/USD");

        let json = r#"{
            "channel": "level3",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [],
                "asks": [],
                "checksum": 0,
                "timestamp": "2024-01-01T00:00:00Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(matches!(result, Some(KrakenSpotWsMessage::L3Snapshot(_))));
    }

    #[rstest]
    fn test_parse_level3_update_without_subscription_filtered() {
        let handler = create_test_handler();
        let json = r#"{
            "channel": "level3",
            "type": "update",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [],
                "asks": [],
                "checksum": 0,
                "timestamp": "2024-01-01T00:00:00Z"
            }]
        }"#;

        let result = handler.parse_message(json);
        assert!(result.is_none());
    }

    #[rstest]
    #[case::empty_data(r#"{"channel":"level3","type":"update","data":[]}"#)]
    #[case::snapshot_missing_bids(
        r#"{"channel":"level3","type":"snapshot","data":[{"symbol":"BTC/USD","asks":[],"checksum":0,"timestamp":"2024-01-01T00:00:00Z"}]}"#
    )]
    #[case::unexpected_type(r#"{"channel":"level3","type":"error","data":[{}]}"#)]
    fn test_parse_level3_malformed_message_is_dropped(#[case] json: &str) {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("level3:BTC/USD");
        handler.subscriptions.confirm_subscribe("level3:BTC/USD");

        let result = handler.parse_message(json);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_level3_snapshot_compact_json() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("level3:BTC/USD");
        handler.subscriptions.confirm_subscribe("level3:BTC/USD");
        let json = r#"{"channel":"level3","type":"snapshot","data":[{"symbol":"BTC/USD","bids":[],"asks":[],"checksum":0,"timestamp":"2024-01-01T00:00:00Z"}]}"#;
        assert!(matches!(
            handler.parse_message(json),
            Some(KrakenSpotWsMessage::L3Snapshot(_))
        ));
    }

    #[rstest]
    fn test_parse_level3_snapshot_preserves_raw_decimal() {
        let handler = create_test_handler();
        handler.subscriptions.mark_subscribe("level3:BTC/USD");
        handler.subscriptions.confirm_subscribe("level3:BTC/USD");

        let json = r#"{
            "channel": "level3",
            "type": "snapshot",
            "data": [{
                "symbol": "BTC/USD",
                "bids": [{
                    "order_id": "order-bid-1",
                    "limit_price": 42000.50000,
                    "order_qty": 0.01000000,
                    "timestamp": "2024-01-01T00:00:00Z"
                }],
                "asks": [],
                "checksum": 0,
                "timestamp": "2024-01-01T00:00:00Z"
            }]
        }"#;

        let Some(KrakenSpotWsMessage::L3Snapshot(snap)) = handler.parse_message(json) else {
            panic!("expected L3 snapshot");
        };
        assert_eq!(snap.bids[0].limit_price.raw, "42000.50000");
        assert_eq!(snap.bids[0].order_qty.raw, "0.01000000");
    }
}
