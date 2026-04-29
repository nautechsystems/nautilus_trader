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

//! WebSocket execution dispatch for the Hyperliquid execution client.
//!
//! Implements the two-tier execution dispatch contract from
//! `docs/developer_guide/adapters.md` (lines 1232-1296):
//!
//! 1. The execution client registers an [`OrderIdentity`] in [`WsDispatchState`]
//!    when it submits an order, and refreshes the cached venue order id when a
//!    modify is sent so the WebSocket consumer can detect cancel-replace.
//! 2. Incoming [`OrderStatusReport`] and [`FillReport`] messages are routed
//!    through [`dispatch_order_status_report`] and [`dispatch_fill_report`].
//!    For tracked orders these build typed [`OrderEventAny`] events and emit
//!    them via [`ExecutionEventEmitter::send_order_event`]. For untracked /
//!    external orders the dispatch falls back to forwarding the raw report.
//!
//! The dispatch state lives in an `Arc<WsDispatchState>` shared between the
//! main client task (which registers identities at submission time) and the
//! spawned WebSocket consumer task.
//!
//! # GH-3827 cancel-replace handling
//!
//! Hyperliquid implements `modify` as a cancel-and-replace: the venue emits an
//! `ACCEPTED(new_voi)` together with a `CANCELED(old_voi)` under the same
//! `client_order_id`. The dispatch detects the replacement leg by comparing
//! `report.venue_order_id` to the last cached value, promotes it to an
//! `OrderUpdated` event, and suppresses the stale cancel so strategies never
//! observe a spurious termination.
//!
//! The pending-modify marker (keyed on `client_order_id`) is set by
//! `modify_order` only after a successful HTTP round-trip and cleared on the
//! matching `ACCEPTED`. It lets dispatch skip an early
//! `CANCELED(old_voi)` that arrives before the replacement `ACCEPTED(new_voi)`
//! on the WebSocket. The documented transport-timeout + WS-race window still
//! applies: if the modify HTTP call fails but the venue actually accepted it,
//! no marker is set, so a cancel-before-accept race in that window would
//! surface as `OrderCanceled`. This matches the Python behaviour we are
//! porting and is the simplest correct answer; verifying the venue-side
//! outcome before emitting would require speculative waiting that drops
//! latency without improving correctness.

use std::{
    collections::VecDeque,
    hash::Hash,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashSet;
use dashmap::{DashMap, DashSet};
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use ustr::Ustr;

pub const DEDUP_CAPACITY: usize = 10_000;

/// Identity metadata captured when an order is submitted through this client.
///
/// Stored in [`WsDispatchState::order_identities`] keyed by the full Nautilus
/// [`ClientOrderId`]. The dispatch functions use the identity to build typed
/// order events for tracked orders without needing access to the engine cache
/// (which is `!Send` and unreachable from the spawned WebSocket task).
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    /// Strategy that owns the order.
    pub strategy_id: StrategyId,
    /// Instrument the order targets.
    pub instrument_id: InstrumentId,
    /// Order side captured at submission.
    pub order_side: OrderSide,
    /// Order type captured at submission.
    pub order_type: OrderType,
    /// Order quantity captured at submission.
    pub quantity: Quantity,
    /// Last known order price. Populated on submission and refreshed from
    /// subsequent status reports so a cancel-replace `ACCEPTED` that omits
    /// `price` can still produce an `OrderUpdated` carrying an accurate value.
    pub price: Option<Price>,
}

/// Bounded FIFO deduplication set.
///
/// When the capacity is reached, the oldest entry is evicted on the next
/// insert. A simple `clear()` at the threshold would drop every recent trade
/// id at once, opening a window where a reconnect or replay right after the
/// rollover could re-emit duplicate `OrderFilled` events; the FIFO window
/// slides instead.
#[derive(Debug)]
pub struct BoundedDedup<T>
where
    T: Eq + Hash + Clone,
{
    order: VecDeque<T>,
    set: AHashSet<T>,
    capacity: usize,
}

