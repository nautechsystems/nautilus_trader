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
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_common::cache::quote::QuoteCache;
use nautilus_core::{AtomicTime, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{
        BookOrder, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, TradeTick,
    },
    enums::{
        AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
    },
    events::{OrderAccepted, OrderCanceled, OrderExpired, OrderUpdated},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
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
    KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesChallengeRequest,
    KrakenFuturesChannel, KrakenFuturesEvent, KrakenFuturesFeed, KrakenFuturesFill,
    KrakenFuturesFillsDelta, KrakenFuturesMessageType, KrakenFuturesOpenOrder,
    KrakenFuturesOpenOrdersCancel, KrakenFuturesOpenOrdersDelta,
    KrakenFuturesPrivateSubscribeRequest, KrakenFuturesTickerData, KrakenFuturesTradeData,
    KrakenFuturesTradeSnapshot, KrakenFuturesWsMessage, classify_futures_message,
};
use crate::common::enums::KrakenOrderSide;

/// Parsed order event from a Kraken Futures WebSocket message.
#[derive(Debug, Clone)]
pub enum ParsedOrderEvent {
    Accepted(OrderAccepted),
    Canceled(OrderCanceled),
    Expired(OrderExpired),
    Updated(OrderUpdated),
    StatusOnly(Box<OrderStatusReport>),
}

/// Cached order info for proper event generation.
#[derive(Debug, Clone)]
struct CachedOrderInfo {
    instrument_id: InstrumentId,
    trader_id: TraderId,
    strategy_id: StrategyId,
}

/// Commands sent from the outer client to the inner message handler.
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    SetClient(WebSocketClient),
    SubscribeTicker(Symbol),
    UnsubscribeTicker(Symbol),
    SubscribeTrade(Symbol),
    UnsubscribeTrade(Symbol),
    SubscribeBook(Symbol),
    UnsubscribeBook(Symbol),
    Disconnect,
    InitializeInstruments(Vec<InstrumentAny>),
    UpdateInstrument(InstrumentAny),
    SetAccountId(AccountId),
    RequestChallenge {
        api_key: String,
        response_tx: tokio::sync::oneshot::Sender<String>,
    },
    SetAuthCredentials {
        api_key: String,
        original_challenge: String,
        signed_challenge: String,
    },
    SubscribeOpenOrders,
    SubscribeFills,
    CacheClientOrder {
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    },
}

impl Debug for HandlerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetClient(_) => f.debug_struct(stringify!(SetClient)).finish(),
            Self::SubscribeTicker(s) => f.debug_tuple("SubscribeTicker").field(s).finish(),
            Self::UnsubscribeTicker(s) => f.debug_tuple("UnsubscribeTicker").field(s).finish(),
            Self::SubscribeTrade(s) => f.debug_tuple("SubscribeTrade").field(s).finish(),
            Self::UnsubscribeTrade(s) => f.debug_tuple("UnsubscribeTrade").field(s).finish(),
            Self::SubscribeBook(s) => f.debug_tuple("SubscribeBook").field(s).finish(),
            Self::UnsubscribeBook(s) => f.debug_tuple("UnsubscribeBook").field(s).finish(),
            Self::Disconnect => write!(f, "Disconnect"),
            Self::InitializeInstruments(v) => f
                .debug_tuple("InitializeInstruments")
                .field(&v.len())
                .finish(),
            Self::UpdateInstrument(i) => f.debug_tuple("UpdateInstrument").field(&i.id()).finish(),
            Self::SetAccountId(id) => f.debug_tuple("SetAccountId").field(id).finish(),
            Self::RequestChallenge { api_key, .. } => {
                let masked = &api_key[..4.min(api_key.len())];
                f.debug_struct(stringify!(RequestChallenge))
                    .field("api_key", &format!("{masked}..."))
                    .finish()
            }
            Self::SetAuthCredentials { api_key, .. } => {
                let masked = &api_key[..4.min(api_key.len())];
                f.debug_struct(stringify!(SetAuthCredentials))
                    .field("api_key", &format!("{masked}..."))
                    .finish()
            }
            Self::SubscribeOpenOrders => write!(f, "SubscribeOpenOrders"),
            Self::SubscribeFills => write!(f, "SubscribeFills"),
            Self::CacheClientOrder {
                client_order_id,
                instrument_id,
                ..
            } => f
                .debug_struct(stringify!(CacheClientOrder))
                .field("client_order_id", client_order_id)
                .field("instrument_id", instrument_id)
                .finish(),
        }
    }
}

/// WebSocket message handler for Kraken Futures.
pub struct FuturesFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    quote_cache: QuoteCache,
    pending_messages: VecDeque<KrakenFuturesWsMessage>,
    account_id: Option<AccountId>,
    api_key: Option<String>,
    original_challenge: Option<String>,
    signed_challenge: Option<String>,
    client_order_cache: AHashMap<ClientOrderId, CachedOrderInfo>,
    venue_order_cache: AHashMap<VenueOrderId, ClientOrderId>,
    pending_challenge_tx: Option<tokio::sync::oneshot::Sender<String>>,
}

