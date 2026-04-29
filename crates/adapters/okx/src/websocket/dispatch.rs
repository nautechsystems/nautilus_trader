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

//! WebSocket message dispatch for the OKX execution client.
//!
//! Routes incoming [`OKXWsMessage`] variants to the appropriate parsing and
//! event emission paths. Tracked orders (submitted through this client) produce
//! proper order events; untracked orders fall back to execution reports for
//! downstream reconciliation.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use dashmap::{DashMap, DashSet};
use nautilus_core::{UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType},
    events::{OrderAccepted, OrderEventAny, OrderFilled, OrderRejected},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::TRIGGERABLE_ORDER_TYPES,
    reports::FillReport,
    types::{Currency, Money, Quantity},
};
use ustr::Ustr;

use crate::{
    common::{
        consts::{OKX_FIELD_CLORDID, OKX_FIELD_SCODE, OKX_FIELD_SMSG, OKX_SUCCESS_CODE},
        enums::OKXOrderStatus,
        parse::{
            is_market_price, parse_client_order_id, parse_millisecond_timestamp, parse_price,
            parse_quantity,
        },
    },
    http::models::{OKXAccount, OKXCancelAlgoOrderResponse, OKXPosition},
    websocket::{
        client::PendingOrderInfo,
        enums::OKXWsOperation,
        handler::is_post_only_auto_cancel,
        messages::{ExecutionReport, OKXOrderMsg, OKXWsMessage},
        parse::{
            OrderStateSnapshot, ParsedOrderEvent, parse_algo_order_msg, parse_order_event,
            parse_order_msg, update_fee_fill_caches,
        },
    },
};

/// Maximum entries in the dedup sets before they are cleared.
const DEDUP_CAPACITY: usize = 10_000;

/// Order identity context stored at submission time, used by the WS dispatch
/// task to produce proper order events without Cache access.
///
/// These fields are immutable for the lifetime of an order and are used to
/// construct proper order events (OrderAccepted, OrderFilled, etc.) instead
/// of execution reports.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

/// Shared state for cross-stream event deduplication between the private
/// and business WebSocket dispatch loops.
///
/// Uses `DashMap`/`DashSet` for concurrent access from both stream tasks
/// and the main thread without mutex contention.
#[derive(Debug)]
pub struct WsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    pub emitted_accepted: DashSet<ClientOrderId>,
    pub triggered_orders: DashSet<ClientOrderId>,
    pub filled_orders: DashSet<ClientOrderId>,
    pub emitted_trades: DashSet<TradeId>,
    pub(crate) pending_orders: Arc<DashMap<String, PendingOrderInfo>>,
    pub(crate) pending_cancels: Arc<DashMap<String, PendingOrderInfo>>,
    pub(crate) pending_amends: Arc<DashMap<String, PendingOrderInfo>>,
    clearing: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            emitted_accepted: DashSet::default(),
            triggered_orders: DashSet::default(),
            filled_orders: DashSet::default(),
            emitted_trades: DashSet::default(),
            pending_orders: Arc::new(DashMap::new()),
            pending_cancels: Arc::new(DashMap::new()),
            pending_amends: Arc::new(DashMap::new()),
            clearing: AtomicBool::new(false),
        }
    }
}

impl WsDispatchState {
    // Creates a dispatch state sharing the pending operation maps
    // with the WebSocket client that populates them
    pub(crate) fn with_pending_maps(
        pending_orders: Arc<DashMap<String, PendingOrderInfo>>,
        pending_cancels: Arc<DashMap<String, PendingOrderInfo>>,
        pending_amends: Arc<DashMap<String, PendingOrderInfo>>,
    ) -> Self {
        Self {
            pending_orders,
            pending_cancels,
            pending_amends,
            ..Default::default()
        }
    }
}

impl WsDispatchState {
    fn evict_if_full(&self, set: &DashSet<ClientOrderId>) {
        if set.len() >= DEDUP_CAPACITY
            && self
                .clearing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            set.clear();
            self.clearing.store(false, Ordering::Release);
        }
    }

    pub(crate) fn insert_accepted(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.emitted_accepted);
        self.emitted_accepted.insert(cid);
    }

    pub(crate) fn insert_filled(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.filled_orders);
        self.filled_orders.insert(cid);
    }

    pub(crate) fn insert_triggered(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.triggered_orders);
        self.triggered_orders.insert(cid);
    }

    /// Returns `true` if this trade was already emitted (duplicate).
    /// Uses atomic insert to avoid TOCTOU races between concurrent streams.
    pub fn check_and_insert_trade(&self, trade_id: TradeId) -> bool {
        self.evict_if_full_trades();
        !self.emitted_trades.insert(trade_id)
    }

    fn evict_if_full_trades(&self) {
        if self.emitted_trades.len() >= DEDUP_CAPACITY
            && self
                .clearing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            self.emitted_trades.clear();
            self.clearing.store(false, Ordering::Release);
        }
    }
}

