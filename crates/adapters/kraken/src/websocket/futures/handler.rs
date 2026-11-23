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

use nautilus_core::string::secret::SecretString;
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use serde::Deserialize;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    book::{BookCommand, BookMessage, BookStream, BookSubscriptions},
    messages::{
        KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesChannel,
        KrakenFuturesFillsDelta, KrakenFuturesMessageType, KrakenFuturesOpenOrdersCancel,
        KrakenFuturesOpenOrdersDelta, KrakenFuturesTickerData, KrakenFuturesTradeData,
        KrakenFuturesWsMessage, classify_futures_message,
    },
};
use crate::common::consts::KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION;

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum FuturesHandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    Subscribe { payload: SecretString },
    Unsubscribe { payload: SecretString },
    RequestChallenge { payload: SecretString },
}

enum Incoming {
    Messages(tokio::sync::mpsc::UnboundedReceiver<Message>),
    Epochs(tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>),
}

impl Incoming {
    async fn recv(&mut self) -> Option<(Option<u64>, Message)> {
        match self {
            Self::Messages(rx) => rx.recv().await.map(|message| (None, message)),
            Self::Epochs(rx) => rx
                .recv()
                .await
                .map(|(epoch, message)| (Some(epoch), message)),
        }
    }
}

/// WebSocket message handler for Kraken Futures.
pub struct FuturesFeedHandler {
    signal: Arc<AtomicBool>,
    inner: Option<Arc<WebSocketClient>>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<FuturesHandlerCommand>,
    raw_rx: Incoming,
    subscriptions: SubscriptionState,
    pending_messages: VecDeque<KrakenFuturesWsMessage>,
    book_channels: Option<BookChannels>,
    books: BookSubscriptions,
}

struct BookChannels {
    commands: tokio::sync::mpsc::UnboundedReceiver<BookCommand>,
    messages: tokio::sync::mpsc::UnboundedSender<BookMessage>,
}

