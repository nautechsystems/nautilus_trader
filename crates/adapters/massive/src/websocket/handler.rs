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

//! Feed handler for parsing Massive WebSocket messages into Nautilus types.

use std::{fmt::Debug, sync::Arc};

use ahash::AHashMap;
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::data::{Bar, QuoteTick, TradeTick, bar::BarType};
use nautilus_network::{RECONNECTED, websocket::WebSocketClient};
use tokio_tungstenite::tungstenite::Message;

use crate::websocket::{
    client::MASSIVE_WS_SUBSCRIPTION_KEYS,
    messages::{MassiveWsEvent, MassiveWsRequest, MassiveWsStatus},
    parse::{parse_ws_aggregate, parse_ws_quote, parse_ws_trade},
};

/// Commands sent from [`super::client::MassiveWebSocketClient`] to the feed handler.
pub enum HandlerCommand {
    /// Provides the network-level WebSocket client.
    SetClient(WebSocketClient),
    /// Sends the raw authentication message (built by the owning client so
    /// the credential never crosses the channel in structured form).
    Authenticate(String),
    /// Sends a subscribe request for the given topics.
    Subscribe(Vec<String>),
    /// Sends an unsubscribe request for the given topics.
    Unsubscribe(Vec<String>),
    /// Disconnects the WebSocket.
    Disconnect,
    /// Registers a bar type for aggregate parsing, keyed by wire topic
    /// (e.g. `AM.AAPL`).
    AddBarType { key: String, bar_type: BarType },
    /// Removes a bar type registration.
    RemoveBarType { key: String },
}

impl Debug for HandlerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetClient(_) => f.write_str("SetClient"),
            Self::Authenticate(_) => f.write_str("Authenticate(***)"),
            Self::Subscribe(topics) => write!(f, "Subscribe({topics:?})"),
            Self::Unsubscribe(topics) => write!(f, "Unsubscribe({topics:?})"),
            Self::Disconnect => f.write_str("Disconnect"),
            Self::AddBarType { key, .. } => write!(f, "AddBarType({key})"),
            Self::RemoveBarType { key } => write!(f, "RemoveBarType({key})"),
        }
    }
}

/// Nautilus-typed messages produced by the feed handler.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Trade tick from the `T` channel.
    Trade(TradeTick),
    /// Quote tick from the `Q` channel.
    Quote(QuoteTick),
    /// Bar from the `A` or `AM` channels.
    Bar(Bar),
    /// The connection was re-established after a drop.
    Reconnected,
    /// An error occurred (including authentication failures).
    Error(String),
}