/// Dispatches a WebSocket message with cross-stream deduplication.
///
/// For orders with a tracked identity (submitted through this client), produces
/// proper order events (OrderAccepted, OrderCanceled, OrderFilled, etc.).
/// For untracked orders (external or pre-existing), falls back to execution
/// reports for downstream reconciliation.
#[expect(clippy::too_many_arguments)]
pub fn dispatch_ws_message(
    message: OKXWsMessage,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    clock: &AtomicTime,
) {
    match message {
        OKXWsMessage::Orders(order_msgs) => {
            let ts_init = clock.get_time_ns();
            dispatch_order_messages(
                &order_msgs,
                emitter,
                state,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                order_state_cache,
                ts_init,
            );
        }
        OKXWsMessage::AlgoOrders(algo_msgs) => {
            let ts_init = clock.get_time_ns();
            let mut reports = Vec::new();

            for msg in algo_msgs {
                match parse_algo_order_msg(&msg, account_id, instruments, ts_init) {
                    Ok(Some(report)) => reports.push(report),
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse algo order message: {e}"),
                }
            }
            dispatch_execution_reports(reports, emitter, state);
        }
        OKXWsMessage::Account(data) => {
            let ts_init = clock.get_time_ns();

            match serde_json::from_value::<Vec<OKXAccount>>(data) {
                Ok(accounts) => {
                    for account in &accounts {
                        match crate::common::parse::parse_account_state(
                            account, account_id, ts_init,
                        ) {
                            Ok(account_state) => emitter.send_account_state(account_state),
                            Err(e) => log::error!("Failed to parse account state: {e}"),
                        }
                    }
                }
                Err(e) => log::error!("Failed to deserialize account data: {e}"),
            }
        }
        OKXWsMessage::Positions(data) => {
            let ts_init = clock.get_time_ns();

            match serde_json::from_value::<Vec<OKXPosition>>(data) {
                Ok(positions) => {
                    for position in positions {
                        let Some(instrument) = instruments.get(&position.inst_id) else {
                            log::warn!("No cached instrument for position: {}", position.inst_id);
                            continue;
                        };
                        let instrument_id = instrument.id();
                        let size_precision = instrument.size_precision();

                        match crate::common::parse::parse_position_status_report(
                            &position,
                            account_id,
                            instrument_id,
                            size_precision,
                            ts_init,
                        ) {
                            Ok(report) => emitter.send_position_report(report),
                            Err(e) => log::error!("Failed to parse position report: {e}"),
                        }
                    }
                }
                Err(e) => log::error!("Failed to deserialize positions data: {e}"),
            }
        }
        OKXWsMessage::OrderResponse {
            id,
            op,
            code,
            msg,
            data,
        } => {
            let ts_init = clock.get_time_ns();

            for item in &data {
                let s_code = item
                    .get(OKX_FIELD_SCODE)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let s_msg = item
                    .get(OKX_FIELD_SMSG)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let cl_ord_id = item
                    .get(OKX_FIELD_CLORDID)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if s_code == OKX_SUCCESS_CODE {
                    log::debug!("Order response ok: op={op:?} cl_ord_id={cl_ord_id}");
                    match op {
                        OKXWsOperation::Order
                        | OKXWsOperation::BatchOrders
                        | OKXWsOperation::OrderAlgo => {
                            state.pending_orders.remove(cl_ord_id);
                        }
                        OKXWsOperation::CancelOrder
                        | OKXWsOperation::BatchCancelOrders
                        | OKXWsOperation::MassCancel
                        | OKXWsOperation::CancelAlgos => {
                            state.pending_cancels.remove(cl_ord_id);
                        }
                        OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders => {
                            state.pending_amends.remove(cl_ord_id);
                        }
                        _ => {}
                    }
                    continue;
                }

                let Some(client_order_id) = parse_client_order_id(cl_ord_id) else {
                    log::warn!(
                        "Order response error without client_order_id: \
                         op={op:?} s_code={s_code} s_msg={s_msg}"
                    );
                    continue;
                };

                let Some(ident) = state.order_identities.get(&client_order_id) else {
                    log::warn!(
                        "Order response error for untracked order: \
                         op={op:?} cl_ord_id={cl_ord_id} s_code={s_code} s_msg={s_msg}"
                    );
                    continue;
                };

                let venue_order_id = item
                    .get("ordId")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(VenueOrderId::new);

                match op {
                    OKXWsOperation::Order | OKXWsOperation::BatchOrders => {
                        state.order_identities.remove(&client_order_id);
                        state.pending_orders.remove(cl_ord_id);
                        emitter.emit_order_rejected_event(
                            ident.strategy_id,
                            ident.instrument_id,
                            client_order_id,
                            s_msg,
                            ts_init,
                            false,
                        );
                    }
                    OKXWsOperation::CancelOrder
                    | OKXWsOperation::BatchCancelOrders
                    | OKXWsOperation::MassCancel => {
                        state.pending_cancels.remove(cl_ord_id);
                        emitter.emit_order_cancel_rejected_event(
                            ident.strategy_id,
                            ident.instrument_id,
                            client_order_id,
                            venue_order_id,
                            s_msg,
                            ts_init,
                        );
                    }
                    OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders => {
                        state.pending_amends.remove(cl_ord_id);
                        emitter.emit_order_modify_rejected_event(
                            ident.strategy_id,
                            ident.instrument_id,
                            client_order_id,
                            venue_order_id,
                            s_msg,
                            ts_init,
                        );
                    }
                    _ => {
                        log::warn!(
                            "Order response error for unhandled op: \
                             op={op:?} cl_ord_id={cl_ord_id} s_code={s_code} s_msg={s_msg}"
                        );
                    }
                }
            }

            if code != "0" && data.is_empty() {
                log::warn!(
                    "Order response error (no data): id={id:?} op={op:?} code={code} msg={msg}"
                );
            }
        }
        OKXWsMessage::SendFailed {
            request_id,
            client_order_id,
            op,
            error,
        } => {
            log::error!("WebSocket send failed: request_id={request_id} error={error}");

            if let Some(client_order_id) = client_order_id {
                let ts_init = clock.get_time_ns();

                match op {
                    Some(
                        OKXWsOperation::Order
                        | OKXWsOperation::BatchOrders
                        | OKXWsOperation::OrderAlgo,
                    ) => {
                        let key = client_order_id.as_str();
                        state.pending_orders.remove(key);
                        if let Some((_, ident)) = state.order_identities.remove(&client_order_id) {
                            emitter.emit_order_rejected_event(
                                ident.strategy_id,
                                ident.instrument_id,
                                client_order_id,
                                &error,
                                ts_init,
                                false,
                            );
                        }
                    }
                    Some(
                        OKXWsOperation::CancelOrder
                        | OKXWsOperation::BatchCancelOrders
                        | OKXWsOperation::MassCancel
                        | OKXWsOperation::CancelAlgos,
                    ) => {
                        let key = client_order_id.as_str();
                        state.pending_cancels.remove(key);
                        if let Some(ident) = state.order_identities.get(&client_order_id) {
                            emitter.emit_order_cancel_rejected_event(
                                ident.strategy_id,
                                ident.instrument_id,
                                client_order_id,
                                None,
                                &error,
                                ts_init,
                            );
                        }
                    }
                    Some(OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders) => {
                        let key = client_order_id.as_str();
                        state.pending_amends.remove(key);
                        if let Some(ident) = state.order_identities.get(&client_order_id) {
                            emitter.emit_order_modify_rejected_event(
                                ident.strategy_id,
                                ident.instrument_id,
                                client_order_id,
                                None,
                                &error,
                                ts_init,
                            );
                        }
                    }
                    _ => {
                        log::warn!(
                            "SendFailed for {client_order_id} with unknown op, cannot emit rejection"
                        );
                    }
                }
            }
        }
        OKXWsMessage::ChannelData { channel, .. } => {
            log::debug!("Ignoring data channel message on execution client: {channel:?}");
        }
        OKXWsMessage::BookData { .. } | OKXWsMessage::Instruments(_) => {
            log::debug!("Ignoring data message on execution client");
        }
        OKXWsMessage::Error(e) => {
            log::warn!(
                "Websocket error: code={} message={} conn_id={:?}",
                e.code,
                e.message,
                e.conn_id
            );
        }
        OKXWsMessage::Reconnected => {
            log::info!("Websocket reconnected");
        }
        OKXWsMessage::Authenticated => {
            log::debug!("Websocket authenticated");
        }
    }
}

