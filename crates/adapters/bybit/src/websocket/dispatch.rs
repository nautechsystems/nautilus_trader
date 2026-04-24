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

//! WebSocket message dispatch for the Bybit execution client.
//!
//! Routes incoming [`BybitWsMessage`] variants to the appropriate parsing and
//! event emission paths. Tracked orders (submitted through this client) produce
//! proper order events; untracked orders fall back to execution reports for
//! downstream reconciliation.

use std::sync::atomic::{AtomicBool, Ordering};

use ahash::AHashMap;
use anyhow::Context;
use dashmap::{DashMap, DashSet};
use nautilus_core::{UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderFilled, OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::TRIGGERABLE_ORDER_TYPES,
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    messages::{BybitWsAccountExecution, BybitWsAccountOrder, BybitWsMessage},
    parse::{parse_millis_i64, parse_ws_account_state, parse_ws_position_status_report},
};
use crate::common::{
    enums::BybitOrderStatus,
    parse::{
        make_bybit_symbol, parse_millis_timestamp, parse_price_with_precision,
        parse_quantity_with_precision,
    },
};

const DEDUP_CAPACITY: usize = 10_000;

const BYBIT_OP_ORDER_CREATE: &str = "order.create";
const BYBIT_OP_ORDER_AMEND: &str = "order.amend";
const BYBIT_OP_ORDER_CANCEL: &str = "order.cancel";

/// Order identity context stored at submission time, used by the WS dispatch
/// task to produce proper order events without Cache access.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

/// Tracks which type of WS request is pending for a given req_id.
#[derive(Debug, Clone, Copy)]
pub enum PendingOperation {
    Place,
    Cancel,
    Amend,
}

/// Shared state for cross-stream event deduplication between the private
/// and trade WebSocket dispatch loops.
pub type PendingRequestData = (
    Vec<ClientOrderId>,
    Vec<Option<VenueOrderId>>,
    PendingOperation,
);

/// Snapshot of an order's price, quantity, and trigger price at last dispatch.
/// Used to detect modifications when Bybit sends back an order with the same
/// status but changed fields.
#[derive(Debug, Clone)]
pub struct OrderStateSnapshot {
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
}

#[derive(Debug)]
pub struct WsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    pub pending_requests: DashMap<String, PendingRequestData>,
    pub order_snapshots: DashMap<ClientOrderId, OrderStateSnapshot>,
    pub emitted_accepted: DashSet<ClientOrderId>,
    pub triggered_orders: DashSet<ClientOrderId>,
    pub filled_orders: DashSet<ClientOrderId>,
    clearing: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            pending_requests: DashMap::new(),
            order_snapshots: DashMap::new(),
            emitted_accepted: DashSet::default(),
            triggered_orders: DashSet::default(),
            filled_orders: DashSet::default(),
            clearing: AtomicBool::new(false),
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

    fn insert_accepted(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.emitted_accepted);
        self.emitted_accepted.insert(cid);
    }

    fn insert_filled(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.filled_orders);
        self.filled_orders.insert(cid);
    }

    fn insert_triggered(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.triggered_orders);
        self.triggered_orders.insert(cid);
    }
}

/// Dispatches a WebSocket message with cross-stream deduplication.
///
/// For orders with a tracked identity (submitted through this client), produces
/// proper order events (OrderAccepted, OrderCanceled, OrderFilled, etc.).
/// For untracked orders (external or pre-existing), falls back to execution
/// reports for downstream reconciliation.
pub fn dispatch_ws_message(
    message: &BybitWsMessage,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    clock: &AtomicTime,
) {
    match message {
        BybitWsMessage::AccountOrder(msg) => {
            let ts_init = clock.get_time_ns();

            for order in &msg.data {
                let symbol = make_bybit_symbol(order.symbol, order.category);
                let Some(instrument) = instruments.get(&symbol) else {
                    log::warn!("No instrument for order update: {symbol}");
                    continue;
                };
                dispatch_order_update(order, instrument, emitter, state, account_id, ts_init);
            }
        }
        BybitWsMessage::AccountExecution(msg) => {
            let ts_init = clock.get_time_ns();

            for exec in &msg.data {
                let symbol = make_bybit_symbol(exec.symbol, exec.category);
                let Some(instrument) = instruments.get(&symbol) else {
                    log::warn!("No instrument for execution update: {symbol}");
                    continue;
                };
                dispatch_execution_fill(exec, instrument, emitter, state, account_id, ts_init);
            }
        }
        BybitWsMessage::AccountWallet(msg) => {
            let ts_init = clock.get_time_ns();
            let ts_event = parse_millis_i64(msg.creation_time, "wallet.creation_time")
                .unwrap_or_else(|e| {
                    log::warn!("Failed to parse wallet creation_time, using ts_init: {e}");
                    ts_init
                });

            for wallet in &msg.data {
                match parse_ws_account_state(wallet, account_id, ts_event, ts_init) {
                    Ok(state) => emitter.send_account_state(state),
                    Err(e) => log::error!("Failed to parse account state: {e}"),
                }
            }
        }
        BybitWsMessage::AccountPosition(msg) => {
            let ts_init = clock.get_time_ns();

            for position in &msg.data {
                let symbol = make_bybit_symbol(position.symbol, position.category);
                let Some(instrument) = instruments.get(&symbol) else {
                    log::warn!("No instrument for position update: {symbol}");
                    continue;
                };

                match parse_ws_position_status_report(position, account_id, instrument, ts_init) {
                    Ok(report) => emitter.send_position_report(report),
                    Err(e) => log::error!("Failed to parse position status report: {e}"),
                }
            }
        }
        BybitWsMessage::OrderResponse(resp) => {
            let ts_init = clock.get_time_ns();
            dispatch_order_response(resp, emitter, state, ts_init);
        }
        BybitWsMessage::Error(e) => {
            log::warn!("WebSocket error: code={} message={}", e.code, e.message);
        }
        BybitWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        BybitWsMessage::Auth(_)
        | BybitWsMessage::Orderbook(_)
        | BybitWsMessage::Trade(_)
        | BybitWsMessage::Kline(_)
        | BybitWsMessage::TickerLinear(_)
        | BybitWsMessage::TickerOption(_) => {}
    }
}

