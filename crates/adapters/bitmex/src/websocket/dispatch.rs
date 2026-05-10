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

//! WebSocket message dispatch for the BitMEX execution client.
//!
//! Routes incoming [`BitmexWsMessage`] variants to the appropriate parsing and
//! event emission paths. Tracked orders (submitted through this client) produce
//! proper order events; untracked orders fall back to execution reports for
//! downstream reconciliation.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::{OrderAccepted, OrderEventAny, OrderFilled, OrderUpdated},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::FillReport,
    types::Currency,
};
use ustr::Ustr;

use crate::{
    common::enums::{BitmexExecType, BitmexOrderType, BitmexPegPriceType},
    http::parse::{InstrumentParseResult, parse_instrument_any},
    websocket::{
        enums::BitmexAction,
        messages::{BitmexExecutionMsg, BitmexTableMessage, BitmexWsMessage, OrderData},
        parse::{
            ParsedOrderEvent, parse_execution_msg, parse_margin_account_state, parse_order_event,
            parse_order_msg, parse_order_update_msg, parse_position_msg, parse_wallet_msg,
        },
    },
};

/// Maximum entries per generation before rotation.
const DEDUP_GENERATION_CAPACITY: usize = 10_000;

/// Order identity context stored at submission time, used by the WS dispatch
/// task to produce proper order events without Cache access.
///
/// These fields are immutable for the lifetime of an order and are used to
/// construct proper order events (`OrderAccepted`, `OrderFilled`, etc.) instead
/// of execution reports.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

/// Two-generation dedup set that avoids the duplicate-emission window caused
/// by wholesale clearing. Holds a current and previous `AHashSet` behind a
/// `Mutex`. When the current set fills up, `std::mem::swap` promotes it to
/// previous and starts a fresh current, all under a single lock acquisition.
/// Membership checks and removals also take the lock briefly.
///
/// The lock is held only for the duration of a hash-set insert (and sometimes
/// a swap + clear), so contention is negligible.
#[derive(Debug)]
struct GenerationalDedupSet {
    inner: Mutex<DedupInner>,
}

#[derive(Debug)]
struct DedupInner {
    current: ahash::AHashSet<ClientOrderId>,
    previous: ahash::AHashSet<ClientOrderId>,
}

impl Default for GenerationalDedupSet {
    fn default() -> Self {
        Self {
            inner: Mutex::new(DedupInner {
                current: ahash::AHashSet::new(),
                previous: ahash::AHashSet::new(),
            }),
        }
    }
}

impl GenerationalDedupSet {
    fn contains(&self, key: &ClientOrderId) -> bool {
        let guard = self.inner.lock().expect("dedup lock poisoned");
        guard.current.contains(key) || guard.previous.contains(key)
    }

    fn insert(&self, key: ClientOrderId) {
        let mut guard = self.inner.lock().expect("dedup lock poisoned");
        let inner = &mut *guard;
        inner.current.insert(key);
        if inner.current.len() >= DEDUP_GENERATION_CAPACITY {
            inner.previous.clear();
            std::mem::swap(&mut inner.current, &mut inner.previous);
        }
    }

    fn remove(&self, key: &ClientOrderId) {
        let mut guard = self.inner.lock().expect("dedup lock poisoned");
        guard.current.remove(key);
        guard.previous.remove(key);
    }
}

/// Shared state for WS dispatch event deduplication and order tracking.
///
/// Uses `DashMap` and mutex-guarded sets for concurrent access from the stream task
/// and the main thread without mutex contention.
#[derive(Debug)]
pub struct WsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    emitted_accepted: GenerationalDedupSet,
    triggered_orders: GenerationalDedupSet,
    filled_orders: GenerationalDedupSet,
    tombstoned: GenerationalDedupSet,
    pub margin_subscribed: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            emitted_accepted: GenerationalDedupSet::default(),
            triggered_orders: GenerationalDedupSet::default(),
            filled_orders: GenerationalDedupSet::default(),
            tombstoned: GenerationalDedupSet::default(),
            margin_subscribed: AtomicBool::new(false),
        }
    }
}