/// Dispatches order messages, producing proper order events for tracked orders
/// and falling back to execution reports for untracked/external orders.
#[expect(clippy::too_many_arguments)]
fn dispatch_order_messages(
    order_msgs: &[OKXOrderMsg],
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    ts_init: UnixNanos,
) {
    for msg in order_msgs {
        let Some(instrument) = instruments.get(&msg.inst_id) else {
            log::warn!("No instrument for {}, skipping order message", msg.inst_id);
            continue;
        };

        let Some(client_order_id) = parse_client_order_id(&msg.cl_ord_id) else {
            log::debug!(
                "Order without client_order_id (ord_id={}), sending as report",
                msg.ord_id
            );
            dispatch_order_msg_as_report(
                msg,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
            continue;
        };

        // Resolve identity: check direct match first, then fall back to the
        // parent algo order ID for triggered child orders. OKX assigns a new
        // cl_ord_id to child orders when an algo/stop triggers, preserving the
        // parent's client order ID in algo_cl_ord_id.
        let (client_order_id, identity) = match state
            .order_identities
            .get(&client_order_id)
            .map(|r| r.clone())
        {
            Some(ident) => (client_order_id, Some(ident)),
            None => {
                if let Some(parent_id) = msg
                    .algo_cl_ord_id
                    .as_deref()
                    .and_then(parse_client_order_id)
                {
                    let parent_ident = state.order_identities.get(&parent_id).map(|r| r.clone());

                    if parent_ident.is_some() {
                        (parent_id, parent_ident)
                    } else {
                        (client_order_id, None)
                    }
                } else {
                    (client_order_id, None)
                }
            }
        };

        if let Some(ident) = identity {
            if is_post_only_auto_cancel(msg) {
                let ts_event = parse_millisecond_timestamp(msg.u_time);
                let rejected = OrderRejected::new(
                    emitter.trader_id(),
                    ident.strategy_id,
                    instrument.id(),
                    client_order_id,
                    account_id,
                    Ustr::from("Post-only order would have taken liquidity"),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    true, // due_post_only
                );
                state.order_identities.remove(&client_order_id);
                order_state_cache.remove(&client_order_id);
                fee_cache.remove(&msg.ord_id);
                filled_qty_cache.remove(&msg.ord_id);
                emitter.send_order_event(OrderEventAny::Rejected(rejected));
                continue;
            }

            let previous_fee = fee_cache.get(&msg.ord_id).copied();
            let previous_filled_qty = filled_qty_cache.get(&msg.ord_id).copied();
            let previous_state = order_state_cache.get(&client_order_id);

            match parse_order_event(
                msg,
                client_order_id,
                account_id,
                emitter.trader_id(),
                ident.strategy_id,
                instrument,
                previous_fee,
                previous_filled_qty,
                previous_state,
                ts_init,
            ) {
                Ok(event) => {
                    update_order_caches(
                        msg,
                        instrument,
                        client_order_id,
                        fee_cache,
                        filled_qty_cache,
                        order_state_cache,
                    );
                    dispatch_parsed_order_event(
                        event,
                        client_order_id,
                        account_id,
                        VenueOrderId::new(msg.ord_id),
                        &ident,
                        instrument,
                        msg.state,
                        emitter,
                        state,
                        order_state_cache,
                        ts_init,
                    );
                }
                Err(e) => log::error!("Failed to parse order event for {client_order_id}: {e}"),
            }
        } else {
            log::debug!(
                "Untracked order {client_order_id} (ord_id={}), sending as report for reconciliation",
                msg.ord_id
            );
            dispatch_order_msg_as_report(
                msg,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
        }
    }
}

/// Dispatches a parsed order event as a proper `OrderEventAny`.
///
/// Guarantees the `Submitted -> Accepted -> ...` lifecycle by synthesizing
/// `OrderAccepted` before any other event when one has not yet been emitted.
/// Duplicate `Accepted` events (e.g. from reconnect replays) are suppressed.
#[expect(clippy::too_many_arguments)]
fn dispatch_parsed_order_event(
    event: ParsedOrderEvent,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    instrument: &InstrumentAny,
    venue_status: OKXOrderStatus,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    ts_init: UnixNanos,
) {
    let is_terminal;

    match event {
        ParsedOrderEvent::Accepted(e) => {
            if state.emitted_accepted.contains(&client_order_id)
                || state.filled_orders.contains(&client_order_id)
                || state.triggered_orders.contains(&client_order_id)
            {
                log::debug!("Skipping duplicate Accepted for {client_order_id}");
                return;
            }
            state.insert_accepted(client_order_id);
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Accepted(e));
        }
        ParsedOrderEvent::Triggered(e) => {
            if state.filled_orders.contains(&client_order_id) {
                log::debug!("Skipping stale Triggered for {client_order_id} (already filled)");
                return;
            }

            if !TRIGGERABLE_ORDER_TYPES.contains(&identity.order_type) {
                log::debug!(
                    "Skipping OrderTriggered for {} order {client_order_id}: market-style stops have no TRIGGERED state",
                    identity.order_type,
                );
                state.insert_triggered(client_order_id);
                return;
            }

            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
            );
            state.insert_triggered(client_order_id);
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Triggered(e));
        }
        ParsedOrderEvent::Canceled(e) => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
            );
            state.triggered_orders.remove(&client_order_id);
            state.filled_orders.remove(&client_order_id);
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Canceled(e));
        }
        ParsedOrderEvent::Expired(e) => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
            );
            state.triggered_orders.remove(&client_order_id);
            state.filled_orders.remove(&client_order_id);
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Expired(e));
        }
        ParsedOrderEvent::Updated(e) => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
            );
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Updated(e));
        }
        ParsedOrderEvent::Fill(fill_report) => {
            let is_duplicate = state.check_and_insert_trade(fill_report.trade_id);
            is_terminal = venue_status == OKXOrderStatus::Filled;

            if is_duplicate {
                log::debug!(
                    "Skipping duplicate fill for {client_order_id}: trade_id={}",
                    fill_report.trade_id
                );
            } else {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    identity,
                    emitter,
                    state,
                    ts_init,
                );
                state.insert_filled(client_order_id);
                state.triggered_orders.remove(&client_order_id);
                let filled = fill_report_to_order_filled(
                    &fill_report,
                    emitter.trader_id(),
                    identity,
                    instrument.quote_currency(),
                );
                emitter.send_order_event(OrderEventAny::Filled(filled));
            }
        }
        ParsedOrderEvent::StatusOnly(report) => {
            is_terminal = matches!(
                report.order_status,
                OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Expired
            );
            emitter.send_order_status_report(*report);
        }
        ParsedOrderEvent::Skipped => return,
    }

    if is_terminal {
        state.order_identities.remove(&client_order_id);
        state.emitted_accepted.remove(&client_order_id);
        order_state_cache.remove(&client_order_id);
        // Keep fee_cache and filled_qty_cache entries: replayed terminal
        // messages go through the untracked report path and need prior
        // cumulative state to avoid re-emitting the full fill quantity
    }
}