impl<T> BoundedDedup<T>
where
    T: Eq + Hash + Clone,
{
    /// Creates a new bounded dedup set with the given `capacity`.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::with_capacity(capacity),
            set: AHashSet::with_capacity(capacity),
            capacity,
        }
    }

    /// Inserts a value. Returns `true` when the value was already present.
    pub fn insert(&mut self, value: T) -> bool {
        if self.set.contains(&value) {
            return true;
        }

        if self.order.len() >= self.capacity
            && let Some(evicted) = self.order.pop_front()
        {
            self.set.remove(&evicted);
        }

        self.order.push_back(value.clone());
        self.set.insert(value);
        false
    }

    /// Returns the number of entries currently tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Returns whether the dedup set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Returns whether the value is currently tracked.
    #[must_use]
    pub fn contains(&self, value: &T) -> bool {
        self.set.contains(value)
    }
}

/// Per-client dispatch state shared between order submission and the
/// WebSocket consumer task.
///
/// Tracks which orders were submitted through this client (so we can route
/// venue events to typed [`OrderEventAny`] emissions for tracked orders and
/// fall back to reports for external orders), provides cross-stream dedup
/// for `OrderAccepted` and `OrderFilled` emissions, and carries the
/// GH-3827 cancel-replace state (`cached_venue_order_ids` and
/// `pending_modify_keys`).
#[derive(Debug)]
pub struct WsDispatchState {
    /// Tracked orders keyed by full Nautilus [`ClientOrderId`].
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    /// Client order IDs for which an `OrderAccepted` event has been emitted.
    pub emitted_accepted: DashSet<ClientOrderId>,
    /// Client order IDs that have reached the filled terminal state.
    ///
    /// Retained past `cleanup_terminal` so that late replay of the same
    /// status or fill does not re-emit events.
    pub filled_orders: DashSet<ClientOrderId>,
    /// Trade IDs for which an `OrderFilled` event has been emitted.
    ///
    /// Bounded FIFO dedup to bound memory while keeping recent trade ids
    /// deduped across reconnects.
    pub emitted_trades: Mutex<BoundedDedup<TradeId>>,
    /// Last venue order id observed for a tracked client order id.
    ///
    /// Populated on the first `OrderAccepted` and refreshed on every
    /// cancel-replace promotion. A later `ACCEPTED` with a different venue
    /// order id under the same client order id is treated as the
    /// replacement leg of a Hyperliquid modify and emitted as `OrderUpdated`.
    pub cached_venue_order_ids: DashMap<ClientOrderId, VenueOrderId>,
    /// Maps `client_order_id` to the old venue order id of an in-flight
    /// modify. Populated by `modify_order` only after a successful HTTP
    /// round-trip and cleared on the matching `ACCEPTED(new_voi)`. A
    /// `CANCELED(old_voi)` arriving while the marker is set is treated as
    /// the cancel leg of a cancel-before-accept race and suppressed so the
    /// later `ACCEPTED(new_voi)` can flow through the `OrderUpdated` path.
    pub pending_modify_keys: DashMap<ClientOrderId, VenueOrderId>,
    /// Cumulative filled quantity per tracked order. Compared against
    /// `OrderIdentity::quantity` to decide when to clean up tracked state.
    pub order_filled_qty: DashMap<ClientOrderId, Quantity>,
    clearing: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            emitted_accepted: DashSet::default(),
            filled_orders: DashSet::default(),
            emitted_trades: Mutex::new(BoundedDedup::new(DEDUP_CAPACITY)),
            cached_venue_order_ids: DashMap::new(),
            pending_modify_keys: DashMap::new(),
            order_filled_qty: DashMap::new(),
            clearing: AtomicBool::new(false),
        }
    }
}