impl FuturesFeedHandler {
    /// Creates a new [`FuturesFeedHandler`] instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<FuturesHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self::with_incoming(signal, cmd_rx, Incoming::Messages(raw_rx), subscriptions)
    }

    pub(super) fn with_epochs(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<FuturesHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self::with_incoming(signal, cmd_rx, Incoming::Epochs(raw_rx), subscriptions)
    }

    fn with_incoming(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<FuturesHandlerCommand>,
        raw_rx: Incoming,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            pending_messages: VecDeque::new(),
            book_channels: None,
            books: BookSubscriptions::default(),
        }
    }

    pub(super) fn manage_books(
        &mut self,
        commands: tokio::sync::mpsc::UnboundedReceiver<BookCommand>,
        messages: tokio::sync::mpsc::UnboundedSender<BookMessage>,
    ) {
        self.book_channels = Some(BookChannels { commands, messages });
    }

    pub(super) fn book_stream(&self, msg: &KrakenFuturesWsMessage) -> Option<BookStream> {
        self.books.stream(msg).cloned()
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
                () = async {
                    self.books.poll_transition(self.inner.as_ref().expect("active book writer")).await;
                }, if self.books.has_transitions() => {}
                command = async {
                    self.book_channels.as_mut().expect("managed book channels").commands.recv().await
                }, if self.book_channels.is_some() && self.inner.is_some() => {
                    let command = command?;
                    self.books.command(command, self.inner.as_ref().expect("active book client"));
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        FuturesHandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by futures handler");
                            self.inner = Some(Arc::new(client));
                        }
                        FuturesHandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");

                            if let Some(client) = self.inner.take() {
                                client.disconnect().await;
                            }
                            return None;
                        }
                        FuturesHandlerCommand::Subscribe { payload }
                        | FuturesHandlerCommand::Unsubscribe { payload } => {
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
                                    "Kraken Futures WS subscribe or unsubscribe dropped, no active client"
                                );
                            }
                        }
                        FuturesHandlerCommand::RequestChallenge { payload } => {
                            if let Some(ref client) = self.inner
                                && let Err(e) = client.send_text(payload.expose_secret().to_owned(), None).await
                            {
                                log::error!("Failed to send text: {e}");
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
                            log::debug!("WebSocket connection closed");
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
                        log::debug!("Received WebSocket reconnected signal");
                        if let Some(epoch) = epoch {
                            self.books.reconnect(epoch, self.inner.as_ref());
                        }
                        return Some(KrakenFuturesWsMessage::Reconnected);
                    }

                    let current_book_epoch =
                        self.book_channels.is_none() || epoch == Some(self.books.epoch());
                    self.parse_message(text, current_book_epoch);

                    if let Some(msg) = self.pending_messages.pop_front() {
                        return Some(msg);
                    }
                }
            }
        }
    }

    fn confirm_book(&mut self, value: &Value, subscribed: bool, success: bool) {
        if value["feed"] != "book" {
            return;
        }

        for symbol in value["product_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            self.confirm_book_product(Ustr::from(symbol), subscribed, success);
        }
    }

    fn confirm_book_product(&mut self, symbol: Ustr, subscribed: bool, success: bool) {
        if let Some(client) = &self.inner {
            let rejected = self.books.confirmed(symbol, subscribed, success, client);
            self.forward_book_rejection(symbol, rejected);
        }
    }

    fn forward_book_rejection(&self, symbol: Ustr, rejected: Option<u64>) {
        if let Some(generation) = rejected
            && let Some(channels) = &self.book_channels
        {
            let _ = channels
                .messages
                .send(BookMessage::Rejected(symbol, generation));
        }
    }

    fn parse_message(&mut self, text: &str, current_book_epoch: bool) {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("Failed to parse message as JSON: {e}");
                return;
            }
        };

        let kind = classify_futures_message(&value);

        if !current_book_epoch
            && (value["feed"] == "book" || matches!(kind, KrakenFuturesMessageType::BookSnapshot))
        {
            return;
        }

        match kind {
            KrakenFuturesMessageType::OpenOrdersSnapshot => {
                log::debug!(
                    "Skipping open_orders_snapshot (REST reconciliation handles initial state)"
                );
            }
            KrakenFuturesMessageType::OpenOrdersCancel => {
                self.handle_open_orders_cancel_text(text);
            }
            KrakenFuturesMessageType::OpenOrdersDelta => {
                self.handle_open_orders_delta_text(text);
            }
            KrakenFuturesMessageType::FillsSnapshot => {
                log::debug!("Skipping fills_snapshot (REST reconciliation handles initial state)");
            }
            KrakenFuturesMessageType::FillsDelta => {
                self.handle_fills_delta_text(text);
            }
            KrakenFuturesMessageType::Ticker => {
                self.handle_ticker_message_text(text);
            }
            KrakenFuturesMessageType::TradeSnapshot => {
                log::debug!("Skipping trade_snapshot (only streaming live trades)");
            }
            KrakenFuturesMessageType::Trade => {
                self.handle_trade_message_text(text);
            }
            KrakenFuturesMessageType::BookSnapshot => {
                self.handle_book_snapshot_text(text);
            }
            KrakenFuturesMessageType::BookDelta => {
                self.handle_book_delta_text(text);
            }
            KrakenFuturesMessageType::Info => {
                log::debug!("Received info message: {text}");
            }
            KrakenFuturesMessageType::Pong => {
                log::debug!("Received text pong response");
            }
            KrakenFuturesMessageType::Subscribed => {
                self.confirm_book(&value, true, true);
                log::debug!("Subscription confirmed: {text}");
            }
            KrakenFuturesMessageType::Unsubscribed => {
                self.confirm_book(&value, false, true);
                log::debug!("Unsubscription confirmed: {text}");
            }
            KrakenFuturesMessageType::Challenge => {
                self.handle_challenge_response_text(text);
            }
            KrakenFuturesMessageType::Heartbeat => {
                log::trace!("Heartbeat received");
            }
            KrakenFuturesMessageType::Error => {
                let message = value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                log::warn!("Kraken Futures WebSocket error: {message}");
            }
            KrakenFuturesMessageType::Alert => {
                let message = value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown alert");

                if current_book_epoch
                    && let Some(symbol) = message
                        .strip_prefix("Couldn't subscribe to invalid product `")
                        .and_then(|product| product.strip_suffix('`'))
                        .and_then(Ustr::from_existing)
                {
                    let rejected = self.books.reject_invalid_product(symbol);
                    self.forward_book_rejection(symbol, rejected);
                }

                log::warn!("Kraken Futures WebSocket alert: {message}");
            }
            KrakenFuturesMessageType::Unknown => {
                match value["event"].as_str() {
                    Some("subscribed_failed") => self.confirm_book(&value, true, false),
                    Some("unsubscribed_failed") => self.confirm_book(&value, false, false),
                    _ => {}
                }

                log::warn!("Unhandled futures message: {text}");
            }
        }
    }

    fn handle_challenge_response_text(&mut self, text: &str) {
        #[derive(Deserialize)]
        struct ChallengeResponse {
            message: String,
        }

        match serde_json::from_str::<ChallengeResponse>(text) {
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

    fn handle_ticker_message_text(&mut self, text: &str) {
        let ticker = match serde_json::from_str::<KrakenFuturesTickerData>(text) {
            Ok(t) => t,
            Err(e) => {
                log::debug!("Failed to parse ticker: {e}");
                return;
            }
        };

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::Ticker(ticker));
    }

    fn handle_trade_message_text(&mut self, text: &str) {
        let trade = match serde_json::from_str::<KrakenFuturesTradeData>(text) {
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

    fn handle_book_snapshot_text(&mut self, text: &str) {
        let snapshot = match serde_json::from_str::<KrakenFuturesBookSnapshot>(text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Failed to parse book snapshot: {e}");
                return;
            }
        };

        let has_book = self.is_subscribed(KrakenFuturesChannel::Book, &snapshot.product_id);
        let has_quotes = self.is_subscribed(KrakenFuturesChannel::Quotes, &snapshot.product_id);
        if self.book_channels.is_none() && !has_book && !has_quotes {
            log::debug!(
                "Received book snapshot for unsubscribed product: {}",
                snapshot.product_id
            );
            return;
        }

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::BookSnapshot(snapshot));
    }

    fn handle_book_delta_text(&mut self, text: &str) {
        let delta = match serde_json::from_str::<KrakenFuturesBookDelta>(text) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to parse book delta: {e}");
                return;
            }
        };

        let has_book = self.is_subscribed(KrakenFuturesChannel::Book, &delta.product_id);
        let has_quotes = self.is_subscribed(KrakenFuturesChannel::Quotes, &delta.product_id);
        if self.book_channels.is_none() && !has_book && !has_quotes {
            log::debug!(
                "Received book delta for unsubscribed product: {}",
                delta.product_id
            );
            return;
        }

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::BookDelta(delta));
    }

    fn handle_open_orders_delta_text(&mut self, text: &str) {
        let delta = match serde_json::from_str::<KrakenFuturesOpenOrdersDelta>(text) {
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

    fn handle_open_orders_cancel_text(&mut self, text: &str) {
        let cancel = match serde_json::from_str::<KrakenFuturesOpenOrdersCancel>(text) {
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

    fn handle_fills_delta_text(&mut self, text: &str) {
        let delta = match serde_json::from_str::<KrakenFuturesFillsDelta>(text) {
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
    use std::time::Duration;

    use nautilus_core::string::secret::REDACTED;
    use nautilus_network::websocket::WebSocketConfig;
    use parking_lot::Mutex;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn create_test_handler() -> FuturesFeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');

        FuturesFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions)
    }

    #[rstest]
    #[tokio::test]
    async fn test_invalid_product_alert_can_precede_the_book_write_result() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = WebSocketConfig::builder()
            .url(format!("ws://{}", listener.local_addr().unwrap()))
            .build()
            .unwrap();

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let _ = futures_util::StreamExt::next(&mut socket).await;
        });

        let mut handler = create_test_handler();

        let client = Arc::new(
            WebSocketClient::builder()
                .config(config)
                .message_handler(Arc::new(|_| {}))
                .connect()
                .await
                .unwrap(),
        );

        handler.inner = Some(client.clone());
        let (_commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (messages, mut message_rx) = tokio::sync::mpsc::unbounded_channel();
        handler.manage_books(command_rx, messages);
        let symbol = Ustr::from("PI_NOT_A_REAL_CONTRACT");
        handler
            .books
            .command(BookCommand::Subscribe(symbol, 41), &client);

        handler.parse_message(
            r#"{"event":"alert","message":"Couldn't subscribe to invalid product `PI_NOT_A_REAL_CONTRACT`"}"#,
            true,
        );

        assert!(
            matches!(message_rx.try_recv().unwrap(), BookMessage::Rejected(rejected, 41) if rejected == symbol)
        );
        assert!(message_rx.try_recv().is_err());
        client.disconnect().await;
        server.await.unwrap();
    }

    #[rstest]
    fn test_command_debug_redacts_payload() {
        let command = FuturesHandlerCommand::RequestChallenge {
            payload: SecretString::from("private-challenge-payload"),
        };

        assert_eq!(
            format!("{command:?}"),
            format!("RequestChallenge {{ payload: {REDACTED} }}")
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
                && metadata.target() == "nautilus_kraken::websocket::futures::handler"
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
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        let subscriptions = SubscriptionState::new(':');

        let mut handler = FuturesFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions);

        cmd_tx
            .send(FuturesHandlerCommand::Subscribe {
                payload: SecretString::from(
                    r#"{"event":"subscribe","feed":"trade","token":"SUBSCRIBE_SECRET_MARKER"}"#
                        .to_string(),
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
            vec![
                "Kraken Futures WS subscribe or unsubscribe dropped, no active client".to_string()
            ],
            "a subscribe without an active client must log the dropped command"
        );
        assert!(
            !messages.join("\n").contains("SUBSCRIBE_SECRET_MARKER"),
            "dropped-command log must not leak the payload"
        );
    }

    #[rstest]
    fn test_parse_ticker_emits_ticker_message() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_ticker.json");

        handler.parse_message(json, true);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::Ticker(ticker) = msg else {
            panic!("Expected Ticker message, was {msg:?}");
        };
        assert_eq!(ticker.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(ticker.bid, Some(dec!(21978.5)));
        assert_eq!(ticker.ask, Some(dec!(21987)));
    }

    #[rstest]
    fn test_parse_trade_emits_trade_message() {
        let mut handler = create_test_handler();
        handler.subscriptions.mark_subscribe("trades:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("trades:PI_XBTUSD");

        let json = include_str!("../../../test_data/ws_futures_trade.json");

        handler.parse_message(json, true);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::Trade(trade) = msg else {
            panic!("Expected Trade message, was {msg:?}");
        };
        assert_eq!(trade.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(trade.price, dec!(34969.5));
        assert_eq!(trade.qty, dec!(15000));
    }

    #[rstest]
    #[tokio::test]
    async fn test_futures_trade_can_precede_the_book_epoch_marker() {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_book_tx, book_rx) = tokio::sync::mpsc::unbounded_channel();
        let (messages, _rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');
        subscriptions.mark_subscribe("trades:PI_XBTUSD");
        subscriptions.confirm_subscribe("trades:PI_XBTUSD");
        let mut handler = FuturesFeedHandler::with_epochs(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            subscriptions,
        );
        handler.manage_books(book_rx, messages);

        for payload in [
            include_str!("../../../test_data/ws_futures_book_snapshot.json"),
            include_str!("../../../test_data/ws_futures_trade.json"),
            RECONNECTED,
        ] {
            raw_tx.send((1, Message::text(payload))).unwrap();
        }

        assert!(matches!(
            handler.next().await,
            Some(KrakenFuturesWsMessage::Trade(trade)) if trade.product_id == Ustr::from("PI_XBTUSD")
        ));
        assert!(matches!(
            handler.next().await,
            Some(KrakenFuturesWsMessage::Reconnected)
        ));
    }

    #[rstest]
    fn test_parse_trade_filters_unsubscribed() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_trade.json");

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

        assert_eq!(handler.pending_messages.len(), 1);
        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::BookDelta(delta) = msg else {
            panic!("Expected BookDelta message, was {msg:?}");
        };
        assert_eq!(delta.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(delta.price, dec!(34981));
    }

    #[rstest]
    fn test_parse_book_delta_preserves_decimal_precision() {
        let mut handler = create_test_handler();
        handler.subscriptions.mark_subscribe("book:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("book:PI_XBTUSD");
        let json = include_str!("../../../test_data/ws_futures_book_precision.json");

        handler.parse_message(json, true);

        let message = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::BookDelta(delta) = message else {
            panic!("Expected BookDelta message, was {message:?}");
        };
        assert_eq!(delta.price, dec!(123456789.123456789));
        assert_eq!(delta.qty, dec!(0.1234567890123456789012345678));
    }

    #[rstest]
    fn test_parse_book_delta_filters_unsubscribed() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_book_delta.json");

        handler.parse_message(json, true);

        assert!(
            handler.pending_messages.is_empty(),
            "Book delta for unsubscribed product should be filtered"
        );
    }

    #[rstest]
    fn test_parse_open_orders_cancel_emits_cancel() {
        let mut handler = create_test_handler();
        let json = include_str!("../../../test_data/ws_futures_open_orders_cancel.json");

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

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

        handler.parse_message(json, true);

        assert!(handler.pending_messages.is_empty());
    }

    #[rstest]
    fn test_info_event_produces_no_message() {
        let mut handler = create_test_handler();
        let json = r#"{"event":"info","version":1}"#;

        handler.parse_message(json, true);

        assert!(handler.pending_messages.is_empty());
    }
}
