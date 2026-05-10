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

//! Feed handler for parsing Coinbase WebSocket messages into Nautilus types.

use std::{fmt::Debug, sync::Arc};

use ahash::AHashMap;
use nautilus_core::{
    AtomicMap, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDeltas, QuoteTick, TradeTick},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::OrderStatusReport,
};
use nautilus_network::{RECONNECTED, websocket::WebSocketClient};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use crate::{
    common::consts::COINBASE,
    websocket::{
        client::COINBASE_WS_SUBSCRIPTION_KEYS,
        messages::{CoinbaseWsMessage, CoinbaseWsSubscription, WsEventType, WsOrderUpdate},
        parse::{
            parse_ws_candle, parse_ws_l2_snapshot, parse_ws_l2_update, parse_ws_ticker,
            parse_ws_trade, parse_ws_user_event_to_order_status_report,
        },
    },
};

fn instrument_id_from_product(product_id: &Ustr) -> InstrumentId {
    InstrumentId::from(format!("{product_id}.{COINBASE}").as_str())
}

fn resolve_instrument_id(aliases: &AtomicMap<Ustr, Ustr>, product_id: &Ustr) -> InstrumentId {
    let resolved = aliases.get_cloned(product_id).unwrap_or(*product_id);
    instrument_id_from_product(&resolved)
}

/// Commands sent from [`super::client::CoinbaseWebSocketClient`] to the feed handler.
pub enum HandlerCommand {
    /// Provides the network-level WebSocket client.
    SetClient(WebSocketClient),
    /// Subscribes to a channel for the given product IDs.
    Subscribe(CoinbaseWsSubscription),
    /// Unsubscribes from a channel.
    Unsubscribe(CoinbaseWsSubscription),
    /// Disconnects the WebSocket.
    Disconnect,
    /// Caches instruments for precision lookups during parsing.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Updates a single instrument in the cache.
    UpdateInstrument(Box<InstrumentAny>),
    /// Registers a bar type for candle parsing.
    AddBarType { key: String, bar_type: BarType },
    /// Removes a bar type registration.
    RemoveBarType { key: String },
    /// Sets the account ID used when emitting user-channel execution reports.
    SetAccountId(AccountId),
}

impl Debug for HandlerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetClient(_) => f.write_str("SetClient"),
            Self::Subscribe(s) => write!(f, "Subscribe({:?})", s.channel),
            Self::Unsubscribe(s) => write!(f, "Unsubscribe({:?})", s.channel),
            Self::Disconnect => f.write_str("Disconnect"),
            Self::InitializeInstruments(v) => write!(f, "InitializeInstruments({})", v.len()),
            Self::UpdateInstrument(i) => write!(f, "UpdateInstrument({})", i.id()),
            Self::AddBarType { key, .. } => write!(f, "AddBarType({key})"),
            Self::RemoveBarType { key } => write!(f, "RemoveBarType({key})"),
            Self::SetAccountId(id) => write!(f, "SetAccountId({id})"),
        }
    }
}

