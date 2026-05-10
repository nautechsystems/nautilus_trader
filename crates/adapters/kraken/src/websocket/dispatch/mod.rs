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

//! WebSocket execution dispatch for the Kraken Spot and Futures clients.
//!
//! Implements the two-tier execution dispatch contract from
//! `docs/developer_guide/adapters.md` (lines 1232-1296):
//!
//! 1. The execution client registers an [`OrderIdentity`] in [`WsDispatchState`]
//!    when it submits an order.
//! 2. WebSocket execution messages are routed through the per-product dispatch
//!    functions in [`futures`] and [`spot`]. For tracked orders the dispatch
//!    builds typed [`OrderEventAny`] events and emits them directly via
//!    [`ExecutionEventEmitter::send_order_event`]. For untracked / external
//!    orders the dispatch falls back to `OrderStatusReport` / `FillReport`
//!    so the engine can reconcile.
//!
//! The dispatch state lives in an `Arc<WsDispatchState>` shared between the
//! main client thread (which registers identities at submission time) and the
//! spawned WebSocket consumer task. `DashMap`/`DashSet` provide lock-free
//! concurrent access.

pub mod futures;
pub mod spot;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use dashmap::{DashMap, DashSet};
use indexmap::IndexSet;
use nautilus_core::{AtomicMap, UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::{OrderAccepted, OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId, VenueOrderId,
    },
    instruments::InstrumentAny,
    reports::FillReport,
    types::{Currency, Quantity},
};

use crate::common::consts::KRAKEN_VENUE;

const DEDUP_CAPACITY: usize = 10_000;

/// Snapshot of the mutable fields seen on a tracked `OpenOrdersDelta`.
///
/// Used by the futures delta path to discriminate partial fills (filled
/// increased), modify acknowledgements (qty / price / trigger_price changed),
/// and no-op deltas (nothing changed) when a follow-up delta arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeltaSnapshot {
    pub qty: Quantity,
    pub filled: Quantity,
    pub limit_price_bits: Option<u64>,
    pub stop_price_bits: Option<u64>,
}

impl DeltaSnapshot {
    pub(crate) fn new(
        qty: Quantity,
        filled: Quantity,
        limit_price: Option<f64>,
        stop_price: Option<f64>,
    ) -> Self {
        Self {
            qty,
            filled,
            limit_price_bits: limit_price.map(f64::to_bits),
            stop_price_bits: stop_price.map(f64::to_bits),
        }
    }

    pub(crate) fn non_fill_fields_match(&self, other: &Self) -> bool {
        self.qty == other.qty
            && self.limit_price_bits == other.limit_price_bits
            && self.stop_price_bits == other.stop_price_bits
    }
}

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
    /// Order quantity captured at submission. Used to detect terminal fills.
    pub quantity: Quantity,
}

