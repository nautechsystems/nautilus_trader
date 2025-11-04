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

//! WebSocket message handler for BitMEX.

use std::sync::{Arc, atomic::AtomicBool};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    data::Data,
    enums::{OrderStatus, OrderType},
    identifiers::{AccountId, ClientOrderId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{AuthTracker, SubscriptionState},
};
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    cache::QuoteCache,
    enums::{BitmexAction, BitmexWsAuthAction, BitmexWsOperation, BitmexWsTopic},
    messages::{
        BitmexHttpRequest, BitmexTableMessage, BitmexWsMessage, NautilusWsMessage, OrderData,
    },
    parse::{
        parse_book_msg_vec, parse_book10_msg_vec, parse_execution_msg, parse_funding_msg,
        parse_instrument_msg, parse_order_msg, parse_order_update_msg, parse_position_msg,
        parse_trade_bin_msg_vec, parse_trade_msg_vec, parse_wallet_msg,
    },
};
use crate::common::enums::BitmexExecType;

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

struct RawFeedHandler {
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    signal: Arc<AtomicBool>,
}

impl RawFeedHandler {
    /// Creates a new [`RawFeedHandler`] instance.
    pub fn new(
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        signal: Arc<AtomicBool>,
    ) -> Self {
        Self { raw_rx, signal }
    }

    /// Get the next message from the WebSocket stream.
    async fn next(&mut self) -> Option<BitmexWsMessage> {
        loop {
            tokio::select! {
                msg = self.raw_rx.recv() => match msg {
                    Some(msg) => match msg {
                        Message::Text(text) => {
                            if text == RECONNECTED {
                                tracing::info!("Received WebSocket reconnection signal");
                                return Some(BitmexWsMessage::Reconnected);
                            }

                            tracing::trace!("Raw websocket message: {text}");

                            if Self::is_heartbeat_message(&text) {
                                tracing::trace!(
                                    "Ignoring heartbeat control message: {text}"
                                );
                                continue;
                            }

                            match serde_json::from_str(&text) {
                                Ok(msg) => match &msg {
                                    BitmexWsMessage::Welcome {
                                        version,
                                        heartbeat_enabled,
                                        limit,
                                        ..
                                    } => {
                                        tracing::info!(
                                            version = version,
                                            heartbeat = heartbeat_enabled,
                                            rate_limit = ?limit.remaining,
                                            "Welcome to the BitMEX Realtime API:",
                                        );
                                    }
                                    BitmexWsMessage::Subscription { .. } => return Some(msg),
                                    BitmexWsMessage::Error { status, error, .. } => {
                                        tracing::error!(
                                            status = status,
                                            error = error,
                                            "Received error from BitMEX"
                                        );
                                    }
                                    _ => return Some(msg),
                                },
                                Err(e) => {
                                    tracing::error!("Failed to parse WebSocket message: {e}: {text}");
                                }
                            }
                        }
                        Message::Binary(msg) => {
                            tracing::debug!("Raw binary: {msg:?}");
                        }
                        Message::Close(_) => {
                            tracing::debug!("Received close message, waiting for reconnection");
                            continue;
                        }
                        msg => match msg {
                            Message::Ping(data) => {
                                tracing::trace!("Received ping frame with {} bytes", data.len());
                            }
                            Message::Pong(data) => {
                                tracing::trace!("Received pong frame with {} bytes", data.len());
                            }
                            Message::Frame(frame) => {
                                tracing::debug!("Received raw frame: {frame:?}");
                            }
                            _ => {
                                tracing::warn!("Unexpected message type: {msg:?}");
                            }
                        },
                    }
                    None => {
                        tracing::info!("WebSocket stream closed");
                        return None;
                    }
                },
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }
                }
            }
        }
    }

    fn is_heartbeat_message(text: &str) -> bool {
        let trimmed = text.trim();

        if !trimmed.starts_with('{') || trimmed.len() > 64 {
            return false;
        }

        trimmed.contains("\"op\":\"ping\"") || trimmed.contains("\"op\":\"pong\"")
    }
}

