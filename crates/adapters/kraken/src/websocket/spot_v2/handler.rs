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

use ahash::AHashMap;
use nautilus_common::cache::quote::QuoteCache;
use nautilus_core::{AtomicTime, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Bar, Data, OrderBookDeltas, QuoteTick},
    events::{OrderAccepted, OrderCanceled, OrderExpired, OrderRejected, OrderUpdated},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{KrakenExecType, KrakenWsChannel},
    messages::{
        KrakenWsBookData, KrakenWsExecutionData, KrakenWsMessage, KrakenWsOhlcData,
        KrakenWsResponse, KrakenWsTickerData, KrakenWsTradeData, NautilusWsMessage,
    },
    parse::{
        parse_book_deltas, parse_quote_tick, parse_trade_tick, parse_ws_bar, parse_ws_fill_report,
        parse_ws_order_status_report,
    },
};

/// Cached information about a client order needed for event generation.
#[derive(Debug, Clone)]
struct CachedOrderInfo {
    instrument_id: InstrumentId,
    trader_id: TraderId,
    strategy_id: StrategyId,
}

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum SpotHandlerCommand {
    SetClient(WebSocketClient),
    Disconnect,
    SendText {
        payload: String,
    },
    InitializeInstruments(Vec<InstrumentAny>),
    UpdateInstrument(InstrumentAny),
    SetAccountId(AccountId),
    CacheClientOrder {
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    },
}

/// Key for buffering OHLC bars: (symbol, interval).
type OhlcBufferKey = (Ustr, u32);

/// Buffered OHLC bar with its interval start time for period detection.
type OhlcBufferEntry = (Bar, UnixNanos);