/// Per-client dispatch state shared between order submission and the
/// WebSocket consumer task.
///
/// Tracks which orders were submitted through this client (so we can route
/// venue events to typed [`OrderEventAny`] emissions for tracked orders, and
/// fall back to reports for external orders), and provides cross-stream
/// dedup for `OrderAccepted` and `OrderFilled` emissions.
#[derive(Debug)]
pub struct WsDispatchState {
    /// Tracked orders keyed by full Nautilus [`ClientOrderId`].
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    /// Client order IDs for which an `OrderAccepted` event has been emitted.
    pub emitted_accepted: DashSet<ClientOrderId>,
    /// Client order IDs that have reached the filled terminal state.
    pub filled_orders: DashSet<ClientOrderId>,
    /// Last snapshot of qty / filled / price / trigger_price seen on a
    /// tracked `OpenOrdersDelta`.
    ///
    /// The futures delta path uses this map to discriminate partial-fill
    /// notifications (the new delta carries `filled` greater than the
    /// previously seen value), modify acknowledgements (a non-fill field
    /// changed), and pure no-op deltas (nothing changed). It is updated only
    /// by the delta path so that the fill path's own cumulative is not
    /// double-counted.
    pub delta_snapshots: DashMap<ClientOrderId, DeltaSnapshot>,
    /// Cumulative filled quantity per tracked client order id, populated by
    /// the fill side of dispatch.
    ///
    /// Compared against `OrderIdentity::quantity` to decide when to clean up
    /// tracked state on a terminal fill.
    pub order_filled_qty: DashMap<ClientOrderId, Quantity>,
    /// Trade IDs for which an `OrderFilled` event has been emitted.
    ///
    /// Bounded FIFO dedup: when capacity is reached, the oldest entry is
    /// evicted on the next insert. A simple `clear()` at the threshold would
    /// drop all recent trade IDs at once, opening a window where a reconnect
    /// or replay immediately after the rollover could re-emit duplicate
    /// `OrderFilled` events.
    pub emitted_trades: Mutex<IndexSet<TradeId>>,
    clearing: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            emitted_accepted: DashSet::default(),
            filled_orders: DashSet::default(),
            delta_snapshots: DashMap::new(),
            order_filled_qty: DashMap::new(),
            emitted_trades: Mutex::new(IndexSet::with_capacity(DEDUP_CAPACITY)),
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
    /// duplicate), `false` otherwise. When the dedup set is at capacity the
    /// oldest entry is evicted to make room, preserving the `DEDUP_CAPACITY`
    /// most recently seen trade IDs.
    #[expect(
        clippy::missing_panics_doc,
        reason = "dedup mutex poisoning is not expected"
    )]
    pub fn check_and_insert_trade(&self, trade_id: TradeId) -> bool {
        let mut set = self.emitted_trades.lock().expect("dedup mutex poisoned");

        if set.contains(&trade_id) {
            return true;
        }

        if set.len() >= DEDUP_CAPACITY {
            set.shift_remove_index(0);
        }

        set.insert(trade_id);
        false
    }

    /// Removes all dispatch state for an order that has reached a terminal state.
    pub fn cleanup_terminal(&self, client_order_id: &ClientOrderId) {
        self.order_identities.remove(client_order_id);
        self.emitted_accepted.remove(client_order_id);
        self.order_filled_qty.remove(client_order_id);
        self.delta_snapshots.remove(client_order_id);
    }

    /// Records cumulative filled quantity for a tracked order. Used by the
    /// fill side of dispatch only.
    pub fn record_filled_qty(&self, client_order_id: ClientOrderId, qty: Quantity) {
        self.order_filled_qty.insert(client_order_id, qty);
    }

    /// Returns the previously recorded cumulative filled quantity, if any.
    #[must_use]
    pub fn previous_filled_qty(&self, client_order_id: &ClientOrderId) -> Option<Quantity> {
        self.order_filled_qty.get(client_order_id).map(|r| *r)
    }

    /// Records the latest delta snapshot for a tracked order. Used by the
    /// delta side of dispatch only.
    pub fn record_delta_snapshot(&self, client_order_id: ClientOrderId, snapshot: DeltaSnapshot) {
        self.delta_snapshots.insert(client_order_id, snapshot);
    }

    /// Returns the previously recorded delta snapshot, if any.
    #[must_use]
    pub fn previous_delta_snapshot(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<DeltaSnapshot> {
        self.delta_snapshots.get(client_order_id).map(|r| *r)
    }

    /// Updates the tracked `quantity` for an order following a successful
    /// modify acknowledgement, leaving all other identity fields untouched.
    pub fn update_identity_quantity(&self, client_order_id: &ClientOrderId, quantity: Quantity) {
        if let Some(mut entry) = self.order_identities.get_mut(client_order_id) {
            entry.quantity = quantity;
        }
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

/// Resolves a Kraken-truncated client order id to its full Nautilus form.
///
/// Kraken truncates non-UUID client order ids to 18 characters; the truncation
/// map is populated at submission time so the WebSocket consumer can recover
/// the original id. Falls back to constructing a fresh `ClientOrderId` from
/// the truncated string when no mapping exists (the order is then treated as
/// external by downstream lookup).
pub(crate) fn resolve_client_order_id(
    truncated: &str,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
) -> ClientOrderId {
    truncated_id_map
        .load()
        .get(truncated)
        .copied()
        .unwrap_or_else(|| ClientOrderId::new(truncated))
}

/// Synthesizes and emits an `OrderAccepted` event when one has not yet been
/// emitted for the given order.
///
/// Used before emitting non-Accepted events (Filled, Canceled, Expired,
/// Updated) so that strategies always observe the canonical
/// `Submitted -> Accepted -> ...` lifecycle even when the venue compresses
/// the acceptance and follow-up event into a single message (fast fills).
#[expect(clippy::too_many_arguments)]
pub(crate) fn ensure_accepted_emitted(
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

/// Builds an [`OrderFilled`] event from a [`FillReport`] and tracked
/// [`OrderIdentity`].
pub(crate) fn fill_report_to_order_filled(
    report: &FillReport,
    trader_id: TraderId,
    identity: &OrderIdentity,
    quote_currency: Currency,
    client_order_id: ClientOrderId,
) -> OrderFilled {
    OrderFilled::new(
        trader_id,
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

/// Looks up an instrument from the shared instruments cache by raw symbol.
pub(crate) fn lookup_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    raw_symbol: &str,
) -> Option<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(raw_symbol), *KRAKEN_VENUE);
    instruments.load().get(&instrument_id).cloned()
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId},
    };
    use rstest::rstest;

    use super::*;

    fn make_identity() -> OrderIdentity {
        OrderIdentity {
            strategy_id: StrategyId::new("EXEC_TESTER-001"),
            instrument_id: InstrumentId::from("PF_XBTUSD.KRAKEN"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: Quantity::from("0.0001"),
        }
    }

    #[rstest]
    fn test_register_and_lookup_identity() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("uuid-1");
        state.register_identity(cid, make_identity());

        let found = state.lookup_identity(&cid);
        assert!(found.is_some());
        let identity = found.unwrap();
        assert_eq!(identity.strategy_id.as_str(), "EXEC_TESTER-001");
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
        let cid = ClientOrderId::new("uuid-2");
        assert!(!state.emitted_accepted.contains(&cid));
        state.insert_accepted(cid);
        assert!(state.emitted_accepted.contains(&cid));
        // Second insert is a no-op.
        state.insert_accepted(cid);
        assert!(state.emitted_accepted.contains(&cid));
    }

    #[rstest]
    fn test_check_and_insert_trade_detects_duplicates() {
        let state = WsDispatchState::new();
        let trade = TradeId::new("trade-1");
        // First insert: not a duplicate.
        assert!(!state.check_and_insert_trade(trade));
        // Second insert: duplicate.
        assert!(state.check_and_insert_trade(trade));
    }

    #[rstest]
    fn test_check_and_insert_trade_fifo_eviction_preserves_recent_ids() {
        // Verifies the dedup window slides rather than collapsing to zero at
        // the capacity threshold. Overshooting by one entry must evict only
        // the oldest (`trade-0`), leaving every other ID still deduped.
        let state = WsDispatchState::new();
        for i in 0..DEDUP_CAPACITY {
            let trade = TradeId::new(format!("trade-{i}").as_str());
            assert!(!state.check_and_insert_trade(trade));
        }
        // At capacity; the next insert evicts `trade-0`.
        let overflow = TradeId::new(format!("trade-{DEDUP_CAPACITY}").as_str());
        assert!(!state.check_and_insert_trade(overflow));

        // Inspect the dedup set directly to confirm FIFO behaviour without
        // perturbing state via another `check_and_insert_trade` call.
        let set = state.emitted_trades.lock().expect("dedup mutex poisoned");
        assert_eq!(set.len(), DEDUP_CAPACITY);
        assert!(
            !set.contains(&TradeId::new("trade-0")),
            "oldest entry should have been evicted",
        );
        assert!(
            set.contains(&TradeId::new("trade-1")),
            "second-oldest remains"
        );
        assert!(
            set.contains(&TradeId::new(
                format!("trade-{}", DEDUP_CAPACITY - 1).as_str(),
            )),
            "most-recent pre-overflow entry remains",
        );
        assert!(
            set.contains(&overflow),
            "the overflow entry was inserted after eviction",
        );
    }

    #[rstest]
    fn test_cleanup_terminal_removes_state() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new("uuid-3");
        state.register_identity(cid, make_identity());
        state.insert_accepted(cid);

        assert!(state.lookup_identity(&cid).is_some());
        assert!(state.emitted_accepted.contains(&cid));

        state.cleanup_terminal(&cid);

        assert!(state.lookup_identity(&cid).is_none());
        assert!(!state.emitted_accepted.contains(&cid));
    }

    #[rstest]
    fn test_resolve_client_order_id_via_truncated_map() {
        let map: Arc<AtomicMap<String, ClientOrderId>> = Arc::new(AtomicMap::new());
        let full = ClientOrderId::new("full-uuid-12345");
        map.insert("trunc-id".to_string(), full);

        let resolved = resolve_client_order_id("trunc-id", &map);
        assert_eq!(resolved, full);
    }

    #[rstest]
    fn test_resolve_client_order_id_falls_back_to_input() {
        let map: Arc<AtomicMap<String, ClientOrderId>> = Arc::new(AtomicMap::new());
        let resolved = resolve_client_order_id("unknown", &map);
        assert_eq!(resolved.as_str(), "unknown");
    }
}