/// Dispatches a single order status update.
///
/// Tracked orders produce lifecycle events (OrderAccepted, OrderTriggered,
/// OrderCanceled, OrderRejected). Untracked orders fall back to
/// `OrderStatusReport` for reconciliation.
fn dispatch_order_update(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let client_order_id = if order.order_link_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(order.order_link_id.as_str()))
    };

    let identity = client_order_id
        .as_ref()
        .and_then(|cid| state.order_identities.get(cid).map(|r| r.clone()));

    if let (Some(client_order_id), Some(identity)) = (client_order_id, identity) {
        let venue_order_id = VenueOrderId::new(order.order_id.as_str());

        match order.order_status {
            BybitOrderStatus::Created | BybitOrderStatus::New | BybitOrderStatus::Untriggered => {
                let snapshot = parse_order_snapshot(order, instrument);

                if state.emitted_accepted.contains(&client_order_id)
                    || state.filled_orders.contains(&client_order_id)
                    || state.triggered_orders.contains(&client_order_id)
                {
                    if let Some(snapshot) = snapshot
                        && is_snapshot_updated(&snapshot, &client_order_id, state)
                    {
                        let updated = OrderUpdated::new(
                            emitter.trader_id(),
                            identity.strategy_id,
                            identity.instrument_id,
                            client_order_id,
                            snapshot.quantity,
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                            Some(venue_order_id),
                            Some(account_id),
                            snapshot.price,
                            snapshot.trigger_price,
                            None,
                            false,
                        );
                        state.order_snapshots.insert(client_order_id, snapshot);
                        emitter.send_order_event(OrderEventAny::Updated(updated));
                        return;
                    }
                    log::debug!("Skipping duplicate Accepted for {client_order_id}");
                    return;
                }

                state.insert_accepted(client_order_id);

                if let Some(snapshot) = snapshot {
                    state.order_snapshots.insert(client_order_id, snapshot);
                }

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
            BybitOrderStatus::Triggered => {
                if state.filled_orders.contains(&client_order_id) {
                    log::debug!("Skipping stale Triggered for {client_order_id} (already filled)");
                    return;
                }

                if !TRIGGERABLE_ORDER_TYPES.contains(&identity.order_type) {
                    log::debug!(
                        "Skipping OrderTriggered for {} order {client_order_id}: market-style stops have no TRIGGERED state",
                        identity.order_type,
                    );
                    return;
                }

                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    state,
                    ts_init,
                );
                state.insert_triggered(client_order_id);
                let triggered = OrderTriggered::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::Triggered(triggered));
            }
            BybitOrderStatus::Rejected => {
                let filled_qty = parse_quantity_with_precision(
                    &order.cum_exec_qty,
                    instrument.size_precision(),
                    "order.cumExecQty",
                )
                .unwrap_or_default();

                if filled_qty.is_positive() {
                    // Partially filled then rejected - treat as canceled
                    ensure_accepted_emitted(
                        client_order_id,
                        account_id,
                        venue_order_id,
                        &identity,
                        emitter,
                        state,
                        ts_init,
                    );
                    let canceled = OrderCanceled::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_init,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    );
                    cleanup_terminal(client_order_id, state);
                    emitter.send_order_event(OrderEventAny::Canceled(canceled));
                } else {
                    let reason = if order.reject_reason.is_empty() {
                        Ustr::from("Order rejected by venue")
                    } else {
                        order.reject_reason
                    };
                    state.order_identities.remove(&client_order_id);
                    state.order_snapshots.remove(&client_order_id);
                    emitter.emit_order_rejected_event(
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        reason.as_str(),
                        ts_init,
                        false,
                    );
                }
            }
            BybitOrderStatus::PartiallyFilled => {
                // Fills arrive on the execution channel; no event needed here.
                // Ensure accepted was emitted so the fill has a valid prior state.
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    state,
                    ts_init,
                );

                // A successful amend on a partially filled order keeps the
                // PartiallyFilled status. Detect price/qty/trigger changes and
                // emit OrderUpdated so PendingUpdate resolves.
                if let Some(snapshot) = parse_order_snapshot(order, instrument)
                    && is_snapshot_updated(&snapshot, &client_order_id, state)
                {
                    let updated = OrderUpdated::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        snapshot.quantity,
                        UUID4::new(),
                        ts_init,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                        snapshot.price,
                        snapshot.trigger_price,
                        None,
                        false,
                    );
                    state.order_snapshots.insert(client_order_id, snapshot);
                    emitter.send_order_event(OrderEventAny::Updated(updated));
                }
            }
            BybitOrderStatus::Filled => {
                // Fills arrive on the execution channel; no event needed here.
                // Ensure accepted was emitted so the fill has a valid prior state.
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    state,
                    ts_init,
                );
                // Identity cleaned up in dispatch_execution_fill when leaves_qty
                // reaches zero, since there is no guaranteed ordering between
                // the order and execution topics.
            }
            BybitOrderStatus::Canceled
            | BybitOrderStatus::PartiallyFilledCanceled
            | BybitOrderStatus::Deactivated => {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    state,
                    ts_init,
                );
                let canceled = OrderCanceled::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                cleanup_terminal(client_order_id, state);
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            }
        }
    } else {
        // Untracked order: fall back to report for reconciliation
        match super::parse::parse_ws_order_status_report(order, instrument, account_id, ts_init) {
            Ok(report) => emitter.send_order_status_report(report),
            Err(e) => log::error!("Failed to parse order status report: {e}"),
        }
    }
}

