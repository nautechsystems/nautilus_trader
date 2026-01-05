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

//! Binance Spot WebSocket message handler.
//!
//! The handler runs in a dedicated Tokio task as the I/O boundary between the client
//! orchestrator and the network layer. It exclusively owns the `WebSocketClient` and
//! processes commands from the client via an unbounded channel.
//!
//! Key responsibilities:
//! - Command processing: Receives `HandlerCommand` from client, executes WebSocket operations.
//! - SBE binary decoding: Routes binary frames to appropriate SBE decoders.
//! - Message transformation: Parses raw venue messages into Nautilus domain events.
//! - Subscription tracking: Manages pending subscription state.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::messages::{
    BinanceWsErrorMsg, BinanceWsErrorResponse, BinanceWsResponse, BinanceWsSubscription,
    HandlerCommand, NautilusWsMessage,
};
use crate::common::{
    fixed::{mantissa_to_price, mantissa_to_quantity},
    sbe::stream::{
        BestBidAskStreamEvent, DepthDiffStreamEvent, DepthSnapshotStreamEvent, MessageHeader,
        StreamDecodeError, TradesStreamEvent, template_id,
    },
};

/// Decoded market data message.
#[derive(Debug)]
pub enum MarketDataMessage {
    /// Trade event.
    Trades(TradesStreamEvent),
    /// Best bid/ask update.
    BestBidAsk(BestBidAskStreamEvent),
    /// Order book snapshot.
    DepthSnapshot(DepthSnapshotStreamEvent),
    /// Order book diff update.
    DepthDiff(DepthDiffStreamEvent),
}

/// Decode an SBE binary frame into a market data message.
///
/// Validates the message header (including schema ID) and routes to the
/// appropriate decoder based on template ID.
pub fn decode_market_data(buf: &[u8]) -> Result<MarketDataMessage, StreamDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate_schema()?;

    match header.template_id {
        template_id::TRADES_STREAM_EVENT => {
            Ok(MarketDataMessage::Trades(TradesStreamEvent::decode(buf)?))
        }
        template_id::BEST_BID_ASK_STREAM_EVENT => Ok(MarketDataMessage::BestBidAsk(
            BestBidAskStreamEvent::decode(buf)?,
        )),
        template_id::DEPTH_SNAPSHOT_STREAM_EVENT => Ok(MarketDataMessage::DepthSnapshot(
            DepthSnapshotStreamEvent::decode(buf)?,
        )),
        template_id::DEPTH_DIFF_STREAM_EVENT => Ok(MarketDataMessage::DepthDiff(
            DepthDiffStreamEvent::decode(buf)?,
        )),
        _ => Err(StreamDecodeError::UnknownTemplateId(header.template_id)),
    }
}

/// Binance Spot WebSocket feed handler.
///
/// Runs in a dedicated Tokio task, processing commands from the client
/// and transforming raw WebSocket messages into Nautilus domain events.
pub(super) struct BinanceSpotWsFeedHandler {
    #[allow(dead_code)] // Reserved for shutdown signal handling
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    #[allow(dead_code)] // Reserved for async message emission
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    subscriptions: SubscriptionState,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    request_id_counter: Arc<AtomicU64>,
    pending_messages: VecDeque<NautilusWsMessage>,
    pending_requests: AHashMap<u64, Vec<String>>,
}