/// Synthesizes and emits `OrderAccepted` if one has not yet been emitted for
/// this order. Handles fast-filling orders that skip the `Live` state on OKX.
fn ensure_accepted_emitted(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if state.emitted_accepted.contains(&client_order_id) {
        return;
    }
    state.insert_accepted(client_order_id);
    let accepted = OrderAccepted::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_init,
        ts_init,
        false,
    );
    emitter.send_order_event(OrderEventAny::Accepted(accepted));
}

/// Converts a [`FillReport`] into an [`OrderFilled`] event using tracked identity.
fn fill_report_to_order_filled(
    report: &FillReport,
    trader_id: TraderId,
    identity: &OrderIdentity,
    quote_currency: Currency,
) -> OrderFilled {
    OrderFilled::new(
        trader_id,
        identity.strategy_id,
        report.instrument_id,
        report
            .client_order_id
            .expect("tracked order has client_order_id"),
        report.venue_order_id,
        report.account_id,
        report.trade_id,
        identity.order_side,
        identity.order_type,
        report.last_qty,
        report.last_px,
        quote_currency,
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        report.ts_init,
        false,
        report.venue_position_id,
        Some(report.commission),
    )
}

/// Falls back to the report path for a single order message.
#[expect(clippy::too_many_arguments)]
fn dispatch_order_msg_as_report(
    msg: &OKXOrderMsg,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    match parse_order_msg(
        msg,
        account_id,
        instruments,
        fee_cache,
        filled_qty_cache,
        ts_init,
    ) {
        Ok(report) => {
            if let Some(instrument) = instruments.get(&msg.inst_id) {
                update_fee_fill_caches(msg, instrument, fee_cache, filled_qty_cache);
            }
            dispatch_execution_reports(vec![report], emitter, state);
        }
        Err(e) => log::error!("Failed to parse order message as report: {e}"),
    }
}