/// Dispatches a single execution (fill) message.
///
/// Tracked orders are parsed directly to [`OrderFilled`]. Untracked orders
/// fall back to [`FillReport`] for reconciliation.
fn dispatch_execution_fill(
    exec: &BybitWsAccountExecution,
    instrument: &InstrumentAny,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    if exec.exec_type.is_exchange_generated() {
        log::warn!(
            "Exchange-generated execution: exec_type={:?}, symbol={}, order_id={}, order_link_id={}, side={:?}, qty={}, price={}",
            exec.exec_type,
            exec.symbol,
            exec.order_id,
            exec.order_link_id,
            exec.side,
            exec.exec_qty,
            exec.exec_price,
        );
    }

    let client_order_id = if exec.order_link_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(exec.order_link_id.as_str()))
    };

    let identity = client_order_id
        .as_ref()
        .and_then(|cid| state.order_identities.get(cid).map(|r| r.clone()));

    if let (Some(client_order_id), Some(identity)) = (client_order_id, identity) {
        let venue_order_id = VenueOrderId::new(exec.order_id.as_str());

        ensure_accepted_emitted(
            client_order_id,
            account_id,
            venue_order_id,
            &identity,
            emitter,
            state,
            ts_init,
        );

        match parse_order_filled(exec, instrument, &identity, emitter, account_id, ts_init) {
            Ok(filled) => {
                state.insert_filled(client_order_id);
                state.triggered_orders.remove(&client_order_id);
                emitter.send_order_event(OrderEventAny::Filled(filled));

                if exec.leaves_qty == "0" {
                    cleanup_terminal(client_order_id, state);
                }
            }
            Err(e) => log::error!("Failed to parse OrderFilled for {client_order_id}: {e}"),
        }
    } else {
        // Untracked: fall back to FillReport for reconciliation
        match super::parse::parse_ws_fill_report(exec, account_id, instrument, ts_init) {
            Ok(report) => emitter.send_fill_report(report),
            Err(e) => log::error!("Failed to parse fill report: {e}"),
        }
    }
}

