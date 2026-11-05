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

use std::{
    collections::VecDeque,
    hash::Hash,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
use arc_swap::ArcSwapOption;
use dashmap::{DashMap, DashSet, mapref::entry::Entry};
use nautilus_core::{UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderFilled, OrderTriggered, OrderUpdated,
    },
    fees::MakerTakerFeeRates,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::TRIGGERABLE_ORDER_TYPES,
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    messages::{
        BybitWsAccountExecution, BybitWsAccountExecutionFast, BybitWsAccountOrder, BybitWsMessage,
    },
    parse::{
        parse_millis_i64, parse_ws_account_state, parse_ws_fill_report_fast,
        parse_ws_position_status_report,
    },
};
use crate::{
    common::{
        enums::{BybitExecType, BybitOrderSide, BybitOrderStatus, BybitProductType},
        parse::{
            bybit_rejection_due_post_only, extract_raw_symbol, get_currency, make_bybit_symbol,
            parse_millis_timestamp, parse_price_with_precision, parse_quantity_with_precision,
        },
    },
    http::{error::is_bybit_ambiguous_order_error_code, models::BybitFeeRate},
    repay::RepayRequest,
};

const DEDUP_CAPACITY: usize = 10_000;

const BYBIT_OP_ORDER_CREATE: &str = "order.create";
const BYBIT_OP_ORDER_AMEND: &str = "order.amend";
const BYBIT_OP_ORDER_CANCEL: &str = "order.cancel";
const BYBIT_OP_ORDER_CREATE_BATCH: &str = "order.create-batch";
const BYBIT_OP_ORDER_AMEND_BATCH: &str = "order.amend-batch";
const BYBIT_OP_ORDER_CANCEL_BATCH: &str = "order.cancel-batch";

/// Order identity context stored at submission time, used by the WS dispatch
/// task to produce proper order events without Cache access.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub venue_position_id: Option<PositionId>,
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
    Vec<Option<UUID4>>,
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

#[derive(Clone, Copy, Debug)]
struct SpotRepayFill {
    quantity: Quantity,
    base_fee: Decimal,
}

#[derive(Debug)]
struct RecentIds<T: Eq + Hash> {
    ids: DashSet<T>,
    order: Mutex<VecDeque<T>>,
}

impl<T: Eq + Hash> Default for RecentIds<T> {
    fn default() -> Self {
        Self {
            ids: DashSet::default(),
            order: Mutex::new(VecDeque::new()),
        }
    }
}

impl<T: Copy + Eq + Hash> RecentIds<T> {
    fn insert(&self, id: T) -> bool {
        let mut order = self.order.lock().unwrap_or_else(|e| e.into_inner());

        if self.ids.contains(&id) {
            return false;
        }

        if order.len() == DEDUP_CAPACITY
            && let Some(oldest) = order.pop_front()
        {
            self.ids.remove(&oldest);
        }

        order.push_back(id);
        self.ids.insert(id)
    }

    fn contains(&self, id: &T) -> bool {
        self.ids.contains(id)
    }

    fn remove(&self, id: T) {
        let mut order = self.order.lock().unwrap_or_else(|e| e.into_inner());
        self.ids.remove(&id);
        order.retain(|existing| *existing != id);
    }
}

#[derive(Debug)]
pub struct WsDispatchState {
    pub fast_execution: bool,
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    pub order_venue_ids: DashMap<ClientOrderId, VenueOrderId>,
    pub venue_order_ids: DashMap<VenueOrderId, ClientOrderId>,
    pub fee_rates: DashMap<InstrumentId, MakerTakerFeeRates>,
    pub pending_requests: DashMap<String, PendingRequestData>,
    pub order_snapshots: DashMap<ClientOrderId, OrderStateSnapshot>,
    pub emitted_accepted: DashSet<ClientOrderId>,
    pub triggered_orders: DashSet<ClientOrderId>,
    pub filled_orders: DashSet<ClientOrderId>,
    fast_execution_ids: RecentIds<TradeId>,
    fast_filled_quantities: DashMap<ClientOrderId, Quantity>,
    pub terminal_order_quantities: DashMap<ClientOrderId, Quantity>,
    untracked_order_ids: RecentIds<VenueOrderId>,
    pending_fast_executions: DashMap<VenueOrderId, Vec<BybitWsAccountExecutionFast>>,
    spot_repay_fills: DashMap<ClientOrderId, SpotRepayFill>,
    repay_tx: ArcSwapOption<tokio::sync::mpsc::UnboundedSender<RepayRequest>>,
    clearing: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            fast_execution: false,
            order_identities: DashMap::new(),
            order_venue_ids: DashMap::new(),
            venue_order_ids: DashMap::new(),
            fee_rates: DashMap::new(),
            pending_requests: DashMap::new(),
            order_snapshots: DashMap::new(),
            emitted_accepted: DashSet::default(),
            triggered_orders: DashSet::default(),
            filled_orders: DashSet::default(),
            fast_execution_ids: RecentIds::default(),
            fast_filled_quantities: DashMap::new(),
            terminal_order_quantities: DashMap::new(),
            untracked_order_ids: RecentIds::default(),
            pending_fast_executions: DashMap::new(),
            spot_repay_fills: DashMap::new(),
            repay_tx: ArcSwapOption::empty(),
            clearing: AtomicBool::new(false),
        }
    }
}

impl WsDispatchState {
    pub(crate) fn new(fast_execution: bool) -> Self {
        Self {
            fast_execution,
            ..Default::default()
        }
    }

    fn evict_if_full<T: Eq + Hash>(&self, set: &DashSet<T>) {
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

    fn insert_fast_execution(&self, trade_id: TradeId) -> bool {
        self.fast_execution_ids.insert(trade_id)
    }

    fn remove_fast_execution(&self, trade_id: TradeId) {
        self.fast_execution_ids.remove(trade_id);
    }

    fn insert_triggered(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.triggered_orders);
        self.triggered_orders.insert(cid);
    }

    pub(crate) fn set_repay_sender(&self, tx: tokio::sync::mpsc::UnboundedSender<RepayRequest>) {
        self.repay_tx.store(Some(Arc::new(tx)));
    }

    pub(crate) fn clear_repay_sender(&self) {
        self.repay_tx.store(None);
    }

    fn enqueue_repay(&self, req: RepayRequest) {
        if let Some(tx) = self.repay_tx.load_full()
            && let Err(e) = tx.send(req)
        {
            log::warn!("Failed to enqueue spot borrow repayment: {e}");
        }
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
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    clock: &AtomicTime,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
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

                dispatch_order_update(order, instrument, account_id, ts_init, state, emitter);
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
        BybitWsMessage::AccountExecutionFast(msg) => {
            let ts_init = clock.get_time_ns();

            for exec in &msg.data {
                let symbol = make_bybit_symbol(exec.symbol, exec.category);
                let Some(instrument) = instruments.get(&symbol) else {
                    log::warn!("No instrument for fast-execution update: {symbol}");
                    continue;
                };

                dispatch_execution_fill_fast(
                    exec, instrument, account_id, ts_init, true, state, emitter,
                );
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
        BybitWsMessage::OrderNotSent { request_id, reason } => {
            dispatch_order_not_sent(request_id, reason, emitter, state, clock.get_time_ns());
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
            flush_pending_fast_executions(
                instruments,
                account_id,
                clock.get_time_ns(),
                state,
                emitter,
            );
        }
        BybitWsMessage::Auth(_)
        | BybitWsMessage::Orderbook(_)
        | BybitWsMessage::Trade(_)
        | BybitWsMessage::Liquidation(_)
        | BybitWsMessage::Kline(_)
        | BybitWsMessage::TickerLinear(_)
        | BybitWsMessage::TickerOption(_) => {}
    }
}

pub(crate) fn cache_execution_fee_rates(
    state: &WsDispatchState,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    product_type: BybitProductType,
    fee_rates: Vec<BybitFeeRate>,
) -> usize {
    let mut parsed_rates = AHashMap::new();

    for fee_rate in fee_rates {
        let key = if product_type == BybitProductType::Option {
            fee_rate.base_coin
        } else {
            Some(fee_rate.symbol)
        };

        let Some(key) = key.filter(|key| !key.is_empty()) else {
            log::warn!(
                "Skipping execution fee rate with no symbol or base coin for {product_type:?}"
            );
            continue;
        };

        match parse_execution_fee_rate(&fee_rate) {
            Ok(rates) => {
                parsed_rates.insert(key, rates);
            }
            Err(e) => log::warn!("Skipping invalid execution fee rate for {key}: {e}"),
        }
    }

    let mut cached = 0;

    for instrument in instruments.values().filter(|instrument| {
        BybitProductType::from_suffix(instrument.id().symbol.as_str()) == Some(product_type)
    }) {
        let key = if product_type == BybitProductType::Option {
            instrument.base_currency().map(|currency| currency.code)
        } else {
            Some(Ustr::from(extract_raw_symbol(
                instrument.id().symbol.as_str(),
            )))
        };

        let Some(rates) = key.and_then(|key| parsed_rates.get(&key)) else {
            continue;
        };

        state.fee_rates.insert(instrument.id(), *rates);
        cached += 1;
    }

    cached
}

fn parse_execution_fee_rate(fee_rate: &BybitFeeRate) -> anyhow::Result<MakerTakerFeeRates> {
    let maker = fee_rate
        .maker_fee_rate
        .parse::<Decimal>()
        .with_context(|| format!("invalid makerFeeRate '{}'", fee_rate.maker_fee_rate))?;
    let taker = fee_rate
        .taker_fee_rate
        .parse::<Decimal>()
        .with_context(|| format!("invalid takerFeeRate '{}'", fee_rate.taker_fee_rate))?;
    Ok(MakerTakerFeeRates::new(maker, taker))
}

/// Dispatches a single order status update.
///
/// Tracked orders produce lifecycle events (OrderAccepted, OrderTriggered,
/// OrderCanceled, OrderRejected). Untracked orders fall back to
/// `OrderStatusReport` for reconciliation.
fn dispatch_order_update(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let client_order_id = if order.order_link_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(order.order_link_id))
    };