impl WsDispatchState {
    /// Creates a new empty dispatch state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an order identity. Called by the execution client at order
    /// submission time, before any WebSocket events for the order can arrive.
    pub fn register_identity(&self, client_order_id: ClientOrderId, identity: OrderIdentity) {
        self.order_identities.insert(client_order_id, identity);
    }

    /// Returns a clone of the identity for the given client order id, if any.
    #[must_use]
    pub fn lookup_identity(&self, client_order_id: &ClientOrderId) -> Option<OrderIdentity> {
        self.order_identities
            .get(client_order_id)
            .map(|r| r.clone())
    }

    /// Refreshes the tracked price for a modify ack when the new report
    /// carries an updated price.
    pub fn update_identity_price(&self, client_order_id: &ClientOrderId, price: Option<Price>) {
        if let Some(price) = price
            && let Some(mut entry) = self.order_identities.get_mut(client_order_id)
        {
            entry.price = Some(price);
        }
    }

    /// Refreshes the tracked quantity for a modify ack.
    pub fn update_identity_quantity(&self, client_order_id: &ClientOrderId, quantity: Quantity) {
        if let Some(mut entry) = self.order_identities.get_mut(client_order_id) {
            entry.quantity = quantity;
        }
    }

    /// Marks an `OrderAccepted` event as emitted for this order.
    pub fn insert_accepted(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.emitted_accepted);
        self.emitted_accepted.insert(cid);
    }

    /// Marks an order as having reached the filled terminal state.
    pub fn insert_filled(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.filled_orders);
        self.filled_orders.insert(cid);
    }

    /// Atomically inserts a trade id into the dedup set.
    ///
    /// Returns `true` when the trade was already present (i.e. it is a
    /// duplicate), `false` otherwise.
    #[allow(
        clippy::missing_panics_doc,
        reason = "dedup mutex poisoning is not expected"
    )]
    pub fn check_and_insert_trade(&self, trade_id: TradeId) -> bool {
        let mut set = self.emitted_trades.lock().expect(MUTEX_POISONED);
        set.insert(trade_id)
    }

    /// Caches the venue order id observed for a tracked client order id.
    pub fn record_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        self.cached_venue_order_ids
            .insert(client_order_id, venue_order_id);
    }

    /// Returns the previously cached venue order id, if any.
    #[must_use]
    pub fn cached_venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.cached_venue_order_ids.get(client_order_id).map(|r| *r)
    }

    /// Marks an in-flight modify for cancel-before-accept suppression.
    pub fn mark_pending_modify(
        &self,
        client_order_id: ClientOrderId,
        old_venue_order_id: VenueOrderId,
    ) {
        self.pending_modify_keys
            .insert(client_order_id, old_venue_order_id);
    }

    /// Clears the pending modify marker for a client order id.
    pub fn clear_pending_modify(&self, client_order_id: &ClientOrderId) {
        self.pending_modify_keys.remove(client_order_id);
    }

    /// Returns the pending modify marker for a client order id, if any.
    #[must_use]
    pub fn pending_modify(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.pending_modify_keys.get(client_order_id).map(|r| *r)
    }

    /// Records cumulative filled quantity for a tracked order.
    pub fn record_filled_qty(&self, client_order_id: ClientOrderId, qty: Quantity) {
        self.order_filled_qty.insert(client_order_id, qty);
    }

    /// Returns the previously recorded cumulative filled quantity, if any.
    #[must_use]
    pub fn previous_filled_qty(&self, client_order_id: &ClientOrderId) -> Option<Quantity> {
        self.order_filled_qty.get(client_order_id).map(|r| *r)
    }

    /// Removes all dispatch state for an order that has reached a terminal state.
    ///
    /// `filled_orders` is intentionally *not* cleared here: the marker is
    /// used to suppress stale replays and must outlive the identity cleanup.
    pub fn cleanup_terminal(&self, client_order_id: &ClientOrderId) {
        self.order_identities.remove(client_order_id);
        self.emitted_accepted.remove(client_order_id);
        self.cached_venue_order_ids.remove(client_order_id);
        self.pending_modify_keys.remove(client_order_id);
        self.order_filled_qty.remove(client_order_id);
    }

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
}