/// Parses a Bybit execution message directly into an [`OrderFilled`] event.
fn parse_order_filled(
    exec: &BybitWsAccountExecution,
    instrument: &InstrumentAny,
    identity: &OrderIdentity,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderFilled> {
    let client_order_id = ClientOrderId::new(exec.order_link_id.as_str());
    let venue_order_id = VenueOrderId::new(exec.order_id.as_str());
    let trade_id =
        TradeId::new_checked(exec.exec_id.as_str()).context("invalid execId in Bybit execution")?;

    let last_qty = parse_quantity_with_precision(
        &exec.exec_qty,
        instrument.size_precision(),
        "execution.execQty",
    )?;
    let last_px = parse_price_with_precision(
        &exec.exec_price,
        instrument.price_precision(),
        "execution.execPrice",
    )?;

    let liquidity_side = if exec.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let fee_decimal: Decimal = exec
        .exec_fee
        .parse()
        .with_context(|| format!("failed to parse execFee='{}'", exec.exec_fee))?;
    let commission_currency = instrument.quote_currency();
    let commission = Money::from_decimal(fee_decimal, commission_currency).with_context(|| {
        format!(
            "failed to create commission from execFee='{}'",
            exec.exec_fee
        )
    })?;

    let ts_event = parse_millis_timestamp(&exec.exec_time, "execution.execTime")?;

    Ok(OrderFilled::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        trade_id,
        identity.order_side,
        identity.order_type,
        last_qty,
        last_px,
        commission_currency,
        liquidity_side,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        None, // venue_position_id
        Some(commission),
    ))
}

/// Handles a Bybit WS order response, emitting rejection events for failures.
fn dispatch_order_response(
    resp: &super::messages::BybitWsOrderResponse,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if resp.ret_code == 0 {
        // Check for per-order failures in batch retExtInfo even on success
        let pending = resp
            .req_id
            .as_ref()
            .and_then(|rid| state.pending_requests.remove(rid))
            .map(|(_, v)| v);

        if let Some((cids, voids, pending_op)) = pending {
            let batch_errors = resp.extract_batch_errors();
            let data_array = resp.data.as_array();

            for (idx, error) in batch_errors.iter().enumerate() {
                if error.code == 0 {
                    continue;
                }

                // Extract orderLinkId from the corresponding data entry
                let cid = data_array
                    .and_then(|arr| arr.get(idx))
                    .and_then(extract_order_link_id_from_data)
                    .or_else(|| cids.get(idx).copied());

                let Some(cid) = cid else {
                    log::warn!(
                        "Batch error at index {idx} without correlation: code={}, msg={}",
                        error.code,
                        error.msg,
                    );
                    continue;
                };

                let Some(identity) = state.order_identities.get(&cid).map(|r| r.clone()) else {
                    log::warn!(
                        "Batch error for untracked order: client_order_id={cid}, msg={}",
                        error.msg,
                    );
                    continue;
                };

                let stored_void = voids.get(idx).and_then(|v| *v);

                emit_rejection_for_op(
                    &pending_op,
                    cid,
                    &identity,
                    stored_void,
                    &error.msg,
                    emitter,
                    state,
                    ts_init,
                );
            }
        }
        return;
    }

    // Remove the pending request entry (if any) to get client_order_ids and op
    let pending = resp
        .req_id
        .as_ref()
        .and_then(|rid| state.pending_requests.remove(rid))
        .map(|(_, v)| v);

    let effective_op = pending
        .as_ref()
        .map(|(_, _, op)| *op)
        .or_else(|| pending_op_from_str(resp.op.as_str()))
        .unwrap_or_else(|| {
            log::warn!("Unknown order operation '{}', defaulting to Place", resp.op);
            PendingOperation::Place
        });

    // For batch rejections (ret_code != 0), emit rejections for ALL orders
    if let Some((cids, voids, _)) = &pending
        && cids.len() > 1
    {
        for (idx, cid) in cids.iter().enumerate() {
            let Some(identity) = state.order_identities.get(cid).map(|r| r.clone()) else {
                log::warn!(
                    "Batch reject for untracked order: client_order_id={cid}, ret_msg={}",
                    resp.ret_msg,
                );
                continue;
            };
            let void = voids.get(idx).and_then(|v| *v);
            emit_rejection_for_op(
                &effective_op,
                *cid,
                &identity,
                void,
                &resp.ret_msg,
                emitter,
                state,
                ts_init,
            );
        }
        return;
    }

    // Single-order rejection path
    let client_order_id = extract_order_link_id_from_data(&resp.data).or_else(|| {
        pending
            .as_ref()
            .and_then(|(cids, _, _)| cids.first().copied())
    });

    let stored_venue_order_id = pending
        .as_ref()
        .and_then(|(_, voids, _)| voids.first().and_then(|v| *v));

    let Some(client_order_id) = client_order_id else {
        log::warn!(
            "Order response error without correlation: op={}, ret_code={}, ret_msg={}, req_id={:?}",
            resp.op,
            resp.ret_code,
            resp.ret_msg,
            resp.req_id,
        );
        return;
    };

    let Some(identity) = state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone())
    else {
        log::warn!(
            "Order response error for untracked order: op={}, client_order_id={client_order_id}, ret_msg={}",
            resp.op,
            resp.ret_msg,
        );
        return;
    };

    let venue_order_id = extract_venue_order_id_from_data(&resp.data).or(stored_venue_order_id);

    emit_rejection_for_op(
        &effective_op,
        client_order_id,
        &identity,
        venue_order_id,
        &resp.ret_msg,
        emitter,
        state,
        ts_init,
    );
}