impl WsDispatchState {
    pub(crate) fn accepted_contains(&self, cid: &ClientOrderId) -> bool {
        self.emitted_accepted.contains(cid)
    }

    pub(crate) fn filled_contains(&self, cid: &ClientOrderId) -> bool {
        self.filled_orders.contains(cid)
    }

    pub(crate) fn triggered_contains(&self, cid: &ClientOrderId) -> bool {
        self.triggered_orders.contains(cid)
    }

    pub(crate) fn insert_accepted(&self, cid: ClientOrderId) {
        self.emitted_accepted.insert(cid);
    }

    pub(crate) fn insert_filled(&self, cid: ClientOrderId) {
        self.filled_orders.insert(cid);
    }

    pub(crate) fn insert_triggered(&self, cid: ClientOrderId) {
        self.triggered_orders.insert(cid);
    }

    pub(crate) fn remove_triggered(&self, cid: &ClientOrderId) {
        self.triggered_orders.remove(cid);
    }

    pub(crate) fn remove_filled(&self, cid: &ClientOrderId) {
        self.filled_orders.remove(cid);
    }

    pub(crate) fn remove_accepted(&self, cid: &ClientOrderId) {
        self.emitted_accepted.remove(cid);
    }

    /// Returns `true` if the order has been tombstoned by the HTTP cancel path.
    pub(crate) fn is_tombstoned(&self, cid: &ClientOrderId) -> bool {
        self.tombstoned.contains(cid)
    }

    /// Tombstones an order so the WS dispatch silently drops all subsequent
    /// messages for it. Call after the HTTP path has already sent a terminal
    /// report (cancel, expire, reject). The tombstone prevents stale WS
    /// messages (Accepted, Triggered) that are still queued from being
    /// processed as untracked orders and re-activating a closed order.
    pub(crate) fn tombstone_order(&self, cid: &ClientOrderId) {
        self.tombstoned.insert(*cid);
        self.order_identities.remove(cid);
        self.remove_accepted(cid);
        self.remove_triggered(cid);
        self.remove_filled(cid);
    }
}