/// Outcome of a single dispatch call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The report was for a tracked order. Typed events have been emitted
    /// (or intentionally skipped, e.g. dedup hit). The caller must not
    /// forward the report as a fallback.
    Tracked,
    /// The report is for an external / untracked order. The caller should
    /// forward the report via [`ExecutionEventEmitter::send_order_status_report`]
    /// or [`ExecutionEventEmitter::send_fill_report`] so the engine can
    /// reconcile.
    External,
    /// The report was recognised as stale (e.g. cancel leg of a
    /// cancel-replace modify, or replay after terminal state). The caller
    /// must drop it without forwarding.
    Skip,
}

/// Dispatches an [`OrderStatusReport`] using the two-tier routing contract.
///
/// Returns [`DispatchOutcome::Tracked`] when the report maps to a tracked
/// order (typed events have been emitted or dedup hit), [`External`] when
/// the caller should forward the report as an untracked fallback, or
/// [`Skip`] when the report is a stale / race leg that must be dropped.
///
/// [`External`]: DispatchOutcome::External
/// [`Skip`]: DispatchOutcome::Skip
pub fn dispatch_order_status_report(
    report: &OrderStatusReport,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    let Some(client_order_id) = report.client_order_id else {
        return DispatchOutcome::External;
    };

    if state.filled_orders.contains(&client_order_id) {
        log::debug!(
            "Skipping stale report for filled order: cid={client_order_id}, status={:?}",
            report.order_status,
        );
        return DispatchOutcome::Skip;
    }

    let Some(identity) = state.lookup_identity(&client_order_id) else {
        return DispatchOutcome::External;
    };

    match report.order_status {
        OrderStatus::Accepted => {
            handle_accepted(report, client_order_id, &identity, state, emitter, ts_init)
        }
        OrderStatus::Triggered => {
            handle_triggered(report, client_order_id, &identity, state, emitter, ts_init)
        }
        OrderStatus::Canceled => {
            handle_canceled(report, client_order_id, &identity, state, emitter, ts_init)
        }
        OrderStatus::Expired => {
            handle_expired(report, client_order_id, &identity, state, emitter, ts_init)
        }
        OrderStatus::Rejected => {
            handle_rejected(report, client_order_id, &identity, state, emitter, ts_init)
        }
        OrderStatus::Filled => handle_filled_marker(client_order_id, state),
        OrderStatus::PartiallyFilled => {
            // Fills come via `FillReport`; nothing to emit from the status path.
            DispatchOutcome::Tracked
        }
        OrderStatus::PendingUpdate
        | OrderStatus::PendingCancel
        | OrderStatus::Submitted
        | OrderStatus::Initialized
        | OrderStatus::Denied
        | OrderStatus::Released
        | OrderStatus::Emulated => DispatchOutcome::Tracked,
    }
}