/// Emits the appropriate rejection event based on the pending operation type.
#[expect(clippy::too_many_arguments)]
fn emit_rejection_for_op(
    pending_op: &PendingOperation,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    venue_order_id: Option<VenueOrderId>,
    reason: &str,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    match pending_op {
        PendingOperation::Place => {
            state.order_identities.remove(&client_order_id);
            emitter.emit_order_rejected_event(
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                reason,
                ts_init,
                false,
            );
        }
        PendingOperation::Cancel => {
            emitter.emit_order_cancel_rejected_event(
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                venue_order_id,
                reason,
                ts_init,
            );
        }
        PendingOperation::Amend => {
            emitter.emit_order_modify_rejected_event(
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                venue_order_id,
                reason,
                ts_init,
            );
        }
    }
}

/// Maps an operation string to a `PendingOperation`.
fn pending_op_from_str(op: &str) -> Option<PendingOperation> {
    match op {
        BYBIT_OP_ORDER_CREATE => Some(PendingOperation::Place),
        BYBIT_OP_ORDER_CANCEL => Some(PendingOperation::Cancel),
        BYBIT_OP_ORDER_AMEND => Some(PendingOperation::Amend),
        _ => None,
    }
}

/// Parses an order snapshot from a WS order message for modification detection.
fn parse_order_snapshot(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
) -> Option<OrderStateSnapshot> {
    let quantity =
        parse_quantity_with_precision(&order.qty, instrument.size_precision(), "order.qty").ok()?;

    let price = if !order.price.is_empty() && order.price != "0" {
        parse_price_with_precision(&order.price, instrument.price_precision(), "order.price").ok()
    } else {
        None
    };

    let trigger_price = if !order.trigger_price.is_empty() && order.trigger_price != "0" {
        parse_price_with_precision(
            &order.trigger_price,
            instrument.price_precision(),
            "order.triggerPrice",
        )
        .ok()
    } else {
        None
    };

    Some(OrderStateSnapshot {
        quantity,
        price,
        trigger_price,
    })
}

/// Returns whether the incoming snapshot differs from the stored snapshot.
fn is_snapshot_updated(
    snapshot: &OrderStateSnapshot,
    client_order_id: &ClientOrderId,
    state: &WsDispatchState,
) -> bool {
    let Some(previous) = state.order_snapshots.get(client_order_id) else {
        return false;
    };

    if let (Some(prev_price), Some(new_price)) = (previous.price, snapshot.price)
        && prev_price != new_price
    {
        return true;
    }

    if let (Some(prev_trigger), Some(new_trigger)) =
        (previous.trigger_price, snapshot.trigger_price)
        && prev_trigger != new_trigger
    {
        return true;
    }

    previous.quantity != snapshot.quantity
}

/// Synthesizes and emits `OrderAccepted` if one has not yet been emitted for
/// this order. Handles fast-filling orders that skip the `New` state on Bybit.
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

/// Removes a terminal order from all tracking sets.
fn cleanup_terminal(client_order_id: ClientOrderId, state: &WsDispatchState) {
    state.order_identities.remove(&client_order_id);
    state.order_snapshots.remove(&client_order_id);
    state.emitted_accepted.remove(&client_order_id);
    state.triggered_orders.remove(&client_order_id);
    state.filled_orders.remove(&client_order_id);
}