/// Updates fee, fill, and order state caches from a raw OKX order message.
fn update_order_caches(
    msg: &OKXOrderMsg,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
) {
    update_fee_fill_caches(msg, instrument, fee_cache, filled_qty_cache);

    let venue_order_id = VenueOrderId::new(msg.ord_id);
    let quantity = parse_quantity(&msg.sz, instrument.size_precision()).unwrap_or_default();
    let price = if is_market_price(&msg.px) {
        None
    } else {
        parse_price(&msg.px, instrument.price_precision()).ok()
    };

    order_state_cache.insert(
        client_order_id,
        OrderStateSnapshot {
            venue_order_id,
            quantity,
            price,
        },
    );
}

/// Dispatches execution reports with cross-stream deduplication.
pub fn dispatch_execution_reports(
    reports: Vec<ExecutionReport>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
) {
    log::debug!("Processing {} execution report(s)", reports.len());

    for report in reports {
        match report {
            ExecutionReport::Order(order_report) => {
                if let Some(cid) = order_report.client_order_id {
                    match order_report.order_status {
                        // Guard form reformats awkwardly across multiple lines
                        #[expect(clippy::collapsible_match)]
                        OrderStatus::Accepted => {
                            if state.filled_orders.contains(&cid)
                                || state.triggered_orders.contains(&cid)
                            {
                                log::debug!(
                                    "Skipping stale OrderStatusReport(Accepted) \
                                     for {cid} (already triggered/filled)"
                                );
                                continue;
                            }
                        }
                        OrderStatus::Triggered => {
                            if state.filled_orders.contains(&cid) {
                                log::debug!(
                                    "Skipping stale OrderStatusReport(Triggered) \
                                     for {cid} (already filled)"
                                );
                                continue;
                            }
                            state.insert_triggered(cid);
                        }
                        OrderStatus::Filled => {
                            state.insert_filled(cid);
                            state.triggered_orders.remove(&cid);
                        }
                        OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Rejected => {
                            state.triggered_orders.remove(&cid);
                            state.filled_orders.remove(&cid);
                        }
                        _ => {}
                    }
                }
                emitter.send_order_status_report(order_report);
            }
            ExecutionReport::Fill(fill_report) => {
                if state.check_and_insert_trade(fill_report.trade_id) {
                    log::debug!(
                        "Skipping duplicate fill report: trade_id={}",
                        fill_report.trade_id
                    );
                    continue;
                }

                if let Some(cid) = fill_report.client_order_id {
                    state.insert_filled(cid);
                    state.triggered_orders.remove(&cid);
                }
                emitter.send_fill_report(fill_report);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AlgoCancelContext {
    pub client_order_id: ClientOrderId,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub venue_order_id: Option<VenueOrderId>,
}

// Contexts must correspond 1:1 with the requests that produced
// the responses (OKX preserves request order in batch responses).
pub fn emit_algo_cancel_rejections(
    responses: &[OKXCancelAlgoOrderResponse],
    contexts: &[AlgoCancelContext],
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    for (i, item) in responses.iter().enumerate() {
        let code = item.s_code.as_deref().unwrap_or(OKX_SUCCESS_CODE);
        if code == OKX_SUCCESS_CODE {
            continue;
        }

        let msg = item.s_msg.as_deref().unwrap_or("");

        if let Some(ctx) = contexts.get(i) {
            let ts = clock.get_time_ns();
            emitter.emit_order_cancel_rejected_event(
                ctx.strategy_id,
                ctx.instrument_id,
                ctx.client_order_id,
                ctx.venue_order_id,
                msg,
                ts,
            );
        } else {
            log::warn!(
                "Algo cancel rejected but no context at index {i}: \
                 algo_id={} sCode={code} sMsg={msg}",
                item.algo_id
            );
        }
    }
}

pub fn emit_batch_cancel_failure(
    contexts: &[AlgoCancelContext],
    error: &str,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    for ctx in contexts {
        let ts = clock.get_time_ns();
        emitter.emit_order_cancel_rejected_event(
            ctx.strategy_id,
            ctx.instrument_id,
            ctx.client_order_id,
            ctx.venue_order_id,
            error,
            ts,
        );
    }
}