/// Dispatches a [`FillReport`] using the two-tier routing contract.
///
/// Returns [`DispatchOutcome::Tracked`] when the fill has been emitted as
/// an `OrderFilled` event (or skipped via trade dedup), [`External`] when
/// the caller should forward the fill via
/// [`ExecutionEventEmitter::send_fill_report`], or [`Skip`] when the fill
/// is a replay for an already-terminal order and must be dropped.
///
/// [`External`]: DispatchOutcome::External
/// [`Skip`]: DispatchOutcome::Skip
pub fn dispatch_fill_report(
    report: &FillReport,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    let Some(client_order_id) = report.client_order_id else {
        return DispatchOutcome::External;
    };

    if state.filled_orders.contains(&client_order_id) {
        log::debug!(
            "Skipping stale fill for filled order: cid={client_order_id}, trade_id={}",
            report.trade_id,
        );
        return DispatchOutcome::Skip;
    }

    let Some(identity) = state.lookup_identity(&client_order_id) else {
        return DispatchOutcome::External;
    };

    if state.check_and_insert_trade(report.trade_id) {
        log::debug!(
            "Skipping duplicate fill for {client_order_id}: trade_id={}",
            report.trade_id
        );
        return DispatchOutcome::Tracked;
    }

    ensure_accepted_emitted(
        client_order_id,
        report.venue_order_id,
        report.account_id,
        &identity,
        state,
        emitter,
        report.ts_event,
        ts_init,
    );

    let filled = OrderFilled::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        report.venue_order_id,
        report.account_id,
        report.trade_id,
        identity.order_side,
        identity.order_type,
        report.last_qty,
        report.last_px,
        report.commission.currency,
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        ts_init,
        false,
        report.venue_position_id,
        Some(report.commission),
    );
    emitter.send_order_event(OrderEventAny::Filled(filled));

    let previous = state
        .previous_filled_qty(&client_order_id)
        .unwrap_or_else(|| Quantity::zero(report.last_qty.precision));
    let cumulative = previous + report.last_qty;
    state.record_filled_qty(client_order_id, cumulative);

    if cumulative >= identity.quantity {
        state.insert_filled(client_order_id);
        state.cleanup_terminal(&client_order_id);
    }

    DispatchOutcome::Tracked
}

fn handle_accepted(
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    let venue_order_id = report.venue_order_id;
    let ts_event = report.ts_last;
    let account_id = report.account_id;

    // Cancel-replace detection: if an earlier ACCEPTED cached a different
    // venue_order_id under the same client_order_id, this ACCEPTED is the
    // replacement leg of a Hyperliquid modify and must be promoted to
    // OrderUpdated. See GH-3827.
    if let Some(cached_voi) = state.cached_venue_order_id(&client_order_id)
        && cached_voi != venue_order_id
    {
        let price = report.price.or(identity.price);
        let Some(price) = price else {
            log::warn!(
                "Cannot emit OrderUpdated for cancel-replace {client_order_id}: \
                 no price on report and no cached price on identity",
            );
            return DispatchOutcome::Skip;
        };

        state.record_venue_order_id(client_order_id, venue_order_id);
        state.update_identity_quantity(&client_order_id, report.quantity);
        state.update_identity_price(&client_order_id, Some(price));
        state.clear_pending_modify(&client_order_id);

        let updated = OrderUpdated::new(
            emitter.trader_id(),
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            report.quantity,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
            Some(price),
            report.trigger_price,
            None,
            false,
        );
        emitter.send_order_event(OrderEventAny::Updated(updated));
        return DispatchOutcome::Tracked;
    }

    if state.emitted_accepted.contains(&client_order_id) {
        // Repeat ACCEPTED for an already-accepted order. Nothing to emit;
        // refresh the cached price so a subsequent cancel-replace without a
        // report price can still recover an accurate value.
        state.update_identity_price(&client_order_id, report.price);
        return DispatchOutcome::Tracked;
    }

    state.insert_accepted(client_order_id);
    state.record_venue_order_id(client_order_id, venue_order_id);
    state.update_identity_price(&client_order_id, report.price);

    let accepted = OrderAccepted::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
    );
    emitter.send_order_event(OrderEventAny::Accepted(accepted));
    DispatchOutcome::Tracked
}

fn handle_triggered(
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    if !matches!(
        identity.order_type,
        OrderType::StopLimit | OrderType::TrailingStopLimit | OrderType::LimitIfTouched
    ) {
        log::debug!(
            "Ignoring TRIGGERED status for non-triggerable order type {:?}: {client_order_id}",
            identity.order_type,
        );
        return DispatchOutcome::Tracked;
    }

    ensure_accepted_emitted(
        client_order_id,
        report.venue_order_id,
        report.account_id,
        identity,
        state,
        emitter,
        report.ts_last,
        ts_init,
    );

    let triggered = OrderTriggered::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        UUID4::new(),
        report.ts_last,
        ts_init,
        false,
        Some(report.venue_order_id),
        Some(report.account_id),
    );
    emitter.send_order_event(OrderEventAny::Triggered(triggered));
    DispatchOutcome::Tracked
}

