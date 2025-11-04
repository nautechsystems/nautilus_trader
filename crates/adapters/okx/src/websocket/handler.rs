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

//! WebSocket message handler for OKX.

use std::{
    collections::VecDeque,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::runtime::get_runtime;
use nautilus_core::{UUID4, nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{OrderStatus, OrderType, TimeInForce},
    events::{AccountState, OrderCancelRejected, OrderModifyRejected, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::OrderStatusReport,
    types::{Money, Quantity},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{AuthTracker, SubscriptionState, TEXT_PING, TEXT_PONG, WebSocketClient},
};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    client::{PendingOrderParams, is_post_only_rejection},
    enums::{OKXSubscriptionEvent, OKXWsChannel, OKXWsOperation},
    messages::{
        ExecutionReport, NautilusWsMessage, OKXAlgoOrderMsg, OKXOrderMsg, OKXWebSocketArg,
        OKXWebSocketError, OKXWsMessage,
    },
    parse::{parse_algo_order_msg, parse_book_msg_vec, parse_order_msg, parse_ws_message_data},
    subscription::topic_from_websocket_arg,
};
use crate::{
    common::{
        consts::{OKX_POST_ONLY_CANCEL_REASON, OKX_POST_ONLY_CANCEL_SOURCE},
        enums::{OKXOrderStatus, OKXOrderType, OKXSide, OKXTargetCurrency, OKXTradeMode},
        parse::{
            parse_account_state, parse_client_order_id, parse_millisecond_timestamp, parse_price,
            parse_quantity,
        },
    },
    http::models::OKXAccount,
};

/// Data cached for pending place requests to correlate with responses.
type PlaceRequestData = (
    PendingOrderParams,
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
);

/// Data cached for pending cancel requests to correlate with responses.
type CancelRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

/// Data cached for pending amend requests to correlate with responses.
type AmendRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

/// Data for pending mass cancel requests.
type MassCancelRequestData = InstrumentId;

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

pub(super) struct RawFeedHandler {
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

    /// Gets the next message from the WebSocket stream.
    pub(super) async fn next(&mut self) -> Option<OKXWsMessage> {
        loop {
            tokio::select! {
                msg = self.raw_rx.recv() => match msg {
                    Some(msg) => match msg {
                        Message::Text(text) => {
                            // Handle ping/pong messages
                            if text == TEXT_PONG {
                                tracing::trace!("Received pong from OKX");
                                continue;
                            }
                            if text == TEXT_PING {
                                tracing::trace!("Received ping from OKX (text)");
                                return Some(OKXWsMessage::Ping);
                            }

                            // Check for reconnection signal
                            if text == RECONNECTED {
                                tracing::debug!("Received WebSocket reconnection signal");
                                return Some(OKXWsMessage::Reconnected);
                            }
                            tracing::trace!("Received WebSocket message: {text}");

                            match serde_json::from_str(&text) {
                                Ok(ws_event) => match &ws_event {
                                    OKXWsMessage::Error { code, msg } => {
                                        tracing::error!("WebSocket error: {code} - {msg}");
                                        return Some(ws_event);
                                    }
                                    OKXWsMessage::Login {
                                        event,
                                        code,
                                        msg,
                                        conn_id,
                                    } => {
                                        if code == "0" {
                                            tracing::info!(
                                                "Successfully authenticated with OKX WebSocket, conn_id={conn_id}"
                                            );
                                        } else {
                                            tracing::error!(
                                                "Authentication failed: {event} {code} - {msg}"
                                            );
                                        }
                                        return Some(ws_event);
                                    }
                                    OKXWsMessage::Subscription {
                                        event,
                                        arg,
                                        conn_id, .. } => {
                                        let channel_str = serde_json::to_string(&arg.channel)
                                            .expect("Invalid OKX websocket channel")
                                            .trim_matches('"')
                                            .to_string();
                                        tracing::debug!(
                                            "{event}d: channel={channel_str}, conn_id={conn_id}"
                                        );
                                        return Some(ws_event);
                                    }
                                    OKXWsMessage::ChannelConnCount {
                                        event: _,
                                        channel,
                                        conn_count,
                                        conn_id,
                                    } => {
                                        let channel_str = serde_json::to_string(&channel)
                                            .expect("Invalid OKX websocket channel")
                                            .trim_matches('"')
                                            .to_string();
                                        tracing::debug!(
                                            "Channel connection status: channel={channel_str}, connections={conn_count}, conn_id={conn_id}",
                                        );
                                        continue;
                                    }
                                    OKXWsMessage::Ping => {
                                        tracing::trace!("Ignoring ping event parsed from text payload");
                                        continue;
                                    }
                                    OKXWsMessage::Data { .. } => return Some(ws_event),
                                    OKXWsMessage::BookData { .. } => return Some(ws_event),
                                    OKXWsMessage::OrderResponse {
                                        id,
                                        op,
                                        code,
                                        msg: _,
                                        data,
                                    } => {
                                        if code == "0" {
                                            tracing::debug!(
                                                "Order operation successful: id={:?}, op={op}, code={code}",
                                                id
                                            );

                                            // Extract success message
                                            if let Some(order_data) = data.first() {
                                                let success_msg = order_data
                                                    .get("sMsg")
                                                    .and_then(|s| s.as_str())
                                                    .unwrap_or("Order operation successful");
                                                tracing::debug!("Order success details: {success_msg}");
                                            }
                                        }
                                        return Some(ws_event);
                                    }
                                    OKXWsMessage::Reconnected => {
                                        // This shouldn't happen as we handle RECONNECTED string directly
                                        tracing::warn!("Unexpected Reconnected event from deserialization");
                                        continue;
                                    }
                                },
                                Err(e) => {
                                    tracing::error!("Failed to parse message: {e}: {text}");
                                    return None;
                                }
                            }
                        }
                        Message::Ping(payload) => {
                            tracing::trace!("Received ping frame from OKX ({} bytes)", payload.len());
                            continue;
                        }
                        Message::Pong(payload) => {
                            tracing::trace!("Received pong frame from OKX ({} bytes)", payload.len());
                            continue;
                        }
                        Message::Binary(msg) => {
                            tracing::debug!("Raw binary: {msg:?}");
                        }
                        Message::Close(_) => {
                            tracing::debug!("Received close message");
                            return None;
                        }
                        msg => {
                            tracing::warn!("Unexpected message: {msg}");
                        }
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
}

pub(super) struct FeedHandler {
    account_id: AccountId,
    inner: Arc<tokio::sync::RwLock<Option<WebSocketClient>>>,
    handler: RawFeedHandler,
    #[allow(dead_code)]
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    pub out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
    pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
    pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
    pending_mass_cancel_requests: Arc<DashMap<String, MassCancelRequestData>>,
    active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
    client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
    emitted_order_accepted: Arc<DashMap<VenueOrderId, ()>>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    last_account_state: Option<AccountState>,
    fee_cache: AHashMap<Ustr, Money>,           // Key is order ID
    filled_qty_cache: AHashMap<Ustr, Quantity>, // Key is order ID
    funding_rate_cache: AHashMap<Ustr, (Ustr, u64)>, // Cache (funding_rate, funding_time) by inst_id
    auth_tracker: AuthTracker,
    pending_messages: VecDeque<NautilusWsMessage>,
    subscriptions_state: SubscriptionState,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: AccountId,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        reader: UnboundedReceiver<Message>,
        signal: Arc<AtomicBool>,
        inner: Arc<tokio::sync::RwLock<Option<WebSocketClient>>>,
        pending_place_requests: Arc<DashMap<String, PlaceRequestData>>,
        pending_cancel_requests: Arc<DashMap<String, CancelRequestData>>,
        pending_amend_requests: Arc<DashMap<String, AmendRequestData>>,
        pending_mass_cancel_requests: Arc<DashMap<String, MassCancelRequestData>>,
        active_client_orders: Arc<DashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>>,
        client_id_aliases: Arc<DashMap<ClientOrderId, ClientOrderId>>,
        emitted_order_accepted: Arc<DashMap<VenueOrderId, ()>>,
        auth_tracker: AuthTracker,
        subscriptions_state: SubscriptionState,
    ) -> Self {
        Self {
            account_id,
            inner,
            handler: RawFeedHandler::new(reader, signal),
            cmd_rx,
            out_tx,
            pending_place_requests,
            pending_cancel_requests,
            pending_amend_requests,
            pending_mass_cancel_requests,
            active_client_orders,
            client_id_aliases,
            emitted_order_accepted,
            instruments_cache: AHashMap::new(),
            last_account_state: None,
            fee_cache: AHashMap::new(),
            filled_qty_cache: AHashMap::new(),
            funding_rate_cache: AHashMap::new(),
            auth_tracker,
            pending_messages: VecDeque::new(),
            subscriptions_state,
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.handler
            .signal
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

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

                Some(event) = self.handler.next() => {
                    let ts_init = clock.get_time_ns();

            match event {
                OKXWsMessage::Ping => {
                    self.schedule_text_pong();
                    continue;
                }
                OKXWsMessage::Login {
                    code, msg, conn_id, ..
                } => {
                    if code == "0" {
                        self.auth_tracker.succeed();
                        continue;
                    }

                    tracing::error!("Authentication failed: {msg}");
                    self.auth_tracker.fail(msg.clone());

                    let error = OKXWebSocketError {
                        code,
                        message: msg,
                        conn_id: Some(conn_id),
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    self.pending_messages
                        .push_back(NautilusWsMessage::Error(error));
                    continue;
                }
                OKXWsMessage::BookData { arg, action, data } => {
                    let Some(inst_id) = arg.inst_id else {
                        tracing::error!("Instrument ID missing for book data event");
                        continue;
                    };

                    let Some(inst) = self.instruments_cache.get(&inst_id) else {
                        continue;
                    };

                    let instrument_id = inst.id();
                    let price_precision = inst.price_precision();
                    let size_precision = inst.size_precision();

                    match parse_book_msg_vec(
                        data,
                        &instrument_id,
                        price_precision,
                        size_precision,
                        action,
                        ts_init,
                    ) {
                        Ok(payloads) => return Some(NautilusWsMessage::Data(payloads)),
                        Err(e) => {
                            tracing::error!("Failed to parse book message: {e}");
                            continue;
                        }
                    }
                }
                OKXWsMessage::OrderResponse {
                    id,
                    op,
                    code,
                    msg,
                    data,
                } => {
                    if code == "0" {
                        tracing::debug!(
                            "Order operation successful: id={id:?} op={op} code={code}"
                        );

                        if op == OKXWsOperation::MassCancel
                            && let Some(request_id) = &id
                            && let Some((_, instrument_id)) =
                                self.pending_mass_cancel_requests.remove(request_id)
                        {
                            tracing::info!(
                                "Mass cancel operation successful for instrument: {}",
                                instrument_id
                            );
                        } else if op == OKXWsOperation::Order
                            && let Some(request_id) = &id
                            && let Some((
                                _,
                                (params, client_order_id, _trader_id, _strategy_id, instrument_id),
                            )) = self.pending_place_requests.remove(request_id)
                        {
                            let (venue_order_id, ts_accepted) = if let Some(first) = data.first() {
                                let ord_id = first
                                    .get("ordId")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(VenueOrderId::new);

                                let ts = first
                                    .get("ts")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .map_or_else(
                                        || clock.get_time_ns(),
                                        |ms| UnixNanos::from(ms * 1_000_000),
                                    );

                                (ord_id, ts)
                            } else {
                                (None, clock.get_time_ns())
                            };

                            if let Some(instrument) = self
                                .instruments_cache
                                .get(&instrument_id.symbol.inner())
                            {
                                match params {
                                    PendingOrderParams::Regular(order_params) => {
                                        let is_explicit_quote_sized = order_params
                                            .tgt_ccy
                                            .is_some_and(|tgt| tgt == OKXTargetCurrency::QuoteCcy);

                                        // SPOT market BUY in cash mode with no tgt_ccy defaults to quote-sizing
                                        let is_implicit_quote_sized =
                                            order_params.tgt_ccy.is_none()
                                                && order_params.side == OKXSide::Buy
                                                && matches!(
                                                    order_params.ord_type,
                                                    OKXOrderType::Market
                                                )
                                                && order_params.td_mode == OKXTradeMode::Cash
                                                && instrument.instrument_class().as_ref() == "SPOT";

                                        if is_explicit_quote_sized || is_implicit_quote_sized {
                                            // For quote-sized orders, sz is in quote currency (USDT),
                                            // not base currency (ETH). We can't accurately parse the
                                            // base quantity without the fill price, so we skip the
                                            // synthetic OrderAccepted and rely on the orders channel
                                            tracing::info!(
                                                "Skipping synthetic OrderAccepted for {} quote-sized order: client_order_id={client_order_id}, venue_order_id={:?}",
                                                if is_explicit_quote_sized {
                                                    "explicit"
                                                } else {
                                                    "implicit"
                                                },
                                                venue_order_id
                                            );
                                            continue;
                                        }

                                        let order_side = order_params.side.into();
                                        let order_type = order_params.ord_type.into();
                                        let time_in_force = match order_params.ord_type {
                                            OKXOrderType::Fok => TimeInForce::Fok,
                                            OKXOrderType::Ioc | OKXOrderType::OptimalLimitIoc => {
                                                TimeInForce::Ioc
                                            }
                                            _ => TimeInForce::Gtc,
                                        };

                                        let size_precision = instrument.size_precision();
                                        let quantity = match parse_quantity(
                                            &order_params.sz,
                                            size_precision,
                                        ) {
                                            Ok(q) => q,
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to parse quantity for accepted order: {e}"
                                                );
                                                continue;
                                            }
                                        };

                                        let filled_qty = Quantity::zero(size_precision);

                                        let mut report = OrderStatusReport::new(
                                            self.account_id,
                                            instrument_id,
                                            Some(client_order_id),
                                            venue_order_id
                                                .unwrap_or_else(|| VenueOrderId::new("PENDING")),
                                            order_side,
                                            order_type,
                                            time_in_force,
                                            OrderStatus::Accepted,
                                            quantity,
                                            filled_qty,
                                            ts_accepted,
                                            ts_accepted, // ts_last same as ts_accepted for new orders
                                            ts_init,
                                            None, // Generate UUID4 automatically
                                        );

                                        if let Some(px) = &order_params.px
                                            && !px.is_empty()
                                            && let Ok(price) =
                                                parse_price(px, instrument.price_precision())
                                        {
                                            report = report.with_price(price);
                                        }

                                        if let Some(true) = order_params.reduce_only {
                                            report = report.with_reduce_only(true);
                                        }

                                        if order_type == OrderType::Limit
                                            && order_params.ord_type == OKXOrderType::PostOnly
                                        {
                                            report = report.with_post_only(true);
                                        }

                                        if let Some(ref v_order_id) = venue_order_id {
                                            self.emitted_order_accepted.insert(*v_order_id, ());
                                        }

                                        tracing::debug!(
                                            "Order accepted: client_order_id={client_order_id}, venue_order_id={:?}",
                                            venue_order_id
                                        );

                                        return Some(NautilusWsMessage::ExecutionReports(vec![
                                            ExecutionReport::Order(report),
                                        ]));
                                    }
                                    PendingOrderParams::Algo(_) => {
                                        tracing::info!(
                                            "Algo order placement confirmed: client_order_id={client_order_id}, venue_order_id={:?}",
                                            venue_order_id
                                        );
                                    }
                                }
                            } else {
                                tracing::error!(
                                    "Instrument not found for accepted order: {instrument_id}"
                                );
                            }
                        }

                        if let Some(first) = data.first()
                            && let Some(success_msg) =
                                first.get("sMsg").and_then(|value| value.as_str())
                        {
                            tracing::debug!("Order details: {success_msg}");
                        }

                        continue;
                    }

                    let error_msg = data
                        .first()
                        .and_then(|d| d.get("sMsg"))
                        .and_then(|s| s.as_str())
                        .unwrap_or(&msg)
                        .to_string();

                    if let Some(first) = data.first() {
                        tracing::debug!(
                            "Error data fields: {}",
                            serde_json::to_string_pretty(first)
                                .unwrap_or_else(|_| "unable to serialize".to_string())
                        );
                    }

                    tracing::warn!(
                        "Order operation failed: id={id:?} op={op} code={code} msg={error_msg}"
                    );

                    if let Some(request_id) = &id {
                        match op {
                            OKXWsOperation::Order => {
                                if let Some((
                                    _,
                                    (
                                        _params,
                                        client_order_id,
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                    ),
                                )) = self.pending_place_requests.remove(request_id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let due_post_only =
                                        is_post_only_rejection(code.as_str(), &data);
                                    let rejected = OrderRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        self.account_id,
                                        Ustr::from(error_msg.as_str()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
                                        due_post_only,
                                    );

                                    return Some(NautilusWsMessage::OrderRejected(rejected));
                                }
                            }
                            OKXWsOperation::CancelOrder => {
                                if let Some((
                                    _,
                                    (
                                        client_order_id,
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        venue_order_id,
                                    ),
                                )) = self.pending_cancel_requests.remove(request_id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderCancelRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        Ustr::from(error_msg.as_str()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
                                        venue_order_id,
                                        Some(self.account_id),
                                    );

                                    return Some(NautilusWsMessage::OrderCancelRejected(rejected));
                                }
                            }
                            OKXWsOperation::AmendOrder => {
                                if let Some((
                                    _,
                                    (
                                        client_order_id,
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        venue_order_id,
                                    ),
                                )) = self.pending_amend_requests.remove(request_id)
                                {
                                    let ts_event = clock.get_time_ns();
                                    let rejected = OrderModifyRejected::new(
                                        trader_id,
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        Ustr::from(error_msg.as_str()),
                                        UUID4::new(),
                                        ts_event,
                                        ts_init,
                                        false, // Not from reconciliation
                                        venue_order_id,
                                        Some(self.account_id),
                                    );

                                    return Some(NautilusWsMessage::OrderModifyRejected(rejected));
                                }
                            }
                            OKXWsOperation::MassCancel => {
                                if let Some((_, instrument_id)) =
                                    self.pending_mass_cancel_requests.remove(request_id)
                                {
                                    tracing::error!(
                                        "Mass cancel operation failed for {}: code={code} msg={error_msg}",
                                        instrument_id
                                    );
                                    let error = OKXWebSocketError {
                                        code,
                                        message: format!(
                                            "Mass cancel failed for {}: {}",
                                            instrument_id, error_msg
                                        ),
                                        conn_id: None,
                                        timestamp: clock.get_time_ns().as_u64(),
                                    };
                                    return Some(NautilusWsMessage::Error(error));
                                } else {
                                    tracing::error!(
                                        "Mass cancel operation failed: code={code} msg={error_msg}"
                                    );
                                }
                            }
                            _ => tracing::warn!("Unhandled operation type for rejection: {op}"),
                        }
                    }

                    let error = OKXWebSocketError {
                        code,
                        message: error_msg,
                        conn_id: None,
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                OKXWsMessage::Data { arg, data } => {
                    let OKXWebSocketArg {
                        channel, inst_id, ..
                    } = arg;

                    match channel {
                        OKXWsChannel::Account => {
                            match serde_json::from_value::<Vec<OKXAccount>>(data) {
                                Ok(accounts) => {
                                    if let Some(account) = accounts.first() {
                                        match parse_account_state(account, self.account_id, ts_init)
                                        {
                                            Ok(account_state) => {
                                                if let Some(last_account_state) =
                                                    &self.last_account_state
                                                    && account_state.has_same_balances_and_margins(
                                                        last_account_state,
                                                    )
                                                {
                                                    continue;
                                                }
                                                self.last_account_state =
                                                    Some(account_state.clone());
                                                return Some(NautilusWsMessage::AccountUpdate(
                                                    account_state,
                                                ));
                                            }
                                            Err(e) => tracing::error!(
                                                "Failed to parse account state: {e}"
                                            ),
                                        }
                                    }
                                }
                                Err(e) => tracing::error!("Failed to parse account data: {e}"),
                            }
                            continue;
                        }
                        OKXWsChannel::Orders => {
                            let orders: Vec<OKXOrderMsg> = match serde_json::from_value(data) {
                                Ok(orders) => orders,
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to deserialize orders channel payload: {e}"
                                    );
                                    continue;
                                }
                            };

                            tracing::debug!(
                                "Received {} order message(s) from orders channel",
                                orders.len()
                            );

                            let mut exec_reports: Vec<ExecutionReport> =
                                Vec::with_capacity(orders.len());

                            for msg in orders {
                                tracing::debug!(
                                    "Processing order message: inst_id={}, cl_ord_id={}, state={:?}, exec_type={:?}",
                                    msg.inst_id,
                                    msg.cl_ord_id,
                                    msg.state,
                                    msg.exec_type
                                );

                                if self.try_handle_post_only_auto_cancel(
                                    &msg,
                                    ts_init,
                                    &mut exec_reports,
                                ) {
                                    continue;
                                }

                                let raw_child = parse_client_order_id(&msg.cl_ord_id);
                                let parent_from_msg = msg
                                    .algo_cl_ord_id
                                    .as_ref()
                                    .filter(|value| !value.is_empty())
                                    .map(ClientOrderId::new);
                                let effective_client_id = self
                                    .register_client_order_aliases(&raw_child, &parent_from_msg);

                                match parse_order_msg(
                                    &msg,
                                    self.account_id,
                                    &self.instruments_cache,
                                    &self.fee_cache,
                                    &self.filled_qty_cache,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        tracing::debug!(
                                            "Successfully parsed execution report: {:?}",
                                            report
                                        );

                                        let is_duplicate_accepted =
                                            if let ExecutionReport::Order(ref status_report) =
                                                report
                                            {
                                                if status_report.order_status
                                                    == OrderStatus::Accepted
                                                {
                                                    self.emitted_order_accepted
                                                        .contains_key(&status_report.venue_order_id)
                                                } else {
                                                    false
                                                }
                                            } else {
                                                false
                                            };

                                        if is_duplicate_accepted {
                                            tracing::debug!(
                                                "Skipping duplicate OrderAccepted for venue_order_id={}",
                                                if let ExecutionReport::Order(ref r) = report {
                                                    r.venue_order_id.to_string()
                                                } else {
                                                    "unknown".to_string()
                                                }
                                            );
                                            continue;
                                        }

                                        if let ExecutionReport::Order(ref status_report) = report
                                            && status_report.order_status == OrderStatus::Accepted
                                        {
                                            self.emitted_order_accepted
                                                .insert(status_report.venue_order_id, ());
                                        }

                                        let adjusted = self.adjust_execution_report(
                                            report,
                                            &effective_client_id,
                                            &raw_child,
                                        );

                                        // Clean up tracking for terminal states
                                        if let ExecutionReport::Order(ref status_report) = adjusted
                                            && matches!(
                                                status_report.order_status,
                                                OrderStatus::Filled
                                                    | OrderStatus::Canceled
                                                    | OrderStatus::Expired
                                                    | OrderStatus::Rejected
                                            )
                                        {
                                            self.emitted_order_accepted
                                                .remove(&status_report.venue_order_id);
                                        }

                                        self.update_caches_with_report(&adjusted);
                                        exec_reports.push(adjusted);
                                    }
                                    Err(e) => tracing::error!("Failed to parse order message: {e}"),
                                }
                            }

                            if !exec_reports.is_empty() {
                                tracing::debug!(
                                    "Pushing {} execution report(s) to message queue",
                                    exec_reports.len()
                                );
                                self.pending_messages
                                    .push_back(NautilusWsMessage::ExecutionReports(exec_reports));
                            } else {
                                tracing::debug!(
                                    "No execution reports generated from order messages"
                                );
                            }

                            if let Some(message) = self.pending_messages.pop_front() {
                                return Some(message);
                            }

                            continue;
                        }
                        OKXWsChannel::OrdersAlgo => {
                            let orders: Vec<OKXAlgoOrderMsg> = match serde_json::from_value(data) {
                                Ok(orders) => orders,
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to deserialize algo orders payload: {e}"
                                    );
                                    continue;
                                }
                            };

                            let mut exec_reports: Vec<ExecutionReport> =
                                Vec::with_capacity(orders.len());

                            for msg in orders {
                                let raw_child = parse_client_order_id(&msg.cl_ord_id);
                                let parent_from_msg = parse_client_order_id(&msg.algo_cl_ord_id);
                                let effective_client_id = self
                                    .register_client_order_aliases(&raw_child, &parent_from_msg);

                                match parse_algo_order_msg(
                                    msg,
                                    self.account_id,
                                    &self.instruments_cache,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        let adjusted = self.adjust_execution_report(
                                            report,
                                            &effective_client_id,
                                            &raw_child,
                                        );
                                        self.update_caches_with_report(&adjusted);
                                        exec_reports.push(adjusted);
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to parse algo order message: {e}");
                                    }
                                }
                            }

                            if !exec_reports.is_empty() {
                                return Some(NautilusWsMessage::ExecutionReports(exec_reports));
                            }

                            continue;
                        }
                        _ => {
                            let Some(inst_id) = inst_id else {
                                tracing::error!("No instrument for channel {:?}", channel);
                                continue;
                            };

                            let Some(instrument) = self.instruments_cache.get(&inst_id) else {
                                tracing::error!(
                                    "No instrument for channel {:?}, inst_id {:?}",
                                    channel,
                                    inst_id
                                );
                                continue;
                            };

                            let instrument_id = instrument.id();
                            let price_precision = instrument.price_precision();
                            let size_precision = instrument.size_precision();

                            match parse_ws_message_data(
                                &channel,
                                data,
                                &instrument_id,
                                price_precision,
                                size_precision,
                                ts_init,
                                &mut self.funding_rate_cache,
                                &self.instruments_cache,
                            ) {
                                Ok(Some(msg)) => {
                                    if let NautilusWsMessage::Instrument(ref inst) = msg {
                                        self.instruments_cache.insert(inst.symbol().inner(), inst.as_ref().clone());
                                    }
                                    return Some(msg);
                                }
                                Ok(None) => continue,
                                Err(e) => {
                                    tracing::error!(
                                        "Error parsing message for channel {:?}: {e}",
                                        channel
                                    );
                                    continue;
                                }
                            }
                        }
                    }
                }
                OKXWsMessage::Error { code, msg } => {
                    let error = OKXWebSocketError {
                        code,
                        message: msg,
                        conn_id: None,
                        timestamp: clock.get_time_ns().as_u64(),
                    };
                    return Some(NautilusWsMessage::Error(error));
                }
                OKXWsMessage::Reconnected => {
                    return Some(NautilusWsMessage::Reconnected);
                }
                OKXWsMessage::Subscription {
                    event,
                    arg,
                    code,
                    msg,
                    ..
                } => {
                    let topic = topic_from_websocket_arg(&arg);
                    let success = code.as_deref().is_none_or(|c| c == "0");

                    match event {
                        OKXSubscriptionEvent::Subscribe => {
                            if success {
                                self.subscriptions_state.confirm_subscribe(&topic);
                            } else {
                                tracing::warn!(?topic, error = ?msg, code = ?code, "Subscription failed");
                                self.subscriptions_state.mark_failure(&topic);
                            }
                        }
                        OKXSubscriptionEvent::Unsubscribe => {
                            if success {
                                self.subscriptions_state.confirm_unsubscribe(&topic);
                            } else {
                                tracing::warn!(?topic, error = ?msg, code = ?code, "Unsubscription failed - restoring subscription");
                                // Venue rejected unsubscribe, so we're still subscribed. Restore state:
                                self.subscriptions_state.confirm_unsubscribe(&topic); // Clear pending_unsubscribe
                                self.subscriptions_state.mark_subscribe(&topic);       // Mark as subscribing
                                self.subscriptions_state.confirm_subscribe(&topic);    // Confirm subscription
                            }
                        }
                    }

                    continue;
                }
                OKXWsMessage::ChannelConnCount { .. } => continue,
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

    pub(super) fn is_post_only_auto_cancel(msg: &OKXOrderMsg) -> bool {
        if msg.state != OKXOrderStatus::Canceled {
            return false;
        }

        let cancel_source_matches = matches!(
            msg.cancel_source.as_deref(),
            Some(source) if source == OKX_POST_ONLY_CANCEL_SOURCE
        );

        let reason_matches = matches!(
            msg.cancel_source_reason.as_deref(),
            Some(reason) if reason.contains("POST_ONLY")
        );

        if !(cancel_source_matches || reason_matches) {
            return false;
        }

        msg.acc_fill_sz
            .as_ref()
            .is_none_or(|filled| filled == "0" || filled.is_empty())
    }

    fn schedule_text_pong(&self) {
        let inner = self.inner.clone();
        get_runtime().spawn(async move {
            let guard = inner.read().await;

            if let Some(client) = guard.as_ref() {
                if let Err(e) = client.send_text(TEXT_PONG.to_string(), None).await {
                    tracing::warn!(error = %e, "Failed to send pong response to OKX text ping");
                } else {
                    tracing::trace!("Sent pong response to OKX text ping");
                }
            } else {
                tracing::debug!("Received text ping with no active websocket client");
            }
        });
    }

    fn try_handle_post_only_auto_cancel(
        &mut self,
        msg: &OKXOrderMsg,
        ts_init: UnixNanos,
        exec_reports: &mut Vec<ExecutionReport>,
    ) -> bool {
        if !Self::is_post_only_auto_cancel(msg) {
            return false;
        }

        let Some(client_order_id) = parse_client_order_id(&msg.cl_ord_id) else {
            return false;
        };

        let Some((_, (trader_id, strategy_id, instrument_id))) =
            self.active_client_orders.remove(&client_order_id)
        else {
            return false;
        };

        self.client_id_aliases.remove(&client_order_id);

        if !exec_reports.is_empty() {
            let reports = std::mem::take(exec_reports);
            self.pending_messages
                .push_back(NautilusWsMessage::ExecutionReports(reports));
        }

        let reason = msg
            .cancel_source_reason
            .as_ref()
            .filter(|reason| !reason.is_empty())
            .map_or_else(
                || Ustr::from(OKX_POST_ONLY_CANCEL_REASON),
                |reason| Ustr::from(reason.as_str()),
            );

        let ts_event = parse_millisecond_timestamp(msg.u_time);
        let rejected = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            reason,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            true,
        );

        self.pending_messages
            .push_back(NautilusWsMessage::OrderRejected(rejected));

        true
    }

    fn register_client_order_aliases(
        &self,
        raw_child: &Option<ClientOrderId>,
        parent_from_msg: &Option<ClientOrderId>,
    ) -> Option<ClientOrderId> {
        if let Some(parent) = parent_from_msg {
            self.client_id_aliases.insert(*parent, *parent);
            if let Some(child) = raw_child.as_ref().filter(|child| **child != *parent) {
                self.client_id_aliases.insert(*child, *parent);
            }
            Some(*parent)
        } else if let Some(child) = raw_child.as_ref() {
            if let Some(mapped) = self.client_id_aliases.get(child) {
                Some(*mapped.value())
            } else {
                self.client_id_aliases.insert(*child, *child);
                Some(*child)
            }
        } else {
            None
        }
    }

    fn adjust_execution_report(
        &self,
        report: ExecutionReport,
        effective_client_id: &Option<ClientOrderId>,
        raw_child: &Option<ClientOrderId>,
    ) -> ExecutionReport {
        match report {
            ExecutionReport::Order(status_report) => {
                let mut adjusted = status_report;
                let mut final_id = *effective_client_id;

                if final_id.is_none() {
                    final_id = adjusted.client_order_id;
                }

                if final_id.is_none()
                    && let Some(child) = raw_child.as_ref()
                    && let Some(mapped) = self.client_id_aliases.get(child)
                {
                    final_id = Some(*mapped.value());
                }

                if let Some(final_id_value) = final_id {
                    if adjusted.client_order_id != Some(final_id_value) {
                        adjusted = adjusted.with_client_order_id(final_id_value);
                    }
                    self.client_id_aliases
                        .insert(final_id_value, final_id_value);

                    if let Some(child) =
                        raw_child.as_ref().filter(|child| **child != final_id_value)
                    {
                        adjusted = adjusted.with_linked_order_ids(vec![*child]);
                    }
                }

                ExecutionReport::Order(adjusted)
            }
            ExecutionReport::Fill(mut fill_report) => {
                let mut final_id = *effective_client_id;
                if final_id.is_none() {
                    final_id = fill_report.client_order_id;
                }
                if final_id.is_none()
                    && let Some(child) = raw_child.as_ref()
                    && let Some(mapped) = self.client_id_aliases.get(child)
                {
                    final_id = Some(*mapped.value());
                }

                if let Some(final_id_value) = final_id {
                    fill_report.client_order_id = Some(final_id_value);
                    self.client_id_aliases
                        .insert(final_id_value, final_id_value);
                }

                ExecutionReport::Fill(fill_report)
            }
        }
    }

    fn update_caches_with_report(&mut self, report: &ExecutionReport) {
        match report {
            ExecutionReport::Fill(fill_report) => {
                let order_id = fill_report.venue_order_id.inner();
                let current_fee = self
                    .fee_cache
                    .get(&order_id)
                    .copied()
                    .unwrap_or_else(|| Money::new(0.0, fill_report.commission.currency));
                let total_fee = current_fee + fill_report.commission;
                self.fee_cache.insert(order_id, total_fee);

                let current_filled_qty = self
                    .filled_qty_cache
                    .get(&order_id)
                    .copied()
                    .unwrap_or_else(|| Quantity::zero(fill_report.last_qty.precision));
                let total_filled_qty = current_filled_qty + fill_report.last_qty;
                self.filled_qty_cache.insert(order_id, total_filled_qty);
            }
            ExecutionReport::Order(status_report) => {
                if matches!(status_report.order_status, OrderStatus::Filled) {
                    self.fee_cache.remove(&status_report.venue_order_id.inner());
                    self.filled_qty_cache
                        .remove(&status_report.venue_order_id.inner());
                }

                if matches!(
                    status_report.order_status,
                    OrderStatus::Canceled
                        | OrderStatus::Expired
                        | OrderStatus::Filled
                        | OrderStatus::Rejected,
                ) {
                    if let Some(client_order_id) = status_report.client_order_id {
                        self.active_client_orders.remove(&client_order_id);
                        self.client_id_aliases.remove(&client_order_id);
                    }
                    if let Some(linked) = &status_report.linked_order_ids {
                        for child in linked {
                            self.client_id_aliases.remove(child);
                        }
                    }
                }
            }
        }
    }
}