/// WebSocket message handler for Kraken.
pub(super) struct SpotFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: SubscriptionState,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    client_order_cache: AHashMap<ClientOrderId, CachedOrderInfo>,
    order_qty_cache: AHashMap<VenueOrderId, f64>,
    quote_cache: QuoteCache,
    book_sequence: u64,
    pending_quotes: Vec<QuoteTick>,
    pending_messages: VecDeque<NautilusWsMessage>,
    account_id: Option<AccountId>,
    ohlc_buffer: AHashMap<OhlcBufferKey, OhlcBufferEntry>,
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
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            instruments_cache: AHashMap::new(),
            client_order_cache: AHashMap::new(),
            order_qty_cache: AHashMap::new(),
            quote_cache: QuoteCache::new(),
            book_sequence: 0,
            pending_quotes: Vec::new(),
            pending_messages: VecDeque::new(),
            account_id: None,
            ohlc_buffer: AHashMap::new(),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    /// Checks if a topic is active (confirmed or pending subscribe).
    fn is_subscribed(&self, topic: &str) -> bool {
        self.subscriptions.all_topics().iter().any(|t| t == topic)
    }

    fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).cloned()
    }

    /// Flushes all buffered OHLC bars to pending messages.
    ///
    /// Called when the stream ends to ensure the last bar for each symbol/interval
    /// is not lost.
    fn flush_ohlc_buffer(&mut self) {
        if self.ohlc_buffer.is_empty() {
            return;
        }

        let bars: Vec<Data> = self
            .ohlc_buffer
            .drain()
            .map(|(_, (bar, _))| Data::Bar(bar))
            .collect();

        if !bars.is_empty() {
            log::debug!("Flushing {} buffered OHLC bars on stream end", bars.len());
            self.pending_messages
                .push_back(NautilusWsMessage::Data(bars));
        }
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        // Check for pending messages first (e.g., from multi-message scenarios like trades)
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        if let Some(quote) = self.pending_quotes.pop() {
            return Some(NautilusWsMessage::Data(vec![Data::Quote(quote)]));
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        SpotHandlerCommand::SetClient(client) => {
                            log::debug!("WebSocketClient received by handler");
                            self.client = Some(client);
                        }
                        SpotHandlerCommand::Disconnect => {
                            log::debug!("Disconnect command received");
                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                        }
                        SpotHandlerCommand::SendText { payload } => {
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_text(payload.clone(), None).await
                            {
                                log::error!("Failed to send text: {e}");
                            }
                        }
                        SpotHandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                // Cache by symbol (ISO 4217-A3 format like "ETH/USD")
                                // which matches what v2 WebSocket messages use
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        SpotHandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                        SpotHandlerCommand::SetAccountId(account_id) => {
                            log::debug!("Account ID set for execution reports: {account_id}");
                            self.account_id = Some(account_id);
                        }
                        SpotHandlerCommand::CacheClientOrder {
                            client_order_id,
                            instrument_id,
                            trader_id,
                            strategy_id,
                        } => {
                            log::debug!(
                                "Cached client order info: \
                                client_order_id={client_order_id}, instrument_id={instrument_id}"
                            );
                            self.client_order_cache.insert(
                                client_order_id,
                                CachedOrderInfo {
                                    instrument_id,
                                    trader_id,
                                    strategy_id,
                                },
                            );
                        }
                    }
                    continue;
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            log::debug!("WebSocket stream closed");
                            self.flush_ohlc_buffer();
                            return self.pending_messages.pop_front();
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

                    if self.signal.load(Ordering::Relaxed) {
                        log::debug!("Stop signal received");
                        self.flush_ohlc_buffer();
                        return self.pending_messages.pop_front();
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
                            self.flush_ohlc_buffer();
                            return self.pending_messages.pop_front();
                        }
                        Message::Frame(_) => {
                            log::trace!("Received raw frame");
                            continue;
                        }
                        _ => continue,
                    };

                    if text == RECONNECTED {
                        log::info!("Received WebSocket reconnected signal");
                        self.quote_cache.clear();
                        return Some(NautilusWsMessage::Reconnected);
                    }

                    let ts_init = self.clock.get_time_ns();

                    if let Some(nautilus_msg) = self.parse_message(&text, ts_init) {
                        return Some(nautilus_msg);
                    }

                    continue;
                }
            }
        }
    }

    fn parse_message(&mut self, text: &str, ts_init: UnixNanos) -> Option<NautilusWsMessage> {
        // Fast pre-filter for high-frequency control messages (no JSON parsing)
        // Heartbeats and status messages are short and share common prefix
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
                Ok(msg) => return self.handle_data_message(msg, ts_init),
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

    fn handle_data_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        match msg.channel {
            KrakenWsChannel::Book => self.handle_book_message(msg, ts_init),
            KrakenWsChannel::Ticker => self.handle_ticker_message(msg, ts_init),
            KrakenWsChannel::Trade => self.handle_trade_message(msg, ts_init),
            KrakenWsChannel::Ohlc => self.handle_ohlc_message(msg, ts_init),
            KrakenWsChannel::Executions => self.handle_executions_message(msg, ts_init),
            _ => {
                log::warn!("Unhandled channel: {:?}", msg.channel);
                None
            }
        }
    }

    fn handle_book_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut all_deltas = Vec::new();
        let mut instrument_id = None;

        for data in msg.data {
            match serde_json::from_value::<KrakenWsBookData>(data) {
                Ok(book_data) => {
                    let symbol = &book_data.symbol;
                    let instrument = self.get_instrument(symbol)?;
                    instrument_id = Some(instrument.id());

                    let price_precision = instrument.price_precision();
                    let size_precision = instrument.size_precision();

                    let has_book = self.is_subscribed(&format!("book:{symbol}"));
                    let has_quotes = self.is_subscribed(&format!("quotes:{symbol}"));

                    if has_quotes {
                        let best_bid = book_data.bids.as_ref().and_then(|bids| bids.first());
                        let best_ask = book_data.asks.as_ref().and_then(|asks| asks.first());

                        let bid_price = best_bid.map(|b| Price::new(b.price, price_precision));
                        let ask_price = best_ask.map(|a| Price::new(a.price, price_precision));
                        let bid_size = best_bid.map(|b| Quantity::new(b.qty, size_precision));
                        let ask_size = best_ask.map(|a| Quantity::new(a.qty, size_precision));

                        if let Ok(quote) = self.quote_cache.process(
                            instrument.id(),
                            bid_price,
                            ask_price,
                            bid_size,
                            ask_size,
                            ts_init,
                            ts_init,
                        ) {
                            self.pending_quotes.push(quote);
                        }
                    }

                    if has_book {
                        match parse_book_deltas(
                            &book_data,
                            &instrument,
                            self.book_sequence,
                            ts_init,
                        ) {
                            Ok(mut deltas) => {
                                self.book_sequence += deltas.len() as u64;
                                all_deltas.append(&mut deltas);
                            }
                            Err(e) => {
                                log::error!("Failed to parse book deltas: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize book data: {e}");
                }
            }
        }

        if all_deltas.is_empty() {
            if let Some(quote) = self.pending_quotes.pop() {
                return Some(NautilusWsMessage::Data(vec![Data::Quote(quote)]));
            }
            None
        } else {
            let deltas = OrderBookDeltas::new(instrument_id?, all_deltas);
            Some(NautilusWsMessage::Deltas(deltas))
        }
    }

    fn handle_ticker_message(
        &self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut quotes = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsTickerData>(data) {
                Ok(ticker_data) => {
                    let instrument = self.get_instrument(&ticker_data.symbol)?;

                    match parse_quote_tick(&ticker_data, &instrument, ts_init) {
                        Ok(quote) => quotes.push(Data::Quote(quote)),
                        Err(e) => {
                            log::error!("Failed to parse quote tick: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize ticker data: {e}");
                }
            }
        }

        if quotes.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::Data(quotes))
        }
    }

    fn handle_trade_message(
        &self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut trades = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsTradeData>(data) {
                Ok(trade_data) => {
                    let instrument = self.get_instrument(&trade_data.symbol)?;

                    match parse_trade_tick(&trade_data, &instrument, ts_init) {
                        Ok(trade) => trades.push(Data::Trade(trade)),
                        Err(e) => {
                            log::error!("Failed to parse trade tick: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize trade data: {e}");
                }
            }
        }

        if trades.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::Data(trades))
        }
    }

    fn handle_ohlc_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut closed_bars = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsOhlcData>(data) {
                Ok(ohlc_data) => {
                    let instrument = self.get_instrument(&ohlc_data.symbol)?;

                    match parse_ws_bar(&ohlc_data, &instrument, ts_init) {
                        Ok(new_bar) => {
                            let key = (ohlc_data.symbol, ohlc_data.interval);
                            let new_interval_begin = UnixNanos::from(
                                ohlc_data.interval_begin.timestamp_nanos_opt().unwrap_or(0) as u64,
                            );

                            // Check if we have a buffered bar for this symbol/interval
                            if let Some((buffered_bar, buffered_interval_begin)) =
                                self.ohlc_buffer.get(&key)
                            {
                                // If interval_begin changed, the buffered bar is closed
                                if new_interval_begin != *buffered_interval_begin {
                                    closed_bars.push(Data::Bar(*buffered_bar));
                                }
                            }

                            // Update buffer with the new (potentially incomplete) bar
                            self.ohlc_buffer.insert(key, (new_bar, new_interval_begin));
                        }
                        Err(e) => {
                            log::error!("Failed to parse bar: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize OHLC data: {e}");
                }
            }
        }

        if closed_bars.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::Data(closed_bars))
        }
    }

    fn handle_executions_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Some(account_id) = self.account_id else {
            log::warn!("Cannot process execution message: account_id not set");
            return None;
        };

        // Process all executions in batch and queue them (snapshots can have many records)
        for data in msg.data {
            match serde_json::from_value::<KrakenWsExecutionData>(data) {
                Ok(exec_data) => {
                    log::debug!(
                        "Received execution message: exec_type={:?}, order_id={}, \
                        order_status={:?}, order_qty={:?}, cum_qty={:?}, last_qty={:?}",
                        exec_data.exec_type,
                        exec_data.order_id,
                        exec_data.order_status,
                        exec_data.order_qty,
                        exec_data.cum_qty,
                        exec_data.last_qty
                    );

                    // Cache order_qty for subsequent messages that may not include it
                    if let Some(qty) = exec_data.order_qty {
                        self.order_qty_cache
                            .insert(VenueOrderId::new(&exec_data.order_id), qty);
                    }

                    // Resolve instrument and cached order info
                    let (instrument, cached_info) = if let Some(ref symbol) = exec_data.symbol {
                        let symbol_ustr = Ustr::from(symbol.as_str());
                        let inst = self.instruments_cache.get(&symbol_ustr).cloned();
                        if inst.is_none() {
                            log::warn!(
                                "No instrument found for symbol: symbol={symbol}, order_id={}",
                                exec_data.order_id
                            );
                        }
                        let cached = exec_data
                            .cl_ord_id
                            .as_ref()
                            .filter(|id| !id.is_empty())
                            .and_then(|id| {
                                self.client_order_cache
                                    .get(&ClientOrderId::new(id))
                                    .cloned()
                            });
                        (inst, cached)
                    } else if let Some(ref cl_ord_id) =
                        exec_data.cl_ord_id.as_ref().filter(|id| !id.is_empty())
                    {
                        let cached = self
                            .client_order_cache
                            .get(&ClientOrderId::new(cl_ord_id))
                            .cloned();
                        let inst = cached.as_ref().and_then(|info| {
                            self.instruments_cache
                                .iter()
                                .find(|(_, inst)| inst.id() == info.instrument_id)
                                .map(|(_, inst)| inst.clone())
                        });
                        (inst, cached)
                    } else {
                        (None, None)
                    };

                    let Some(instrument) = instrument else {
                        log::debug!(
                            "Execution missing symbol and order not in cache (external order): \
                            order_id={}, cl_ord_id={:?}, exec_type={:?}",
                            exec_data.order_id,
                            exec_data.cl_ord_id,
                            exec_data.exec_type
                        );
                        continue;
                    };

                    let cached_order_qty = self
                        .order_qty_cache
                        .get(&VenueOrderId::new(&exec_data.order_id))
                        .copied();
                    let ts_event = chrono::DateTime::parse_from_rfc3339(&exec_data.timestamp)
                        .map(|t| UnixNanos::from(t.timestamp_nanos_opt().unwrap_or(0) as u64))
                        .unwrap_or(ts_init);

                    // Emit proper order events when we have cached info, otherwise fall back
                    // to OrderStatusReport for external orders or reconciliation
                    if let Some(ref info) = cached_info {
                        let client_order_id = exec_data
                            .cl_ord_id
                            .as_ref()
                            .map(ClientOrderId::new)
                            .expect("cl_ord_id should exist if cached");
                        let venue_order_id = VenueOrderId::new(&exec_data.order_id);

                        match exec_data.exec_type {
                            KrakenExecType::PendingNew => {
                                // Order received and validated - emit accepted
                                let accepted = OrderAccepted::new(
                                    info.trader_id,
                                    info.strategy_id,
                                    instrument.id(),
                                    client_order_id,
                                    venue_order_id,
                                    account_id,
                                    UUID4::new(),
                                    ts_event,
                                    ts_init,
                                    false,
                                );
                                self.pending_messages
                                    .push_back(NautilusWsMessage::OrderAccepted(accepted));
                            }
                            KrakenExecType::New => {
                                // Order is now live - already accepted, skip
                            }
                            KrakenExecType::Canceled => {
                                // Check if this is a post-only rejection based on reason
                                // Kraken sends reason="Post only order" for post-only rejections
                                let is_post_only_rejection = exec_data
                                    .reason
                                    .as_ref()
                                    .is_some_and(|r| r.eq_ignore_ascii_case("Post only order"));

                                if is_post_only_rejection {
                                    let reason = exec_data
                                        .reason
                                        .as_deref()
                                        .unwrap_or("Post-only order would have crossed");
                                    let rejected = OrderRejected::new(
                                        info.trader_id,
                                        info.strategy_id,
                                        instrument.id(),
                                        client_order_id,
                                        account_id,
                                        Ustr::from(reason),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false,
                                        true, // due_post_only
                                    );
                                    self.pending_messages
                                        .push_back(NautilusWsMessage::OrderRejected(rejected));
                                } else {
                                    let canceled = OrderCanceled::new(
                                        info.trader_id,
                                        info.strategy_id,
                                        instrument.id(),
                                        client_order_id,
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false,
                                        Some(venue_order_id),
                                        Some(account_id),
                                    );
                                    self.pending_messages
                                        .push_back(NautilusWsMessage::OrderCanceled(canceled));
                                }
                            }
                            KrakenExecType::Expired => {
                                let expired = OrderExpired::new(
                                    info.trader_id,
                                    info.strategy_id,
                                    instrument.id(),
                                    client_order_id,
                                    UUID4::new(),
                                    ts_event,
                                    ts_init,
                                    false,
                                    Some(venue_order_id),
                                    Some(account_id),
                                );
                                self.pending_messages
                                    .push_back(NautilusWsMessage::OrderExpired(expired));
                            }
                            KrakenExecType::Amended | KrakenExecType::Restated => {
                                // For modifications, emit OrderUpdated
                                if let Some(order_qty) = exec_data.order_qty.or(cached_order_qty) {
                                    let updated = OrderUpdated::new(
                                        info.trader_id,
                                        info.strategy_id,
                                        instrument.id(),
                                        client_order_id,
                                        Quantity::new(order_qty, instrument.size_precision()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false,
                                        Some(venue_order_id),
                                        Some(account_id),
                                        None, // price
                                        None, // trigger_price
                                        None, // protection_price
                                    );
                                    self.pending_messages
                                        .push_back(NautilusWsMessage::OrderUpdated(updated));
                                }
                            }
                            KrakenExecType::Trade | KrakenExecType::Filled => {
                                // Trades use OrderStatusReport + FillReport
                                let has_complete_trade_data =
                                    exec_data.last_qty.is_some_and(|q| q > 0.0)
                                        && exec_data.last_price.is_some_and(|p| p > 0.0);

                                if let Ok(status_report) = parse_ws_order_status_report(
                                    &exec_data,
                                    &instrument,
                                    account_id,
                                    cached_order_qty,
                                    ts_init,
                                ) {
                                    self.pending_messages.push_back(
                                        NautilusWsMessage::OrderStatusReport(Box::new(
                                            status_report,
                                        )),
                                    );
                                }

                                if has_complete_trade_data
                                    && let Ok(fill_report) = parse_ws_fill_report(
                                        &exec_data,
                                        &instrument,
                                        account_id,
                                        ts_init,
                                    )
                                {
                                    self.pending_messages
                                        .push_back(NautilusWsMessage::FillReport(Box::new(
                                            fill_report,
                                        )));
                                }
                            }
                            KrakenExecType::IcebergRefill => {
                                // Iceberg order refill - treat similar to order update
                                if let Some(order_qty) = exec_data.order_qty.or(cached_order_qty) {
                                    let updated = OrderUpdated::new(
                                        info.trader_id,
                                        info.strategy_id,
                                        instrument.id(),
                                        client_order_id,
                                        Quantity::new(order_qty, instrument.size_precision()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false,
                                        Some(venue_order_id),
                                        Some(account_id),
                                        None,
                                        None,
                                        None,
                                    );
                                    self.pending_messages
                                        .push_back(NautilusWsMessage::OrderUpdated(updated));
                                }
                            }
                            KrakenExecType::Status => {
                                // Status update without state change - emit OrderStatusReport
                                if let Ok(status_report) = parse_ws_order_status_report(
                                    &exec_data,
                                    &instrument,
                                    account_id,
                                    cached_order_qty,
                                    ts_init,
                                ) {
                                    self.pending_messages.push_back(
                                        NautilusWsMessage::OrderStatusReport(Box::new(
                                            status_report,
                                        )),
                                    );
                                }
                            }
                        }
                    } else {
                        // No cached info - external order or reconciliation, use OrderStatusReport
                        if exec_data.exec_type == KrakenExecType::Trade
                            || exec_data.exec_type == KrakenExecType::Filled
                        {
                            let has_order_data = exec_data.order_qty.is_some()
                                || cached_order_qty.is_some()
                                || exec_data.cum_qty.is_some();

                            let has_complete_trade_data =
                                exec_data.last_qty.is_some_and(|q| q > 0.0)
                                    && exec_data.last_price.is_some_and(|p| p > 0.0);

                            if has_order_data
                                && let Ok(status_report) = parse_ws_order_status_report(
                                    &exec_data,
                                    &instrument,
                                    account_id,
                                    cached_order_qty,
                                    ts_init,
                                )
                            {
                                self.pending_messages.push_back(
                                    NautilusWsMessage::OrderStatusReport(Box::new(status_report)),
                                );
                            }

                            if has_complete_trade_data
                                && let Ok(fill_report) = parse_ws_fill_report(
                                    &exec_data,
                                    &instrument,
                                    account_id,
                                    ts_init,
                                )
                            {
                                self.pending_messages
                                    .push_back(NautilusWsMessage::FillReport(Box::new(
                                        fill_report,
                                    )));
                            }
                        } else if let Ok(report) = parse_ws_order_status_report(
                            &exec_data,
                            &instrument,
                            account_id,
                            cached_order_qty,
                            ts_init,
                        ) {
                            self.pending_messages
                                .push_back(NautilusWsMessage::OrderStatusReport(Box::new(report)));
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize execution data: {e}");
                }
            }
        }

        // Return first queued message (rest returned via next() pending check)
        self.pending_messages.pop_front()
    }
}