fn handle_canceled(
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    let venue_order_id = report.venue_order_id;

    // Stale cancel suppression: if the cached venue_order_id has already
    // been advanced by a cancel-replace promotion, this CANCELED refers to
    // the old leg and has already been handled as OrderUpdated. See GH-3827.
    if let Some(cached_voi) = state.cached_venue_order_id(&client_order_id)
        && cached_voi != venue_order_id
    {
        log::debug!(
            "Skipping stale CANCELED for {venue_order_id} (cached {cached_voi}) on {client_order_id}",
        );
        return DispatchOutcome::Skip;
    }

    // Cancel-before-accept race: an in-flight modify may deliver
    // CANCELED(old_voi) before the replacement ACCEPTED(new_voi). The
    // pending marker (set only after a confirmed modify HTTP success) lets
    // us suppress the old leg so the later ACCEPTED can route through
    // OrderUpdated. See GH-3827.
    if let Some(pending_old) = state.pending_modify(&client_order_id)
        && pending_old == venue_order_id
    {
        log::debug!(
            "Skipping cancel-before-accept leg for {client_order_id}: venue_order_id={venue_order_id}",
        );
        return DispatchOutcome::Skip;
    }

    ensure_accepted_emitted(
        client_order_id,
        venue_order_id,
        report.account_id,
        identity,
        state,
        emitter,
        report.ts_last,
        ts_init,
    );

    let canceled = OrderCanceled::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        UUID4::new(),
        report.ts_last,
        ts_init,
        false,
        Some(venue_order_id),
        Some(report.account_id),
    );
    emitter.send_order_event(OrderEventAny::Canceled(canceled));

    // Retain the filled marker so any late replay of the cancel is
    // suppressed even after the identity state has been cleaned up.
    state.insert_filled(client_order_id);
    state.cleanup_terminal(&client_order_id);
    DispatchOutcome::Tracked
}

fn handle_expired(
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    ensure_accepted_emitted(
        client_order_id,
        report.venue_order_id,
        report.account_id,
        identity,
        state,
        emitter,
        report.ts_last,
        ts_init,
    );

    let expired = OrderExpired::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        UUID4::new(),
        report.ts_last,
        ts_init,
        false,
        Some(report.venue_order_id),
        Some(report.account_id),
    );
    emitter.send_order_event(OrderEventAny::Expired(expired));
    state.insert_filled(client_order_id);
    state.cleanup_terminal(&client_order_id);
    DispatchOutcome::Tracked
}

fn handle_rejected(
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_init: UnixNanos,
) -> DispatchOutcome {
    let reason = report
        .cancel_reason
        .clone()
        .unwrap_or_else(|| "Order rejected by exchange".to_string());
    let rejected = OrderRejected::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        report.account_id,
        Ustr::from(&reason),
        UUID4::new(),
        report.ts_last,
        ts_init,
        false,
        false,
    );
    emitter.send_order_event(OrderEventAny::Rejected(rejected));
    state.insert_filled(client_order_id);
    state.cleanup_terminal(&client_order_id);
    DispatchOutcome::Tracked
}

fn handle_filled_marker(
    _client_order_id: ClientOrderId,
    _state: &WsDispatchState,
) -> DispatchOutcome {
    // A status-only `FILLED` marker does not carry fill data; the actual
    // `OrderFilled` is emitted from `dispatch_fill_report` when the matching
    // trade arrives. Do *not* set `filled_orders` here, otherwise the
    // follow-up fill would be classified as a stale replay and dropped
    // before the terminal `OrderFilled` event can be emitted. The fill
    // path installs the marker itself once the cumulative fill quantity
    // matches the tracked order quantity.
    DispatchOutcome::Tracked
}