/// Carrier for a single user-channel order update.
///
/// Pairs the parsed [`OrderStatusReport`] with the resolved instrument and
/// the raw venue payload so downstream consumers (e.g. the execution client)
/// can diff cumulative quantity and fees against their own tracked state.
///
/// `is_snapshot` is true when the wrapping `WsUserEvent` was a `snapshot`
/// type. Snapshots restate the current cumulative state of every open order
/// and must NOT be interpreted as fresh fills, otherwise a cold start (or
/// any state-clearing reconnect) would synthesize phantom fills covering the
/// entire pre-existing cumulative quantity.
#[derive(Debug, Clone)]
pub struct UserOrderUpdate {
    pub report: Box<OrderStatusReport>,
    pub update: Box<WsOrderUpdate>,
    pub instrument: InstrumentAny,
    pub is_snapshot: bool,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Nautilus-typed messages produced by the feed handler.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Trade tick from market_trades channel.
    Trade(TradeTick),
    /// Quote tick from ticker channel.
    Quote(QuoteTick),
    /// Order book deltas from l2_data channel.
    Deltas(OrderBookDeltas),
    /// Bar from candles channel.
    Bar(Bar),
    /// Order status update from the user channel.
    UserOrder(Box<UserOrderUpdate>),
    /// Futures balance summary snapshot from the
    /// `futures_balance_summary` channel.
    FuturesBalanceSummary(Box<crate::websocket::messages::WsFcmBalanceSummary>),
    /// The connection was re-established after a drop.
    Reconnected,
    /// An error occurred during message processing.
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
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    /// Shared with [`super::client::CoinbaseWebSocketClient`]; consulted in
    /// `resolve_instrument_id` to re-key inbound messages whose wire `product_id`
    /// is the canonical alias of a subscribed/submitted product.
    subscription_aliases: Arc<AtomicMap<Ustr, Ustr>>,
    bar_types: AHashMap<String, BarType>,
    account_id: Option<AccountId>,
    buffer: Vec<NautilusWsMessage>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub fn new(
        signal: Arc<std::sync::atomic::AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscription_aliases: Arc<AtomicMap<Ustr, Ustr>>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            instruments: AHashMap::new(),
            subscription_aliases,
            bar_types: AHashMap::new(),
            account_id: None,
            buffer: Vec::new(),
        }
    }

    fn resolve_instrument_id(&self, product_id: &Ustr) -> InstrumentId {
        resolve_instrument_id(&self.subscription_aliases, product_id)
    }

    /// Sets the account ID used to stamp user-channel execution reports.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
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
                        HandlerCommand::Subscribe(sub) => {
                            self.send_subscription(&sub).await;
                        }
                        HandlerCommand::Unsubscribe(sub) => {
                            self.send_subscription(&sub).await;
                        }
                        HandlerCommand::Disconnect => {
                            if let Some(client) = self.client.take() {
                                // Transition to CLOSED immediately without waiting
                                // for ACTIVE (avoids blocking during reconnect)
                                client.notify_closed();
                            }
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments.insert(inst.id(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments.insert(inst.id(), *inst);
                        }
                        HandlerCommand::AddBarType { key, bar_type } => {
                            self.bar_types.insert(key, bar_type);
                        }
                        HandlerCommand::RemoveBarType { key } => {
                            self.bar_types.remove(&key);
                        }
                        HandlerCommand::SetAccountId(account_id) => {
                            self.account_id = Some(account_id);
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

    async fn send_subscription(&self, sub: &CoinbaseWsSubscription) {
        let Some(client) = &self.client else {
            log::warn!("Cannot send subscription, no WebSocket client set");
            return;
        };

        match serde_json::to_string(sub) {
            Ok(json) => {
                if let Err(e) = client
                    .send_text(json, Some(COINBASE_WS_SUBSCRIPTION_KEYS.as_slice()))
                    .await
                {
                    log::error!("Failed to send subscription: {e}");
                }
            }
            Err(e) => log::error!("Failed to serialize subscription: {e}"),
        }
    }

    fn handle_text(&mut self, text: &str) -> Option<NautilusWsMessage> {
        if text == RECONNECTED {
            return Some(NautilusWsMessage::Reconnected);
        }

        let ts_init = self.clock.get_time_ns();

        let msg: CoinbaseWsMessage = match serde_json::from_str(text) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Failed to parse WS message: {e}");
                return None;
            }
        };

        match msg {
            CoinbaseWsMessage::L2Data {
                timestamp, events, ..
            } => self.handle_l2_events(&events, &timestamp, ts_init),
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                self.handle_market_trades(&events, ts_init)
            }
            CoinbaseWsMessage::Ticker {
                timestamp, events, ..
            }
            | CoinbaseWsMessage::TickerBatch {
                timestamp, events, ..
            } => self.handle_ticker(&events, &timestamp, ts_init),
            CoinbaseWsMessage::Candles { events, .. } => self.handle_candles(&events, ts_init),
            CoinbaseWsMessage::Heartbeats { .. } => None,
            CoinbaseWsMessage::Subscriptions { events, .. } => {
                // Coinbase emits this after every subscribe and unsubscribe
                // with the full current subscription set, so it's noisy at
                // INFO and not strictly a "confirmation" of the latest action.
                log::debug!("Subscription state: {events:?}");
                None
            }
            CoinbaseWsMessage::User {
                timestamp, events, ..
            } => self.handle_user_events(&events, &timestamp, ts_init),
            CoinbaseWsMessage::FuturesBalanceSummary { events, .. } => {
                self.handle_futures_balance_summary(events)
            }
            CoinbaseWsMessage::Status { events, .. } => {
                log::debug!(
                    "Ignoring {} status events until venue status handling lands",
                    events.len()
                );
                None
            }
        }
    }

    fn handle_l2_events(
        &mut self,
        events: &[crate::websocket::messages::WsL2DataEvent],
        timestamp: &str,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let ts_event = match crate::http::parse::parse_rfc3339_timestamp(timestamp) {
            Ok(ts) => ts,
            Err(e) => {
                log::warn!("Failed to parse L2 message timestamp {timestamp}: {e}");
                ts_init
            }
        };

        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            let instrument_id = self.resolve_instrument_id(&event.product_id);

            let instrument = match self.instruments.get(&instrument_id) {
                Some(inst) => inst,
                None => {
                    log::warn!("No instrument cached for {instrument_id}");
                    continue;
                }
            };

            let result = match event.event_type {
                WsEventType::Snapshot => parse_ws_l2_snapshot(event, instrument, ts_event, ts_init),
                WsEventType::Update => parse_ws_l2_update(event, instrument, ts_event, ts_init),
            };

            match result {
                Ok(deltas) => {
                    let msg = NautilusWsMessage::Deltas(deltas);

                    if first.is_none() {
                        first = Some(msg);
                    } else {
                        self.buffer.push(msg);
                    }
                }
                Err(e) => log::warn!("Failed to parse L2 event: {e}"),
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }

    fn handle_market_trades(
        &mut self,
        events: &[crate::websocket::messages::WsMarketTradesEvent],
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        for event in events {
            for trade in &event.trades {
                let instrument_id = self.resolve_instrument_id(&trade.product_id);

                let instrument = match self.instruments.get(&instrument_id) {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                match parse_ws_trade(trade, instrument, ts_init) {
                    Ok(tick) => {
                        self.buffer_remaining_trades(events, event, trade, ts_init);
                        // Reverse so pop() drains in exchange order
                        self.buffer.reverse();
                        return Some(NautilusWsMessage::Trade(tick));
                    }
                    Err(e) => log::warn!("Failed to parse trade: {e}"),
                }
            }
        }
        None
    }

    fn buffer_remaining_trades(
        &mut self,
        events: &[crate::websocket::messages::WsMarketTradesEvent],
        current_event: &crate::websocket::messages::WsMarketTradesEvent,
        current_trade: &crate::websocket::messages::WsTrade,
        ts_init: UnixNanos,
    ) {
        let mut found_current = false;

        for event in events {
            let is_current_event = std::ptr::eq(event, current_event);

            for trade in &event.trades {
                if !found_current {
                    if is_current_event && std::ptr::eq(trade, current_trade) {
                        found_current = true;
                    }
                    continue;
                }

                let instrument_id = self.resolve_instrument_id(&trade.product_id);

                if let Some(instrument) = self.instruments.get(&instrument_id)
                    && let Ok(tick) = parse_ws_trade(trade, instrument, ts_init)
                {
                    self.buffer.push(NautilusWsMessage::Trade(tick));
                }
            }
        }
    }

    fn handle_ticker(
        &mut self,
        events: &[crate::websocket::messages::WsTickerEvent],
        timestamp: &str,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let ts_event = crate::http::parse::parse_rfc3339_timestamp(timestamp).unwrap_or(ts_init);

        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            for ticker in &event.tickers {
                let instrument_id = self.resolve_instrument_id(&ticker.product_id);

                let instrument = match self.instruments.get(&instrument_id) {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                match parse_ws_ticker(ticker, instrument, ts_event, ts_init) {
                    Ok(quote) => {
                        let msg = NautilusWsMessage::Quote(quote);

                        if first.is_none() {
                            first = Some(msg);
                        } else {
                            self.buffer.push(msg);
                        }
                    }
                    Err(e) => log::warn!("Failed to parse ticker: {e}"),
                }
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }

    fn handle_user_events(
        &mut self,
        events: &[crate::websocket::messages::WsUserEvent],
        timestamp: &str,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let Some(account_id) = self.account_id else {
            log::debug!(
                "Dropping user event: account_id not set (call SetAccountId after connect)"
            );
            return None;
        };

        let ts_event = match crate::http::parse::parse_rfc3339_timestamp(timestamp) {
            Ok(ts) => ts,
            Err(e) => {
                log::warn!("Failed to parse user message timestamp {timestamp}: {e}");
                ts_init
            }
        };

        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            let is_snapshot = matches!(event.event_type, WsEventType::Snapshot);

            for order in &event.orders {
                let instrument_id = self.resolve_instrument_id(&order.product_id);
                let instrument = match self.instruments.get(&instrument_id).cloned() {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                self.emit_user_event_messages(
                    order,
                    &instrument,
                    account_id,
                    is_snapshot,
                    ts_event,
                    ts_init,
                    &mut first,
                );
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_user_event_messages(
        &mut self,
        order: &WsOrderUpdate,
        instrument: &InstrumentAny,
        account_id: AccountId,
        is_snapshot: bool,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        first: &mut Option<NautilusWsMessage>,
    ) {
        let report = match parse_ws_user_event_to_order_status_report(
            order, instrument, account_id, ts_event, ts_init,
        ) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Failed to parse user order update: {e}");
                return;
            }
        };

        let msg = NautilusWsMessage::UserOrder(Box::new(UserOrderUpdate {
            report: Box::new(report),
            update: Box::new(order.clone()),
            instrument: instrument.clone(),
            is_snapshot,
            ts_event,
            ts_init,
        }));

        if first.is_none() {
            *first = Some(msg);
        } else {
            self.buffer.push(msg);
        }
    }

    fn handle_futures_balance_summary(
        &mut self,
        events: Vec<crate::websocket::messages::WsFuturesBalanceSummaryEvent>,
    ) -> Option<NautilusWsMessage> {
        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            let msg = NautilusWsMessage::FuturesBalanceSummary(Box::new(event.fcm_balance_summary));

            if first.is_none() {
                first = Some(msg);
            } else {
                self.buffer.push(msg);
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }

    fn handle_candles(
        &mut self,
        events: &[crate::websocket::messages::WsCandlesEvent],
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            for candle in &event.candles {
                let key = candle.product_id.as_str();

                let bar_type = match self.bar_types.get(key) {
                    Some(bt) => *bt,
                    None => {
                        log::debug!("No bar type registered for {key}");
                        continue;
                    }
                };

                let instrument_id = self.resolve_instrument_id(&candle.product_id);

                let instrument = match self.instruments.get(&instrument_id) {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                match parse_ws_candle(candle, bar_type, instrument, ts_init) {
                    Ok(bar) => {
                        let msg = NautilusWsMessage::Bar(bar);

                        if first.is_none() {
                            first = Some(msg);
                        } else {
                            self.buffer.push(msg);
                        }
                    }
                    Err(e) => log::warn!("Failed to parse candle: {e}"),
                }
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use nautilus_model::{
        identifiers::{Symbol, Venue},
        instruments::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_fixture;

    fn test_handler() -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        FeedHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            Arc::new(AtomicMap::new()),
        )
    }

    fn btc_usd_instrument() -> InstrumentAny {
        let instrument_id =
            InstrumentId::new(Symbol::new("BTC-USD"), Venue::new(Ustr::from("COINBASE")));
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("BTC-USD"),
            Currency::get_or_create_crypto("BTC"),
            Currency::get_or_create_crypto("USD"),
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            Some(Quantity::from("0.00000001")),
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
    fn test_handle_text_drops_user_channel_when_account_id_unset() {
        let json = load_test_fixture("ws_user.json");
        let mut handler = test_handler();

        // account_id is intentionally left unset; events should be dropped
        assert!(handler.handle_text(&json).is_none());
        assert!(handler.buffer.is_empty());
    }

    #[rstest]
    fn test_handle_user_event_emits_user_order_update() {
        use nautilus_model::{
            enums::{OrderSide, OrderStatus},
            identifiers::AccountId,
            types::Quantity,
        };

        use crate::common::enums::CoinbaseProductType;

        let json = load_test_fixture("ws_user.json");
        let mut handler = test_handler();
        handler.set_account_id(AccountId::new("COINBASE-001"));
        handler
            .instruments
            .insert(btc_usd_instrument().id(), btc_usd_instrument());

        let msg = handler
            .handle_text(&json)
            .expect("handler should emit a user-channel update");

        match msg {
            NautilusWsMessage::UserOrder(carrier) => {
                // Status report fields.
                assert_eq!(carrier.report.account_id.as_str(), "COINBASE-001");
                assert_eq!(carrier.report.instrument_id, btc_usd_instrument().id());
                assert_eq!(
                    carrier.report.venue_order_id.as_str(),
                    "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
                );
                assert_eq!(
                    carrier.report.client_order_id.unwrap().as_str(),
                    "11111-000000-000001"
                );
                assert_eq!(carrier.report.order_side, OrderSide::Buy);
                assert_eq!(carrier.report.order_status, OrderStatus::Accepted);
                assert_eq!(carrier.report.filled_qty, Quantity::from("0.00000000"));
                assert_eq!(carrier.report.quantity, Quantity::from("0.00100000"));

                // Raw venue update fields.
                assert_eq!(carrier.update.product_id, "BTC-USD");
                assert_eq!(carrier.update.product_type, CoinbaseProductType::Spot);
                assert_eq!(carrier.update.cumulative_quantity, "0");
                assert_eq!(carrier.update.leaves_quantity, "0.001");

                // Carrier metadata.
                assert_eq!(carrier.instrument.id(), btc_usd_instrument().id());
                assert!(carrier.ts_event.as_u64() > 0);
            }
            other => panic!("expected UserOrder, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_text_ignores_status_channel() {
        let json = r#"{
          "channel": "status",
          "client_id": "",
          "timestamp": "2023-02-09T20:29:49.753424311Z",
          "sequence_num": 0,
          "events": [
            {
              "type": "snapshot",
              "products": [
                {
                  "product_type": "SPOT",
                  "id": "BTC-USD",
                  "base_currency": "BTC",
                  "quote_currency": "USD",
                  "base_increment": "0.00000001",
                  "quote_increment": "0.01",
                  "display_name": "BTC/USD",
                  "status": "online",
                  "status_message": "",
                  "min_market_funds": "1"
                }
              ]
            }
          ]
        }"#;
        let mut handler = test_handler();

        assert!(handler.handle_text(json).is_none());
        assert!(handler.buffer.is_empty());
    }

    #[rstest]
    fn test_handle_l2_update_uses_batch_timestamp_for_all_deltas() {
        let json = load_test_fixture("ws_l2_data_update.json");
        let mut handler = test_handler();
        handler
            .instruments
            .insert(btc_usd_instrument().id(), btc_usd_instrument());

        let msg = handler
            .handle_text(&json)
            .expect("handler should emit deltas for a valid L2 update");

        let deltas = match msg {
            NautilusWsMessage::Deltas(d) => d,
            other => panic!("expected Deltas, was {other:?}"),
        };

        assert!(!deltas.deltas.is_empty());
        let expected_ts = deltas.deltas[0].ts_event;
        for delta in &deltas.deltas {
            assert_eq!(
                delta.ts_event, expected_ts,
                "all deltas in a batch must share ts_event"
            );
        }
    }

    #[rstest]
    fn test_handle_l2_update_malformed_timestamp_falls_back_to_ts_init() {
        let json = load_test_fixture("ws_l2_data_update.json")
            .replace("2026-04-07T14:30:01.456789Z", "not-a-valid-timestamp");
        let mut handler = test_handler();
        handler
            .instruments
            .insert(btc_usd_instrument().id(), btc_usd_instrument());

        let msg = handler
            .handle_text(&json)
            .expect("handler should still emit deltas when timestamp is malformed");

        let deltas = match msg {
            NautilusWsMessage::Deltas(d) => d,
            other => panic!("expected Deltas, was {other:?}"),
        };

        assert!(!deltas.deltas.is_empty());
        for delta in &deltas.deltas {
            assert_eq!(
                delta.ts_event, delta.ts_init,
                "malformed timestamp must fall back to ts_init"
            );
        }
    }

    #[rstest]
    fn test_handle_text_emits_futures_balance_summary_snapshot() {
        use rust_decimal::Decimal;

        let json = r#"{
          "channel": "futures_balance_summary",
          "client_id": "",
          "timestamp": "2023-02-09T20:33:57.609931463Z",
          "sequence_num": 0,
          "events": [
            {
              "type": "snapshot",
              "fcm_balance_summary": {
                "futures_buying_power": "100.00",
                "total_usd_balance": "200.00",
                "cbi_usd_balance": "300.00",
                "cfm_usd_balance": "400.00",
                "total_open_orders_hold_amount": "500.00",
                "unrealized_pnl": "600.00",
                "daily_realized_pnl": "0",
                "initial_margin": "700.00",
                "available_margin": "800.00",
                "liquidation_threshold": "900.00",
                "liquidation_buffer_amount": "1000.00",
                "liquidation_buffer_percentage": "1000",
                "intraday_margin_window_measure": {
                  "margin_window_type": "FCM_MARGIN_WINDOW_TYPE_INTRADAY",
                  "margin_level": "MARGIN_LEVEL_TYPE_BASE",
                  "initial_margin": "100.00",
                  "maintenance_margin": "200.00",
                  "liquidation_buffer_percentage": "1000",
                  "total_hold": "100.00",
                  "futures_buying_power": "400.00"
                },
                "overnight_margin_window_measure": {
                  "margin_window_type": "FCM_MARGIN_WINDOW_TYPE_OVERNIGHT",
                  "margin_level": "MARGIN_LEVEL_TYPE_BASE",
                  "initial_margin": "300.00",
                  "maintenance_margin": "200.00",
                  "liquidation_buffer_percentage": "1000",
                  "total_hold": "-30.00",
                  "futures_buying_power": "2000.00"
                }
              }
            }
          ]
        }"#;
        let mut handler = test_handler();

        let msg = handler
            .handle_text(json)
            .expect("handler should emit a futures balance summary");
        match msg {
            NautilusWsMessage::FuturesBalanceSummary(summary) => {
                assert_eq!(summary.futures_buying_power, Decimal::from(100));
                assert_eq!(summary.total_usd_balance, Decimal::from(200));
                assert_eq!(summary.total_open_orders_hold_amount, Decimal::from(500));
                assert_eq!(summary.available_margin, Decimal::from(800));
                let intraday = &summary.intraday_margin_window_measure;
                assert_eq!(intraday.initial_margin, Decimal::from(100));
                assert_eq!(intraday.maintenance_margin, Decimal::from(200));
                let overnight = &summary.overnight_margin_window_measure;
                assert_eq!(overnight.initial_margin, Decimal::from(300));
                assert_eq!(overnight.maintenance_margin, Decimal::from(200));
                // `total_hold` carries negative values on the wire; ensure
                // the signed decimal survives the round trip.
                assert_eq!(overnight.total_hold, "-30".parse::<Decimal>().unwrap());
            }
            other => panic!("expected FuturesBalanceSummary, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_text_routes_reconnected_sentinel() {
        let mut handler = test_handler();
        let result = handler.handle_text(RECONNECTED);
        assert!(matches!(result, Some(NautilusWsMessage::Reconnected)));
    }

    #[rstest]
    fn test_signal_release_acquire_exits_handler_loop() {
        use std::sync::atomic::Ordering;

        let signal = Arc::new(AtomicBool::new(false));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handler =
            FeedHandler::new(signal.clone(), cmd_rx, raw_rx, Arc::new(AtomicMap::new()));

        signal.store(true, Ordering::Release);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async { handler.next().await });
        assert!(result.is_none(), "{result:?}");
    }
}