impl BinanceSpotWsFeedHandler {
    /// Creates a new handler instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        subscriptions: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            subscriptions,
            instruments_cache: AHashMap::new(),
            request_id_counter,
            pending_messages: VecDeque::new(),
            pending_requests: AHashMap::new(),
        }
    }

    /// Main event loop - processes commands and raw messages.
    ///
    /// Returns `Some(message)` when there's output to emit, `None` when disconnected.
    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        // Return any pending messages first
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                        HandlerCommand::Subscribe { streams } => {
                            if let Err(e) = self.handle_subscribe(streams).await {
                                log::error!("Failed to handle subscribe command: {e}");
                            }
                        }
                        HandlerCommand::Unsubscribe { streams } => {
                            if let Err(e) = self.handle_unsubscribe(streams).await {
                                log::error!("Failed to handle unsubscribe command: {e}");
                            }
                        }
                    }
                }
                Some(msg) = self.raw_rx.recv() => {
                    if let Message::Text(ref text) = msg
                        && text.as_str() == RECONNECTED
                    {
                        log::info!("Handler received reconnection signal");
                        return Some(NautilusWsMessage::Reconnected);
                    }

                    let messages = self.handle_message(msg);
                    if !messages.is_empty() {
                        let mut iter = messages.into_iter();
                        let first = iter.next();
                        self.pending_messages.extend(iter);
                        if let Some(msg) = first {
                            return Some(msg);
                        }
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    /// Handle incoming WebSocket message.
    fn handle_message(&mut self, msg: Message) -> Vec<NautilusWsMessage> {
        match msg {
            Message::Binary(data) => self.handle_binary_frame(&data),
            Message::Text(text) => self.handle_text_frame(&text),
            Message::Close(_) => {
                log::debug!("Received close frame");
                vec![]
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => vec![],
        }
    }

    /// Handle binary SBE frame.
    fn handle_binary_frame(&mut self, data: &[u8]) -> Vec<NautilusWsMessage> {
        match decode_market_data(data) {
            Ok(MarketDataMessage::Trades(event)) => self.parse_trades_event(event),
            Ok(MarketDataMessage::BestBidAsk(event)) => self.parse_bbo_event(event),
            Ok(MarketDataMessage::DepthSnapshot(event)) => self.parse_depth_snapshot(event),
            Ok(MarketDataMessage::DepthDiff(event)) => self.parse_depth_diff(event),
            Err(e) => {
                log::error!("SBE decode error: {e}");
                vec![NautilusWsMessage::RawBinary(data.to_vec())]
            }
        }
    }

    /// Handle text JSON frame.
    fn handle_text_frame(&mut self, text: &str) -> Vec<NautilusWsMessage> {
        if let Ok(response) = serde_json::from_str::<BinanceWsResponse>(text) {
            self.handle_subscription_response(response);
            return vec![];
        }

        // Error response includes id for request correlation
        if let Ok(error) = serde_json::from_str::<BinanceWsErrorResponse>(text) {
            if let Some(id) = error.id
                && let Some(streams) = self.pending_requests.remove(&id)
            {
                for stream in &streams {
                    self.subscriptions.mark_failure(stream);
                }
                log::warn!(
                    "Subscription request failed: id={id}, streams={streams:?}, code={}, msg={}",
                    error.code,
                    error.msg
                );
            }
            return vec![NautilusWsMessage::Error(BinanceWsErrorMsg {
                code: error.code,
                msg: error.msg,
            })];
        }

        if let Ok(value) = serde_json::from_str(text) {
            vec![NautilusWsMessage::RawJson(value)]
        } else {
            log::warn!("Failed to parse JSON message: {text}");
            vec![]
        }
    }

    /// Handle subscription response.
    fn handle_subscription_response(&mut self, response: BinanceWsResponse) {
        if let Some(streams) = self.pending_requests.remove(&response.id) {
            if response.result.is_none() {
                // Success - confirm subscriptions
                for stream in &streams {
                    self.subscriptions.confirm_subscribe(stream);
                }
                log::debug!("Subscription confirmed: streams={streams:?}");
            } else {
                // Failure - mark streams as failed
                for stream in &streams {
                    self.subscriptions.mark_failure(stream);
                }
                log::warn!(
                    "Subscription failed: streams={streams:?}, result={:?}",
                    response.result
                );
            }
        } else {
            log::debug!("Received response for unknown request: id={}", response.id);
        }
    }

    /// Parse trades stream event into Nautilus TradeTicks.
    fn parse_trades_event(&self, event: TradesStreamEvent) -> Vec<NautilusWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!("No instrument in cache for trades: symbol={}", event.symbol);
            return vec![];
        };

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let trades: Vec<Data> = event
            .trades
            .iter()
            .map(|t| {
                let price =
                    mantissa_to_price(t.price_mantissa, event.price_exponent, price_precision);
                let size = mantissa_to_quantity(t.qty_mantissa, event.qty_exponent, size_precision);
                let ts_event = UnixNanos::from(event.transact_time_us as u64 * 1000); // us to ns

                let trade = TradeTick::new(
                    instrument_id,
                    price,
                    size,
                    if t.is_buyer_maker {
                        AggressorSide::Seller
                    } else {
                        AggressorSide::Buyer
                    },
                    TradeId::new(t.id.to_string()),
                    ts_event,
                    ts_event, // ts_init same as ts_event
                );
                Data::from(trade)
            })
            .collect();

        if trades.is_empty() {
            vec![]
        } else {
            vec![NautilusWsMessage::Data(trades)]
        }
    }

    /// Parse best bid/ask event into Nautilus QuoteTick.
    fn parse_bbo_event(&self, event: BestBidAskStreamEvent) -> Vec<NautilusWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!("No instrument in cache for BBO: symbol={}", event.symbol);
            return vec![];
        };

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let bid_price = mantissa_to_price(
            event.bid_price_mantissa,
            event.price_exponent,
            price_precision,
        );
        let bid_size =
            mantissa_to_quantity(event.bid_qty_mantissa, event.qty_exponent, size_precision);
        let ask_price = mantissa_to_price(
            event.ask_price_mantissa,
            event.price_exponent,
            price_precision,
        );
        let ask_size =
            mantissa_to_quantity(event.ask_qty_mantissa, event.qty_exponent, size_precision);
        let ts_event = UnixNanos::from(event.event_time_us as u64 * 1000); // us to ns

        let quote = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_event,
        );

        vec![NautilusWsMessage::Data(vec![Data::from(quote)])]
    }

    /// Parse depth snapshot event into Nautilus OrderBookDeltas.
    fn parse_depth_snapshot(&self, event: DepthSnapshotStreamEvent) -> Vec<NautilusWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!(
                "No instrument in cache for depth snapshot: symbol={}",
                event.symbol
            );
            return vec![];
        };

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_event = UnixNanos::from(event.event_time_us as u64 * 1000);

        let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len() + 1);

        // Add clear delta first
        deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_event));

        // Add bid levels
        for (i, level) in event.bids.iter().enumerate() {
            let price =
                mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
            let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);
            let flags = if i == event.bids.len() - 1 && event.asks.is_empty() {
                RecordFlag::F_LAST as u8
            } else {
                0
            };

            let order = BookOrder::new(
                OrderSide::Buy,
                price,
                size,
                0, // order_id
            );

            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                0, // sequence
                ts_event,
                ts_event,
            ));
        }

        // Add ask levels
        for (i, level) in event.asks.iter().enumerate() {
            let price =
                mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
            let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);
            let flags = if i == event.asks.len() - 1 {
                RecordFlag::F_LAST as u8
            } else {
                0
            };

            let order = BookOrder::new(
                OrderSide::Sell,
                price,
                size,
                0, // order_id
            );

            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                0, // sequence
                ts_event,
                ts_event,
            ));
        }

        if deltas.len() <= 1 {
            return vec![];
        }

        vec![NautilusWsMessage::Deltas(OrderBookDeltas::new(
            instrument_id,
            deltas,
        ))]
    }

    /// Parse depth diff event into Nautilus OrderBookDeltas.
    fn parse_depth_diff(&self, event: DepthDiffStreamEvent) -> Vec<NautilusWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!(
                "No instrument in cache for depth diff: symbol={}",
                event.symbol
            );
            return vec![];
        };

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_event = UnixNanos::from(event.event_time_us as u64 * 1000);

        let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len());

        // Add bid updates
        for (i, level) in event.bids.iter().enumerate() {
            let price =
                mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
            let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);

            // Zero size means delete, otherwise update
            let action = if level.qty_mantissa == 0 {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let flags = if i == event.bids.len() - 1 && event.asks.is_empty() {
                RecordFlag::F_LAST as u8
            } else {
                0
            };

            let order = BookOrder::new(
                OrderSide::Buy,
                price,
                size,
                0, // order_id
            );

            deltas.push(OrderBookDelta::new(
                instrument_id,
                action,
                order,
                flags,
                0, // sequence
                ts_event,
                ts_event,
            ));
        }

        // Add ask updates
        for (i, level) in event.asks.iter().enumerate() {
            let price =
                mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
            let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);

            let action = if level.qty_mantissa == 0 {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let flags = if i == event.asks.len() - 1 {
                RecordFlag::F_LAST as u8
            } else {
                0
            };

            let order = BookOrder::new(
                OrderSide::Sell,
                price,
                size,
                0, // order_id
            );

            deltas.push(OrderBookDelta::new(
                instrument_id,
                action,
                order,
                flags,
                0, // sequence
                ts_event,
                ts_event,
            ));
        }

        if deltas.is_empty() {
            return vec![];
        }

        vec![NautilusWsMessage::Deltas(OrderBookDeltas::new(
            instrument_id,
            deltas,
        ))]
    }

    /// Handle subscribe command.
    async fn handle_subscribe(&mut self, streams: Vec<String>) -> anyhow::Result<()> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let request = BinanceWsSubscription::subscribe(streams.clone(), request_id);
        let payload = serde_json::to_string(&request)?;

        // Track pending request for confirmation
        self.pending_requests.insert(request_id, streams.clone());

        // Mark streams as pending
        for stream in &streams {
            self.subscriptions.mark_subscribe(stream);
        }

        self.send_text(payload).await?;
        Ok(())
    }

    /// Handle unsubscribe command.
    async fn handle_unsubscribe(&mut self, streams: Vec<String>) -> anyhow::Result<()> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let request = BinanceWsSubscription::unsubscribe(streams.clone(), request_id);
        let payload = serde_json::to_string(&request)?;

        self.send_text(payload).await?;

        // Immediately confirm unsubscribe (don't wait for response)
        // We don't track unsubscribe failures - the stream will simply stop
        for stream in &streams {
            self.subscriptions.mark_unsubscribe(stream);
            self.subscriptions.confirm_unsubscribe(stream);
        }

        Ok(())
    }

    /// Send text message via WebSocket.
    async fn send_text(&self, payload: String) -> anyhow::Result<()> {
        let Some(client) = &self.inner else {
            anyhow::bail!("No active WebSocket client");
        };
        client
            .send_text(payload, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::sbe::stream::STREAM_SCHEMA_ID;

    #[rstest]
    fn test_decode_empty_buffer() {
        let err = decode_market_data(&[]).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_short_buffer() {
        let buf = [0u8; 5];
        let err = decode_market_data(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_wrong_schema() {
        let mut buf = [0u8; 100];
        buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::BEST_BID_ASK_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&99u16.to_le_bytes()); // Wrong schema
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        let err = decode_market_data(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::SchemaMismatch { .. }));
    }

    #[rstest]
    fn test_decode_unknown_template() {
        let mut buf = [0u8; 100];
        buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&9999u16.to_le_bytes()); // Unknown template
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        let err = decode_market_data(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::UnknownTemplateId(9999)));
    }
}