/// Synthesizes and emits an `OrderAccepted` event when one has not yet been
/// emitted for the given order.
///
/// Used before emitting non-Accepted events so strategies always observe the
/// canonical `Submitted -> Accepted -> ...` lifecycle even when the venue
/// compresses the placement and follow-up event into a single message (fast
/// fills).
#[allow(clippy::too_many_arguments)]
fn ensure_accepted_emitted(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    if state.emitted_accepted.contains(&client_order_id) {
        return;
    }
    state.insert_accepted(client_order_id);
    state.record_venue_order_id(client_order_id, venue_order_id);

    let accepted = OrderAccepted::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
    );
    emitter.send_order_event(OrderEventAny::Accepted(accepted));
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId};
    use rstest::rstest;

    use super::*;

    fn make_identity() -> OrderIdentity {
        OrderIdentity {
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: Quantity::from("0.0001"),
            price: None,
        }
    }

    #[rstest]
    fn test_register_and_lookup_identity() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("O-001");
        state.register_identity(cid, make_identity());

        let found = state.lookup_identity(&cid);
        assert!(found.is_some());
        let identity = found.unwrap();
        assert_eq!(identity.strategy_id.as_str(), "S-001");
        assert_eq!(identity.order_side, OrderSide::Buy);
    }

    #[rstest]
    fn test_lookup_identity_missing_returns_none() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("not-tracked");
        assert!(state.lookup_identity(&cid).is_none());
    }

    #[rstest]
    fn test_insert_accepted_dedup() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("O-002");
        assert!(!state.emitted_accepted.contains(&cid));
        state.insert_accepted(cid);
        assert!(state.emitted_accepted.contains(&cid));
        state.insert_accepted(cid);
        assert!(state.emitted_accepted.contains(&cid));
    }

    #[rstest]
    fn test_check_and_insert_trade_detects_duplicates() {
        let state = WsDispatchState::new();
        let trade = TradeId::new("trade-1");
        assert!(!state.check_and_insert_trade(trade));
        assert!(state.check_and_insert_trade(trade));
    }

    #[rstest]
    fn test_bounded_dedup_fifo_eviction_preserves_recent_ids() {
        let mut dedup: BoundedDedup<TradeId> = BoundedDedup::new(3);
        assert!(!dedup.insert(TradeId::new("t-0")));
        assert!(!dedup.insert(TradeId::new("t-1")));
        assert!(!dedup.insert(TradeId::new("t-2")));
        assert_eq!(dedup.len(), 3);

        // Overflow evicts the oldest.
        assert!(!dedup.insert(TradeId::new("t-3")));
        assert_eq!(dedup.len(), 3);
        assert!(!dedup.contains(&TradeId::new("t-0")));
        assert!(dedup.contains(&TradeId::new("t-1")));
        assert!(dedup.contains(&TradeId::new("t-3")));
    }

    #[rstest]
    fn test_pending_modify_roundtrip() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("O-010");
        let voi = VenueOrderId::new("v-1");

        assert!(state.pending_modify(&cid).is_none());
        state.mark_pending_modify(cid, voi);
        assert_eq!(state.pending_modify(&cid), Some(voi));
        state.clear_pending_modify(&cid);
        assert!(state.pending_modify(&cid).is_none());
    }

    #[rstest]
    fn test_cleanup_terminal_preserves_filled_marker() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("O-020");
        state.register_identity(cid, make_identity());
        state.insert_accepted(cid);
        state.insert_filled(cid);
        state.cleanup_terminal(&cid);

        assert!(state.lookup_identity(&cid).is_none());
        assert!(!state.emitted_accepted.contains(&cid));
        // `filled_orders` outlives `cleanup_terminal` so replays stay suppressed.
        assert!(state.filled_orders.contains(&cid));
    }
}