    let identity = client_order_id
        .as_ref()
        .and_then(|cid| state.order_identities.get(cid).map(|r| r.clone()));
    let venue_order_id = VenueOrderId::new(order.order_id);
    if let (Some(client_order_id), Some(identity)) = (client_order_id, identity) {
        record_venue_order_id(client_order_id, venue_order_id, state);
        drain_pending_fast_executions(
            venue_order_id,
            instrument,
            account_id,
            ts_init,
            state,
            emitter,
        );

        match order.order_status {
            BybitOrderStatus::Created | BybitOrderStatus::New | BybitOrderStatus::Untriggered => {
                dispatch_order_created(
                    order,
                    instrument,
                    client_order_id,
                    &identity,
                    account_id,
                    ts_init,
                    state,
                    emitter,
                );
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
                    ts_init,
                    state,
                    emitter,
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
                dispatch_order_rejected(
                    order,
                    instrument,
                    client_order_id,
                    &identity,
                    account_id,
                    ts_init,
                    state,
                    emitter,
                );
            }
            BybitOrderStatus::PartiallyFilled => {
                // Fills arrive on the execution channel; no event needed here.
                // Ensure accepted was emitted so the fill has a valid prior state.
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    ts_init,
                    state,
                    emitter,
                );

                // A successful amend on a partially filled order keeps the
                // PartiallyFilled status. Detect price/qty/trigger changes and
                // emit OrderUpdated so PendingUpdate resolves.
                emit_order_updated_if_changed(
                    order,
                    instrument,
                    client_order_id,
                    &identity,
                    account_id,
                    ts_init,
                    false,
                    state,
                    emitter,
                );
            }
            BybitOrderStatus::Filled => {
                dispatch_order_filled(
                    order,
                    instrument,
                    client_order_id,
                    &identity,
                    account_id,
                    ts_init,
                    state,
                    emitter,
                );
            }
            BybitOrderStatus::Canceled
            | BybitOrderStatus::PartiallyFilledCanceled
            | BybitOrderStatus::Deactivated => {
                dispatch_order_canceled(
                    order,
                    instrument,
                    client_order_id,
                    &identity,
                    account_id,
                    ts_init,
                    state,
                    emitter,
                );
            }
        }
    } else {
        state.untracked_order_ids.insert(venue_order_id);
        drain_pending_fast_executions(
            venue_order_id,
            instrument,
            account_id,
            ts_init,
            state,
            emitter,
        );

        // Untracked order: fall back to report for reconciliation
        match super::parse::parse_ws_order_status_report(order, instrument, account_id, ts_init) {
            Ok(report) => emitter.send_order_status_report(report),
            Err(e) => log::error!("Failed to parse order status report: {e}"),
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "status handling shares the dispatcher's order context"
)]
fn dispatch_order_created(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let snapshot = parse_order_snapshot(order, instrument);

    if state.emitted_accepted.contains(&client_order_id)
        || state.filled_orders.contains(&client_order_id)
        || state.triggered_orders.contains(&client_order_id)
    {
        if emit_order_updated_if_changed(
            order,
            instrument,
            client_order_id,
            identity,
            account_id,
            ts_init,
            false,
            state,
            emitter,
        ) {
            return;
        }

        log::debug!("Skipping duplicate Accepted for {client_order_id}");
        return;
    }

    state.insert_accepted(client_order_id);

    // BBO orders resolve their limit price venue-side: emit
    // OrderUpdated after OrderAccepted when the seed diverges.
    let venue_differs_from_submitted = snapshot
        .as_ref()
        .is_some_and(|s| is_snapshot_updated(s, &client_order_id, state));
    if let Some(snapshot) = snapshot.as_ref() {
        state
            .order_snapshots
            .insert(client_order_id, snapshot.clone());
    }

    let venue_order_id = VenueOrderId::new(order.order_id);

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

    if venue_differs_from_submitted {
        emit_order_updated_if_changed(
            order,
            instrument,
            client_order_id,
            identity,
            account_id,
            ts_init,
            true,
            state,
            emitter,
        );
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "status handling shares the dispatcher's order context"
)]
fn dispatch_order_rejected(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let filled_qty = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    )
    .unwrap_or_default();
    let venue_order_id = VenueOrderId::new(order.order_id);

    if filled_qty.is_positive() {
        // Partially filled then rejected - treat as canceled
        ensure_accepted_emitted(
            client_order_id,
            account_id,
            venue_order_id,
            identity,
            ts_init,
            state,
            emitter,
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
            None,
        );
        cleanup_terminal_when_ready(client_order_id, filled_qty, state);
        emitter.send_order_event(OrderEventAny::Canceled(canceled));
    } else {
        let reason = if order.reject_reason.is_empty() {
            Ustr::from("Order rejected by venue")
        } else {
            order.reject_reason
        };

        cleanup_terminal_when_ready(client_order_id, filled_qty, state);

        emitter.emit_order_rejected_event(
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            reason.as_str(),
            ts_init,
            bybit_rejection_due_post_only(reason.as_str()),
        );
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "status handling shares the dispatcher's order context"
)]
fn dispatch_order_filled(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    // Fills arrive on the execution channel; no event needed here.
    // Ensure accepted was emitted so the fill has a valid prior state.
    let venue_order_id = VenueOrderId::new(order.order_id);
    ensure_accepted_emitted(
        client_order_id,
        account_id,
        venue_order_id,
        identity,
        ts_init,
        state,
        emitter,
    );

    // Reconcile seed against venue values before the fill (BBO
    // orders may land directly on Filled).
    emit_order_updated_if_changed(
        order,
        instrument,
        client_order_id,
        identity,
        account_id,
        ts_init,
        false,
        state,
        emitter,
    );

    if let Ok(terminal_qty) = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    ) {
        state
            .terminal_order_quantities
            .insert(client_order_id, terminal_qty);
        if state.fast_execution && fast_fill_is_terminal(client_order_id, state) {
            cleanup_terminal(client_order_id, state);
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "status handling shares the dispatcher's order context"
)]
fn dispatch_order_canceled(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let filled_qty = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    )
    .unwrap_or_default();

    // Bybit reports a post-only order that would take liquidity as
    // Cancelled with rejectReason=EC_PostOnlyWillTakeLiquidity,
    // not Rejected. Surface it as OrderRejected carrying due_post_only.
    if filled_qty.is_zero() && bybit_rejection_due_post_only(order.reject_reason.as_str()) {
        cleanup_terminal_when_ready(client_order_id, filled_qty, state);
        emitter.emit_order_rejected_event(
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            order.reject_reason.as_str(),
            ts_init,
            true,
        );
    } else {
        let venue_order_id = VenueOrderId::new(order.order_id);
        ensure_accepted_emitted(
            client_order_id,
            account_id,
            venue_order_id,
            identity,
            ts_init,
            state,
            emitter,
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
            None,
        );
        cleanup_terminal_when_ready(client_order_id, filled_qty, state);
        emitter.send_order_event(OrderEventAny::Canceled(canceled));
    }
}

pub(crate) fn has_pending_fast_execution(
    venue_order_id: VenueOrderId,
    state: &WsDispatchState,
) -> bool {
    state.pending_fast_executions.contains_key(&venue_order_id)
}

pub(crate) fn drain_pending_fast_executions(
    venue_order_id: VenueOrderId,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let Some((_, executions)) = state.pending_fast_executions.remove(&venue_order_id) else {
        return;
    };

    for exec in executions {
        dispatch_execution_fill_fast(
            &exec, instrument, account_id, ts_init, false, state, emitter,
        );
    }
}