/// Tries to extract `orderLinkId` from the response data Value.
fn extract_order_link_id_from_data(data: &serde_json::Value) -> Option<ClientOrderId> {
    data.get("orderLinkId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(ClientOrderId::new)
}

/// Tries to extract `orderId` from the response data Value.
fn extract_venue_order_id_from_data(data: &serde_json::Value) -> Option<VenueOrderId> {
    data.get("orderId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(VenueOrderId::new)
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use nautilus_common::messages::{ExecutionEvent, execution::ExecutionReport};
    use nautilus_core::{
        UnixNanos,
        time::{AtomicTime, get_atomic_clock_realtime},
    };
    use nautilus_live::emitter::ExecutionEventEmitter;
    use nautilus_model::{
        enums::{AccountType, OrderSide, OrderType},
        events::OrderEventAny,
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
        instruments::{Instrument, InstrumentAny},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{parse::parse_linear_instrument, testing::load_test_json},
        http::models::{BybitFeeRate, BybitInstrumentLinearResponse},
        websocket::messages::BybitWsMessage,
    };

    fn sample_fee_rate(
        symbol: &str,
        taker: &str,
        maker: &str,
        base_coin: Option<&str>,
    ) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_linear_instrument(instrument, &fee_rate, ts, ts).unwrap()
    }

    fn build_instruments(instruments: &[InstrumentAny]) -> AHashMap<Ustr, InstrumentAny> {
        let mut map = AHashMap::new();
        for inst in instruments {
            map.insert(inst.id().symbol.inner(), inst.clone());
        }
        map
    }

    fn test_account_id() -> AccountId {
        AccountId::from("BYBIT-001")
    }

    fn create_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let clock = get_atomic_clock_realtime();
        let trader_id = TraderId::from("TESTER-001");
        let account_id = test_account_id();
        let mut emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn default_identity() -> OrderIdentity {
        OrderIdentity {
            instrument_id: InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
            strategy_id: StrategyId::from("S-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        }
    }

    #[rstest]
    fn test_dispatch_tracked_canceled_order_emits_accepted_then_canceled() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        // Fixture has orderStatus=Cancelled
        let json = load_test_json("ws_account_order.json");
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_str(&json).unwrap();

        if let Some(order) = msg.data.first()
            && !order.order_link_id.is_empty()
        {
            let cid = ClientOrderId::new(order.order_link_id.as_str());
            state.order_identities.insert(cid, default_identity());
        }

        let ws_msg = BybitWsMessage::AccountOrder(msg);
        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        // First: synthesized Accepted
        let event1 = rx.try_recv().unwrap();
        assert!(
            matches!(event1, ExecutionEvent::Order(OrderEventAny::Accepted(ref a)) if a.strategy_id == StrategyId::from("S-001")),
            "Expected Accepted, found {event1:?}"
        );

        // Second: Canceled (from Cancelled status)
        let event2 = rx.try_recv().unwrap();
        assert!(
            matches!(event2, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
            "Expected Canceled, found {event2:?}"
        );
    }

    #[rstest]
    fn test_dispatch_untracked_order_emits_report() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_order.json");
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_str(&json).unwrap();

        // No identity registered → untracked
        let ws_msg = BybitWsMessage::AccountOrder(msg);
        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::Order(_))
        ));
    }

    #[rstest]
    fn test_dispatch_tracked_execution_emits_order_filled() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_execution.json");
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&json).unwrap();

        // Register identity for the execution's orderLinkId
        if let Some(exec) = msg.data.first()
            && !exec.order_link_id.is_empty()
        {
            let cid = ClientOrderId::new(exec.order_link_id.as_str());
            state.order_identities.insert(cid, default_identity());
        }

        let ws_msg = BybitWsMessage::AccountExecution(msg);
        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        // First event should be synthesized Accepted
        let event1 = rx.try_recv().unwrap();
        assert!(
            matches!(event1, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
            "Expected Accepted, found {event1:?}"
        );

        // Second event should be OrderFilled
        let event2 = rx.try_recv().unwrap();
        match event2 {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.strategy_id, StrategyId::from("S-001"));
                assert_eq!(filled.order_side, OrderSide::Buy);
                assert_eq!(filled.order_type, OrderType::Limit);
            }
            other => panic!("Expected Filled event, found {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_untracked_execution_emits_fill_report() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_execution.json");
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&json).unwrap();

        // No identity registered → untracked
        let ws_msg = BybitWsMessage::AccountExecution(msg);
        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::Fill(_))
        ));
    }

    #[rstest]
    fn test_dispatch_wallet_emits_account_state() {
        let instruments = AHashMap::new();
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_wallet.json");
        let msg: crate::websocket::messages::BybitWsAccountWalletMsg =
            serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::AccountWallet(msg);

        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, ExecutionEvent::Account(_)));
    }

    #[rstest]
    fn test_dispatch_data_message_ignored() {
        let instruments = AHashMap::new();
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_public_trade.json");
        let msg: crate::websocket::messages::BybitWsTradeMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::Trade(msg);

        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_accepted_dedup_prevents_duplicate() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        // Fixture has orderStatus=Cancelled. Patch to New for this dedup test.
        let json = load_test_json("ws_account_order.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["orderStatus"] = serde_json::Value::String("New".to_string());
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_value(value).unwrap();

        if let Some(order) = msg.data.first()
            && !order.order_link_id.is_empty()
        {
            let cid = ClientOrderId::new(order.order_link_id.as_str());
            state.order_identities.insert(cid, default_identity());
        }

        let ws_msg = BybitWsMessage::AccountOrder(msg.clone());
        dispatch_ws_message(
            &ws_msg,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        // Dispatch the same message again: dedup should suppress the duplicate
        let ws_msg2 = BybitWsMessage::AccountOrder(msg);
        dispatch_ws_message(
            &ws_msg2,
            &emitter,
            &state,
            test_account_id(),
            &instruments,
            clock,
        );

        assert!(rx.try_recv().is_err());
    }

    fn new_order_value() -> serde_json::Value {
        let json = load_test_json("ws_account_order.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["orderStatus"] = serde_json::Value::String("New".to_string());
        value
    }

    struct DispatchTestContext {
        instruments: AHashMap<Ustr, InstrumentAny>,
        emitter: ExecutionEventEmitter,
        rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        clock: &'static AtomicTime,
        state: WsDispatchState,
    }

    impl DispatchTestContext {
        fn new() -> Self {
            let instrument = linear_instrument();
            let instruments = build_instruments(std::slice::from_ref(&instrument));
            let (emitter, rx) = create_emitter();
            let clock = get_atomic_clock_realtime();
            let state = WsDispatchState::default();
            Self {
                instruments,
                emitter,
                rx,
                clock,
                state,
            }
        }

        fn accept_order(&mut self, value: &serde_json::Value) {
            let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
                serde_json::from_value(value.clone()).unwrap();

            if let Some(order) = msg.data.first()
                && !order.order_link_id.is_empty()
                && !self
                    .state
                    .order_identities
                    .contains_key(&ClientOrderId::new(order.order_link_id.as_str()))
            {
                let cid = ClientOrderId::new(order.order_link_id.as_str());
                self.state.order_identities.insert(cid, default_identity());
            }

            self.dispatch_value(value);

            let event = self.rx.try_recv().unwrap();
            assert!(
                matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
                "Expected Accepted, found {event:?}"
            );
        }

        fn dispatch_value(&self, value: &serde_json::Value) {
            let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
                serde_json::from_value(value.clone()).unwrap();
            let ws_msg = BybitWsMessage::AccountOrder(msg);
            dispatch_ws_message(
                &ws_msg,
                &self.emitter,
                &self.state,
                test_account_id(),
                &self.instruments,
                self.clock,
            );
        }

        fn recv_updated(&mut self) -> OrderUpdated {
            let event = self.rx.try_recv().unwrap();
            match event {
                ExecutionEvent::Order(OrderEventAny::Updated(updated)) => updated,
                other => panic!("Expected Updated event, found {other:?}"),
            }
        }
    }

    #[rstest]
    fn test_dispatch_order_updated_on_price_change() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        ctx.accept_order(&value);

        let mut amended = value;
        amended["data"][0]["price"] = serde_json::Value::String("31000".to_string());
        ctx.dispatch_value(&amended);

        let updated = ctx.recv_updated();
        assert_eq!(updated.client_order_id, ClientOrderId::from("client-1"));
        assert_eq!(updated.price, Some(Price::from("31000.00")));
        assert_eq!(updated.quantity, Quantity::from("0.010"));
        assert_eq!(updated.trigger_price, None);
        assert!(updated.venue_order_id.is_some());
    }

    #[rstest]
    fn test_dispatch_order_updated_on_quantity_change() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        ctx.accept_order(&value);

        let mut amended = value;
        amended["data"][0]["qty"] = serde_json::Value::String("0.020".to_string());
        ctx.dispatch_value(&amended);

        let updated = ctx.recv_updated();
        assert_eq!(updated.quantity, Quantity::from("0.020"));
        assert_eq!(updated.price, Some(Price::from("30000.00")));
    }

    #[rstest]
    fn test_dispatch_order_updated_on_trigger_price_change() {
        let mut ctx = DispatchTestContext::new();
        let mut value = new_order_value();
        value["data"][0]["triggerPrice"] = serde_json::Value::String("29000".to_string());
        ctx.accept_order(&value);

        let mut amended = value;
        amended["data"][0]["triggerPrice"] = serde_json::Value::String("28000".to_string());
        ctx.dispatch_value(&amended);

        let updated = ctx.recv_updated();
        assert_eq!(updated.trigger_price, Some(Price::from("28000.00")));
        assert_eq!(updated.price, Some(Price::from("30000.00")));
    }

    #[rstest]
    fn test_dispatch_dedup_suppresses_identical_after_snapshot() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        ctx.accept_order(&value);

        ctx.dispatch_value(&value);

        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no event for identical redelivery"
        );
    }

    #[rstest]
    fn test_dispatch_order_updated_stores_snapshot_for_subsequent_change() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        ctx.accept_order(&value);

        let mut amended1 = value.clone();
        amended1["data"][0]["price"] = serde_json::Value::String("31000".to_string());
        ctx.dispatch_value(&amended1);
        let _ = ctx.recv_updated();

        let mut amended2 = value;
        amended2["data"][0]["price"] = serde_json::Value::String("32000".to_string());
        ctx.dispatch_value(&amended2);

        let updated = ctx.recv_updated();
        assert_eq!(updated.price, Some(Price::from("32000.00")));
    }

    #[rstest]
    #[case::price_changed(
        Some(Price::from("100.00")),
        None,
        Quantity::from("1.000"),
        Some(Price::from("200.00")),
        None,
        Quantity::from("1.000"),
        true
    )]
    #[case::trigger_changed(
        None,
        Some(Price::from("100.00")),
        Quantity::from("1.000"),
        None,
        Some(Price::from("90.00")),
        Quantity::from("1.000"),
        true
    )]
    #[case::qty_changed(
        Some(Price::from("100.00")),
        None,
        Quantity::from("1.000"),
        Some(Price::from("100.00")),
        None,
        Quantity::from("2.000"),
        true
    )]
    #[case::no_change(
        Some(Price::from("100.00")),
        None,
        Quantity::from("1.000"),
        Some(Price::from("100.00")),
        None,
        Quantity::from("1.000"),
        false
    )]
    fn test_is_snapshot_updated(
        #[case] prev_price: Option<Price>,
        #[case] prev_trigger: Option<Price>,
        #[case] prev_qty: Quantity,
        #[case] new_price: Option<Price>,
        #[case] new_trigger: Option<Price>,
        #[case] new_qty: Quantity,
        #[case] expected: bool,
    ) {
        let state = WsDispatchState::default();
        let cid = ClientOrderId::from("test-1");
        state.order_snapshots.insert(
            cid,
            OrderStateSnapshot {
                quantity: prev_qty,
                price: prev_price,
                trigger_price: prev_trigger,
            },
        );

        let new_snapshot = OrderStateSnapshot {
            quantity: new_qty,
            price: new_price,
            trigger_price: new_trigger,
        };
        assert_eq!(is_snapshot_updated(&new_snapshot, &cid, &state), expected);
    }

    #[rstest]
    fn test_is_snapshot_updated_no_previous() {
        let state = WsDispatchState::default();
        let cid = ClientOrderId::from("test-1");

        let new_snapshot = OrderStateSnapshot {
            quantity: Quantity::from("1.000"),
            price: Some(Price::from("100.00")),
            trigger_price: None,
        };
        assert!(!is_snapshot_updated(&new_snapshot, &cid, &state));
    }

    #[rstest]
    #[case::limit_order("30000", "0", Some(Price::from("30000.00")), None)]
    #[case::conditional("0", "29000", None, Some(Price::from("29000.00")))]
    #[case::both(
        "30000",
        "29000",
        Some(Price::from("30000.00")),
        Some(Price::from("29000.00"))
    )]
    fn test_parse_order_snapshot(
        #[case] price: &str,
        #[case] trigger: &str,
        #[case] expected_price: Option<Price>,
        #[case] expected_trigger: Option<Price>,
    ) {
        let instrument = linear_instrument();
        let json = load_test_json("ws_account_order.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["price"] = serde_json::Value::String(price.to_string());
        value["data"][0]["triggerPrice"] = serde_json::Value::String(trigger.to_string());
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_value(value).unwrap();

        let snapshot = parse_order_snapshot(&msg.data[0], &instrument).unwrap();
        assert_eq!(snapshot.price, expected_price);
        assert_eq!(snapshot.trigger_price, expected_trigger);
        assert_eq!(snapshot.quantity, Quantity::from("0.010"));
    }

    #[rstest]
    fn test_parse_order_snapshot_invalid_qty_returns_none() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_account_order.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["qty"] = serde_json::Value::String(String::new());
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_value(value).unwrap();

        assert!(parse_order_snapshot(&msg.data[0], &instrument).is_none());
    }

    #[rstest]
    fn test_dispatch_order_updated_on_partially_filled_price_change() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        ctx.accept_order(&value);

        let mut amended = value;
        amended["data"][0]["orderStatus"] =
            serde_json::Value::String("PartiallyFilled".to_string());
        amended["data"][0]["cumExecQty"] = serde_json::Value::String("0.005".to_string());
        amended["data"][0]["price"] = serde_json::Value::String("31000".to_string());
        ctx.dispatch_value(&amended);

        let updated = ctx.recv_updated();
        assert_eq!(updated.client_order_id, ClientOrderId::from("client-1"));
        assert_eq!(updated.price, Some(Price::from("31000.00")));
    }
}
