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

//! WebSocket message handler for Kraken Spot v2.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::cache::quote::QuoteCache;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Data, OrderBookDeltas, QuoteTick},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::websocket::WebSocketClient;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{KrakenExecType, KrakenWsChannel},
    messages::{
        KrakenWsBookData, KrakenWsExecutionData, KrakenWsMessage, KrakenWsResponse,
        KrakenWsTickerData, KrakenWsTradeData, NautilusWsMessage,
    },
    parse::{
        parse_book_deltas, parse_quote_tick, parse_trade_tick, parse_ws_fill_report,
        parse_ws_order_status_report,
    },
};

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
        client_order_id: String,
        instrument_id: InstrumentId,
    },
}

/// WebSocket message handler for Kraken.
pub(super) struct SpotFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: Arc<DashMap<String, KrakenWsChannel>>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    client_order_instruments: AHashMap<String, InstrumentId>,
    order_qty_cache: AHashMap<String, f64>,
    quote_cache: QuoteCache,
    book_sequence: u64,
    pending_quotes: Vec<QuoteTick>,
    pending_messages: VecDeque<NautilusWsMessage>,
    account_id: Option<AccountId>,
}

impl SpotFeedHandler {
    /// Creates a new [`SpotFeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: Arc<DashMap<String, KrakenWsChannel>>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            instruments_cache: AHashMap::new(),
            client_order_instruments: AHashMap::new(),
            order_qty_cache: AHashMap::new(),
            quote_cache: QuoteCache::new(),
            book_sequence: 0,
            pending_quotes: Vec::new(),
            pending_messages: VecDeque::new(),
            account_id: None,
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).cloned()
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
                            tracing::debug!("WebSocketClient received by handler");
                            self.client = Some(client);
                        }
                        SpotHandlerCommand::Disconnect => {
                            tracing::debug!("Disconnect command received");
                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                        }
                        SpotHandlerCommand::SendText { payload } => {
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_text(payload.clone(), None).await
                            {
                                tracing::error!(error = %e, "Failed to send text");
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
                            tracing::debug!(%account_id, "Account ID set for execution reports");
                            self.account_id = Some(account_id);
                        }
                        SpotHandlerCommand::CacheClientOrder {
                            client_order_id,
                            instrument_id,
                        } => {
                            tracing::debug!(
                                %client_order_id,
                                %instrument_id,
                                "Cached client_order_id -> instrument mapping"
                            );
                            self.client_order_instruments
                                .insert(client_order_id, instrument_id);
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

                    if let Message::Ping(data) = &msg {
                        tracing::trace!("Received ping frame with {} bytes", data.len());
                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            tracing::warn!(error = %e, "Failed to send pong frame");
                        }
                        continue;
                    }

                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }

                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(data) => {
                            match String::from_utf8(data.to_vec()) {
                                Ok(text) => text,
                                Err(e) => {
                                    tracing::warn!("Failed to decode binary message: {e}");
                                    continue;
                                }
                            }
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
                        _ => continue,
                    };

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
        // Try to parse as a data message first
        if let Ok(msg) = serde_json::from_str::<KrakenWsMessage>(text) {
            return self.handle_data_message(msg, ts_init);
        }

        // Check for control messages (heartbeat, status, subscription responses)
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            if value.get("channel").and_then(|v| v.as_str()) == Some("heartbeat") {
                tracing::trace!("Received heartbeat");
                return None;
            }

            if value.get("channel").and_then(|v| v.as_str()) == Some("status") {
                tracing::debug!("Received status message");
                return None;
            }

            if value.get("method").is_some() {
                if let Ok(response) = serde_json::from_value::<KrakenWsResponse>(value) {
                    match response {
                        KrakenWsResponse::Subscribe(sub) => {
                            if sub.success {
                                if let Some(result) = &sub.result {
                                    tracing::debug!(
                                        channel = ?result.channel,
                                        req_id = ?sub.req_id,
                                        "Subscription confirmed"
                                    );
                                } else {
                                    tracing::debug!(req_id = ?sub.req_id, "Subscription confirmed");
                                }
                            } else {
                                tracing::warn!(
                                    error = ?sub.error,
                                    req_id = ?sub.req_id,
                                    "Subscription failed"
                                );
                            }
                        }
                        KrakenWsResponse::Unsubscribe(unsub) => {
                            if unsub.success {
                                tracing::debug!(req_id = ?unsub.req_id, "Unsubscription confirmed");
                            } else {
                                tracing::warn!(
                                    error = ?unsub.error,
                                    req_id = ?unsub.req_id,
                                    "Unsubscription failed"
                                );
                            }
                        }
                        KrakenWsResponse::Pong(pong) => {
                            tracing::trace!(req_id = ?pong.req_id, "Received pong");
                        }
                        KrakenWsResponse::Other => {
                            tracing::debug!("Received unknown subscription response");
                        }
                    }
                } else {
                    tracing::debug!("Received subscription response (failed to parse details)");
                }
                return None;
            }
        }

        tracing::warn!("Failed to parse message: {text}");
        None
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
                tracing::warn!("Unhandled channel: {:?}", msg.channel);
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

                    let has_book = self.subscriptions.contains_key(&format!("book:{symbol}"));
                    let has_quotes = self.subscriptions.contains_key(&format!("quotes:{symbol}"));

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
                                tracing::error!("Failed to parse book deltas: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize book data: {e}");
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
                            tracing::error!("Failed to parse quote tick: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize ticker data: {e}");
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
                            tracing::error!("Failed to parse trade tick: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize trade data: {e}");
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
        &self,
        _msg: KrakenWsMessage,
        _ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        // OHLC/Bar parsing not yet implemented in parse.rs
        tracing::debug!("OHLC message received but parsing not yet implemented");
        None
    }

    fn handle_executions_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Some(account_id) = self.account_id else {
            tracing::warn!("Cannot process execution message: account_id not set");
            return None;
        };

        // Process all executions in batch and queue them (snapshots can have many records)
        for data in msg.data {
            match serde_json::from_value::<KrakenWsExecutionData>(data) {
                Ok(exec_data) => {
                    tracing::debug!(
                        exec_type = ?exec_data.exec_type,
                        order_id = %exec_data.order_id,
                        order_status = ?exec_data.order_status,
                        order_qty = ?exec_data.order_qty,
                        cum_qty = ?exec_data.cum_qty,
                        last_qty = ?exec_data.last_qty,
                        "Received execution message"
                    );

                    // Cache order_qty for subsequent messages that may not include it
                    if let Some(qty) = exec_data.order_qty {
                        self.order_qty_cache.insert(exec_data.order_id.clone(), qty);
                    }

                    // Resolve instrument: symbol -> cl_ord_id cache
                    let instrument = if let Some(ref symbol) = exec_data.symbol {
                        let symbol_ustr = Ustr::from(symbol.as_str());
                        if let Some(inst) = self.instruments_cache.get(&symbol_ustr).cloned() {
                            Some(inst)
                        } else {
                            tracing::warn!(
                                symbol = %symbol,
                                order_id = %exec_data.order_id,
                                "No instrument found for symbol"
                            );
                            None
                        }
                    } else if let Some(ref cl_ord_id) = exec_data.cl_ord_id {
                        // Check cl_ord_id cache (handles race where WS arrives before HTTP response)
                        self.client_order_instruments
                            .get(cl_ord_id)
                            .and_then(|instrument_id| {
                                self.instruments_cache
                                    .iter()
                                    .find(|(_, inst)| inst.id() == *instrument_id)
                                    .map(|(_, inst)| inst.clone())
                            })
                    } else {
                        None
                    };

                    let Some(instrument) = instrument else {
                        tracing::debug!(
                            order_id = %exec_data.order_id,
                            cl_ord_id = ?exec_data.cl_ord_id,
                            exec_type = ?exec_data.exec_type,
                            "Execution missing symbol and order not in cache (external order)"
                        );
                        continue;
                    };

                    // Trade executions emit OrderStatusReport first (so engine knows the order),
                    // then FillReport on next iteration
                    let cached_order_qty = self.order_qty_cache.get(&exec_data.order_id).copied();

                    if exec_data.exec_type == KrakenExecType::Trade {
                        match parse_ws_order_status_report(
                            &exec_data,
                            &instrument,
                            account_id,
                            cached_order_qty,
                            ts_init,
                        ) {
                            Ok(status_report) => {
                                self.pending_messages.push_back(
                                    NautilusWsMessage::OrderStatusReport(Box::new(status_report)),
                                );
                                match parse_ws_fill_report(
                                    &exec_data,
                                    &instrument,
                                    account_id,
                                    ts_init,
                                ) {
                                    Ok(fill_report) => {
                                        self.pending_messages.push_back(
                                            NautilusWsMessage::FillReport(Box::new(fill_report)),
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to parse fill report: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to parse order status report for trade: {e}"
                                );
                            }
                        }
                    } else {
                        match parse_ws_order_status_report(
                            &exec_data,
                            &instrument,
                            account_id,
                            cached_order_qty,
                            ts_init,
                        ) {
                            Ok(report) => {
                                self.pending_messages.push_back(
                                    NautilusWsMessage::OrderStatusReport(Box::new(report)),
                                );
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse order status report: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize execution data: {e}");
                }
            }
        }

        // Return first queued message (rest returned via next() pending check)
        self.pending_messages.pop_front()
    }
}