/// Processes raw WebSocket messages into Nautilus domain types.
#[derive(Debug)]
pub struct FeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<std::sync::atomic::AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    bar_types: AHashMap<String, BarType>,
    bars_timestamp_on_close: bool,
    buffer: Vec<NautilusWsMessage>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub fn new(
        signal: Arc<std::sync::atomic::AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        bars_timestamp_on_close: bool,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            bar_types: AHashMap::new(),
            bars_timestamp_on_close,
            buffer: Vec::new(),
        }
    }

    /// Polls for the next output message, processing commands and raw messages.
    ///
    /// Returns `None` when the handler should shut down.
    pub async fn next(&mut self) -> Option<NautilusWsMessage> {
        // Check signal before draining buffer so disconnect takes
        // priority over pending buffered messages
        if self.signal.load(std::sync::atomic::Ordering::Acquire) {
            self.buffer.clear();
            return None;
        }

        if let Some(msg) = self.buffer.pop() {
            return Some(msg);
        }

        loop {
            if self.signal.load(std::sync::atomic::Ordering::Acquire) {
                return None;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            self.client = Some(client);
                        }
                        HandlerCommand::Authenticate(message) => {
                            self.send_text(message).await;
                        }
                        HandlerCommand::Subscribe(topics) => {
                            self.send_request(&MassiveWsRequest::subscribe(&topics)).await;
                        }
                        HandlerCommand::Unsubscribe(topics) => {
                            self.send_request(&MassiveWsRequest::unsubscribe(&topics)).await;
                        }
                        HandlerCommand::Disconnect => {
                            if let Some(client) = self.client.take() {
                                // Transition to CLOSED immediately without waiting
                                // for ACTIVE (avoids blocking during reconnect)
                                client.notify_closed();
                            }
                            return None;
                        }
                        HandlerCommand::AddBarType { key, bar_type } => {
                            self.bar_types.insert(key, bar_type);
                        }
                        HandlerCommand::RemoveBarType { key } => {
                            self.bar_types.remove(&key);
                        }
                    }
                }
                Some(raw) = self.raw_rx.recv() => {
                    match raw {
                        Message::Text(text) => {
                            if let Some(msg) = self.handle_text(&text) {
                                return Some(msg);
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                log::error!("Failed to send pong: {e}");
                            }
                        }
                        Message::Close(_) => return None,
                        _ => {}
                    }
                }
                else => return None,
            }
        }
    }

    async fn send_request(&self, request: &MassiveWsRequest) {
        match serde_json::to_string(request) {
            Ok(json) => self.send_text(json).await,
            Err(e) => log::error!("Failed to serialize request: {e}"),
        }
    }

    async fn send_text(&self, text: String) {
        let Some(client) = &self.client else {
            log::warn!("Cannot send message, no WebSocket client set");
            return;
        };

        if let Err(e) = client
            .send_text(text, Some(MASSIVE_WS_SUBSCRIPTION_KEYS.as_slice()))
            .await
        {
            log::error!("Failed to send message: {e}");
        }
    }

    fn handle_text(&mut self, text: &str) -> Option<NautilusWsMessage> {
        if text == RECONNECTED {
            return Some(NautilusWsMessage::Reconnected);
        }

        let ts_init = self.clock.get_time_ns();

        // Events arrive in JSON arrays; parse elements individually so an
        // unknown event type does not drop the whole batch.
        let items: Vec<serde_json::Value> = match serde_json::from_str(text) {
            Ok(serde_json::Value::Array(items)) => items,
            Ok(single) => vec![single],
            Err(e) => {
                log::warn!("Failed to parse WS message: {e}");
                return None;
            }
        };

        let mut first: Option<NautilusWsMessage> = None;

        for item in items {
            let event: MassiveWsEvent = match serde_json::from_value(item) {
                Ok(event) => event,
                Err(e) => {
                    log::debug!("Skipping unrecognized WS event: {e}");
                    continue;
                }
            };

            if let Some(msg) = self.handle_event(&event, ts_init) {
                if first.is_none() {
                    first = Some(msg);
                } else {
                    self.buffer.push(msg);
                }
            }
        }

        if first.is_some() {
            // Reverse so pop() drains in wire order
            self.buffer.reverse();
        }
        first
    }

    fn handle_event(
        &self,
        event: &MassiveWsEvent,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        match event {
            MassiveWsEvent::Trade(trade) => match parse_ws_trade(trade, ts_init) {
                Ok(Some(tick)) => Some(NautilusWsMessage::Trade(tick)),
                Ok(None) => None,
                Err(e) => {
                    log::warn!("Failed to parse trade: {e}");
                    None
                }
            },
            MassiveWsEvent::Quote(quote) => match parse_ws_quote(quote, ts_init) {
                Ok(Some(tick)) => Some(NautilusWsMessage::Quote(tick)),
                Ok(None) => None,
                Err(e) => {
                    log::warn!("Failed to parse quote: {e}");
                    None
                }
            },
            MassiveWsEvent::AggregateSecond(agg) => self.handle_aggregate(agg, "A", ts_init),
            MassiveWsEvent::AggregateMinute(agg) => self.handle_aggregate(agg, "AM", ts_init),
            MassiveWsEvent::Status(status) => self.handle_status(status),
        }
    }

    fn handle_aggregate(
        &self,
        agg: &crate::websocket::messages::MassiveWsAggregate,
        channel: &str,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let key = format!("{channel}.{}", agg.sym);

        let bar_type = match self.bar_types.get(&key) {
            Some(bt) => *bt,
            None => {
                log::debug!("No bar type registered for {key}");
                return None;
            }
        };

        match parse_ws_aggregate(agg, bar_type, self.bars_timestamp_on_close, ts_init) {
            Ok(bar) => Some(NautilusWsMessage::Bar(bar)),
            Err(e) => {
                log::warn!("Failed to parse aggregate: {e}");
                None
            }
        }
    }

    fn handle_status(&self, status: &MassiveWsStatus) -> Option<NautilusWsMessage> {
        match status.status.as_str() {
            "connected" => {
                log::debug!("Massive WebSocket connected");
                None
            }
            "auth_success" => {
                log::info!("Massive WebSocket authenticated");
                None
            }
            "auth_failed" | "auth_timeout" => {
                let message = status.message.as_deref().unwrap_or("unknown");
                Some(NautilusWsMessage::Error(format!(
                    "Authentication failed: {message}"
                )))
            }
            "success" => {
                log::debug!(
                    "Massive WebSocket: {}",
                    status.message.as_deref().unwrap_or("success")
                );
                None
            }
            "error" => {
                let message = status.message.as_deref().unwrap_or("unknown");
                Some(NautilusWsMessage::Error(format!("Venue error: {message}")))
            }
            "max_connections" => {
                // The venue enforces a per-account connection limit; another
                // consumer of the same API key will terminate this session.
                let message = status.message.as_deref().unwrap_or("unknown");
                Some(NautilusWsMessage::Error(format!(
                    "Connection limit exceeded (another session may be using this API key): {message}"
                )))
            }
            other => {
                log::debug!("Unhandled status '{other}': {:?}", status.message);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use nautilus_model::{
        data::bar::BarSpecification,
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::InstrumentId,
        types::Price,
    };
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_fixture;

    fn test_handler() -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        FeedHandler::new(Arc::new(AtomicBool::new(false)), cmd_rx, raw_rx, true)
    }

    #[rstest]
    fn test_handle_text_trade() {
        let mut handler = test_handler();
        let json = load_test_fixture("ws_trade.json");

        let msg = handler.handle_text(&json).expect("expected a message");
        let NautilusWsMessage::Trade(tick) = msg else {
            panic!("expected Trade, was {msg:?}");
        };
        assert_eq!(tick.instrument_id.to_string(), "MSFT.MASSIVE");
        assert_eq!(tick.price, Price::from("114.125"));
        assert!(handler.buffer.is_empty());
    }

    #[rstest]
    fn test_handle_text_quote() {
        let mut handler = test_handler();
        let json = load_test_fixture("ws_quote.json");

        let msg = handler.handle_text(&json).expect("expected a message");
        assert!(matches!(msg, NautilusWsMessage::Quote(_)));
    }

    #[rstest]
    fn test_handle_text_aggregates_require_registration() {
        let mut handler = test_handler();
        let json = load_test_fixture("ws_aggregates.json");

        // Without registered bar types, aggregates are dropped
        assert!(handler.handle_text(&json).is_none());

        let spce_bar_type = BarType::new(
            InstrumentId::from("SPCE.MASSIVE"),
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last),
            AggregationSource::External,
        );
        handler
            .bar_types
            .insert("A.SPCE".to_string(), spce_bar_type);

        let msg = handler.handle_text(&json).expect("expected a message");
        let NautilusWsMessage::Bar(bar) = msg else {
            panic!("expected Bar, was {msg:?}");
        };
        assert_eq!(bar.bar_type, spce_bar_type);
        // Bar stamped on window end (timestamp_on_close = true)
        assert_eq!(bar.ts_event.as_u64(), 1_610_144_869_000_000_000);
        // The GTE minute aggregate is still unregistered
        assert!(handler.buffer.is_empty());
    }

    #[rstest]
    fn test_handle_text_status_events_produce_no_output() {
        let mut handler = test_handler();
        let json = load_test_fixture("ws_status.json");
        assert!(handler.handle_text(&json).is_none());
    }

    #[rstest]
    fn test_handle_text_max_connections_emits_error() {
        let mut handler = test_handler();
        let json = r#"[{"ev":"status","status":"max_connections","message":"Maximum number of websocket connections exceeded"}]"#;

        let msg = handler.handle_text(json).expect("expected a message");
        let NautilusWsMessage::Error(text) = msg else {
            panic!("expected Error, was {msg:?}");
        };
        assert!(text.contains("Connection limit exceeded"));
    }

    #[rstest]
    fn test_handle_text_auth_failed_emits_error() {
        let mut handler = test_handler();
        let json = r#"[{"ev":"status","status":"auth_failed","message":"invalid key"}]"#;

        let msg = handler.handle_text(json).expect("expected a message");
        let NautilusWsMessage::Error(text) = msg else {
            panic!("expected Error, was {msg:?}");
        };
        assert!(text.contains("invalid key"));
    }

    #[rstest]
    fn test_handle_text_unknown_event_skipped() {
        let mut handler = test_handler();
        let json = r#"[{"ev":"LULD","sym":"AAPL"},{"ev":"status","status":"connected"}]"#;
        assert!(handler.handle_text(json).is_none());
    }

    #[rstest]
    fn test_handle_text_batch_preserves_order() {
        let mut handler = test_handler();
        let json = r#"[
            {"ev":"T","sym":"AAPL","i":"1","p":100.01,"s":10,"t":1536036818784,"q":1},
            {"ev":"T","sym":"AAPL","i":"2","p":100.02,"s":20,"t":1536036818785,"q":2},
            {"ev":"T","sym":"AAPL","i":"3","p":100.03,"s":30,"t":1536036818786,"q":3}
        ]"#;

        let first = handler.handle_text(json).expect("expected a message");
        let NautilusWsMessage::Trade(tick) = first else {
            panic!("expected Trade");
        };
        assert_eq!(tick.trade_id.to_string(), "1");

        let NautilusWsMessage::Trade(second) = handler.buffer.pop().unwrap() else {
            panic!("expected buffered Trade");
        };
        assert_eq!(second.trade_id.to_string(), "2");

        let NautilusWsMessage::Trade(third) = handler.buffer.pop().unwrap() else {
            panic!("expected buffered Trade");
        };
        assert_eq!(third.trade_id.to_string(), "3");
    }

    #[rstest]
    fn test_handle_text_reconnected_sentinel() {
        let mut handler = test_handler();
        let result = handler.handle_text(RECONNECTED);
        assert!(matches!(result, Some(NautilusWsMessage::Reconnected)));
    }

    #[rstest]
    fn test_signal_exits_handler_loop() {
        use std::sync::atomic::Ordering;

        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler = FeedHandler::new(signal.clone(), cmd_rx, raw_rx, true);

        signal.store(true, Ordering::Release);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async { handler.next().await });
        assert!(result.is_none(), "{result:?}");
    }
}