/// Top-level dispatch for all BitMEX WebSocket messages on the execution stream.
#[expect(clippy::too_many_arguments)]
pub fn dispatch_ws_message(
    ts_init: UnixNanos,
    message: BitmexWsMessage,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
    order_type_cache: &mut AHashMap<ClientOrderId, OrderType>,
    order_symbol_cache: &mut AHashMap<ClientOrderId, Ustr>,
    account_id: AccountId,
) {
    match message {
        BitmexWsMessage::Table(table_msg) => match table_msg {
            BitmexTableMessage::Order { data, .. } => {
                dispatch_order_messages(
                    data,
                    emitter,
                    state,
                    instruments_by_symbol,
                    order_type_cache,
                    order_symbol_cache,
                    account_id,
                    ts_init,
                );
            }
            BitmexTableMessage::Execution { data, .. } => {
                dispatch_execution_messages(
                    data,
                    emitter,
                    state,
                    instruments_by_symbol,
                    order_symbol_cache,
                    ts_init,
                );
            }
            BitmexTableMessage::Position { data, .. } => {
                for pos_msg in data {
                    let Some(instrument) = instruments_by_symbol.get(&pos_msg.symbol) else {
                        log::error!(
                            "Instrument cache miss: position dropped for symbol={}, account={}",
                            pos_msg.symbol,
                            pos_msg.account,
                        );
                        continue;
                    };
                    let report = parse_position_msg(&pos_msg, instrument, ts_init);
                    emitter.send_position_report(report);
                }
            }
            BitmexTableMessage::Wallet { data, .. } => {
                if !state.margin_subscribed.load(Ordering::Relaxed) {
                    for wallet_msg in data {
                        let acct_state = parse_wallet_msg(&wallet_msg, ts_init);
                        emitter.send_account_state(acct_state);
                    }
                }
            }
            BitmexTableMessage::Margin { data, .. } => {
                state.margin_subscribed.store(true, Ordering::Relaxed);

                for margin_msg in data {
                    let acct_state = parse_margin_account_state(&margin_msg, ts_init);
                    emitter.send_account_state(acct_state);
                }
            }
            BitmexTableMessage::Instrument { action, data } => {
                if matches!(action, BitmexAction::Partial | BitmexAction::Insert) {
                    for msg in data {
                        match msg.try_into() {
                            Ok(http_inst) => match parse_instrument_any(&http_inst, ts_init) {
                                InstrumentParseResult::Ok(boxed) => {
                                    let inst = *boxed;
                                    let symbol = inst.symbol().inner();
                                    instruments_by_symbol.insert(symbol, inst);
                                }
                                InstrumentParseResult::Unsupported { .. }
                                | InstrumentParseResult::Inactive { .. } => {}
                                InstrumentParseResult::Failed { symbol, error, .. } => {
                                    log::warn!("Failed to parse instrument {symbol}: {error}");
                                }
                            },
                            Err(e) => {
                                log::debug!("Skipping instrument (missing required fields): {e}");
                            }
                        }
                    }
                }
            }
            BitmexTableMessage::OrderBookL2 { .. }
            | BitmexTableMessage::OrderBookL2_25 { .. }
            | BitmexTableMessage::OrderBook10 { .. }
            | BitmexTableMessage::Quote { .. }
            | BitmexTableMessage::Trade { .. }
            | BitmexTableMessage::TradeBin1m { .. }
            | BitmexTableMessage::TradeBin5m { .. }
            | BitmexTableMessage::TradeBin1h { .. }
            | BitmexTableMessage::TradeBin1d { .. }
            | BitmexTableMessage::Funding { .. } => {
                log::debug!("Ignoring BitMEX data message on execution stream");
            }
            _ => {
                log::warn!("Unhandled table message type on execution stream");
            }
        },
        BitmexWsMessage::Reconnected => {
            order_type_cache.clear();
            order_symbol_cache.clear();
            log::info!("BitMEX execution websocket reconnected");
        }
        BitmexWsMessage::Authenticated => {
            log::debug!("BitMEX execution websocket authenticated");
        }
    }
}