pub(super) struct FeedHandler {
    handler: RawFeedHandler,
    pub out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    #[allow(
        dead_code,
        reason = "May be needed for future account-specific processing"
    )]
    account_id: AccountId,
    pub auth_tracker: AuthTracker,
    pub subscriptions: SubscriptionState,
    pub signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    order_type_cache: Arc<DashMap<ClientOrderId, OrderType>>,
    order_symbol_cache: Arc<DashMap<ClientOrderId, Ustr>>,
    quote_cache: QuoteCache,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<Message>,
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        account_id: AccountId,
        auth_tracker: AuthTracker,
        subscriptions: SubscriptionState,
        order_type_cache: Arc<DashMap<ClientOrderId, OrderType>>,
        order_symbol_cache: Arc<DashMap<ClientOrderId, Ustr>>,
    ) -> Self {
        let handler = RawFeedHandler::new(receiver, signal.clone());

        Self {
            handler,
            cmd_rx,
            out_tx,
            account_id,
            auth_tracker,
            subscriptions,
            signal,
            instruments_cache: AHashMap::new(),
            order_type_cache,
            order_symbol_cache,
            quote_cache: QuoteCache::new(),
        }
    }

    #[inline]
    fn get_instrument(
        cache: &AHashMap<Ustr, InstrumentAny>,
        symbol: &Ustr,
    ) -> Option<InstrumentAny> {
        cache.get(symbol).cloned()
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        let clock = get_atomic_clock_realtime();

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                    }
                    // Continue processing following command
                    continue;
                }

                Some(msg) = self.handler.next() => {
            match msg {
                BitmexWsMessage::Reconnected => {
                    // Return reconnection signal to outer loop
                    self.quote_cache.clear();
                    return Some(NautilusWsMessage::Reconnected);
                }
                BitmexWsMessage::Subscription {
                    success,
                    subscribe,
                    request,
                    error,
                } => {
                    self.handle_subscription_message(
                        success,
                        subscribe.as_ref(),
                        request.as_ref(),
                        error.as_deref(),
                    );
                    continue;
                }
                BitmexWsMessage::Table(table_msg) => {
                    let ts_init = clock.get_time_ns();

                    return Some(match table_msg {
                        BitmexTableMessage::OrderBookL2 { action, data } => {
                            if data.is_empty() {
                                continue;
                            }
                            let data = parse_book_msg_vec(
                                data,
                                action,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::OrderBookL2_25 { action, data } => {
                            if data.is_empty() {
                                continue;
                            }
                            let data = parse_book_msg_vec(
                                data,
                                action,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::OrderBook10 { data, .. } => {
                            if data.is_empty() {
                                continue;
                            }
                            let data = parse_book10_msg_vec(
                                data,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::Quote { mut data, .. } => {
                            // Index symbols may return empty quote data
                            if data.is_empty() {
                                continue;
                            }

                            let msg = data.remove(0);
                            let Some(instrument) = Self::get_instrument(&self.instruments_cache, &msg.symbol) else {
                                tracing::error!(
                                    "Instrument cache miss: quote message dropped for symbol={}",
                                    msg.symbol
                                );
                                continue;
                            };

                            if let Some(quote) =
                                self.quote_cache.process(&msg, &instrument, ts_init)
                            {
                                NautilusWsMessage::Data(vec![Data::Quote(quote)])
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Trade { data, .. } => {
                            if data.is_empty() {
                                continue;
                            }
                            let data =
                                parse_trade_msg_vec(data, &self.instruments_cache, ts_init);

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin1m { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin1m,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin5m { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin5m,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin1h { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin1h,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        BitmexTableMessage::TradeBin1d { action, data } => {
                            if action == BitmexAction::Partial || data.is_empty() {
                                continue;
                            }
                            let data = parse_trade_bin_msg_vec(
                                data,
                                BitmexWsTopic::TradeBin1d,
                                &self.instruments_cache,
                                ts_init,
                            );

                            NautilusWsMessage::Data(data)
                        }
                        // Execution messages
                        // Note: BitMEX may send duplicate order status updates for the same order
                        // (e.g., immediate response + stream update). This is expected behavior.
                        BitmexTableMessage::Order { data, .. } => {
                            // Process all orders in the message
                            let mut reports = Vec::with_capacity(data.len());

                            for order_data in data {
                                match order_data {
                                    OrderData::Full(order_msg) => {
                                        let Some(instrument) =
                                            Self::get_instrument(&self.instruments_cache, &order_msg.symbol)
                                        else {
                                            tracing::error!(
                                                "Instrument cache miss: order message dropped for symbol={}, order_id={}",
                                                order_msg.symbol,
                                                order_msg.order_id
                                            );
                                            continue;
                                        };

                                        match parse_order_msg(
                                            &order_msg,
                                            &instrument,
                                            &self.order_type_cache,
                                        ) {
                                            Ok(report) => {
                                                // Cache the order type and symbol AFTER successful parse
                                                if let Some(client_order_id) = &order_msg.cl_ord_id
                                                {
                                                    let client_order_id =
                                                        ClientOrderId::new(client_order_id);

                                                    if let Some(ord_type) = &order_msg.ord_type {
                                                        let order_type: OrderType =
                                                            (*ord_type).into();
                                                        self.order_type_cache
                                                            .insert(client_order_id, order_type);
                                                    }

                                                    // Cache symbol for execution message routing
                                                    self.order_symbol_cache
                                                        .insert(client_order_id, order_msg.symbol);
                                                }

                                                if is_terminal_order_status(report.order_status)
                                                    && let Some(client_id) = report.client_order_id
                                                {
                                                    self.order_type_cache.remove(&client_id);
                                                    self.order_symbol_cache.remove(&client_id);
                                                }

                                                reports.push(report);
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    error = %e,
                                                    symbol = %order_msg.symbol,
                                                    order_id = %order_msg.order_id,
                                                    time_in_force = ?order_msg.time_in_force,
                                                    "Failed to parse full order message - potential data loss"
                                                );
                                                // TODO: Add metric counter for parse failures
                                                continue;
                                            }
                                        }
                                    }
                                    OrderData::Update(msg) => {
                                        let Some(instrument) = Self::get_instrument(&self.instruments_cache, &msg.symbol)
                                        else {
                                            tracing::error!(
                                                "Instrument cache miss: order update dropped for symbol={}, order_id={}",
                                                msg.symbol,
                                                msg.order_id
                                            );
                                            continue;
                                        };

                                        // Populate cache for execution message routing (handles edge case where update arrives before full snapshot)
                                        if let Some(cl_ord_id) = &msg.cl_ord_id {
                                            let client_order_id = ClientOrderId::new(cl_ord_id);
                                            self.order_symbol_cache
                                                .insert(client_order_id, msg.symbol);
                                        }

                                        if let Some(event) = parse_order_update_msg(
                                            &msg,
                                            &instrument,
                                            self.account_id,
                                        ) {
                                            return Some(NautilusWsMessage::OrderUpdated(event));
                                        } else {
                                            tracing::warn!(
                                                order_id = %msg.order_id,
                                                price = ?msg.price,
                                                "Skipped order update message (insufficient data)"
                                            );
                                        }
                                    }
                                }
                            }

                            if reports.is_empty() {
                                continue;
                            }

                            NautilusWsMessage::OrderStatusReports(reports)
                        }
                        BitmexTableMessage::Execution { data, .. } => {
                            let mut fills = Vec::with_capacity(data.len());

                            for exec_msg in data {
                                // Try to get symbol, fall back to cache lookup if missing
                                let symbol_opt = if let Some(sym) = &exec_msg.symbol {
                                    Some(*sym)
                                } else if let Some(cl_ord_id) = &exec_msg.cl_ord_id {
                                    // Try to look up symbol from order_symbol_cache
                                    let client_order_id = ClientOrderId::new(cl_ord_id);
                                    self.order_symbol_cache
                                        .get(&client_order_id)
                                        .map(|r| *r.value())
                                } else {
                                    None
                                };

                                let Some(symbol) = symbol_opt else {
                                    // Symbol missing - log appropriately based on exec type and whether we had clOrdID
                                    if let Some(cl_ord_id) = &exec_msg.cl_ord_id {
                                        if exec_msg.exec_type == Some(BitmexExecType::Trade) {
                                            tracing::warn!(
                                                cl_ord_id = %cl_ord_id,
                                                exec_id = ?exec_msg.exec_id,
                                                ord_rej_reason = ?exec_msg.ord_rej_reason,
                                                text = ?exec_msg.text,
                                                "Execution message missing symbol and not found in cache"
                                            );
                                        } else {
                                            tracing::debug!(
                                                cl_ord_id = %cl_ord_id,
                                                exec_id = ?exec_msg.exec_id,
                                                exec_type = ?exec_msg.exec_type,
                                                ord_rej_reason = ?exec_msg.ord_rej_reason,
                                                text = ?exec_msg.text,
                                                "Execution message missing symbol and not found in cache"
                                            );
                                        }
                                    } else {
                                        // CancelReject messages without symbol/clOrdID are expected when using
                                        // redundant cancel broadcasting - one cancel succeeds, others arrive late
                                        // and BitMEX responds with CancelReject but doesn't populate the fields
                                        if exec_msg.exec_type == Some(BitmexExecType::CancelReject)
                                        {
                                            tracing::debug!(
                                                exec_id = ?exec_msg.exec_id,
                                                order_id = ?exec_msg.order_id,
                                                "CancelReject message missing symbol/clOrdID (expected with redundant cancels)"
                                            );
                                        } else {
                                            tracing::warn!(
                                                exec_id = ?exec_msg.exec_id,
                                                order_id = ?exec_msg.order_id,
                                                exec_type = ?exec_msg.exec_type,
                                                ord_rej_reason = ?exec_msg.ord_rej_reason,
                                                text = ?exec_msg.text,
                                                "Execution message missing both symbol and clOrdID, cannot process"
                                            );
                                        }
                                    }
                                    continue;
                                };

                                let Some(instrument) = Self::get_instrument(&self.instruments_cache, &symbol) else {
                                    tracing::error!(
                                        "Instrument cache miss: execution message dropped for symbol={}, exec_id={:?}, exec_type={:?}, Liquidation/ADL fills may be lost",
                                        symbol,
                                        exec_msg.exec_id,
                                        exec_msg.exec_type
                                    );
                                    continue;
                                };

                                if let Some(fill) = parse_execution_msg(exec_msg, &instrument) {
                                    fills.push(fill);
                                }
                            }

                            if fills.is_empty() {
                                continue;
                            }
                            NautilusWsMessage::FillReports(fills)
                        }
                        BitmexTableMessage::Position { data, .. } => {
                            if let Some(pos_msg) = data.into_iter().next() {
                                let Some(instrument) = Self::get_instrument(&self.instruments_cache, &pos_msg.symbol) else {
                                    tracing::error!(
                                        "Instrument cache miss: position message dropped for symbol={}, account={}",
                                        pos_msg.symbol,
                                        pos_msg.account
                                    );
                                    continue;
                                };
                                let report = parse_position_msg(pos_msg, &instrument);
                                NautilusWsMessage::PositionStatusReport(report)
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Wallet { data, .. } => {
                            if let Some(wallet_msg) = data.into_iter().next() {
                                let account_state = parse_wallet_msg(wallet_msg, ts_init);
                                NautilusWsMessage::AccountState(account_state)
                            } else {
                                continue;
                            }
                        }
                        BitmexTableMessage::Margin { .. } => {
                            // Skip margin messages - BitMEX uses account-level cross-margin
                            // which doesn't map well to Nautilus's per-instrument margin model
                            continue;
                        }
                        BitmexTableMessage::Instrument { action, data } => {
                            let ts_init = clock.get_time_ns();

                            match action {
                                BitmexAction::Partial | BitmexAction::Insert => {
                                    let mut instruments = Vec::with_capacity(data.len());
                                    let mut temp_cache = AHashMap::new();

                                    let data_for_prices = data.clone();

                                    for msg in data {
                                        match msg.try_into() {
                                            Ok(http_inst) => {
                                                match crate::http::parse::parse_instrument_any(
                                                    &http_inst, ts_init,
                                                ) {
                                                    Some(instrument_any) => {
                                                        let symbol =
                                                            instrument_any.symbol().inner();
                                                        temp_cache
                                                            .insert(symbol, instrument_any.clone());
                                                        instruments.push(instrument_any);
                                                    }
                                                    None => {
                                                        log::warn!(
                                                            "Failed to parse instrument from WebSocket"
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                log::debug!(
                                                    "Skipping instrument (missing required fields): {e}"
                                                );
                                            }
                                        }
                                    }

                                    // Update instruments_cache with new instruments
                                    for (symbol, instrument) in temp_cache.iter() {
                                        self.instruments_cache.insert(*symbol, instrument.clone());
                                    }

                                    if !instruments.is_empty()
                                        && let Err(e) = self
                                            .out_tx
                                            .send(NautilusWsMessage::Instruments(instruments))
                                    {
                                        tracing::error!("Error sending instruments: {e}");
                                    }

                                    let mut data_msgs = Vec::with_capacity(data_for_prices.len());

                                    for msg in data_for_prices {
                                        let parsed =
                                            parse_instrument_msg(msg, &temp_cache, ts_init);
                                        data_msgs.extend(parsed);
                                    }

                                    if data_msgs.is_empty() {
                                        continue;
                                    }
                                    NautilusWsMessage::Data(data_msgs)
                                }
                                BitmexAction::Update => {
                                    let mut data_msgs = Vec::with_capacity(data.len());

                                    for msg in data {
                                        let parsed = parse_instrument_msg(
                                            msg,
                                            &self.instruments_cache,
                                            ts_init,
                                        );
                                        data_msgs.extend(parsed);
                                    }

                                    if data_msgs.is_empty() {
                                        continue;
                                    }
                                    NautilusWsMessage::Data(data_msgs)
                                }
                                BitmexAction::Delete => {
                                    log::info!(
                                        "Received instrument delete action for {} instrument(s)",
                                        data.len()
                                    );
                                    continue;
                                }
                            }
                        }
                        BitmexTableMessage::Funding { data, .. } => {
                            let ts_init = clock.get_time_ns();
                            let mut funding_updates = Vec::with_capacity(data.len());

                            for msg in data {
                                if let Some(parsed) = parse_funding_msg(msg, ts_init) {
                                    funding_updates.push(parsed);
                                }
                            }

                            if !funding_updates.is_empty() {
                                NautilusWsMessage::FundingRateUpdates(funding_updates)
                            } else {
                                continue;
                            }
                        }
                        _ => {
                            // Other message types not yet implemented
                            tracing::warn!("Unhandled table message type: {table_msg:?}");
                            continue;
                        }
                    });
                }
                BitmexWsMessage::Welcome { .. } | BitmexWsMessage::Error { .. } => continue,
            }
                }

                // Handle shutdown - either channel closed or stream ended
                else => {
                    tracing::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn handle_subscription_message(
        &self,
        success: bool,
        subscribe: Option<&String>,
        request: Option<&BitmexHttpRequest>,
        error: Option<&str>,
    ) {
        if let Some(req) = request {
            if req
                .op
                .eq_ignore_ascii_case(BitmexWsAuthAction::AuthKeyExpires.as_ref())
            {
                if success {
                    tracing::info!("Authenticated BitMEX WebSocket session");
                    self.auth_tracker.succeed();
                } else {
                    let reason = error.unwrap_or("Authentication rejected").to_string();
                    tracing::error!(error = %reason, "Authentication failed");
                    self.auth_tracker.fail(reason);
                }
                return;
            }

            if req
                .op
                .eq_ignore_ascii_case(BitmexWsOperation::Subscribe.as_ref())
            {
                self.handle_subscription_ack(success, request, subscribe, error);
                return;
            }

            if req
                .op
                .eq_ignore_ascii_case(BitmexWsOperation::Unsubscribe.as_ref())
            {
                self.handle_unsubscribe_ack(success, request, subscribe, error);
                return;
            }
        }

        if subscribe.is_some() {
            self.handle_subscription_ack(success, request, subscribe, error);
            return;
        }

        if let Some(error) = error {
            tracing::warn!(
                success = success,
                error = error,
                "Unhandled subscription control message"
            );
        }
    }

    fn handle_subscription_ack(
        &self,
        success: bool,
        request: Option<&BitmexHttpRequest>,
        subscribe: Option<&String>,
        error: Option<&str>,
    ) {
        let topics = Self::topics_from_request(request, subscribe);

        if topics.is_empty() {
            tracing::debug!("Subscription acknowledgement without topics");
            return;
        }

        for topic in topics {
            if success {
                self.subscriptions.confirm_subscribe(topic);
                tracing::debug!(topic = topic, "Subscription confirmed");
            } else {
                self.subscriptions.mark_failure(topic);
                let reason = error.unwrap_or("Subscription rejected");
                tracing::error!(topic = topic, error = reason, "Subscription failed");
            }
        }
    }

    fn handle_unsubscribe_ack(
        &self,
        success: bool,
        request: Option<&BitmexHttpRequest>,
        subscribe: Option<&String>,
        error: Option<&str>,
    ) {
        let topics = Self::topics_from_request(request, subscribe);

        if topics.is_empty() {
            tracing::debug!("Unsubscription acknowledgement without topics");
            return;
        }

        for topic in topics {
            if success {
                tracing::debug!(topic = topic, "Unsubscription confirmed");
                self.subscriptions.confirm_unsubscribe(topic);
            } else {
                let reason = error.unwrap_or("Unsubscription rejected");
                tracing::error!(
                    topic = topic,
                    error = reason,
                    "Unsubscription failed - restoring subscription"
                );
                // Venue rejected unsubscribe, so we're still subscribed. Restore state:
                self.subscriptions.confirm_unsubscribe(topic); // Clear pending_unsubscribe
                self.subscriptions.mark_subscribe(topic); // Mark as subscribing
                self.subscriptions.confirm_subscribe(topic); // Confirm subscription
            }
        }
    }

    fn topics_from_request<'a>(
        request: Option<&'a BitmexHttpRequest>,
        fallback: Option<&'a String>,
    ) -> Vec<&'a str> {
        if let Some(req) = request
            && !req.args.is_empty()
        {
            return req.args.iter().filter_map(|arg| arg.as_str()).collect();
        }

        fallback.into_iter().map(|topic| topic.as_str()).collect()
    }
}

fn is_terminal_order_status(status: OrderStatus) -> bool {
    matches!(
        status,
        OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Rejected | OrderStatus::Filled,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_heartbeat_message_detection() {
        assert!(RawFeedHandler::is_heartbeat_message("{\"op\":\"ping\"}"));
        assert!(RawFeedHandler::is_heartbeat_message("{\"op\":\"pong\"}"));
        assert!(!RawFeedHandler::is_heartbeat_message(
            "{\"op\":\"subscribe\",\"args\":[\"trade:XBTUSD\"]}"
        ));
    }
}