pub(crate) fn flush_pending_fast_executions(
    instruments: &AHashMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let venue_order_ids = state
        .pending_fast_executions
        .iter()
        .map(|entry| *entry.key())
        .collect::<Vec<_>>();

    for venue_order_id in venue_order_ids {
        let Some(execution) = state
            .pending_fast_executions
            .get(&venue_order_id)
            .and_then(|executions| executions.first().cloned())
        else {
            continue;
        };

        let symbol = make_bybit_symbol(execution.symbol, execution.category);

        let Some(instrument) = instruments.get(&symbol) else {
            log::warn!("Cannot flush fast execution without instrument: {symbol}");
            continue;
        };

        drain_pending_fast_executions(
            venue_order_id,
            instrument,
            account_id,
            ts_init,
            state,
            emitter,
        );
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
    if exec.exec_type == BybitExecType::Funding {
        log::debug!(
            "Skipping funding execution: symbol={}, order_id={}, exec_id={}",
            exec.symbol,
            exec.order_id,
            exec.exec_id,
        );
        return;
    }

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
        Some(ClientOrderId::new(exec.order_link_id))
    };

    let identity = client_order_id
        .as_ref()
        .and_then(|cid| state.order_identities.get(cid).map(|r| r.clone()));

    if let (Some(client_order_id), Some(identity)) = (client_order_id, identity) {
        match parse_order_filled(exec, instrument, &identity, emitter, account_id, ts_init) {
            Ok(filled) => {
                let venue_order_id = filled.venue_order_id;
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    ts_init,
                    state,
                    emitter,
                );

                let is_spot_buy =
                    exec.category == BybitProductType::Spot && exec.side == BybitOrderSide::Buy;

                if is_spot_buy {
                    record_spot_repay_fill(client_order_id, &filled, instrument, state);
                }

                state.insert_filled(client_order_id);
                state.triggered_orders.remove(&client_order_id);
                emitter.send_order_event(OrderEventAny::Filled(filled));

                if exec.leaves_qty == "0" {
                    if is_spot_buy {
                        enqueue_spot_repay(client_order_id, instrument, state);
                    }

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

/// Dispatches a single fast-execution (fill) message.
///
/// The fast channel lacks fee and exec-type fields, so all fills route to
/// [`FillReport`]. Commission is derived from cached account fee rates when available,
/// with a zero fallback. Liquidity side is derived from the payload's `isMaker` flag.
/// Subscribe to the standard `execution` channel for venue-authoritative fees.
fn dispatch_execution_fill_fast(
    exec: &BybitWsAccountExecutionFast,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
    defer_untracked: bool,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let Ok(trade_id) = TradeId::new_checked(exec.exec_id.as_str()) else {
        log::error!(
            "Invalid execId in Bybit WebSocket fast-execution payload: {}",
            exec.exec_id
        );
        return;
    };

    if defer_untracked && !state.insert_fast_execution(trade_id) {
        log::debug!("Skipping duplicate fast execution: {trade_id}");
        return;
    }

    let venue_order_id = VenueOrderId::new(exec.order_id);
    let client_order_id = if exec.order_link_id.is_empty() {
        let venue_mapping = state.venue_order_ids.entry(venue_order_id);

        let client_order_id = match &venue_mapping {
            Entry::Occupied(entry) => Some(*entry.get()),
            Entry::Vacant(_) => None,
        };

        // Hold the mapping guard through any pending enqueue so REST correlation cannot interleave
        if client_order_id.is_none()
            && defer_untracked
            && !state.untracked_order_ids.contains(&venue_order_id)
        {
            state
                .pending_fast_executions
                .entry(venue_order_id)
                .or_default()
                .push(exec.clone());
            return;
        }

        client_order_id
    } else {
        Some(ClientOrderId::new(exec.order_link_id))
    };

    let identity = client_order_id
        .as_ref()
        .and_then(|cid| state.order_identities.get(cid).map(|r| r.clone()));

    let venue_position_id =
        if let (Some(client_order_id), Some(identity)) = (client_order_id, identity.as_ref()) {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                ts_init,
                state,
                emitter,
            );
            identity.venue_position_id
        } else {
            None
        };

    let fee_rates = state.fee_rates.get(&instrument.id()).map(|rates| *rates);

    match parse_ws_fill_report_fast(
        exec,
        account_id,
        instrument,
        fee_rates,
        client_order_id,
        venue_position_id,
        ts_init,
    ) {
        Ok(report) => {
            let last_qty = report.last_qty;
            emitter.send_fill_report(report);

            if let Some(client_order_id) = client_order_id
                && identity.is_some()
            {
                state.insert_filled(client_order_id);
                record_fast_fill(client_order_id, last_qty, state);
            }
        }
        Err(e) => {
            state.remove_fast_execution(trade_id);
            log::error!("Failed to parse fast fill report: {e}");
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
    let client_order_id = ClientOrderId::new(exec.order_link_id);
    let venue_order_id = VenueOrderId::new(exec.order_id);
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
    let price_currency = instrument.quote_currency();
    let commission_currency = get_currency(&exec.fee_currency);

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
        price_currency,
        liquidity_side,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        identity.venue_position_id,
        Some(commission),
        None,
    ))
}

/// Handles a Bybit WS order command response.
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

        if let Some((cids, voids, pending_op, command_ids)) = pending {
            let batch_errors = resp.extract_batch_errors();
            let data_array = resp.data.as_array();

            for (idx, error) in batch_errors.iter().enumerate() {
                if error.code == 0 {
                    continue;
                }

                if is_bybit_ambiguous_order_error_code(error.code) {
                    log::warn!(
                        "Ambiguous batch order item failure at index {idx}: code={}, msg={}; awaiting reconciliation",
                        error.code,
                        error.msg,
                    );
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
                    command_ids.get(idx).copied().flatten(),
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

    let is_batch_response = is_batch_order_op(resp.op.as_str())
        || pending
            .as_ref()
            .is_some_and(|(cids, _, _, _)| cids.len() > 1);

    if is_batch_response {
        if is_bybit_ambiguous_order_error_code(resp.ret_code) {
            let order_count = pending.as_ref().map_or(0, |(cids, _, _, _)| cids.len());
            log::warn!(
                "Ambiguous batch order response failure for {order_count} orders: op={}, ret_code={}, ret_msg={}; awaiting reconciliation",
                resp.op,
                resp.ret_code,
                resp.ret_msg,
            );
            return;
        }

        let Some((client_order_ids, venue_order_ids, pending_op, command_ids)) = pending else {
            log::warn!(
                "Batch order response error without correlation: op={}, ret_code={}, ret_msg={}, req_id={:?}",
                resp.op,
                resp.ret_code,
                resp.ret_msg,
                resp.req_id,
            );
            return;
        };

        for (index, client_order_id) in client_order_ids.into_iter().enumerate() {
            let Some(identity) = state
                .order_identities
                .get(&client_order_id)
                .map(|identity| identity.clone())
            else {
                log::warn!(
                    "Batch order response error for untracked order: op={}, client_order_id={client_order_id}, ret_msg={}",
                    resp.op,
                    resp.ret_msg,
                );
                continue;
            };

            emit_rejection_for_op(
                &pending_op,
                client_order_id,
                &identity,
                venue_order_ids.get(index).copied().flatten(),
                &resp.ret_msg,
                emitter,
                state,
                ts_init,
                command_ids.get(index).copied().flatten(),
            );
        }

        return;
    }

    if is_bybit_ambiguous_order_error_code(resp.ret_code) {
        log::warn!(
            "Ambiguous order response failure: op={}, ret_code={}, ret_msg={}; awaiting reconciliation",
            resp.op,
            resp.ret_code,
            resp.ret_msg,
        );
        return;
    }

    let effective_op = pending
        .as_ref()
        .map(|(_, _, op, _)| *op)
        .or_else(|| pending_op_from_str(resp.op.as_str()))
        .unwrap_or_else(|| {
            log::warn!("Unknown order operation '{}', defaulting to Place", resp.op);
            PendingOperation::Place
        });

    // Single-order rejection path
    let client_order_id = extract_order_link_id_from_data(&resp.data).or_else(|| {
        pending
            .as_ref()
            .and_then(|(cids, _, _, _)| cids.first().copied())
    });

    let stored_venue_order_id = pending
        .as_ref()
        .and_then(|(_, voids, _, _)| voids.first().and_then(|v| *v));

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
        pending.as_ref().and_then(|(cids, _, _, command_ids)| {
            cids.iter()
                .position(|id| *id == client_order_id)
                .and_then(|index| command_ids.get(index).copied().flatten())
        }),
    );
}

fn dispatch_order_not_sent(
    request_id: &str,
    reason: &str,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    let Some((_, (client_order_ids, venue_order_ids, operation, command_ids))) =
        state.pending_requests.remove(request_id)
    else {
        log::warn!("Unsent order command has no request correlation: {request_id}");
        return;
    };

    for (index, client_order_id) in client_order_ids.into_iter().enumerate() {
        let Some(identity) = state
            .order_identities
            .get(&client_order_id)
            .map(|entry| entry.clone())
        else {
            continue;
        };

        let venue_order_id = venue_order_ids.get(index).copied().flatten();
        let causation_id = command_ids.get(index).copied().flatten();

        match operation {
            PendingOperation::Place => emit_rejection_for_op(
                &operation,
                client_order_id,
                &identity,
                venue_order_id,
                reason,
                emitter,
                state,
                ts_init,
                causation_id,
            ),
            PendingOperation::Amend => emitter.emit_order_modify_denied_event(
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                venue_order_id,
                reason,
                ts_init,
                causation_id,
            ),
            PendingOperation::Cancel => emitter.emit_order_cancel_denied_event(
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                venue_order_id,
                reason,
                ts_init,
                causation_id,
            ),
        }
    }
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
    causation_id: Option<UUID4>,
) {
    match pending_op {
        PendingOperation::Place => {
            state.order_identities.remove(&client_order_id);
            state.order_snapshots.remove(&client_order_id);
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
                causation_id,
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
                causation_id,
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

fn is_batch_order_op(op: &str) -> bool {
    matches!(
        op,
        BYBIT_OP_ORDER_CREATE_BATCH | BYBIT_OP_ORDER_AMEND_BATCH | BYBIT_OP_ORDER_CANCEL_BATCH
    )
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

#[expect(
    clippy::too_many_arguments,
    reason = "updates share the dispatcher's order context"
)]
fn emit_order_updated_if_changed(
    order: &BybitWsAccountOrder,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    ts_init: UnixNanos,
    force: bool,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) -> bool {
    let Some(snapshot) = parse_order_snapshot(order, instrument) else {
        return false;
    };

    if !force && !is_snapshot_updated(&snapshot, &client_order_id, state) {
        return false;
    }

    let venue_order_id = VenueOrderId::new(order.order_id);

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
    true
}

pub(crate) fn record_venue_order_id(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    state: &WsDispatchState,
) {
    state
        .order_venue_ids
        .insert(client_order_id, venue_order_id);
    state
        .venue_order_ids
        .insert(venue_order_id, client_order_id);
}

/// Synthesizes and emits `OrderAccepted` if one has not yet been emitted for
/// this order. Handles fast-filling orders that skip the `New` state on Bybit.
fn ensure_accepted_emitted(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    ts_init: UnixNanos,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    record_venue_order_id(client_order_id, venue_order_id, state);
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

pub(crate) fn cleanup_terminal_when_ready(
    client_order_id: ClientOrderId,
    terminal_qty: Quantity,
    state: &WsDispatchState,
) {
    state
        .terminal_order_quantities
        .insert(client_order_id, terminal_qty);
    if state.fast_execution && !fast_fill_is_terminal(client_order_id, state) {
        return;
    }

    cleanup_terminal(client_order_id, state);
}

fn record_fast_fill(client_order_id: ClientOrderId, last_qty: Quantity, state: &WsDispatchState) {
    let mut overflow = false;
    state
        .fast_filled_quantities
        .entry(client_order_id)
        .and_modify(|total| {
            if let Some(new_total) = total.checked_add(last_qty) {
                *total = new_total;
            } else {
                overflow = true;
            }
        })
        .or_insert(last_qty);

    if overflow {
        log::warn!("Fast fill quantity overflow for {client_order_id}, retaining order state");
        return;
    }

    if fast_fill_is_terminal(client_order_id, state) {
        cleanup_terminal(client_order_id, state);
    }
}

fn fast_fill_is_terminal(client_order_id: ClientOrderId, state: &WsDispatchState) -> bool {
    let Some(terminal_qty) = state.terminal_order_quantities.get(&client_order_id) else {
        return false;
    };

    if terminal_qty.is_zero() {
        return true;
    }

    state
        .fast_filled_quantities
        .get(&client_order_id)
        .is_some_and(|filled_qty| *filled_qty >= *terminal_qty)
}

/// Removes a terminal order from all tracking sets.
pub(crate) fn cleanup_terminal(client_order_id: ClientOrderId, state: &WsDispatchState) {
    if let Some((_, venue_order_id)) = state.order_venue_ids.remove(&client_order_id) {
        state.venue_order_ids.remove(&venue_order_id);
    }

    state.order_identities.remove(&client_order_id);
    state.order_snapshots.remove(&client_order_id);
    state.emitted_accepted.remove(&client_order_id);
    state.triggered_orders.remove(&client_order_id);
    state.filled_orders.remove(&client_order_id);
    state.fast_filled_quantities.remove(&client_order_id);
    state.terminal_order_quantities.remove(&client_order_id);
    state.spot_repay_fills.remove(&client_order_id);
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

fn record_spot_repay_fill(
    client_order_id: ClientOrderId,
    filled: &OrderFilled,
    instrument: &InstrumentAny,
    state: &WsDispatchState,
) {
    let Some(base_currency) = instrument.base_currency() else {
        return;
    };

    let base_fee = filled
        .commission
        .filter(|fee| fee.currency.code == base_currency.code)
        .map_or(Decimal::ZERO, |fee| fee.as_decimal().max(Decimal::ZERO));

    let fill = SpotRepayFill {
        quantity: filled.last_qty,
        base_fee,
    };

    state
        .spot_repay_fills
        .entry(client_order_id)
        .and_modify(|total| {
            total.quantity = total.quantity + fill.quantity;
            total.base_fee += fill.base_fee;
        })
        .or_insert(fill);
}

/// Enqueues an auto-repay for a fully-filled SPOT BUY.
fn enqueue_spot_repay(
    client_order_id: ClientOrderId,
    instrument: &InstrumentAny,
    state: &WsDispatchState,
) {
    let Some(base_currency) = instrument.base_currency() else {
        return;
    };

    let Some((_, fill)) = state.spot_repay_fills.remove(&client_order_id) else {
        return;
    };

    state.enqueue_repay(RepayRequest {
        coin: base_currency.code,
        quantity: fill.quantity,
        base_fee: fill.base_fee,
        repayment_precision: fill.quantity.precision.max(base_currency.precision),
    });
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use nautilus_common::messages::{ExecutionEvent, execution::ExecutionReport};
    use nautilus_core::{
        UnixNanos,
        time::{AtomicTime, get_atomic_clock_realtime},
    };
    use nautilus_live::{emitter::ExecutionEventEmitter, testing::ExecutionHarness};
    use nautilus_model::{
        accounts::MarginAccount,
        enums::{AccountType, OrderSide, OrderStatus, OrderType},
        events::OrderEventAny,
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId, VenueOrderId,
        },
        instruments::{Instrument, InstrumentAny},
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        reports::FillReport,
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            enums::{BybitExecType, BybitOrderSide, BybitProductType},
            parse::{
                parse_inverse_instrument, parse_linear_instrument, parse_option_instrument,
                parse_spot_instrument,
            },
            testing::load_test_json,
        },
        http::models::{
            BybitInstrumentInverseResponse, BybitInstrumentLinearResponse,
            BybitInstrumentOptionResponse, BybitInstrumentSpotResponse,
        },
        websocket::messages::{
            BybitWsAccountExecutionFastMsg, BybitWsMessage, BybitWsOrderResponse,
        },
    };

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_linear_instrument(instrument, ts, ts).unwrap()
    }

    fn spot_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_spot.json");
        let response: BybitInstrumentSpotResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_spot_instrument(instrument, ts, ts).unwrap()
    }

    fn inverse_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_inverse.json");
        let response: BybitInstrumentInverseResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_inverse_instrument(instrument, ts, ts).unwrap()
    }

    fn option_instrument_with_distinct_settlement() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_option_symbol_id.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_option_instrument(instrument, ts, ts).unwrap()
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
            venue_position_id: None,
        }
    }

    #[rstest]
    #[case::base_fee("BTC", "0.0000015", "0.0000025", "0.000004")]
    #[case::base_rebate("BTC", "-0.0000015", "-0.0000025", "0")]
    #[case::quote_fee("USDT", "0.075", "0.125", "0")]
    fn test_spot_repay_uses_accumulated_execution_quantity(
        #[case] fee_currency: &str,
        #[case] first_fee: &str,
        #[case] second_fee: &str,
        #[case] expected_base_fee: &str,
    ) {
        let instrument = spot_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, _rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();
        let (repay_tx, mut repay_rx) = tokio::sync::mpsc::unbounded_channel();
        state.set_repay_sender(repay_tx);

        let json = load_test_json("ws_account_execution.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["category"] = serde_json::Value::String("spot".to_string());
        value["data"][0]["symbol"] = serde_json::Value::String("BTCUSDT".to_string());
        value["data"][0]["side"] = serde_json::Value::String("Buy".to_string());
        value["data"][0]["orderType"] = serde_json::Value::String("Market".to_string());
        value["data"][0]["orderQty"] = serde_json::Value::String("100".to_string());
        value["data"][0]["execQty"] = serde_json::Value::String("0.0015".to_string());
        value["data"][0]["leavesQty"] = serde_json::Value::String("0.0025".to_string());
        value["data"][0]["execFee"] = serde_json::Value::String(first_fee.to_string());
        value["data"][0]["feeCurrency"] = serde_json::Value::String(fee_currency.to_string());

        let first: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_value(value.clone()).unwrap();
        let client_order_id = ClientOrderId::new(first.data[0].order_link_id);
        state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id: InstrumentId::from("BTCUSDT-SPOT.BYBIT"),
                order_type: OrderType::Market,
                ..default_identity()
            },
        );

        dispatch_ws_message(
            &BybitWsMessage::AccountExecution(first),
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        assert!(repay_rx.try_recv().is_err());

        value["data"][0]["execId"] = serde_json::Value::String("second-execution".to_string());
        value["data"][0]["execQty"] = serde_json::Value::String("0.0025".to_string());
        value["data"][0]["leavesQty"] = serde_json::Value::String("0".to_string());
        value["data"][0]["execFee"] = serde_json::Value::String(second_fee.to_string());
        let second: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_value(value).unwrap();

        dispatch_ws_message(
            &BybitWsMessage::AccountExecution(second),
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        let repay = repay_rx.try_recv().expect("expected a repay request");
        assert_eq!(repay.coin, "BTC");
        assert_eq!(repay.quantity, Quantity::from("0.0040"));
        assert_eq!(
            repay.base_fee,
            expected_base_fee.parse::<Decimal>().unwrap()
        );
        assert_eq!(repay.repayment_precision, 8);
        assert!(repay_rx.try_recv().is_err());
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
            let cid = ClientOrderId::new(order.order_link_id);
            state.order_identities.insert(cid, default_identity());
        }

        let ws_msg = BybitWsMessage::AccountOrder(msg);
        dispatch_ws_message(
            &ws_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
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
    fn test_dispatch_tracked_post_only_cancel_emits_rejected() {
        const BYBIT_POST_ONLY_REJECT_REASON: &str = "EC_PostOnlyWillTakeLiquidity";

        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        // Bybit reports a post-only order that would take liquidity as
        // orderStatus=Cancelled with rejectReason=EC_PostOnlyWillTakeLiquidity.
        let json = load_test_json("ws_account_order.json");
        let mut msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_str(&json).unwrap();

        let order = msg.data.first_mut().expect("fixture has an order");
        order.reject_reason = Ustr::from(BYBIT_POST_ONLY_REJECT_REASON);
        order.cum_exec_qty = "0".to_string();
        let cid = ClientOrderId::new(order.order_link_id);
        state.order_identities.insert(cid, default_identity());

        let ws_msg = BybitWsMessage::AccountOrder(msg);
        dispatch_ws_message(
            &ws_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event else {
            panic!("Expected Rejected, found {event:?}");
        };

        assert!(rejected.due_post_only);
        assert_eq!(rejected.reason, BYBIT_POST_ONLY_REJECT_REASON);
        assert_eq!(rejected.client_order_id, cid);
        assert!(rx.try_recv().is_err(), "expected only a single event");
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
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
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
            let cid = ClientOrderId::new(exec.order_link_id);
            state.order_identities.insert(cid, default_identity());
        }

        let ws_msg = BybitWsMessage::AccountExecution(msg);
        dispatch_ws_message(
            &ws_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
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
    #[case::commission_overflow("execFee", "9999999999999999999999999999")]
    #[case::commission_parse("execFee", "invalid")]
    #[case::qty("execQty", "invalid")]
    #[case::price("execPrice", "invalid")]
    #[case::timestamp("execTime", "invalid")]
    #[case::trade_id("execId", "")]
    fn test_dispatch_invalid_execution_preserves_state(
        #[case] field: &str,
        #[case] value: &str,
        #[values(false, true)] spot: bool,
    ) {
        let instrument = if spot {
            spot_instrument()
        } else {
            linear_instrument()
        };

        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let state = WsDispatchState::default();
        let (repay_tx, mut repay_rx) = tokio::sync::mpsc::unbounded_channel();
        state.set_repay_sender(repay_tx);
        let mut payload: serde_json::Value =
            serde_json::from_str(&load_test_json("ws_account_execution.json")).unwrap();
        payload["data"][0][field] = serde_json::Value::String(value.to_string());
        payload["data"][0]["side"] = serde_json::Value::String("Buy".to_string());
        if spot {
            payload["data"][0]["category"] = serde_json::Value::String("spot".to_string());
        }

        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_value(payload).unwrap();
        let cid = ClientOrderId::new(msg.data[0].order_link_id);

        let identity = OrderIdentity {
            instrument_id: instrument.id(),
            venue_position_id: Some(PositionId::from("P-001")),
            ..default_identity()
        };

        state.order_identities.insert(cid, identity.clone());
        state.order_snapshots.insert(
            cid,
            OrderStateSnapshot {
                quantity: Quantity::from("0.500"),
                price: Some(Price::from("95900.1")),
                trigger_price: Some(Price::from("95800.2")),
            },
        );

        state.insert_triggered(cid);

        dispatch_ws_message(
            &BybitWsMessage::AccountExecution(msg),
            test_account_id(),
            &instruments,
            get_atomic_clock_realtime(),
            &state,
            &emitter,
        );

        assert!(rx.try_recv().is_err());
        assert!(repay_rx.try_recv().is_err());
        assert!(state.emitted_accepted.is_empty());
        assert!(state.filled_orders.is_empty());
        assert!(state.spot_repay_fills.is_empty());
        assert!(state.pending_requests.is_empty());
        assert_eq!(state.triggered_orders.len(), 1);
        assert!(state.triggered_orders.contains(&cid));
        assert_eq!(state.order_identities.len(), 1);
        let retained = state.order_identities.get(&cid).unwrap();
        assert_eq!(retained.instrument_id, identity.instrument_id);
        assert_eq!(retained.strategy_id, identity.strategy_id);
        assert_eq!(retained.order_side, identity.order_side);
        assert_eq!(retained.order_type, identity.order_type);
        assert_eq!(retained.venue_position_id, identity.venue_position_id);
        assert_eq!(state.order_snapshots.len(), 1);
        let snapshot = state.order_snapshots.get(&cid).unwrap();
        assert_eq!(snapshot.quantity, Quantity::from("0.500"));
        assert_eq!(snapshot.price, Some(Price::from("95900.1")));
        assert_eq!(snapshot.trigger_price, Some(Price::from("95800.2")));
    }

    #[rstest]
    #[case::partial("0.5", "1.000", OrderStatus::PartiallyFilled)]
    #[case::terminal("0", "0.500", OrderStatus::Filled)]
    fn test_dispatch_execution_commission_failure_recovery_and_replay(
        #[case] leaves_qty: &str,
        #[case] order_qty: &str,
        #[case] expected_status: OrderStatus,
    ) {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let state = WsDispatchState::default();
        let clock = AtomicTime::new(false, UnixNanos::new(1_746_270_400_400_000_000));
        let mut msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&load_test_json("ws_account_execution.json")).unwrap();
        msg.data[0].leaves_qty = leaves_qty.to_string();
        msg.data[0].side = BybitOrderSide::Buy;
        msg.data[0].exec_fee = "9999999999999999999999999999".to_string();
        let cid = ClientOrderId::new(msg.data[0].order_link_id);
        let identity = default_identity();
        state.order_identities.insert(cid, identity.clone());

        let harness = ExecutionHarness::new(
            emitter.trader_id(),
            *crate::common::consts::BYBIT_CLIENT_ID,
            test_account_id(),
            instrument.clone(),
        );
        let wallet: crate::websocket::messages::BybitWsAccountWalletMsg =
            serde_json::from_str(&load_test_json("ws_account_wallet.json")).unwrap();
        let account_state = parse_ws_account_state(
            &wallet.data[0],
            test_account_id(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let account = MarginAccount::new(account_state, false);
        harness
            .cache()
            .borrow_mut()
            .add_account(account.into())
            .unwrap();
        let mut order = OrderTestBuilder::new(identity.order_type)
            .trader_id(emitter.trader_id())
            .strategy_id(identity.strategy_id)
            .instrument_id(instrument.id())
            .client_order_id(cid)
            .side(identity.order_side)
            .quantity(Quantity::from(order_qty))
            .price(Price::from("94942.5"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
        order.apply(submitted).unwrap();
        harness
            .cache()
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        dispatch_ws_message(
            &BybitWsMessage::AccountExecution(msg.clone()),
            test_account_id(),
            &instruments,
            &clock,
            &state,
            &emitter,
        );

        assert!(rx.try_recv().is_err());
        assert!(state.emitted_accepted.is_empty());
        assert!(state.filled_orders.is_empty());
        assert_eq!(state.order_identities.len(), 1);

        msg.data[0].exec_fee = "0.01234567".to_string();
        let message = BybitWsMessage::AccountExecution(msg);
        dispatch_ws_message(
            &message,
            test_account_id(),
            &instruments,
            &clock,
            &state,
            &emitter,
        );

        let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = rx.try_recv().unwrap()
        else {
            panic!("expected Accepted before Filled");
        };

        let expected_accepted = OrderAccepted::new(
            emitter.trader_id(),
            identity.strategy_id,
            instrument.id(),
            cid,
            VenueOrderId::from("9aac161b-8ed6-450d-9cab-c5cc67c21784"),
            test_account_id(),
            accepted.event_id,
            clock.get_time_ns(),
            clock.get_time_ns(),
            false,
        );
        assert_eq!(accepted, expected_accepted);
        harness
            .exec_engine()
            .borrow_mut()
            .process(&OrderEventAny::Accepted(accepted));

        let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = rx.try_recv().unwrap() else {
            panic!("expected Filled after Accepted");
        };

        let commission = Money::from("0.01234567 USDT");

        let expected_filled = OrderFilled::new(
            emitter.trader_id(),
            identity.strategy_id,
            instrument.id(),
            cid,
            VenueOrderId::from("9aac161b-8ed6-450d-9cab-c5cc67c21784"),
            test_account_id(),
            TradeId::from("0ab1bdf7-4219-438b-b30a-32ec863018f7"),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.500"),
            Price::from("95900.1"),
            Currency::USDT(),
            LiquiditySide::Taker,
            filled.event_id,
            UnixNanos::new(1_746_270_400_353_000_000),
            clock.get_time_ns(),
            false,
            None,
            Some(commission),
            None,
        );
        assert_eq!(filled, expected_filled);
        assert!(rx.try_recv().is_err());
        harness
            .exec_engine()
            .borrow_mut()
            .process(&OrderEventAny::Filled(filled.clone()));
        let applied = harness.cache().borrow().order(&cid).unwrap().clone();
        assert_eq!(applied.status(), expected_status);
        assert_eq!(applied.filled_qty(), Quantity::from("0.500"));
        assert_eq!(
            applied.commissions().get(&Currency::USDT()),
            Some(&commission)
        );
        assert_eq!(applied.trade_ids(), vec![&filled.trade_id]);
        let position_id = *harness.cache().borrow().position_id(&cid).unwrap();
        let position = harness
            .cache()
            .borrow()
            .position(&position_id)
            .unwrap()
            .clone();
        assert_eq!(position.quantity, Quantity::from("0.500"));
        assert_eq!(position.commissions(), vec![commission]);
        let mut expected_position_fill = filled.clone();
        expected_position_fill.position_id = Some(position_id);
        assert_eq!(position.events, vec![expected_position_fill]);

        dispatch_ws_message(
            &message,
            test_account_id(),
            &instruments,
            &clock,
            &state,
            &emitter,
        );
        let replay = rx.try_recv().unwrap();
        match replay {
            ExecutionEvent::Order(OrderEventAny::Filled(replayed)) => {
                let expected_replay = OrderFilled {
                    event_id: replayed.event_id,
                    ..filled.clone()
                };

                assert_eq!(replayed, expected_replay);
                assert_eq!(leaves_qty, "0.5");
                harness
                    .exec_engine()
                    .borrow_mut()
                    .process(&OrderEventAny::Filled(replayed));
            }
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                let expected_report = FillReport::new(
                    filled.account_id,
                    filled.instrument_id,
                    filled.venue_order_id,
                    filled.trade_id,
                    filled.order_side,
                    filled.last_qty,
                    filled.last_px,
                    commission,
                    filled.liquidity_side,
                    Some(cid),
                    None,
                    filled.ts_event,
                    filled.ts_init,
                    Some(report.report_id),
                );
                assert_eq!(*report, expected_report);
                assert_eq!(leaves_qty, "0");
                harness
                    .exec_engine()
                    .borrow_mut()
                    .reconcile_fill_report(&report);
            }
            other => panic!("unexpected replay: {other:?}"),
        }

        harness
            .exec_engine()
            .borrow_mut()
            .process(&OrderEventAny::Filled(filled));

        assert!(rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value(harness.cache().borrow().order(&cid).unwrap().clone()).unwrap(),
            serde_json::to_value(&applied).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                harness
                    .cache()
                    .borrow()
                    .position(&position_id)
                    .unwrap()
                    .clone()
            )
            .unwrap(),
            serde_json::to_value(&position).unwrap()
        );
    }

    #[rstest]
    fn parse_order_filled_uses_payload_fee_currency() {
        let instrument = linear_instrument();
        let (emitter, _rx) = create_emitter();

        let json = load_test_json("ws_account_execution.json");
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&json).unwrap();

        let mut exec = msg.data[0].clone();
        exec.fee_currency = Ustr::from("BTC");

        let filled = parse_order_filled(
            &exec,
            &instrument,
            &default_identity(),
            &emitter,
            test_account_id(),
            UnixNanos::default(),
        )
        .unwrap();

        let commission = filled.commission.expect("commission present");
        assert_eq!(commission.currency.code, "BTC");
    }

    #[rstest]
    #[case::spot_base_fee(BybitProductType::Spot, "BTC", "0.000001", "USDT")]
    #[case::spot_quote_fee(BybitProductType::Spot, "USDT", "0.1", "USDT")]
    #[case::linear_fee(BybitProductType::Linear, "USDT", "0.1", "USDT")]
    #[case::inverse_fee(BybitProductType::Inverse, "BTC", "0.000001", "USD")]
    #[case::spot_zero_fee(BybitProductType::Spot, "BTC", "0", "USDT")]
    #[case::inverse_rebate(BybitProductType::Inverse, "BTC", "-0.000001", "USD")]
    fn parse_order_filled_separates_price_and_commission_currencies(
        #[case] category: BybitProductType,
        #[case] fee_currency: &str,
        #[case] fee: &str,
        #[case] expected_price_currency: &str,
    ) {
        let instrument = match category {
            BybitProductType::Spot => spot_instrument(),
            BybitProductType::Linear => linear_instrument(),
            BybitProductType::Inverse => inverse_instrument(),
            _ => unreachable!("unsupported test product"),
        };

        let (emitter, _rx) = create_emitter();
        let json = load_test_json("ws_account_execution.json");
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&json).unwrap();
        let mut exec = msg.data[0].clone();
        exec.category = category;
        exec.symbol = instrument.raw_symbol().inner();
        exec.exec_qty = "1".to_string();
        exec.exec_fee = fee.to_string();
        exec.fee_currency = Ustr::from(fee_currency);

        let identity = OrderIdentity {
            instrument_id: instrument.id(),
            order_side: OrderSide::Sell,
            order_type: OrderType::Market,
            ..default_identity()
        };

        let filled = parse_order_filled(
            &exec,
            &instrument,
            &identity,
            &emitter,
            test_account_id(),
            UnixNanos::default(),
        )
        .unwrap();

        let commission = filled.commission.expect("commission present");
        assert_eq!(commission.currency.code, fee_currency);
        assert_eq!(commission.as_decimal(), fee.parse::<Decimal>().unwrap());
        assert_eq!(filled.currency.code, expected_price_currency);
    }

    #[rstest]
    fn test_dispatch_tracked_execution_preserves_venue_position_id() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_execution.json");
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_str(&json).unwrap();
        let venue_position_id = PositionId::from("BTCUSDT-LINEAR.BYBIT-LONG");

        if let Some(exec) = msg.data.first()
            && !exec.order_link_id.is_empty()
        {
            let cid = ClientOrderId::new(exec.order_link_id);
            state.order_identities.insert(
                cid,
                OrderIdentity {
                    venue_position_id: Some(venue_position_id),
                    ..default_identity()
                },
            );
        }

        let ws_msg = BybitWsMessage::AccountExecution(msg);
        dispatch_ws_message(
            &ws_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        let _accepted = rx.try_recv().unwrap();
        let event = rx.try_recv().unwrap();
        match event {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.position_id, Some(venue_position_id));
            }
            other => panic!("Expected Filled event, found {other:?}"),
        }
    }

    fn fee_rate(symbol: &str, base_coin: Option<&str>, maker: &str, taker: &str) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    #[rstest]
    fn test_cache_execution_fee_rates_scopes_products_and_fans_out_options() {
        let linear = linear_instrument();
        let spot = spot_instrument();
        let option_1 = option_instrument_with_distinct_settlement();
        let mut option_2 = option_instrument_with_distinct_settlement();

        let InstrumentAny::CryptoOption(ref mut option) = option_2 else {
            panic!("expected crypto option");
        };

        option.id = InstrumentId::from("BTC-30JUN25-110000-C-OPTION.BYBIT");
        let instruments = build_instruments(&[
            linear.clone(),
            spot.clone(),
            option_1.clone(),
            option_2.clone(),
        ]);
        let state = WsDispatchState::default();

        let linear_cached = cache_execution_fee_rates(
            &state,
            &instruments,
            BybitProductType::Linear,
            vec![fee_rate("BTCUSDT", None, "0.0001", "0.0006")],
        );
        assert_eq!(linear_cached, 1);
        assert_eq!(
            *state.fee_rates.get(&linear.id()).unwrap(),
            MakerTakerFeeRates::new(dec!(0.0001), dec!(0.0006),)
        );
        assert!(!state.fee_rates.contains_key(&spot.id()));

        let spot_cached = cache_execution_fee_rates(
            &state,
            &instruments,
            BybitProductType::Spot,
            vec![fee_rate("BTCUSDT", None, "0.001", "0.002")],
        );
        assert_eq!(spot_cached, 1);
        assert_eq!(
            *state.fee_rates.get(&spot.id()).unwrap(),
            MakerTakerFeeRates::new(dec!(0.001), dec!(0.002),)
        );
        assert_eq!(
            *state.fee_rates.get(&linear.id()).unwrap(),
            MakerTakerFeeRates::new(dec!(0.0001), dec!(0.0006),)
        );

        let option_cached = cache_execution_fee_rates(
            &state,
            &instruments,
            BybitProductType::Option,
            vec![fee_rate("", Some("BTC"), "0.0002", "0.0003")],
        );
        assert_eq!(option_cached, 2);
        assert!(state.fee_rates.contains_key(&option_1.id()));
        assert!(state.fee_rates.contains_key(&option_2.id()));
    }

    #[rstest]
    fn test_cache_execution_fee_rates_skips_malformed_rates() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let state = WsDispatchState::default();

        let cached = cache_execution_fee_rates(
            &state,
            &instruments,
            BybitProductType::Linear,
            vec![fee_rate("BTCUSDT", None, "invalid", "0.0006")],
        );

        assert_eq!(cached, 0);
        assert!(state.fee_rates.is_empty());
    }

    fn fast_execution_msg(is_maker: bool, order_link_id: &str) -> BybitWsAccountExecutionFastMsg {
        BybitWsAccountExecutionFastMsg {
            topic: Ustr::from("execution.fast"),
            id: String::new(),
            creation_time: 1_716_800_399_338,
            data: vec![BybitWsAccountExecutionFast {
                category: BybitProductType::Linear,
                symbol: Ustr::from("BTCUSDT"),
                exec_id: "fast-1".to_string(),
                exec_price: "50000.0".to_string(),
                exec_qty: "0.5".to_string(),
                order_id: Ustr::from("ord-1"),
                order_link_id: Ustr::from(order_link_id),
                side: BybitOrderSide::Buy,
                exec_time: "1716800399334".to_string(),
                is_maker,
                seq: 42,
            }],
        }
    }

    #[rstest]
    fn test_dispatch_tracked_fast_execution_preserves_venue_position_id() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();
        let venue_position_id = PositionId::from("BTCUSDT-LINEAR.BYBIT-LONG");

        // Taker fast fill (orderLinkId populated) so the identity lookup hits.
        let msg = fast_execution_msg(false, "link-1");
        let cid = ClientOrderId::new(msg.data[0].order_link_id);
        state.order_identities.insert(
            cid,
            OrderIdentity {
                venue_position_id: Some(venue_position_id),
                ..default_identity()
            },
        );

        let ws_msg = BybitWsMessage::AccountExecutionFast(msg);
        dispatch_ws_message(
            &ws_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        // First event: synthesized OrderAccepted from the identity hit.
        let event1 = rx.try_recv().unwrap();
        assert!(
            matches!(event1, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
            "Expected Accepted, found {event1:?}",
        );

        // Second event: FillReport carrying the hedge venue_position_id.
        let event2 = rx.try_recv().unwrap();
        match event2 {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.venue_position_id, Some(venue_position_id));
                assert_eq!(report.client_order_id, Some(cid));
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }
    }

    fn order_msg(
        order_link_id: &str,
        status: &str,
        cum_exec_qty: &str,
    ) -> crate::websocket::messages::BybitWsAccountOrderMsg {
        let json = load_test_json("ws_account_order.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["orderId"] = serde_json::Value::String("ord-1".to_string());
        value["data"][0]["orderLinkId"] = serde_json::Value::String(order_link_id.to_string());
        value["data"][0]["orderStatus"] = serde_json::Value::String(status.to_string());
        value["data"][0]["price"] = serde_json::Value::String("50000.0".to_string());
        value["data"][0]["qty"] = serde_json::Value::String("0.5".to_string());
        value["data"][0]["cumExecQty"] = serde_json::Value::String(cum_exec_qty.to_string());
        serde_json::from_value(value).unwrap()
    }

    fn assert_fast_state_cleared(state: &WsDispatchState, client_order_id: ClientOrderId) {
        assert!(!state.order_identities.contains_key(&client_order_id));
        assert!(!state.order_venue_ids.contains_key(&client_order_id));
        assert!(
            !state
                .venue_order_ids
                .contains_key(&VenueOrderId::from("ord-1"))
        );
        assert!(!state.filled_orders.contains(&client_order_id));
        assert!(!state.fast_filled_quantities.contains_key(&client_order_id));
        assert!(
            !state
                .terminal_order_quantities
                .contains_key(&client_order_id)
        );
        assert!(state.pending_fast_executions.is_empty());
    }

    #[rstest]
    #[case::fast_first(true)]
    #[case::order_first(false)]
    fn test_dispatch_untracked_fast_execution_emits_fill_report_without_position(
        #[case] fast_first: bool,
    ) {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let fast_msg = BybitWsMessage::AccountExecutionFast(fast_execution_msg(true, ""));
        let order_msg = BybitWsMessage::AccountOrder(order_msg("", "Filled", "0.5"));

        if fast_first {
            dispatch_ws_message(
                &fast_msg,
                test_account_id(),
                &instruments,
                clock,
                &state,
                &emitter,
            );
            assert!(rx.try_recv().is_err());
            assert_eq!(state.pending_fast_executions.len(), 1);
        }

        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        if !fast_first {
            dispatch_ws_message(
                &fast_msg,
                test_account_id(),
                &instruments,
                clock,
                &state,
                &emitter,
            );
        }

        let first = rx.try_recv().unwrap();
        let second = rx.try_recv().unwrap();

        let report = if fast_first {
            assert!(matches!(
                second,
                ExecutionEvent::Report(ExecutionReport::Order(_))
            ));

            match first {
                ExecutionEvent::Report(ExecutionReport::Fill(report)) => report,
                other => panic!("Expected FillReport first, found {other:?}"),
            }
        } else {
            assert!(matches!(
                first,
                ExecutionEvent::Report(ExecutionReport::Order(_))
            ));

            match second {
                ExecutionEvent::Report(ExecutionReport::Fill(report)) => report,
                other => panic!("Expected FillReport second, found {other:?}"),
            }
        };

        assert_eq!(report.client_order_id, None);
        assert_eq!(report.venue_position_id, None);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.commission.as_decimal(), Decimal::ZERO);
        assert!(state.pending_fast_executions.is_empty());
    }

    #[rstest]
    fn test_dispatch_reconnected_flushes_pending_fast_execution() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(true);
        let fast_msg = BybitWsMessage::AccountExecutionFast(fast_execution_msg(true, ""));
        dispatch_ws_message(
            &fast_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(state.pending_fast_executions.len(), 1);

        dispatch_ws_message(
            &BybitWsMessage::Reconnected,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.client_order_id, None);
                assert_eq!(report.last_qty, Quantity::from("0.5"));
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }

        assert!(state.pending_fast_executions.is_empty());
    }

    #[rstest]
    #[case::rejected("Rejected")]
    #[case::partially_filled_canceled("PartiallyFilledCanceled")]
    fn test_dispatch_partially_filled_terminal_defers_cleanup_until_fast_fill(
        #[case] status: &str,
    ) {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, _rx) = create_emitter();
        let clock = get_atomic_clock_realtime();

        let state = WsDispatchState {
            fast_execution: true,
            ..Default::default()
        };

        let client_order_id = ClientOrderId::from("link-1");
        state
            .order_identities
            .insert(client_order_id, default_identity());
        let order_msg = BybitWsMessage::AccountOrder(order_msg("link-1", status, "0.5"));
        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert!(state.order_identities.contains_key(&client_order_id));
        assert_eq!(
            *state
                .terminal_order_quantities
                .get(&client_order_id)
                .unwrap(),
            Quantity::from("0.5")
        );

        let fast_msg = BybitWsMessage::AccountExecutionFast(fast_execution_msg(true, ""));
        dispatch_ws_message(
            &fast_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert_fast_state_cleared(&state, client_order_id);
    }

    #[rstest]
    fn test_dispatch_tracked_terminal_emits_deferred_fill_before_cancel() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(true);
        let client_order_id = ClientOrderId::from("link-1");
        state
            .order_identities
            .insert(client_order_id, default_identity());
        let fast_msg = BybitWsMessage::AccountExecutionFast(fast_execution_msg(true, ""));
        dispatch_ws_message(
            &fast_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        assert!(rx.try_recv().is_err());

        let order_msg =
            BybitWsMessage::AccountOrder(order_msg("link-1", "PartiallyFilledCanceled", "0.5"));
        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Report(ExecutionReport::Fill(_))
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Canceled(_))
        ));
        assert_fast_state_cleared(&state, client_order_id);
    }

    #[rstest]
    fn test_dispatch_zero_terminal_cleans_immediately() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, _rx) = create_emitter();
        let clock = get_atomic_clock_realtime();

        let state = WsDispatchState {
            fast_execution: true,
            ..Default::default()
        };

        let client_order_id = ClientOrderId::from("link-1");
        state
            .order_identities
            .insert(client_order_id, default_identity());
        let order_msg = BybitWsMessage::AccountOrder(order_msg("link-1", "Cancelled", "0"));

        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert_fast_state_cleared(&state, client_order_id);
    }

    #[rstest]
    fn test_dispatch_standard_rejected_cleans_venue_correlation() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, _rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();
        let client_order_id = ClientOrderId::from("link-1");
        state
            .order_identities
            .insert(client_order_id, default_identity());
        let order_msg = BybitWsMessage::AccountOrder(order_msg("link-1", "Rejected", "0"));

        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert_fast_state_cleared(&state, client_order_id);
    }

    #[rstest]
    fn test_dispatch_fast_execution_parse_failure_allows_valid_replay() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();
        let mut invalid = fast_execution_msg(false, "link-1");
        invalid.data[0].exec_qty = "invalid".to_string();
        let invalid = BybitWsMessage::AccountExecutionFast(invalid);

        dispatch_ws_message(
            &invalid,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        assert!(rx.try_recv().is_err());

        let valid = BybitWsMessage::AccountExecutionFast(fast_execution_msg(false, "link-1"));
        dispatch_ws_message(
            &valid,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.trade_id.to_string(), "fast-1");
                assert_eq!(report.last_qty, Quantity::from("0.5"));
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_recent_ids_evicts_oldest_without_clearing_history() {
        let state = WsDispatchState::default();
        let first = TradeId::from("first");
        assert!(state.insert_fast_execution(first));

        for index in 0..DEDUP_CAPACITY {
            assert!(state.insert_fast_execution(TradeId::from(format!("id-{index}"))));
        }

        assert!(!state.fast_execution_ids.contains(&first));
        assert_eq!(state.fast_execution_ids.ids.len(), DEDUP_CAPACITY);
        assert!(state.insert_fast_execution(first));
        assert_eq!(state.fast_execution_ids.ids.len(), DEDUP_CAPACITY);
        assert!(!state.insert_fast_execution(first));
    }

    #[rstest]
    fn test_fast_execution_correlation_and_enqueue_are_atomic() {
        for _ in 0..32 {
            let instrument = linear_instrument();
            let instruments = build_instruments(std::slice::from_ref(&instrument));
            let (emitter, mut rx) = create_emitter();
            let clock = get_atomic_clock_realtime();
            let state = Arc::new(WsDispatchState::new(true));
            let client_order_id = ClientOrderId::from("link-1");
            state
                .order_identities
                .insert(client_order_id, default_identity());
            let venue_order_id = VenueOrderId::from("ord-1");
            let barrier = Arc::new(std::sync::Barrier::new(2));

            std::thread::scope(|scope| {
                let state_ws = Arc::clone(&state);
                let barrier_ws = Arc::clone(&barrier);
                let emitter_ws = emitter.clone();
                let instruments_ws = &instruments;

                scope.spawn(move || {
                    barrier_ws.wait();
                    let msg = BybitWsMessage::AccountExecutionFast(fast_execution_msg(true, ""));
                    dispatch_ws_message(
                        &msg,
                        test_account_id(),
                        instruments_ws,
                        clock,
                        &state_ws,
                        &emitter_ws,
                    );
                });

                let state_rest = Arc::clone(&state);
                let barrier_rest = Arc::clone(&barrier);
                let emitter_rest = emitter.clone();

                scope.spawn(move || {
                    barrier_rest.wait();
                    record_venue_order_id(client_order_id, venue_order_id, &state_rest);
                    drain_pending_fast_executions(
                        venue_order_id,
                        &instrument,
                        test_account_id(),
                        clock.get_time_ns(),
                        &state_rest,
                        &emitter_rest,
                    );
                });
            });

            assert!(state.pending_fast_executions.is_empty());
            let mut fill_count = 0;

            while let Ok(event) = rx.try_recv() {
                if matches!(event, ExecutionEvent::Report(ExecutionReport::Fill(_))) {
                    fill_count += 1;
                }
            }

            assert_eq!(fill_count, 1);
        }
    }

    #[rstest]
    #[case::fast_first(true)]
    #[case::order_first(false)]
    fn test_dispatch_maker_fast_execution_correlates_and_cleans_terminal_state(
        #[case] fast_first: bool,
    ) {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(true);
        let client_order_id = ClientOrderId::from("link-1");
        let venue_position_id = PositionId::from("BTCUSDT-LINEAR.BYBIT-LONG");
        state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                venue_position_id: Some(venue_position_id),
                ..default_identity()
            },
        );

        let fast_msg = BybitWsMessage::AccountExecutionFast(fast_execution_msg(true, ""));
        let order_msg = BybitWsMessage::AccountOrder(order_msg("link-1", "Filled", "0.5"));

        if fast_first {
            dispatch_ws_message(
                &fast_msg,
                test_account_id(),
                &instruments,
                clock,
                &state,
                &emitter,
            );
            assert!(rx.try_recv().is_err());
        }

        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        if !fast_first {
            dispatch_ws_message(
                &fast_msg,
                test_account_id(),
                &instruments,
                clock,
                &state,
                &emitter,
            );
        }

        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.client_order_id, Some(client_order_id));
                assert_eq!(report.venue_position_id, Some(venue_position_id));
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }

        assert_fast_state_cleared(&state, client_order_id);
    }

    #[rstest]
    fn test_dispatch_fast_execution_waits_for_terminal_quantity_and_dedups() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, _rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();
        let client_order_id = ClientOrderId::from("link-1");
        state
            .order_identities
            .insert(client_order_id, default_identity());
        let mut first = fast_execution_msg(false, "link-1");
        first.data[0].exec_qty = "0.25".to_string();
        let first = BybitWsMessage::AccountExecutionFast(first);

        dispatch_ws_message(
            &first,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        dispatch_ws_message(
            &first,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        assert_eq!(
            *state.fast_filled_quantities.get(&client_order_id).unwrap(),
            Quantity::from("0.25")
        );

        let order_msg = BybitWsMessage::AccountOrder(order_msg("link-1", "Filled", "0.5"));
        dispatch_ws_message(
            &order_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );
        assert!(state.order_identities.contains_key(&client_order_id));

        let mut second = fast_execution_msg(false, "link-1");
        second.data[0].exec_id = "fast-2".to_string();
        second.data[0].exec_qty = "0.25".to_string();
        let second = BybitWsMessage::AccountExecutionFast(second);
        dispatch_ws_message(
            &second,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert_fast_state_cleared(&state, client_order_id);
        assert_eq!(state.fast_execution_ids.ids.len(), 2);
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
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::Fill(_))
        ));
    }

    #[rstest]
    #[case::untracked("")]
    #[case::tracked("test-order-link-001")]
    fn test_dispatch_funding_execution_emits_no_event(#[case] order_link_id: &str) {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_execution_funding.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["orderLinkId"] = serde_json::Value::String(order_link_id.to_string());
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_value(value).unwrap();
        let execution = &msg.data[0];

        assert_eq!(execution.exec_type, BybitExecType::Funding);

        if !execution.order_link_id.is_empty() {
            state.order_identities.insert(
                ClientOrderId::new(execution.order_link_id),
                default_identity(),
            );
        }

        dispatch_ws_message(
            &BybitWsMessage::AccountExecution(msg),
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[rstest]
    #[case::corporate_action("CorporateAction")]
    #[case::forward_split_settle("ForwardSplitSettle")]
    #[case::reverse_split_settle("ReverseSplitSettle")]
    #[case::dividend("Dividend")]
    #[case::unrecognized("StockMerger")]
    fn test_dispatch_venue_initiated_execution_emits_only_fill_report(#[case] exec_type: &str) {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (emitter, mut rx) = create_emitter();
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::default();

        let json = load_test_json("ws_account_execution_adl.json");
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["data"][0]["execType"] = serde_json::Value::String(exec_type.to_string());
        let msg: crate::websocket::messages::BybitWsAccountExecutionMsg =
            serde_json::from_value(value).unwrap();
        let execution = &msg.data[0];

        assert!(execution.order_link_id.is_empty());

        dispatch_ws_message(
            &BybitWsMessage::AccountExecution(msg),
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        let event = rx.try_recv().unwrap();
        match event {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.client_order_id, None);
                assert_eq!(
                    report.venue_order_id,
                    VenueOrderId::from("9aac161b-8ed6-450d-9cab-c5cc67c21785")
                );
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }
        assert!(rx.try_recv().is_err());
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
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
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
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        rx.try_recv().unwrap_err();
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
            let cid = ClientOrderId::new(order.order_link_id);
            state.order_identities.insert(cid, default_identity());
        }

        let ws_msg = BybitWsMessage::AccountOrder(msg.clone());
        dispatch_ws_message(
            &ws_msg,
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
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
            test_account_id(),
            &instruments,
            clock,
            &state,
            &emitter,
        );

        rx.try_recv().unwrap_err();
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
                    .contains_key(&ClientOrderId::new(order.order_link_id))
            {
                let cid = ClientOrderId::new(order.order_link_id);
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
                test_account_id(),
                &self.instruments,
                self.clock,
                &self.state,
                &self.emitter,
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
        let _result = ctx.recv_updated();

        let mut amended2 = value;
        amended2["data"][0]["price"] = serde_json::Value::String("32000".to_string());
        ctx.dispatch_value(&amended2);

        let updated = ctx.recv_updated();
        assert_eq!(updated.price, Some(Price::from("32000.00")));
    }

    #[rstest]
    fn test_dispatch_accepted_with_seeded_snapshot_emits_updated_for_bbo() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        let cid = ClientOrderId::from("client-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.order_snapshots.insert(
            cid,
            OrderStateSnapshot {
                quantity: Quantity::from("0.010"),
                price: Some(Price::from("29000.00")),
                trigger_price: None,
            },
        );

        ctx.dispatch_value(&value);

        let accepted = ctx.rx.try_recv().unwrap();
        assert!(
            matches!(accepted, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
            "Expected Accepted first, found {accepted:?}"
        );

        let updated = ctx.recv_updated();
        assert_eq!(updated.client_order_id, cid);
        assert_eq!(updated.price, Some(Price::from("30000.00")));
        assert_eq!(updated.quantity, Quantity::from("0.010"));
        assert!(updated.venue_order_id.is_some());
    }

    #[rstest]
    fn test_emit_rejection_for_place_clears_snapshot() {
        let ctx = DispatchTestContext::new();
        let cid = ClientOrderId::from("client-1");
        let identity = default_identity();
        ctx.state.order_identities.insert(cid, identity.clone());
        ctx.state.order_snapshots.insert(
            cid,
            OrderStateSnapshot {
                quantity: Quantity::from("0.010"),
                price: Some(Price::from("30000.00")),
                trigger_price: None,
            },
        );

        emit_rejection_for_op(
            &PendingOperation::Place,
            cid,
            &identity,
            None,
            "rejected",
            &ctx.emitter,
            &ctx.state,
            UnixNanos::from(1u64),
            None,
        );

        assert!(!ctx.state.order_identities.contains_key(&cid));
        assert!(!ctx.state.order_snapshots.contains_key(&cid));
    }

    fn order_response(
        op: &str,
        ret_code: i64,
        ret_msg: &str,
        req_id: &str,
        data: serde_json::Value,
        ret_ext_info: Option<serde_json::Value>,
    ) -> BybitWsOrderResponse {
        BybitWsOrderResponse {
            op: Ustr::from(op),
            conn_id: Some("test-conn-id".to_string()),
            ret_code,
            ret_msg: ret_msg.to_string(),
            data,
            req_id: Some(req_id.to_string()),
            header: None,
            ret_ext_info,
        }
    }

    #[rstest]
    fn test_dispatch_local_not_sent_emits_order_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cid = ClientOrderId::from("not-sent-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-not-sent".to_string(),
            (vec![cid], vec![None], PendingOperation::Place, Vec::new()),
        );
        dispatch_order_not_sent(
            "req-not-sent",
            "Order command was not written",
            &ctx.emitter,
            &ctx.state,
            UnixNanos::from(1u64),
        );

        let event = ctx.rx.try_recv().expect("expected OrderRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(ref rejected))
                if rejected.client_order_id == cid
                    && rejected.reason == "Order command was not written"),
            "Expected OrderRejected for {cid}, found {event:?}"
        );
        assert!(!ctx.state.order_identities.contains_key(&cid));
        assert!(!ctx.state.pending_requests.contains_key("req-not-sent"));
    }

    #[rstest]
    #[case::modify(PendingOperation::Amend)]
    #[case::cancel(PendingOperation::Cancel)]
    fn test_dispatch_local_refusal_preserves_each_command(#[case] operation: PendingOperation) {
        let mut ctx = DispatchTestContext::new();
        let first = ClientOrderId::from("O-LOCAL-FIRST");
        let second = ClientOrderId::from("O-LOCAL-SECOND");
        let first_cause = UUID4::new();
        let second_cause = UUID4::new();
        let venue_order_id = VenueOrderId::from("V-LOCAL-FIRST");
        ctx.state.order_identities.insert(first, default_identity());
        ctx.state
            .order_identities
            .insert(second, default_identity());
        ctx.state.pending_requests.insert(
            "req-local".to_string(),
            (
                vec![first, second],
                vec![Some(venue_order_id), None],
                operation,
                vec![Some(first_cause), Some(second_cause)],
            ),
        );
        dispatch_order_not_sent(
            "req-local",
            "Command was not written",
            &ctx.emitter,
            &ctx.state,
            UnixNanos::from(17),
        );

        for (client_order_id, cause, venue) in [
            (first, first_cause, Some(venue_order_id)),
            (second, second_cause, None),
        ] {
            let ExecutionEvent::Order(event) = ctx.rx.try_recv().unwrap() else {
                panic!("expected order outcome")
            };

            let (actual_cause, reason) = match (operation, &event) {
                (PendingOperation::Amend, OrderEventAny::ModifyDenied(event)) => {
                    (event.causation_id, event.reason.as_str())
                }
                (PendingOperation::Cancel, OrderEventAny::CancelDenied(event)) => {
                    (event.causation_id, event.reason.as_str())
                }
                _ => panic!("unexpected refusal {event:?}"),
            };

            assert_eq!(actual_cause, Some(cause));
            assert_eq!(reason, "Command was not written");
            assert_eq!(event.client_order_id(), client_order_id);
            assert_eq!(event.venue_order_id(), venue);
            assert_eq!(event.strategy_id(), default_identity().strategy_id);
            assert_eq!(event.instrument_id(), default_identity().instrument_id);
            assert_eq!(event.ts_event(), UnixNanos::from(17));
        }

        assert!(ctx.rx.try_recv().is_err());
        assert!(!ctx.state.pending_requests.contains_key("req-local"));
        assert!(ctx.state.order_identities.contains_key(&first));
        assert!(ctx.state.order_identities.contains_key(&second));
    }

    #[rstest]
    fn test_dispatch_single_cancel_rejection_emits_cancel_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cause = UUID4::new();
        let cid = ClientOrderId::from("cancel-reject-1");
        let venue_order_id = VenueOrderId::from("venue-cancel-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-cancel".to_string(),
            (
                vec![cid],
                vec![Some(venue_order_id)],
                PendingOperation::Cancel,
                vec![Some(cause)],
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL,
            110001,
            "Order does not exist.",
            "req-cancel",
            serde_json::json!({
                "orderId": venue_order_id.to_string(),
                "orderLinkId": cid.to_string(),
            }),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        let event = ctx.rx.try_recv().expect("expected CancelRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::CancelRejected(ref rejected)) if rejected.client_order_id == cid && rejected.causation_id == Some(cause)),
            "Expected CancelRejected for {cid}, found {event:?}"
        );
        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no extra events for single cancel rejection"
        );
    }

    #[rstest]
    fn test_dispatch_single_modify_rejection_emits_modify_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cause = UUID4::new();
        let cid = ClientOrderId::from("modify-reject-1");
        let venue_order_id = VenueOrderId::from("venue-modify-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-modify".to_string(),
            (
                vec![cid],
                vec![Some(venue_order_id)],
                PendingOperation::Amend,
                vec![Some(cause)],
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_AMEND,
            110003,
            "Order price exceeds allowable range.",
            "req-modify",
            serde_json::json!({
                "orderId": venue_order_id.to_string(),
                "orderLinkId": cid.to_string(),
            }),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        let event = ctx.rx.try_recv().expect("expected ModifyRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::ModifyRejected(ref rejected)) if rejected.client_order_id == cid && rejected.causation_id == Some(cause)),
            "Expected ModifyRejected for {cid}, found {event:?}"
        );
        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no extra events for single modify rejection"
        );
    }

    #[rstest]
    fn test_dispatch_single_cancel_rate_limit_emits_cancel_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cid = ClientOrderId::from("cancel-rate-limit-1");
        let venue_order_id = VenueOrderId::from("venue-cancel-rate-limit-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-cancel-rate-limit".to_string(),
            (
                vec![cid],
                vec![Some(venue_order_id)],
                PendingOperation::Cancel,
                Vec::new(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL,
            10429,
            "System level frequency protection.",
            "req-cancel-rate-limit",
            serde_json::json!({
                "orderId": venue_order_id.to_string(),
                "orderLinkId": cid.to_string(),
            }),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        let event = ctx.rx.try_recv().expect("expected CancelRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::CancelRejected(ref rejected))
                if rejected.client_order_id == cid && rejected.venue_order_id == Some(venue_order_id)),
            "Expected CancelRejected for {cid}, found {event:?}"
        );
        assert!(ctx.state.order_identities.contains_key(&cid));
    }

    #[rstest]
    fn test_dispatch_single_modify_server_error_keeps_outcome_unresolved() {
        let mut ctx = DispatchTestContext::new();
        let cid = ClientOrderId::from("modify-server-error-1");
        let venue_order_id = VenueOrderId::from("venue-modify-server-error-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-modify-server-error".to_string(),
            (
                vec![cid],
                vec![Some(venue_order_id)],
                PendingOperation::Amend,
                Vec::new(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_AMEND,
            10016,
            "Internal server error.",
            "req-modify-server-error",
            serde_json::json!({
                "orderId": venue_order_id.to_string(),
                "orderLinkId": cid.to_string(),
            }),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no ModifyRejected event for server error response"
        );
        assert!(ctx.state.order_identities.contains_key(&cid));
    }

    #[rstest]
    fn test_dispatch_batch_cancel_top_level_failure_does_not_emit_cancel_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cid_1 = ClientOrderId::from("batch-cancel-1");
        let cid_2 = ClientOrderId::from("batch-cancel-2");
        ctx.state.order_identities.insert(cid_1, default_identity());
        ctx.state.order_identities.insert(cid_2, default_identity());
        ctx.state.pending_requests.insert(
            "req-batch-cancel".to_string(),
            (
                vec![cid_1, cid_2],
                vec![
                    Some(VenueOrderId::from("venue-batch-1")),
                    Some(VenueOrderId::from("venue-batch-2")),
                ],
                PendingOperation::Cancel,
                Vec::new(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL_BATCH,
            10016,
            "Internal server error.",
            "req-batch-cancel",
            serde_json::json!({}),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no CancelRejected events for ambiguous batch failure"
        );
        assert!(ctx.state.order_identities.contains_key(&cid_1));
        assert!(ctx.state.order_identities.contains_key(&cid_2));
    }

    #[rstest]
    fn test_dispatch_single_item_batch_cancel_top_level_failure_does_not_emit_cancel_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cid = ClientOrderId::from("batch-cancel-single");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-batch-cancel-single".to_string(),
            (
                vec![cid],
                vec![Some(VenueOrderId::from("venue-batch-single"))],
                PendingOperation::Cancel,
                Vec::new(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL_BATCH,
            10016,
            "Internal server error.",
            "req-batch-cancel-single",
            serde_json::json!({}),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no CancelRejected event for ambiguous one-item batch failure"
        );
        assert!(ctx.state.order_identities.contains_key(&cid));
    }

    #[rstest]
    fn test_dispatch_batch_cancel_top_level_rate_limit_emits_cancel_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cid_1 = ClientOrderId::from("batch-rate-1");
        let cid_2 = ClientOrderId::from("batch-rate-2");
        let venue_1 = VenueOrderId::from("venue-rate-1");
        let venue_2 = VenueOrderId::from("venue-rate-2");
        ctx.state.order_identities.insert(cid_1, default_identity());
        ctx.state.order_identities.insert(cid_2, default_identity());
        ctx.state.pending_requests.insert(
            "req-batch-rate".to_string(),
            (
                vec![cid_1, cid_2],
                vec![Some(venue_1), Some(venue_2)],
                PendingOperation::Cancel,
                Vec::new(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL_BATCH,
            10006,
            "Too many visits.",
            "req-batch-rate",
            serde_json::json!({}),
            None,
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        let first = ctx.rx.try_recv().expect("expected first CancelRejected");
        let second = ctx.rx.try_recv().expect("expected second CancelRejected");
        assert!(
            matches!(first, ExecutionEvent::Order(OrderEventAny::CancelRejected(ref rejected))
                if rejected.client_order_id == cid_1 && rejected.venue_order_id == Some(venue_1))
        );
        assert!(
            matches!(second, ExecutionEvent::Order(OrderEventAny::CancelRejected(ref rejected))
                if rejected.client_order_id == cid_2 && rejected.venue_order_id == Some(venue_2))
        );
    }

    #[rstest]
    fn test_dispatch_batch_cancel_per_item_error_emits_only_failed_item(
        #[values(false, true)] repeated: bool,
    ) {
        let mut ctx = DispatchTestContext::new();
        let cid_1 = ClientOrderId::from("batch-cancel-ok");
        let cid_2 = if repeated {
            cid_1
        } else {
            ClientOrderId::from("batch-cancel-reject")
        };

        let causes = [UUID4::new(), UUID4::new()];
        ctx.state.order_identities.insert(cid_1, default_identity());
        ctx.state.order_identities.insert(cid_2, default_identity());
        ctx.state.pending_requests.insert(
            "req-batch-mixed".to_string(),
            (
                vec![cid_1, cid_2],
                vec![
                    Some(VenueOrderId::from("venue-batch-ok")),
                    Some(VenueOrderId::from("venue-batch-reject")),
                ],
                PendingOperation::Cancel,
                causes.into_iter().map(Some).collect(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL_BATCH,
            0,
            "OK",
            "req-batch-mixed",
            serde_json::json!({
                "list": [
                    {"orderId": "venue-batch-ok", "orderLinkId": cid_1.to_string()},
                    {"orderId": "venue-batch-reject", "orderLinkId": cid_2.to_string()}
                ]
            }),
            Some(serde_json::json!({
                "list": [
                    {"code": 0, "msg": "OK"},
                    {"code": 170213, "msg": "Order does not exist."}
                ]
            })),
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        let event = ctx.rx.try_recv().expect("expected CancelRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::CancelRejected(ref rejected)) if rejected.client_order_id == cid_2 && rejected.causation_id == Some(causes[1])),
            "Expected CancelRejected for failed batch item {cid_2}, found {event:?}"
        );
        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no CancelRejected event for successful batch item"
        );
    }

    #[rstest]
    fn test_dispatch_batch_cancel_per_item_rate_limit_emits_cancel_rejected() {
        let mut ctx = DispatchTestContext::new();
        let cid = ClientOrderId::from("batch-cancel-rate-limit");
        let venue_order_id = VenueOrderId::from("venue-batch-rate-limit");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.pending_requests.insert(
            "req-batch-rate-limit".to_string(),
            (
                vec![cid],
                vec![Some(venue_order_id)],
                PendingOperation::Cancel,
                Vec::new(),
            ),
        );

        let response = order_response(
            BYBIT_OP_ORDER_CANCEL_BATCH,
            0,
            "OK",
            "req-batch-rate-limit",
            serde_json::json!({
                "list": [
                    {"orderId": venue_order_id.to_string(), "orderLinkId": cid.to_string()}
                ]
            }),
            Some(serde_json::json!({
                "list": [
                    {"code": 10006, "msg": "Too many visits."}
                ]
            })),
        );

        dispatch_order_response(&response, &ctx.emitter, &ctx.state, UnixNanos::from(1u64));

        let event = ctx.rx.try_recv().expect("expected CancelRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::CancelRejected(ref rejected))
                if rejected.client_order_id == cid && rejected.venue_order_id == Some(venue_order_id)),
            "Expected CancelRejected for {cid}, found {event:?}"
        );
        assert!(ctx.state.order_identities.contains_key(&cid));
    }

    #[rstest]
    fn test_dispatch_filled_with_seeded_snapshot_emits_updated_for_bbo() {
        let mut ctx = DispatchTestContext::new();
        let mut value = new_order_value();
        value["data"][0]["orderStatus"] = serde_json::Value::String("Filled".to_string());
        value["data"][0]["cumExecQty"] = serde_json::Value::String("0.010".to_string());
        let cid = ClientOrderId::from("client-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.order_snapshots.insert(
            cid,
            OrderStateSnapshot {
                quantity: Quantity::from("0.010"),
                price: Some(Price::from("29000.00")),
                trigger_price: None,
            },
        );

        ctx.dispatch_value(&value);

        let accepted_event = ctx.rx.try_recv().unwrap();
        let accepted = match accepted_event {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => accepted,
            other => panic!("Expected Accepted first, found {other:?}"),
        };
        assert_eq!(accepted.client_order_id, cid);
        assert!(!accepted.venue_order_id.to_string().is_empty());

        let updated = ctx.recv_updated();
        assert_eq!(updated.client_order_id, cid);
        assert_eq!(updated.price, Some(Price::from("30000.00")));
        assert_eq!(updated.quantity, Quantity::from("0.010"));
        assert_eq!(updated.trigger_price, None);
        assert_eq!(updated.venue_order_id, Some(accepted.venue_order_id));

        let stored = ctx.state.order_snapshots.get(&cid).unwrap();
        assert_eq!(stored.price, Some(Price::from("30000.00")));
    }

    #[rstest]
    fn test_dispatch_accepted_with_matching_seeded_snapshot_no_updated() {
        let mut ctx = DispatchTestContext::new();
        let value = new_order_value();
        let cid = ClientOrderId::from("client-1");
        ctx.state.order_identities.insert(cid, default_identity());
        ctx.state.order_snapshots.insert(
            cid,
            OrderStateSnapshot {
                quantity: Quantity::from("0.010"),
                price: Some(Price::from("30000.00")),
                trigger_price: None,
            },
        );

        ctx.dispatch_value(&value);

        let accepted = ctx.rx.try_recv().unwrap();
        assert!(matches!(
            accepted,
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        assert!(
            ctx.rx.try_recv().is_err(),
            "Expected no OrderUpdated when seed matches venue snapshot"
        );
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