impl FuturesFeedHandler {
    /// Creates a new [`FuturesFeedHandler`] instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
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
            account_id: None,
            api_key: None,
            original_challenge: None,
            signed_challenge: None,
            client_order_cache: AHashMap::new(),
            venue_order_cache: AHashMap::new(),
            pending_challenge_tx: None,
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn is_subscribed(&self, channel: KrakenFuturesChannel, symbol: &Ustr) -> bool {
        let channel_ustr = Ustr::from(channel.as_ref());
        self.subscriptions.is_subscribed(&channel_ustr, symbol)
    }

    fn get_instrument(&self, symbol: &Ustr) -> Option<&InstrumentAny> {
        self.instruments_cache.get(symbol)
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub async fn next(&mut self) -> Option<KrakenFuturesWsMessage> {
        // First drain any pending messages from previous ticker processing
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by futures handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::SubscribeTicker(symbol) => {
                            self.send_subscribe(KrakenFuturesFeed::Ticker, &symbol).await;
                        }
                        HandlerCommand::UnsubscribeTicker(symbol) => {
                            self.send_unsubscribe(KrakenFuturesFeed::Ticker, &symbol).await;
                        }
                        HandlerCommand::SubscribeTrade(symbol) => {
                            self.send_subscribe(KrakenFuturesFeed::Trade, &symbol).await;
                        }
                        HandlerCommand::UnsubscribeTrade(symbol) => {
                            self.send_unsubscribe(KrakenFuturesFeed::Trade, &symbol).await;
                        }
                        HandlerCommand::SubscribeBook(symbol) => {
                            self.send_subscribe(KrakenFuturesFeed::Book, &symbol).await;
                        }
                        HandlerCommand::UnsubscribeBook(symbol) => {
                            self.send_unsubscribe(KrakenFuturesFeed::Book, &symbol).await;
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");
                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.raw_symbol().inner(), inst);
                            }
                            let count = self.instruments_cache.len();
                            log::debug!("Initialized {count} instruments in futures handler cache");
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.raw_symbol().inner(), inst);
                        }
                        HandlerCommand::SetAccountId(account_id) => {
                            log::debug!("Setting account_id for futures handler: {account_id}");
                            self.account_id = Some(account_id);
                        }
                        HandlerCommand::RequestChallenge { api_key, response_tx } => {
                            log::debug!("Requesting challenge for authentication");
                            self.pending_challenge_tx = Some(response_tx);
                            self.send_challenge_request(&api_key).await;
                        }
                        HandlerCommand::SetAuthCredentials { api_key, original_challenge, signed_challenge } => {
                            log::debug!("Setting auth credentials for futures handler");
                            self.api_key = Some(api_key);
                            self.original_challenge = Some(original_challenge);
                            self.signed_challenge = Some(signed_challenge);
                        }
                        HandlerCommand::SubscribeOpenOrders => {
                            self.send_private_subscribe(KrakenFuturesFeed::OpenOrders).await;
                        }
                        HandlerCommand::SubscribeFills => {
                            self.send_private_subscribe(KrakenFuturesFeed::Fills).await;
                        }
                        HandlerCommand::CacheClientOrder {
                            client_order_id,
                            venue_order_id,
                            instrument_id,
                            trader_id,
                            strategy_id,
                        } => {
                            self.client_order_cache.insert(
                                client_order_id,
                                CachedOrderInfo {
                                    instrument_id,
                                    trader_id,
                                    strategy_id,
                                },
                            );
                            if let Some(venue_id) = venue_order_id {
                                self.venue_order_cache.insert(venue_id, client_order_id);
                            }
                        }
                    }
                    continue;
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

                    // Handle control frames first (borrow msg to avoid moving)
                    match &msg {
                        Message::Ping(data) => {
                            let len = data.len();
                            log::trace!("Received ping frame with {len} bytes");
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                log::warn!("Failed to send pong frame: {e}");
                            }
                            continue;
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
                        _ => {}
                    }

                    // Extract text without allocation (Utf8Bytes derefs to &str)
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
                        self.quote_cache.clear();
                        return Some(KrakenFuturesWsMessage::Reconnected);
                    }

                    let ts_init = self.clock.get_time_ns();
                    self.parse_message(text, ts_init);

                    // Return first pending message if any were produced
                    if let Some(msg) = self.pending_messages.pop_front() {
                        return Some(msg);
                    }

                    continue;
                }
            }
        }
    }

    async fn send_subscribe(&self, feed: KrakenFuturesFeed, symbol: &Symbol) {
        if let Some(ref client) = self.client {
            let feed_str = serde_json::to_string(&feed).unwrap_or_default();
            let feed_str = feed_str.trim_matches('"');
            let msg = format!(
                r#"{{"event":"subscribe","feed":"{feed_str}","product_ids":["{symbol}"]}}"#
            );
            if let Err(e) = client.send_text(msg, None).await {
                log::error!("Failed to send {feed:?} subscribe: {e}");
            }
        }
    }

    async fn send_unsubscribe(&self, feed: KrakenFuturesFeed, symbol: &Symbol) {
        if let Some(ref client) = self.client {
            let feed_str = serde_json::to_string(&feed).unwrap_or_default();
            let feed_str = feed_str.trim_matches('"');
            let msg = format!(
                r#"{{"event":"unsubscribe","feed":"{feed_str}","product_ids":["{symbol}"]}}"#
            );
            if let Err(e) = client.send_text(msg, None).await {
                log::error!("Failed to send {feed:?} unsubscribe: {e}");
            }
        }
    }

    async fn send_private_subscribe(&self, feed: KrakenFuturesFeed) {
        let Some(ref client) = self.client else {
            log::error!("Cannot subscribe to {feed:?}: no WebSocket client");
            return;
        };

        let Some(ref api_key) = self.api_key else {
            log::error!("Cannot subscribe to {feed:?}: no API key set");
            return;
        };

        let Some(ref original_challenge) = self.original_challenge else {
            log::error!("Cannot subscribe to {feed:?}: no challenge set");
            return;
        };

        let Some(ref signed_challenge) = self.signed_challenge else {
            log::error!("Cannot subscribe to {feed:?}: no signed challenge set");
            return;
        };

        let request = KrakenFuturesPrivateSubscribeRequest {
            event: KrakenFuturesEvent::Subscribe,
            feed,
            api_key: api_key.clone(),
            original_challenge: original_challenge.clone(),
            signed_challenge: signed_challenge.clone(),
        };

        let msg = match serde_json::to_string(&request) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to serialize {feed:?} subscribe request: {e}");
                return;
            }
        };

        if let Err(e) = client.send_text(msg, None).await {
            log::error!("Failed to send {feed:?} subscribe: {e}");
        } else {
            log::debug!("Sent private subscribe request for {feed:?}");
        }
    }

    async fn send_challenge_request(&self, api_key: &str) {
        let Some(ref client) = self.client else {
            log::error!("Cannot request challenge: no WebSocket client");
            return;
        };

        let request = KrakenFuturesChallengeRequest {
            event: KrakenFuturesEvent::Challenge,
            api_key: api_key.to_string(),
        };

        let msg = match serde_json::to_string(&request) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to serialize challenge request: {e}");
                return;
            }
        };

        if let Err(e) = client.send_text(msg, None).await {
            log::error!("Failed to send challenge request: {e}");
        } else {
            log::debug!("Sent challenge request for authentication");
        }
    }

    fn parse_message(&mut self, text: &str, ts_init: UnixNanos) {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("Failed to parse message as JSON: {e}");
                return;
            }
        };

        match classify_futures_message(&value) {
            // Private feeds (execution)
            KrakenFuturesMessageType::OpenOrdersSnapshot => {
                log::debug!(
                    "Skipping open_orders_snapshot (REST reconciliation handles initial state)"
                );
            }
            KrakenFuturesMessageType::OpenOrdersCancel => {
                self.handle_open_orders_cancel_value(value, ts_init);
            }
            KrakenFuturesMessageType::OpenOrdersDelta => {
                self.handle_open_orders_delta_value(value, ts_init);
            }
            KrakenFuturesMessageType::FillsSnapshot => {
                log::debug!("Skipping fills_snapshot (REST reconciliation handles initial state)");
            }
            KrakenFuturesMessageType::FillsDelta => {
                self.handle_fills_delta_value(value, ts_init);
            }
            // Public feeds (market data)
            KrakenFuturesMessageType::Ticker => {
                self.handle_ticker_message_value(value, ts_init);
            }
            KrakenFuturesMessageType::TradeSnapshot => {
                self.handle_trade_snapshot_value(value, ts_init);
            }
            KrakenFuturesMessageType::Trade => {
                self.handle_trade_message_value(value, ts_init);
            }
            KrakenFuturesMessageType::BookSnapshot => {
                self.handle_book_snapshot_value(value, ts_init);
            }
            KrakenFuturesMessageType::BookDelta => {
                self.handle_book_delta_value(value, ts_init);
            }
            // Control messages
            KrakenFuturesMessageType::Info => {
                log::debug!("Received info message: {text}");
            }
            KrakenFuturesMessageType::Pong => {
                log::trace!("Received pong response");
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
            KrakenFuturesMessageType::Unknown => {
                log::debug!("Unhandled message: {text}");
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

                if let Some(tx) = self.pending_challenge_tx.take() {
                    if tx.send(response.message).is_err() {
                        log::warn!("Failed to send challenge response - receiver dropped");
                    }
                } else {
                    log::warn!("Received challenge but no pending request");
                }
            }
            Err(e) => {
                log::error!("Failed to parse challenge response: {e}");
            }
        }
    }

    fn emit_order_event(&mut self, event: ParsedOrderEvent) {
        match event {
            ParsedOrderEvent::Accepted(accepted) => {
                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::OrderAccepted(accepted));
            }
            ParsedOrderEvent::Canceled(canceled) => {
                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::OrderCanceled(canceled));
            }
            ParsedOrderEvent::Expired(expired) => {
                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::OrderExpired(expired));
            }
            ParsedOrderEvent::Updated(updated) => {
                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::OrderUpdated(updated));
            }
            ParsedOrderEvent::StatusOnly(report) => {
                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::OrderStatusReport(report));
            }
        }
    }

    fn handle_ticker_message_value(&mut self, value: Value, ts_init: UnixNanos) {
        let ticker = match serde_json::from_value::<KrakenFuturesTickerData>(value) {
            Ok(t) => t,
            Err(e) => {
                log::debug!("Failed to parse ticker: {e}");
                return;
            }
        };

        let (instrument_id, price_precision) = {
            let Some(instrument) = self.get_instrument(&ticker.product_id) else {
                let product_id = &ticker.product_id;
                log::debug!("Instrument not found for product_id: {product_id}");
                return;
            };
            (instrument.id(), instrument.price_precision())
        };

        let ts_event = ticker
            .time
            .map_or(ts_init, |t| UnixNanos::from((t as u64) * 1_000_000));

        let has_mark = self.is_subscribed(KrakenFuturesChannel::Mark, &ticker.product_id);
        let has_index = self.is_subscribed(KrakenFuturesChannel::Index, &ticker.product_id);

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
                .push_back(KrakenFuturesWsMessage::MarkPrice(update));
        }

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
                .push_back(KrakenFuturesWsMessage::IndexPrice(update));
        }
    }

    fn handle_trade_message_value(&mut self, value: Value, ts_init: UnixNanos) {
        let trade = match serde_json::from_value::<KrakenFuturesTradeData>(value) {
            Ok(t) => t,
            Err(e) => {
                log::trace!("Failed to parse trade: {e}");
                return;
            }
        };

        if !self.is_subscribed(KrakenFuturesChannel::Trades, &trade.product_id) {
            return;
        }

        let (instrument_id, price_precision, size_precision) = {
            let Some(instrument) = self.get_instrument(&trade.product_id) else {
                return;
            };
            (
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            )
        };

        let size = Quantity::new(trade.qty, size_precision);
        if size.is_zero() {
            let product_id = trade.product_id;
            let raw_qty = trade.qty;
            log::warn!("Skipping zero quantity trade for {product_id} (raw qty: {raw_qty})");
            return;
        }

        let ts_event = UnixNanos::from((trade.time as u64) * 1_000_000);
        let aggressor_side = match trade.side {
            KrakenOrderSide::Buy => AggressorSide::Buyer,
            KrakenOrderSide::Sell => AggressorSide::Seller,
        };
        let trade_id = trade.uid.unwrap_or_else(|| trade.seq.to_string());

        let trade_tick = TradeTick::new(
            instrument_id,
            Price::new(trade.price, price_precision),
            size,
            aggressor_side,
            TradeId::new(&trade_id),
            ts_event,
            ts_init,
        );

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::Trade(trade_tick));
    }

    fn handle_trade_snapshot_value(&mut self, value: Value, ts_init: UnixNanos) {
        let snapshot = match serde_json::from_value::<KrakenFuturesTradeSnapshot>(value) {
            Ok(s) => s,
            Err(e) => {
                log::trace!("Failed to parse trade snapshot: {e}");
                return;
            }
        };

        if !self.is_subscribed(KrakenFuturesChannel::Trades, &snapshot.product_id) {
            return;
        }

        let (instrument_id, price_precision, size_precision) = {
            let Some(instrument) = self.get_instrument(&snapshot.product_id) else {
                return;
            };
            (
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            )
        };

        for trade in snapshot.trades {
            let size = Quantity::new(trade.qty, size_precision);
            if size.is_zero() {
                let product_id = snapshot.product_id;
                let raw_qty = trade.qty;
                log::warn!(
                    "Skipping zero quantity trade in snapshot for {product_id} (raw qty: {raw_qty})"
                );
                continue;
            }

            let ts_event = UnixNanos::from((trade.time as u64) * 1_000_000);
            let aggressor_side = match trade.side {
                KrakenOrderSide::Buy => AggressorSide::Buyer,
                KrakenOrderSide::Sell => AggressorSide::Seller,
            };
            let trade_id = trade.uid.unwrap_or_else(|| trade.seq.to_string());

            let trade_tick = TradeTick::new(
                instrument_id,
                Price::new(trade.price, price_precision),
                size,
                aggressor_side,
                TradeId::new(&trade_id),
                ts_event,
                ts_init,
            );

            self.pending_messages
                .push_back(KrakenFuturesWsMessage::Trade(trade_tick));
        }
    }

    fn handle_book_snapshot_value(&mut self, value: Value, ts_init: UnixNanos) {
        let snapshot = match serde_json::from_value::<KrakenFuturesBookSnapshot>(value) {
            Ok(s) => s,
            Err(e) => {
                log::trace!("Failed to parse book snapshot: {e}");
                return;
            }
        };

        let has_book = self.is_subscribed(KrakenFuturesChannel::Book, &snapshot.product_id);
        let has_quotes = self.is_subscribed(KrakenFuturesChannel::Quotes, &snapshot.product_id);

        if !has_book && !has_quotes {
            return;
        }

        let (instrument_id, price_precision, size_precision) = {
            let Some(instrument) = self.get_instrument(&snapshot.product_id) else {
                return;
            };
            (
                instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
            )
        };

        let ts_event = UnixNanos::from((snapshot.timestamp as u64) * 1_000_000);

        let best_bid = snapshot
            .bids
            .iter()
            .filter(|l| l.qty > 0.0)
            .max_by(|a, b| a.price.total_cmp(&b.price));
        let best_ask = snapshot
            .asks
            .iter()
            .filter(|l| l.qty > 0.0)
            .min_by(|a, b| a.price.total_cmp(&b.price));

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
                        .push_back(KrakenFuturesWsMessage::Quote(quote));
                }
                Err(e) => {
                    log::trace!("Quote cache process error: {e}");
                }
            }
        }

        if has_book {
            let mut deltas = Vec::with_capacity(snapshot.bids.len() + snapshot.asks.len() + 1);

            deltas.push(OrderBookDelta::clear(
                instrument_id,
                snapshot.seq as u64,
                ts_event,
                ts_init,
            ));

            for bid in &snapshot.bids {
                let size = Quantity::new(bid.qty, size_precision);
                if size.is_zero() {
                    continue;
                }
                let order = BookOrder::new(
                    OrderSide::Buy,
                    Price::new(bid.price, price_precision),
                    size,
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

            for ask in &snapshot.asks {
                let size = Quantity::new(ask.qty, size_precision);
                if size.is_zero() {
                    continue;
                }
                let order = BookOrder::new(
                    OrderSide::Sell,
                    Price::new(ask.price, price_precision),
                    size,
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
                .push_back(KrakenFuturesWsMessage::BookDeltas(book_deltas));
        }
    }

    fn handle_book_delta_value(&mut self, value: Value, ts_init: UnixNanos) {
        let delta = match serde_json::from_value::<KrakenFuturesBookDelta>(value) {
            Ok(d) => d,
            Err(e) => {
                log::trace!("Failed to parse book delta: {e}");
                return;
            }
        };

        let has_book = self.is_subscribed(KrakenFuturesChannel::Book, &delta.product_id);
        let has_quotes = self.is_subscribed(KrakenFuturesChannel::Quotes, &delta.product_id);

        if !has_book && !has_quotes {
            return;
        }

        let Some(instrument) = self.get_instrument(&delta.product_id) else {
            return;
        };

        let ts_event = UnixNanos::from((delta.timestamp as u64) * 1_000_000);
        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let side: OrderSide = delta.side.into();

        if has_quotes && delta.qty > 0.0 {
            let price = Price::new(delta.price, price_precision);
            let size = Quantity::new(delta.qty, size_precision);

            let (bid_price, ask_price, bid_size, ask_size) = match side {
                OrderSide::Buy => (Some(price), None, Some(size), None),
                OrderSide::Sell => (None, Some(price), None, Some(size)),
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
                    .push_back(KrakenFuturesWsMessage::Quote(quote));
            }
        }

        if has_book {
            let size = Quantity::new(delta.qty, size_precision);
            let action = if size.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let order = BookOrder::new(side, Price::new(delta.price, price_precision), size, 0);

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
                .push_back(KrakenFuturesWsMessage::BookDeltas(book_deltas));
        }
    }

    fn handle_open_orders_delta_value(&mut self, value: Value, ts_init: UnixNanos) {
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

        if let Some(event) = self.parse_order_event(
            &delta.order,
            ts_init,
            delta.is_cancel,
            delta.reason.as_deref(),
        ) {
            self.emit_order_event(event);
        }
    }

    fn handle_open_orders_cancel_value(&mut self, value: Value, ts_init: UnixNanos) {
        // Already classified - we know it's a cancel with is_cancel=true and no "order" field
        // Check if this is a fill-related cancel (skip those - fills feed handles them)
        if let Some(reason) = value.get("reason").and_then(|r| r.as_str())
            && (reason == "full_fill" || reason == "partial_fill")
        {
            log::debug!(
                "Skipping open_orders cancel for fill (handled by fills feed): reason={reason}"
            );
            return;
        }

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

        let Some(account_id) = self.account_id else {
            log::warn!("Cannot process cancel: account_id not set");
            return;
        };

        let venue_order_id_key = VenueOrderId::new(&cancel.order_id);

        let (client_order_id, info) = if let Some(cli_ord_id) =
            cancel.cli_ord_id.as_ref().filter(|id| !id.is_empty())
        {
            let client_order_id_key = ClientOrderId::new(cli_ord_id);
            if let Some(info) = self.client_order_cache.get(&client_order_id_key) {
                (client_order_id_key, info.clone())
            } else if let Some(mapped_cli_ord_id) = self.venue_order_cache.get(&venue_order_id_key)
            {
                if let Some(info) = self.client_order_cache.get(mapped_cli_ord_id) {
                    (*mapped_cli_ord_id, info.clone())
                } else {
                    log::debug!(
                        "Cancel received for unknown order (not in cache): \
                        order_id={}, cli_ord_id={cli_ord_id}",
                        cancel.order_id
                    );
                    return;
                }
            } else {
                log::debug!(
                    "Cancel received for unknown order (not in cache): \
                    order_id={}, cli_ord_id={cli_ord_id}",
                    cancel.order_id
                );
                return;
            }
        } else if let Some(mapped_cli_ord_id) = self.venue_order_cache.get(&venue_order_id_key) {
            if let Some(info) = self.client_order_cache.get(mapped_cli_ord_id) {
                (*mapped_cli_ord_id, info.clone())
            } else {
                log::debug!(
                    "Cancel received but mapped order not in cache: order_id={}",
                    cancel.order_id
                );
                return;
            }
        } else {
            log::debug!(
                "Cancel received without cli_ord_id and no venue mapping (external order): \
                order_id={}",
                cancel.order_id
            );
            return;
        };

        let venue_order_id = VenueOrderId::new(&cancel.order_id);

        let canceled = OrderCanceled::new(
            info.trader_id,
            info.strategy_id,
            info.instrument_id,
            client_order_id,
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
        );

        self.pending_messages
            .push_back(KrakenFuturesWsMessage::OrderCanceled(canceled));
    }

    fn handle_fills_delta_value(&mut self, value: Value, ts_init: UnixNanos) {
        let delta = match serde_json::from_value::<KrakenFuturesFillsDelta>(value) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to parse fills delta: {e}");
                return;
            }
        };

        log::debug!("Received fills delta: fill_count={}", delta.fills.len());

        for fill in &delta.fills {
            log::debug!(
                "Processing fill: fill_id={}, order_id={}",
                fill.fill_id,
                fill.order_id
            );

            if let Some(report) = self.parse_fill_to_report(fill, ts_init) {
                self.pending_messages
                    .push_back(KrakenFuturesWsMessage::FillReport(Box::new(report)));
            }
        }
    }

    /// Parses a Kraken Futures order message into a proper order event.
    ///
    /// Returns the appropriate event type based on order status:
    /// - New orders with no fills -> `OrderAccepted`
    /// - Canceled orders -> `OrderCanceled` or `OrderExpired` (based on reason)
    /// - Orders without cached info -> `StatusOnly` (for reconciliation)
    fn parse_order_event(
        &self,
        order: &KrakenFuturesOpenOrder,
        ts_init: UnixNanos,
        is_cancel: bool,
        cancel_reason: Option<&str>,
    ) -> Option<ParsedOrderEvent> {
        let Some(account_id) = self.account_id else {
            log::warn!("Cannot process order: account_id not set");
            return None;
        };

        let instrument = self
            .instruments_cache
            .get(&Ustr::from(order.instrument.as_str()))?;

        let instrument_id = instrument.id();

        if order.qty <= 0.0 {
            log::warn!(
                "Skipping order with invalid quantity: order_id={}, qty={}",
                order.order_id,
                order.qty
            );
            return None;
        }

        let ts_event = UnixNanos::from((order.last_update_time as u64) * 1_000_000);
        let venue_order_id = VenueOrderId::new(&order.order_id);

        let client_order_id = order
            .cli_ord_id
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| ClientOrderId::new(s.as_str()));

        let cached_info = order
            .cli_ord_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .and_then(|id| self.client_order_cache.get(&ClientOrderId::new(id)));

        // External orders or snapshots fall back to OrderStatusReport for reconciliation
        let Some(info) = cached_info else {
            return self
                .parse_order_to_status_report(order, ts_init, is_cancel)
                .map(|r| ParsedOrderEvent::StatusOnly(Box::new(r)));
        };

        let client_order_id = client_order_id.expect("client_order_id should exist if cached");

        let status = if is_cancel {
            OrderStatus::Canceled
        } else if order.filled >= order.qty {
            OrderStatus::Filled
        } else if order.filled > 0.0 {
            OrderStatus::PartiallyFilled
        } else {
            OrderStatus::Accepted
        };

        match status {
            OrderStatus::Accepted => Some(ParsedOrderEvent::Accepted(OrderAccepted::new(
                info.trader_id,
                info.strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
            ))),
            OrderStatus::Canceled => {
                // Detect expiry by cancel reason keywords
                let is_expired = cancel_reason.is_some_and(|r| {
                    let r_lower = r.to_lowercase();
                    r_lower.contains("expir")
                        || r_lower.contains("gtd")
                        || r_lower.contains("timeout")
                });

                if is_expired {
                    Some(ParsedOrderEvent::Expired(OrderExpired::new(
                        info.trader_id,
                        info.strategy_id,
                        instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    )))
                } else {
                    Some(ParsedOrderEvent::Canceled(OrderCanceled::new(
                        info.trader_id,
                        info.strategy_id,
                        instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    )))
                }
            }

            // Fill events are handled separately via the fills feed
            OrderStatus::PartiallyFilled | OrderStatus::Filled => self
                .parse_order_to_status_report(order, ts_init, is_cancel)
                .map(|r| ParsedOrderEvent::StatusOnly(Box::new(r))),
            _ => self
                .parse_order_to_status_report(order, ts_init, is_cancel)
                .map(|r| ParsedOrderEvent::StatusOnly(Box::new(r))),
        }
    }

    /// Parses a Kraken Futures order into an `OrderStatusReport`.
    ///
    /// Used for snapshots (reconciliation) and external orders.
    fn parse_order_to_status_report(
        &self,
        order: &KrakenFuturesOpenOrder,
        ts_init: UnixNanos,
        is_cancel: bool,
    ) -> Option<OrderStatusReport> {
        let Some(account_id) = self.account_id else {
            log::warn!("Cannot process order: account_id not set");
            return None;
        };

        let instrument = self
            .instruments_cache
            .get(&Ustr::from(order.instrument.as_str()))?;

        let instrument_id = instrument.id();
        let size_precision = instrument.size_precision();

        let side = if order.direction == 0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let order_type = match order.order_type.as_str() {
            "limit" | "lmt" => OrderType::Limit,
            "stop" | "stp" => OrderType::StopLimit,
            "take_profit" => OrderType::LimitIfTouched,
            "market" | "mkt" => OrderType::Market,
            _ => OrderType::Limit,
        };

        let status = if is_cancel {
            OrderStatus::Canceled
        } else if order.filled >= order.qty {
            OrderStatus::Filled
        } else if order.filled > 0.0 {
            OrderStatus::PartiallyFilled
        } else {
            OrderStatus::Accepted
        };

        if order.qty <= 0.0 {
            log::warn!(
                "Skipping order with invalid quantity: order_id={}, qty={}",
                order.order_id,
                order.qty
            );
            return None;
        }

        let ts_event = UnixNanos::from((order.last_update_time as u64) * 1_000_000);

        let client_order_id = order
            .cli_ord_id
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| ClientOrderId::new(s.as_str()));

        let filled_qty = if order.filled <= 0.0 {
            Quantity::zero(size_precision)
        } else {
            Quantity::new(order.filled, size_precision)
        };

        Some(OrderStatusReport::new(
            account_id,
            instrument_id,
            client_order_id,
            VenueOrderId::new(&order.order_id),
            side,
            order_type,
            TimeInForce::Gtc,
            status,
            Quantity::new(order.qty, size_precision),
            filled_qty,
            ts_event, // ts_accepted
            ts_event, // ts_last
            ts_init,
            Some(UUID4::new()),
        ))
    }

    fn parse_fill_to_report(
        &self,
        fill: &KrakenFuturesFill,
        ts_init: UnixNanos,
    ) -> Option<FillReport> {
        let Some(account_id) = self.account_id else {
            log::warn!("Cannot process fill: account_id not set");
            return None;
        };

        // Resolve instrument: try message field first, then fall back to cache
        let instrument = if let Some(ref symbol) = fill.instrument {
            self.instruments_cache.get(symbol).cloned()
        } else if let Some(ref cli_ord_id) = fill.cli_ord_id.as_ref().filter(|id| !id.is_empty()) {
            // Fall back to client order cache
            self.client_order_cache
                .get(&ClientOrderId::new(cli_ord_id))
                .and_then(|info| {
                    self.instruments_cache
                        .iter()
                        .find(|(_, inst)| inst.id() == info.instrument_id)
                        .map(|(_, inst)| inst.clone())
                })
        } else {
            None
        };

        let Some(instrument) = instrument else {
            log::warn!(
                "Cannot resolve instrument for fill: fill_id={}, order_id={}, cli_ord_id={:?}",
                fill.fill_id,
                fill.order_id,
                fill.cli_ord_id
            );
            return None;
        };

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        if fill.qty <= 0.0 {
            log::warn!(
                "Skipping fill with invalid quantity: fill_id={}, qty={}",
                fill.fill_id,
                fill.qty
            );
            return None;
        }

        let side = if fill.buy {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let ts_event = UnixNanos::from((fill.time as u64) * 1_000_000);

        let client_order_id = fill
            .cli_ord_id
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| ClientOrderId::new(s.as_str()));

        let commission = Money::new(fill.fee_paid.unwrap_or(0.0), instrument.quote_currency());

        Some(FillReport::new(
            account_id,
            instrument_id,
            VenueOrderId::new(&fill.order_id),
            TradeId::new(&fill.fill_id),
            side,
            Quantity::new(fill.qty, size_precision),
            Price::new(fill.price, price_precision),
            commission,
            LiquiditySide::NoLiquiditySide, // Not provided
            client_order_id,
            None, // venue_position_id
            ts_event,
            ts_init,
            Some(UUID4::new()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        instruments::{CryptoFuture, InstrumentAny},
        types::Currency,
    };
    use rstest::rstest;

    use super::*;

    fn create_test_handler() -> FuturesFeedHandler {
        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new(':');

        FuturesFeedHandler::new(signal, cmd_rx, raw_rx, subscriptions)
    }

    fn create_test_instrument() -> InstrumentAny {
        InstrumentAny::CryptoFuture(CryptoFuture::new(
            InstrumentId::from("PI_XBTUSD.KRAKEN"),
            Symbol::from("PI_XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            UnixNanos::default(),
            UnixNanos::default(),
            1, // price_precision
            0, // size_precision
            Price::from("0.5"),
            Quantity::from(1),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_book_snapshot_filters_zero_quantity_bids() {
        let mut handler = create_test_handler();
        let instrument = create_test_instrument();
        handler
            .instruments_cache
            .insert(Ustr::from("PI_XBTUSD"), instrument);

        handler.subscriptions.mark_subscribe("book:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("book:PI_XBTUSD");

        let json = include_str!("../../../test_data/ws_futures_book_snapshot_with_zero_qty.json");
        let ts_init = UnixNanos::from(1_000_000_000);

        handler.parse_message(json, ts_init);

        assert_eq!(handler.pending_messages.len(), 1);

        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::BookDeltas(deltas) = msg else {
            panic!("Expected BookDeltas message");
        };

        // Fixture has 3 bids (1 zero qty) + 2 asks (1 zero qty) = 3 valid + 1 clear = 4
        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        for delta in &deltas.deltas[1..] {
            assert!(
                !delta.order.size.is_zero(),
                "Found zero-quantity delta that should have been filtered: {delta:?}"
            );
        }
    }

    #[rstest]
    fn test_book_snapshot_filters_zero_quantity_asks() {
        let mut handler = create_test_handler();
        let instrument = create_test_instrument();
        handler
            .instruments_cache
            .insert(Ustr::from("PI_XBTUSD"), instrument);

        handler.subscriptions.mark_subscribe("book:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("book:PI_XBTUSD");

        let json = include_str!("../../../test_data/ws_futures_book_snapshot_with_zero_qty.json");
        let ts_init = UnixNanos::from(1_000_000_000);

        handler.parse_message(json, ts_init);

        let msg = handler.pending_messages.pop_front().unwrap();
        let KrakenFuturesWsMessage::BookDeltas(deltas) = msg else {
            panic!("Expected BookDeltas message");
        };

        // Only 1 ask should remain (the one with qty 2300, not the zero qty one)
        let sell_deltas: Vec<_> = deltas
            .deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Sell)
            .collect();

        assert_eq!(sell_deltas.len(), 1);
        assert_eq!(sell_deltas[0].order.price.as_f64(), 34912.0);
    }

    #[rstest]
    fn test_trade_filters_zero_quantity() {
        let mut handler = create_test_handler();
        let instrument = create_test_instrument();
        handler
            .instruments_cache
            .insert(Ustr::from("PI_XBTUSD"), instrument);

        handler.subscriptions.mark_subscribe("trades:PI_XBTUSD");
        handler.subscriptions.confirm_subscribe("trades:PI_XBTUSD");

        let json = r#"{
            "feed": "trade",
            "product_id": "PI_XBTUSD",
            "time": 1612269825817,
            "side": "buy",
            "qty": 0.0,
            "price": 34900.0,
            "seq": 12345
        }"#;
        let ts_init = UnixNanos::from(1_000_000_000);

        handler.parse_message(json, ts_init);

        assert!(
            handler.pending_messages.is_empty(),
            "Zero quantity trade should be filtered out"
        );
    }
}