/// Dispatches order messages, routing tracked orders to events and untracked
/// orders to reports.
#[expect(clippy::too_many_arguments)]
fn dispatch_order_messages(
    data: Vec<OrderData>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    order_type_cache: &mut AHashMap<ClientOrderId, OrderType>,
    order_symbol_cache: &mut AHashMap<ClientOrderId, Ustr>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    for order_data in data {
        match order_data {
            OrderData::Full(order_msg) => {
                let Some(instrument) = instruments_by_symbol.get(&order_msg.symbol) else {
                    log::error!(
                        "Instrument cache miss: order dropped for symbol={}, order_id={}",
                        order_msg.symbol,
                        order_msg.order_id,
                    );
                    continue;
                };

                let client_order_id = order_msg.cl_ord_id.map(ClientOrderId::new);

                // Update caches before tombstone check so execution messages
                // that arrive later can still resolve the symbol
                if let Some(ref cid) = client_order_id {
                    if let Some(ord_type) = &order_msg.ord_type {
                        let order_type: OrderType = if *ord_type == BitmexOrderType::Pegged
                            && order_msg.peg_price_type == Some(BitmexPegPriceType::TrailingStopPeg)
                        {
                            if order_msg.price.is_some() {
                                OrderType::TrailingStopLimit
                            } else {
                                OrderType::TrailingStopMarket
                            }
                        } else {
                            (*ord_type).into()
                        };
                        order_type_cache.insert(*cid, order_type);
                    }
                    order_symbol_cache.insert(*cid, order_msg.symbol);
                }

                // Skip tombstoned orders (already handled by HTTP cancel path)
                if let Some(ref cid) = client_order_id
                    && state.is_tombstoned(cid)
                {
                    log::debug!("Skipping tombstoned order {cid}");
                    continue;
                }

                let identity = client_order_id
                    .and_then(|cid| state.order_identities.get(&cid).map(|r| (cid, r.clone())));

                if let Some((cid, ident)) = identity {
                    // Tracked order: produce order events
                    if let Some(event) = parse_order_event(
                        &order_msg,
                        cid,
                        account_id,
                        emitter.trader_id(),
                        ident.strategy_id,
                        ts_init,
                    ) {
                        let venue_order_id = VenueOrderId::new(order_msg.order_id.to_string());
                        dispatch_parsed_order_event(
                            event,
                            cid,
                            account_id,
                            venue_order_id,
                            &ident,
                            emitter,
                            state,
                            ts_init,
                        );
                    }

                    // Clean up caches on terminal status
                    if order_msg.ord_status.is_terminal() {
                        order_type_cache.remove(&cid);
                        order_symbol_cache.remove(&cid);
                    }
                } else {
                    // Untracked order: fall back to report
                    match parse_order_msg(&order_msg, instrument, order_type_cache, ts_init) {
                        Ok(report) => {
                            if report.order_status.is_closed()
                                && let Some(cid) = report.client_order_id
                            {
                                order_type_cache.remove(&cid);
                                order_symbol_cache.remove(&cid);
                            }
                            emitter.send_order_status_report(report);
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to parse order report: error={e}, symbol={}, order_id={}",
                                order_msg.symbol,
                                order_msg.order_id,
                            );
                        }
                    }
                }
            }
            OrderData::Update(msg) => {
                let Some(instrument) = instruments_by_symbol.get(&msg.symbol) else {
                    log::error!(
                        "Instrument cache miss: order update dropped for symbol={}, order_id={}",
                        msg.symbol,
                        msg.order_id,
                    );
                    continue;
                };

                // Populate cache for execution message routing
                if let Some(cl_ord_id) = &msg.cl_ord_id {
                    let client_order_id = ClientOrderId::new(cl_ord_id);
                    order_symbol_cache.insert(client_order_id, msg.symbol);
                }

                let identity = msg.cl_ord_id.as_ref().and_then(|cl| {
                    let cid = ClientOrderId::new(cl);
                    state.order_identities.get(&cid).map(|r| (cid, r.clone()))
                });

                if let Some((cid, ident)) = identity {
                    // Tracked: enrich with identity context
                    if let Some(event) =
                        parse_order_update_msg(&msg, instrument, account_id, ts_init)
                    {
                        let enriched = OrderUpdated::new(
                            emitter.trader_id(),
                            ident.strategy_id,
                            event.instrument_id,
                            cid,
                            event.quantity,
                            event.event_id,
                            event.ts_event,
                            event.ts_init,
                            false,
                            event.venue_order_id,
                            Some(account_id),
                            event.price,
                            event.trigger_price,
                            event.protection_price,
                            false, // is_quote_quantity
                        );
                        ensure_accepted_emitted(
                            cid,
                            account_id,
                            enriched
                                .venue_order_id
                                .unwrap_or_else(|| VenueOrderId::new(msg.order_id.to_string())),
                            &ident,
                            emitter,
                            state,
                            ts_init,
                        );
                        emitter.send_order_event(OrderEventAny::Updated(enriched));
                    } else {
                        log::warn!(
                            "Skipped order update (insufficient data): order_id={}, price={:?}",
                            msg.order_id,
                            msg.price,
                        );
                    }
                } else {
                    log::debug!(
                        "Skipping order update for untracked order: order_id={}",
                        msg.order_id,
                    );
                }
            }
        }
    }
}

