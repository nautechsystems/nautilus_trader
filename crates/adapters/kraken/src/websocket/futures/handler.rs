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

//! WebSocket message handler for Kraken Futures.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use dashmap::DashSet;
use nautilus_common::cache::quote::QuoteCache;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{
        BookOrder, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, TradeTick,
    },
    enums::{AggressorSide, BookAction},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::websocket::WebSocketClient;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::messages::{
    FuturesWsMessage, KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesTickerData,
    KrakenFuturesTradeData, KrakenFuturesTradeSnapshot,
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Subscribe to a product's ticker feed.
    SubscribeTicker(String),
    /// Unsubscribe from a product's ticker feed.
    UnsubscribeTicker(String),
    /// Subscribe to a product's trade feed.
    SubscribeTrade(String),
    /// Unsubscribe from a product's trade feed.
    UnsubscribeTrade(String),
    /// Subscribe to a product's book feed.
    SubscribeBook(String),
    /// Unsubscribe from a product's book feed.
    UnsubscribeBook(String),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

/// WebSocket message handler for Kraken Futures.
pub struct FuturesFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: Arc<DashSet<String>>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    quote_cache: QuoteCache,
    pending_messages: VecDeque<FuturesWsMessage>,
}

impl FuturesFeedHandler {
    /// Creates a new [`FuturesFeedHandler`] instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: Arc<DashSet<String>>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            instruments_cache: AHashMap::new(),
            quote_cache: QuoteCache::new(),
            pending_messages: VecDeque::new(),
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn get_instrument(&self, symbol: &Ustr) -> Option<&InstrumentAny> {
        self.instruments_cache.get(symbol)
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub async fn next(&mut self) -> Option<FuturesWsMessage> {
        // First drain any pending messages from previous ticker processing
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            tracing::debug!("WebSocketClient received by futures handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::SubscribeTicker(product_id) => {
                            self.send_subscribe("ticker", &product_id).await;
                        }
                        HandlerCommand::UnsubscribeTicker(product_id) => {
                            self.send_unsubscribe("ticker", &product_id).await;
                        }
                        HandlerCommand::SubscribeTrade(product_id) => {
                            self.send_subscribe("trade", &product_id).await;
                        }
                        HandlerCommand::UnsubscribeTrade(product_id) => {
                            self.send_unsubscribe("trade", &product_id).await;
                        }
                        HandlerCommand::SubscribeBook(product_id) => {
                            self.send_subscribe("book", &product_id).await;
                        }
                        HandlerCommand::UnsubscribeBook(product_id) => {
                            self.send_unsubscribe("book", &product_id).await;
                        }
                        HandlerCommand::Disconnect => {
                            tracing::debug!("Disconnect command received");
                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                // Key by raw_symbol (e.g., "PI_XBTUSD") since that's what
                                // WebSocket messages use
                                self.instruments_cache.insert(inst.raw_symbol().inner(), inst);
                            }
                            tracing::debug!(
                                "Initialized {} instruments in futures handler cache",
                                self.instruments_cache.len()
                            );
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.raw_symbol().inner(), inst);
                        }
                    }
                    continue;
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            tracing::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }

                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(data) => {
                            match std::str::from_utf8(&data) {
                                Ok(s) => s.to_string(),
                                Err(_) => continue,
                            }
                        }
                        Message::Ping(data) => {
                            tracing::trace!("Received ping frame with {} bytes", data.len());
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                tracing::warn!(error = %e, "Failed to send pong frame");
                            }
                            continue;
                        }
                        Message::Pong(_) => {
                            tracing::trace!("Received pong");
                            continue;
                        }
                        Message::Close(_) => {
                            tracing::info!("WebSocket connection closed");
                            return None;
                        }
                        Message::Frame(_) => {
                            tracing::trace!("Received raw frame");
                            continue;
                        }
                    };

                    let ts_init = self.clock.get_time_ns();
                    self.parse_message(&text, ts_init);

                    // Return first pending message if any were produced
                    if let Some(msg) = self.pending_messages.pop_front() {
                        return Some(msg);
                    }

                    continue;
                }
            }
        }
    }

    async fn send_subscribe(&self, feed: &str, product_id: &str) {
        if let Some(ref client) = self.client {
            let msg = format!(
                r#"{{"event":"subscribe","feed":"{feed}","product_ids":["{product_id}"]}}"#
            );
            if let Err(e) = client.send_text(msg, None).await {
                tracing::error!("Failed to send {feed} subscribe: {e}");
            }
        }
    }

    async fn send_unsubscribe(&self, feed: &str, product_id: &str) {
        if let Some(ref client) = self.client {
            let msg = format!(
                r#"{{"event":"unsubscribe","feed":"{feed}","product_ids":["{product_id}"]}}"#
            );
            if let Err(e) = client.send_text(msg, None).await {
                tracing::error!("Failed to send {feed} unsubscribe: {e}");
            }
        }
    }

    fn parse_message(&mut self, text: &str, ts_init: UnixNanos) {
        // Route to appropriate handler based on feed type
        // Note: Order matters - check more specific patterns first
        if text.contains("\"feed\":\"ticker\"") && text.contains("\"product_id\"") {
            self.handle_ticker_message(text, ts_init);
        } else if text.contains("\"feed\":\"trade_snapshot\"") {
            self.handle_trade_snapshot(text, ts_init);
        } else if text.contains("\"feed\":\"trade\"") && text.contains("\"product_id\"") {
            self.handle_trade_message(text, ts_init);
        } else if text.contains("\"feed\":\"book_snapshot\"") {
            self.handle_book_snapshot(text, ts_init);
        } else if text.contains("\"feed\":\"book\"") && text.contains("\"side\"") {
            self.handle_book_delta(text, ts_init);
        } else if text.contains("\"event\":\"info\"") {
            tracing::debug!("Received info message: {text}");
        } else if text.contains("\"event\":\"subscribed\"") {
            tracing::debug!("Subscription confirmed: {text}");
        } else if text.contains("\"feed\":\"heartbeat\"") {
            tracing::trace!("Heartbeat received");
        } else {
            tracing::debug!("Unhandled message: {text}");
        }
    }

    fn handle_ticker_message(&mut self, text: &str, ts_init: UnixNanos) {
        let ticker = match serde_json::from_str::<KrakenFuturesTickerData>(text) {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("Failed to parse ticker: {e}");
                return;
            }
        };

        // Extract instrument info upfront to avoid borrow conflicts
        let (instrument_id, price_precision) = {
            let Some(instrument) = self.get_instrument(&Ustr::from(ticker.product_id.as_str()))
            else {
                tracing::debug!("Instrument not found for product_id: {}", ticker.product_id);
                return;
            };
            (instrument.id(), instrument.price_precision())
        };

        let ts_event = ticker
            .time
            .map(|t| UnixNanos::from((t as u64) * 1_000_000))
            .unwrap_or(ts_init);

        let product_id = &ticker.product_id;
        let mark_key = format!("mark:{product_id}");
        let index_key = format!("index:{product_id}");
        let has_mark = self.subscriptions.contains(&mark_key);
        let has_index = self.subscriptions.contains(&index_key);

        // Enqueue mark price if present and subscribed
        if let Some(mark_price) = ticker.mark_price
            && has_mark
        {
            let update = MarkPriceUpdate::new(
                instrument_id,
                Price::new(mark_price, price_precision),
                ts_event,
                ts_init,
            );
            self.pending_messages
                .push_back(FuturesWsMessage::MarkPrice(update));
        }

        // Enqueue index price if present and subscribed
        if let Some(index_price) = ticker.index
            && has_index
        {
            let update = IndexPriceUpdate::new(
                instrument_id,
                Price::new(index_price, price_precision),
                ts_event,
                ts_init,
            );
            self.pending_messages
                .push_back(FuturesWsMessage::IndexPrice(update));
        }
    }

    fn handle_trade_message(&mut self, text: &str, ts_init: UnixNanos) {
        let trade = match serde_json::from_str::<KrakenFuturesTradeData>(text) {
            Ok(t) => t,
            Err(e) => {
                tracing::trace!("Failed to parse trade: {e}");
                return;
            }
        };

        // Check if subscribed to trades for this product
        if !self
            .subscriptions
            .contains(&format!("trades:{}", trade.product_id))
        {
            return;
        }

        // Extract instrument info upfront to avoid borrow conflicts
        let (instrument_id, price_precision, size_precision) = {
            let Some(instrument) = self.get_instrument(&Ustr::from(trade.product_id.as_str()))
            else {
                return;
            };
            (
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            )
        };

        let ts_event = UnixNanos::from((trade.time as u64) * 1_000_000);

        let aggressor_side = match trade.side.as_str() {
            "buy" => AggressorSide::Buyer,
            "sell" => AggressorSide::Seller,
            _ => AggressorSide::NoAggressor,
        };

        let trade_id = trade.uid.unwrap_or_else(|| trade.seq.to_string());

        let trade_tick = TradeTick::new(
            instrument_id,
            Price::new(trade.price, price_precision),
            Quantity::new(trade.qty, size_precision),
            aggressor_side,
            TradeId::new(&trade_id),
            ts_event,
            ts_init,
        );

        self.pending_messages
            .push_back(FuturesWsMessage::Trade(trade_tick));
    }

    fn handle_trade_snapshot(&mut self, text: &str, ts_init: UnixNanos) {
        let snapshot = match serde_json::from_str::<KrakenFuturesTradeSnapshot>(text) {
            Ok(s) => s,
            Err(e) => {
                tracing::trace!("Failed to parse trade snapshot: {e}");
                return;
            }
        };

        // Check if subscribed to trades for this product
        if !self
            .subscriptions
            .contains(&format!("trades:{}", snapshot.product_id))
        {
            return;
        }

        // Extract instrument info upfront
        let (instrument_id, price_precision, size_precision) = {
            let Some(instrument) = self.get_instrument(&Ustr::from(snapshot.product_id.as_str()))
            else {
                return;
            };
            (
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            )
        };

        // Process each trade in the snapshot
        for trade in snapshot.trades {
            let ts_event = UnixNanos::from((trade.time as u64) * 1_000_000);

            let aggressor_side = match trade.side.as_str() {
                "buy" => AggressorSide::Buyer,
                "sell" => AggressorSide::Seller,
                _ => AggressorSide::NoAggressor,
            };

            let trade_id = trade.uid.unwrap_or_else(|| trade.seq.to_string());

            let trade_tick = TradeTick::new(
                instrument_id,
                Price::new(trade.price, price_precision),
                Quantity::new(trade.qty, size_precision),
                aggressor_side,
                TradeId::new(&trade_id),
                ts_event,
                ts_init,
            );

            self.pending_messages
                .push_back(FuturesWsMessage::Trade(trade_tick));
        }
    }

    fn handle_book_snapshot(&mut self, text: &str, ts_init: UnixNanos) {
        let snapshot = match serde_json::from_str::<KrakenFuturesBookSnapshot>(text) {
            Ok(s) => s,
            Err(e) => {
                tracing::trace!("Failed to parse book snapshot: {e}");
                return;
            }
        };

        let product_id = &snapshot.product_id;

        // Check subscriptions
        let has_book = self.subscriptions.contains(&format!("book:{product_id}"));
        let has_quotes = self.subscriptions.contains(&format!("quotes:{product_id}"));

        if !has_book && !has_quotes {
            return;
        }

        // Extract instrument info upfront to avoid borrow conflicts
        let (instrument_id, price_precision, size_precision) = {
            let Some(instrument) = self.get_instrument(&Ustr::from(snapshot.product_id.as_str()))
            else {
                return;
            };
            (
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            )
        };

        let ts_event = UnixNanos::from((snapshot.timestamp as u64) * 1_000_000);

        // Extract best bid/ask for quotes
        let best_bid = snapshot
            .bids
            .iter()
            .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
        let best_ask = snapshot
            .asks
            .iter()
            .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

        // Emit quote if subscribed, using QuoteCache for handling partial updates
        if has_quotes {
            let bid_price = best_bid.map(|b| Price::new(b.price, price_precision));
            let ask_price = best_ask.map(|a| Price::new(a.price, price_precision));
            let bid_size = best_bid.map(|b| Quantity::new(b.qty, size_precision));
            let ask_size = best_ask.map(|a| Quantity::new(a.qty, size_precision));

            match self.quote_cache.process(
                instrument_id,
                bid_price,
                ask_price,
                bid_size,
                ask_size,
                ts_event,
                ts_init,
            ) {
                Ok(quote) => {
                    self.pending_messages
                        .push_back(FuturesWsMessage::Quote(quote));
                }
                Err(e) => {
                    tracing::trace!("Quote cache miss for {instrument_id}: {e}");
                }
            }
        }

        // Emit book deltas if subscribed
        if has_book {
            let mut deltas = Vec::new();

            // Clear action first
            deltas.push(OrderBookDelta::clear(
                instrument_id,
                snapshot.seq as u64,
                ts_event,
                ts_init,
            ));

            // Add bids
            for level in &snapshot.bids {
                let order = BookOrder::new(
                    nautilus_model::enums::OrderSide::Buy,
                    Price::new(level.price, price_precision),
                    Quantity::new(level.qty, size_precision),
                    0,
                );
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    order,
                    0,
                    snapshot.seq as u64,
                    ts_event,
                    ts_init,
                ));
            }

            // Add asks
            for level in &snapshot.asks {
                let order = BookOrder::new(
                    nautilus_model::enums::OrderSide::Sell,
                    Price::new(level.price, price_precision),
                    Quantity::new(level.qty, size_precision),
                    0,
                );
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    order,
                    0,
                    snapshot.seq as u64,
                    ts_event,
                    ts_init,
                ));
            }

            let book_deltas = OrderBookDeltas::new(instrument_id, deltas);
            self.pending_messages
                .push_back(FuturesWsMessage::BookDeltas(book_deltas));
        }
    }

    fn handle_book_delta(&mut self, text: &str, ts_init: UnixNanos) {
        let delta = match serde_json::from_str::<KrakenFuturesBookDelta>(text) {
            Ok(d) => d,
            Err(e) => {
                tracing::trace!("Failed to parse book delta: {e}");
                return;
            }
        };

        let product_id = &delta.product_id;

        // Check subscriptions - quotes also uses book feed
        let has_book = self.subscriptions.contains(&format!("book:{product_id}"));
        let has_quotes = self.subscriptions.contains(&format!("quotes:{product_id}"));

        // Need at least one subscription to process
        if !has_book && !has_quotes {
            return;
        }

        let Some(instrument) = self.get_instrument(&Ustr::from(delta.product_id.as_str())) else {
            return;
        };

        let ts_event = UnixNanos::from((delta.timestamp as u64) * 1_000_000);
        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let side = match delta.side.as_str() {
            "buy" => nautilus_model::enums::OrderSide::Buy,
            "sell" => nautilus_model::enums::OrderSide::Sell,
            _ => return,
        };

        // Emit quote update if subscribed (QuoteCache handles partial updates)
        // Note: This assumes the delta represents top-of-book, which is an approximation.
        // For accurate BBO tracking, would need to maintain full local order book.
        if has_quotes && delta.qty > 0.0 {
            let price = Price::new(delta.price, price_precision);
            let size = Quantity::new(delta.qty, size_precision);

            let (bid_price, ask_price, bid_size, ask_size) = match side {
                nautilus_model::enums::OrderSide::Buy => (Some(price), None, Some(size), None),
                nautilus_model::enums::OrderSide::Sell => (None, Some(price), None, Some(size)),
                _ => (None, None, None, None),
            };

            if let Ok(quote) = self.quote_cache.process(
                instrument_id,
                bid_price,
                ask_price,
                bid_size,
                ask_size,
                ts_event,
                ts_init,
            ) {
                self.pending_messages
                    .push_back(FuturesWsMessage::Quote(quote));
            }
        }

        // Emit book delta if subscribed
        if has_book {
            let action = if delta.qty == 0.0 {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let order = BookOrder::new(
                side,
                Price::new(delta.price, price_precision),
                Quantity::new(delta.qty, size_precision),
                0,
            );

            let book_delta = OrderBookDelta::new(
                instrument_id,
                action,
                order,
                0,
                delta.seq as u64,
                ts_event,
                ts_init,
            );

            let book_deltas = OrderBookDeltas::new(instrument_id, vec![book_delta]);
            self.pending_messages
                .push_back(FuturesWsMessage::BookDeltas(book_deltas));
        }
    }
}