/// Dispatches execution (fill) messages, routing tracked orders to
/// `OrderFilled` events and untracked orders to `FillReport`.
fn dispatch_execution_messages(
    data: Vec<BitmexExecutionMsg>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    order_symbol_cache: &AHashMap<ClientOrderId, Ustr>,
    ts_init: UnixNanos,
) {
    for exec_msg in data {
        let symbol_opt = if let Some(sym) = &exec_msg.symbol {
            Some(*sym)
        } else if let Some(cl_ord_id) = &exec_msg.cl_ord_id {
            let client_order_id = ClientOrderId::new(cl_ord_id);
            order_symbol_cache.get(&client_order_id).copied()
        } else {
            None
        };

        let Some(symbol) = symbol_opt else {
            if let Some(cl_ord_id) = &exec_msg.cl_ord_id {
                if exec_msg.exec_type == Some(BitmexExecType::Trade) {
                    log::warn!(
                        "Execution missing symbol and not in cache: \
                        cl_ord_id={cl_ord_id}, exec_id={:?}",
                        exec_msg.exec_id,
                    );
                } else {
                    log::debug!(
                        "Execution missing symbol and not in cache: \
                        cl_ord_id={cl_ord_id}, exec_type={:?}",
                        exec_msg.exec_type,
                    );
                }
            } else if exec_msg.exec_type == Some(BitmexExecType::CancelReject) {
                log::debug!(
                    "CancelReject missing symbol/clOrdID (expected with redundant cancels): \
                    exec_id={:?}, order_id={:?}",
                    exec_msg.exec_id,
                    exec_msg.order_id,
                );
            } else {
                log::warn!(
                    "Execution missing both symbol and clOrdID: \
                    exec_id={:?}, order_id={:?}, exec_type={:?}",
                    exec_msg.exec_id,
                    exec_msg.order_id,
                    exec_msg.exec_type,
                );
            }
            continue;
        };

        let Some(instrument) = instruments_by_symbol.get(&symbol) else {
            log::error!(
                "Instrument cache miss: execution dropped for symbol={}, exec_id={:?}, exec_type={:?}",
                symbol,
                exec_msg.exec_id,
                exec_msg.exec_type,
            );
            continue;
        };

        let Some(fill) = parse_execution_msg(exec_msg, instrument, ts_init) else {
            continue;
        };

        let identity = fill
            .client_order_id
            .and_then(|cid| state.order_identities.get(&cid).map(|r| (cid, r.clone())));

        if let Some((cid, ident)) = identity {
            // Tracked: produce OrderFilled event
            let venue_order_id = fill.venue_order_id;
            ensure_accepted_emitted(
                cid,
                fill.account_id,
                venue_order_id,
                &ident,
                emitter,
                state,
                ts_init,
            );
            state.insert_filled(cid);
            state.remove_triggered(&cid);
            let filled = fill_report_to_order_filled(
                &fill,
                emitter.trader_id(),
                &ident,
                instrument.quote_currency(),
            );
            emitter.send_order_event(OrderEventAny::Filled(filled));
        } else {
            // Untracked: forward as FillReport
            emitter.send_fill_report(fill);
        }
    }
}

/// Dispatches a parsed order event with lifecycle synthesis and deduplication.
///
/// Guarantees the `Submitted -> Accepted -> ...` lifecycle by synthesizing
/// `OrderAccepted` before any other event when one has not yet been emitted.
#[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn dispatch_parsed_order_event(
    event: ParsedOrderEvent,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    let is_terminal;

    match event {
        ParsedOrderEvent::Accepted(e) => {
            if state.accepted_contains(&client_order_id)
                || state.filled_contains(&client_order_id)
                || state.triggered_contains(&client_order_id)
            {
                log::debug!("Skipping duplicate Accepted for {client_order_id}");
                return;
            }
            state.insert_accepted(client_order_id);
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Accepted(e));
        }
        ParsedOrderEvent::Triggered(e) => {
            if state.filled_contains(&client_order_id) {
                log::debug!("Skipping stale Triggered for {client_order_id} (already filled)");
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
            state.remove_triggered(&client_order_id);
            state.remove_filled(&client_order_id);
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
            state.remove_triggered(&client_order_id);
            state.remove_filled(&client_order_id);
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Expired(e));
        }
        ParsedOrderEvent::Rejected(e) => {
            state.remove_triggered(&client_order_id);
            state.remove_filled(&client_order_id);
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Rejected(e));
        }
    }

    if is_terminal {
        state.order_identities.remove(&client_order_id);
        state.remove_accepted(&client_order_id);
    }
}

/// Synthesizes and emits `OrderAccepted` if one has not yet been emitted for
/// this order. Handles fast-filling orders that skip the `New` state.
fn ensure_accepted_emitted(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if state.accepted_contains(&client_order_id) {
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
pub(crate) fn fill_report_to_order_filled(
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
