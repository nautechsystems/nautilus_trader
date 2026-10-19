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

//! Shared state for the Derive execution WebSocket dispatch loop.
//!
//! Holds instrument precision, identity context for submitted orders and active
//! orders restored from the cache, and the cross-stream deduplication gates that
//! keep replay frames and concurrent `.orders` / `.trades` updates from emitting
//! duplicate events.
//!
//! Tracked orders (those whose identity was registered at submission or restored)
//! produce proper order events (`OrderAccepted`, `OrderFilled`, `OrderCanceled`,
//! `OrderExpired`, `OrderRejected`). Untracked frames fall back to execution
//! reports for downstream reconciliation.

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType},
    events::OrderEventAny,
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use parking_lot::{Mutex, ReentrantMutex, ReentrantMutexGuard};
use rust_decimal::Decimal;

use crate::http::models::{DeriveReplaceResult, DeriveTrade};

/// Capacity for the cross-source trade-id dedup cache. Sized to cover any
/// reconciliation lookback window plausible for live trading.
pub const TRADE_DEDUP_CAPACITY: usize = 4_096;

/// Capacity for the per-order accepted / filled dedup caches. Tracks active
/// and recently-terminal orders so reconnect replays do not re-emit lifecycle
/// events; need only span the live-stream replay window plus a margin.
pub const ORDER_DEDUP_CAPACITY: usize = 1_024;

/// Order identity captured at submission or restored so the dispatch task can build
/// proper order events without consulting the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

#[derive(Debug, Clone)]
pub(crate) struct ReplaceCompletion {
    pub old_venue_order_id: VenueOrderId,
    pub nonce: u64,
    pub identity: OrderIdentity,
    pub result: Result<DeriveReplaceResult, String>,
    pub trades: Vec<DeriveTrade>,
}

/// Shared dispatch state for the Derive WS execution loop.
///
/// Instrument precision populates from instrument definitions and governs
/// execution report values. `order_identities` populates on submission and from
/// active cached orders before streaming, and is consulted by both the `.orders`
/// and `.trades` dispatch paths to decide whether a frame belongs to a tracked or external
/// order. `pending_modifies` and `bound_venue_order_ids` track the in-flight
/// and current venue order id of a `private/replace` so the dispatch suppresses
/// events for the superseded leg.
#[derive(Debug)]
pub struct WsDispatchState {
    subaccount_id: u64,
    delivery: ReentrantMutex<()>,
    order_identities: Mutex<AHashMap<ClientOrderId, OrderIdentity>>,
    instrument_precisions: Mutex<AHashMap<InstrumentId, (u8, u8)>>,
    emitted_accepted: Mutex<AHashSet<ClientOrderId>>,
    triggers_active: Mutex<AHashSet<ClientOrderId>>,
    terminal_orders: Mutex<FifoCacheMap<ClientOrderId, OrderBinding, ORDER_DEDUP_CAPACITY>>,
    terminal_revision: Mutex<Arc<()>>,
    #[expect(
        clippy::type_complexity,
        reason = "order quantity, cumulative fills, and price share one lock for atomic updates"
    )]
    order_shapes: Mutex<AHashMap<ClientOrderId, (Quantity, Decimal, Option<Price>)>>,
    emitted_canceled: Mutex<FifoCache<ClientOrderId, ORDER_DEDUP_CAPACITY>>,
    filled_orders: Mutex<FifoCache<ClientOrderId, ORDER_DEDUP_CAPACITY>>,
    emitted_trades: Mutex<FifoCache<TradeId, TRADE_DEDUP_CAPACITY>>,
    bound_venue_order_ids: Mutex<AHashMap<ClientOrderId, VenueOrderId>>,
    pending_modifies: Mutex<AHashMap<ClientOrderId, VenueOrderId>>,
    modify_nonces: Mutex<AHashMap<ClientOrderId, u64>>,
    deferred_trades: Mutex<AHashMap<ClientOrderId, Vec<DeriveTrade>>>,
    undelivered_trades: Mutex<AHashMap<(String, String), DeriveTrade>>,
    replace_completions: Mutex<AHashMap<ClientOrderId, ReplaceCompletion>>,
    venue_order_legs: Mutex<AHashMap<ClientOrderId, AHashSet<VenueOrderId>>>,
    modify_targets: Mutex<AHashMap<ClientOrderId, (Quantity, Option<Price>)>>,
    unresolved_bindings: Mutex<AHashMap<ClientOrderId, AHashSet<VenueOrderId>>>,
}

impl WsDispatchState {
    /// Creates dispatch state scoped to one native Derive subaccount.
    #[must_use]
    pub fn new(subaccount_id: u64) -> Self {
        Self {
            subaccount_id,
            delivery: Default::default(),
            order_identities: Default::default(),
            instrument_precisions: Default::default(),
            emitted_accepted: Default::default(),
            triggers_active: Default::default(),
            terminal_orders: Default::default(),
            terminal_revision: Default::default(),
            order_shapes: Default::default(),
            emitted_canceled: Default::default(),
            filled_orders: Default::default(),
            emitted_trades: Default::default(),
            bound_venue_order_ids: Default::default(),
            pending_modifies: Default::default(),
            modify_nonces: Default::default(),
            deferred_trades: Default::default(),
            undelivered_trades: Default::default(),
            replace_completions: Default::default(),
            venue_order_legs: Default::default(),
            modify_targets: Default::default(),
            unresolved_bindings: Default::default(),
        }
    }

    pub(crate) const fn subaccount_id(&self) -> u64 {
        self.subaccount_id
    }

    pub(crate) fn owns_subaccount(&self, subaccount_id: i64) -> bool {
        u64::try_from(subaccount_id).ok() == Some(self.subaccount_id)
    }

    pub(crate) fn delivery_guard(&self) -> ReentrantMutexGuard<'_, ()> {
        self.delivery.lock()
    }

    /// Records price and size precision for execution report parsing.
    pub(crate) fn register_instrument_precision(
        &self,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) {
        self.instrument_precisions
            .lock()
            .insert(instrument_id, (price_precision, size_precision));
    }

    /// Returns price and size precision for an instrument, when registered.
    #[must_use]
    pub(crate) fn instrument_precision(&self, instrument_id: &InstrumentId) -> Option<(u8, u8)> {
        self.instrument_precisions
            .lock()
            .get(instrument_id)
            .copied()
    }

    /// Registers an order identity captured at submission so subsequent WS
    /// frames for the same client_order_id resolve to the tracked path.
    pub fn register_identity(&self, client_order_id: ClientOrderId, identity: OrderIdentity) {
        self.terminal_orders.lock().remove(&client_order_id);
        self.order_identities
            .lock()
            .insert(client_order_id, identity);
    }

    /// Returns the registered identity for a client order, when one was
    /// captured at submission time.
    #[must_use]
    pub fn identity(&self, client_order_id: &ClientOrderId) -> Option<OrderIdentity> {
        self.order_identities.lock().get(client_order_id).copied()
    }

    pub(crate) fn restore_order(&self, order: &OrderAny) {
        let _delivery = self.delivery_guard();
        let client_order_id = order.client_order_id();
        if self.identity(&client_order_id).is_some()
            || self.is_terminal(&client_order_id, order.instrument_id())
        {
            return;
        }

        self.register_identity(
            client_order_id,
            OrderIdentity {
                instrument_id: order.instrument_id(),
                strategy_id: order.strategy_id(),
                order_side: order.order_side(),
                order_type: order.order_type(),
            },
        );

        self.order_shapes.lock().insert(
            client_order_id,
            (
                order.filled_qty() + order.leaves_qty(),
                order.filled_qty().as_decimal(),
                order.price(),
            ),
        );

        if let Some(venue_order_id) = order.venue_order_id() {
            self.record_venue_order_id(client_order_id, venue_order_id);
            if order.status() == OrderStatus::PendingUpdate {
                self.mark_pending_modify(client_order_id, venue_order_id);
            }
        }

        let mut current_filled = false;

        for event in order.events() {
            match event {
                OrderEventAny::Accepted(accepted) => {
                    self.mark_accepted(client_order_id);
                    self.record_venue_leg(client_order_id, accepted.venue_order_id);
                }
                OrderEventAny::Updated(updated) => {
                    if let Some(venue_order_id) = updated.venue_order_id {
                        self.record_venue_leg(client_order_id, venue_order_id);
                    }
                }
                OrderEventAny::Filled(fill) => {
                    self.record_venue_leg(client_order_id, fill.venue_order_id);
                    self.mark_accepted(client_order_id);
                    self.check_and_insert_trade(fill.trade_id);
                    current_filled |= order.venue_order_id() == Some(fill.venue_order_id);
                }
                _ => {}
            }
        }

        if order.is_triggered() == Some(true) || (order.is_triggered().is_some() && current_filled)
        {
            self.mark_trigger_active(client_order_id);
        }
    }

    /// Retires active ownership and preserves bounded terminal replay state.
    pub fn forget(&self, client_order_id: &ClientOrderId) {
        let _delivery = self.delivery_guard();

        if let Some(binding) = self.order_binding(client_order_id) {
            let mut terminal = self.terminal_orders.lock();
            if terminal.len() == terminal.capacity() && !terminal.contains_key(client_order_id) {
                *self.terminal_revision.lock() = Arc::new(());
            }

            terminal.insert(*client_order_id, binding);
        }

        self.order_identities.lock().remove(client_order_id);
        self.order_shapes.lock().remove(client_order_id);
        self.emitted_accepted.lock().remove(client_order_id);
        self.triggers_active.lock().remove(client_order_id);
        self.bound_venue_order_ids.lock().remove(client_order_id);
        self.venue_order_legs.lock().remove(client_order_id);
        self.pending_modifies.lock().remove(client_order_id);
        self.modify_nonces.lock().remove(client_order_id);
        self.modify_targets.lock().remove(client_order_id);
        self.unresolved_bindings.lock().remove(client_order_id);
    }

    pub(crate) fn mark_trigger_active(&self, client_order_id: ClientOrderId) {
        self.triggers_active.lock().insert(client_order_id);
    }

    pub(crate) fn trigger_active(&self, client_order_id: &ClientOrderId) -> bool {
        let _delivery = self.delivery_guard();
        self.triggers_active.lock().contains(client_order_id)
    }

    pub(crate) fn take_identity(&self, client_order_id: &ClientOrderId) -> Option<OrderIdentity> {
        let identity = self.identity(client_order_id);
        if identity.is_some() {
            self.forget(client_order_id);
        }

        identity
    }

    pub(crate) fn terminal_revision(&self) -> Arc<()> {
        Arc::clone(&self.terminal_revision.lock())
    }

    pub(crate) fn known_instrument(&self, client_order_id: &ClientOrderId) -> Option<InstrumentId> {
        self.identity(client_order_id)
            .map(|identity| identity.instrument_id)
            .or_else(|| {
                self.terminal_orders
                    .lock()
                    .get(client_order_id)
                    .map(|binding| binding.identity.instrument_id)
            })
    }

    pub(crate) fn is_terminal(
        &self,
        client_order_id: &ClientOrderId,
        instrument_id: InstrumentId,
    ) -> bool {
        self.terminal_orders
            .lock()
            .get(client_order_id)
            .is_some_and(|binding| binding.identity.instrument_id == instrument_id)
    }

    pub(crate) fn record_order_shape(
        &self,
        client_order_id: ClientOrderId,
        quantity: Quantity,
        price: Option<Price>,
    ) {
        self.order_shapes
            .lock()
            .entry(client_order_id)
            .and_modify(|entry| {
                entry.0 = quantity;
                entry.2 = price;
            })
            .or_insert((quantity, Decimal::ZERO, price));
    }

    pub(crate) fn order_shape(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(Quantity, Option<Price>)> {
        self.order_shapes
            .lock()
            .get(client_order_id)
            .map(|entry| (entry.0, entry.2))
    }

    pub(crate) fn record_fill(&self, client_order_id: ClientOrderId, quantity: Quantity) {
        let _delivery = self.delivery_guard();

        let complete = self
            .order_shapes
            .lock()
            .get_mut(&client_order_id)
            .is_some_and(|entry| {
                entry.1 += quantity.as_decimal();
                entry.1 >= entry.0.as_decimal()
            });

        if complete
            && self.pending_modify(&client_order_id).is_none()
            && !self.binding_unresolved(&client_order_id)
        {
            self.forget(&client_order_id);
        }
    }

    /// Records the venue order id currently bound to a tracked client order.
    pub fn record_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        self.record_venue_leg(client_order_id, venue_order_id);
        self.bound_venue_order_ids
            .lock()
            .insert(client_order_id, venue_order_id);
    }

    pub(crate) fn record_venue_leg(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        self.venue_order_legs
            .lock()
            .entry(client_order_id)
            .or_default()
            .insert(venue_order_id);
    }

    pub(crate) fn knows_venue_leg(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> bool {
        self.venue_order_legs
            .lock()
            .get(client_order_id)
            .is_some_and(|legs| legs.contains(&venue_order_id))
    }

    pub(crate) fn venue_order_legs(&self, client_order_id: &ClientOrderId) -> Vec<VenueOrderId> {
        self.venue_order_legs
            .lock()
            .get(client_order_id)
            .map(|legs| legs.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn order_binding(&self, client_order_id: &ClientOrderId) -> Option<OrderBinding> {
        let _delivery = self.delivery_guard();
        self.identity(client_order_id)
            .map(|identity| {
                let mut venue_order_legs = self.venue_order_legs(client_order_id);
                venue_order_legs.sort_unstable();

                OrderBinding {
                    identity,
                    venue_order_id: self.bound_venue_order_id(client_order_id),
                    venue_order_legs,
                }
            })
            .or_else(|| self.terminal_orders.lock().get(client_order_id).cloned())
    }

    pub(crate) fn defer_binding(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        self.unresolved_bindings
            .lock()
            .entry(client_order_id)
            .or_default()
            .insert(venue_order_id);
    }

    pub(crate) fn binding_unresolved(&self, client_order_id: &ClientOrderId) -> bool {
        self.unresolved_bindings
            .lock()
            .get(client_order_id)
            .is_some_and(|legs| {
                legs.iter()
                    .any(|venue_order_id| !self.knows_venue_leg(client_order_id, *venue_order_id))
            })
    }

    pub(crate) fn unresolved_client_order_id(
        &self,
        instrument_id: Option<InstrumentId>,
    ) -> Option<ClientOrderId> {
        let mut ids: Vec<_> = self.unresolved_bindings.lock().keys().copied().collect();
        ids.extend(self.pending_modifies.lock().keys().copied());
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().find(|client_order_id| {
            self.identity(client_order_id).is_some_and(|identity| {
                instrument_id.is_none_or(|instrument_id| identity.instrument_id == instrument_id)
            }) && (self.binding_unresolved(client_order_id)
                || self.pending_modify(client_order_id).is_some())
        })
    }

    pub(crate) fn has_active_orders(&self) -> bool {
        !self.order_identities.lock().is_empty()
    }

    pub(crate) fn defer_trade(&self, client_order_id: ClientOrderId, trade: DeriveTrade) {
        let mut deferred = self.deferred_trades.lock();
        let trades = deferred.entry(client_order_id).or_default();
        if !trades.iter().any(|existing| {
            existing.trade_id == trade.trade_id && existing.order_id == trade.order_id
        }) {
            trades.push(trade);
        }
    }

    pub(crate) fn take_deferred_trades(&self, client_order_id: &ClientOrderId) -> Vec<DeriveTrade> {
        self.deferred_trades
            .lock()
            .remove(client_order_id)
            .unwrap_or_default()
    }

    pub(crate) fn retain_trade(&self, trade: DeriveTrade) {
        self.undelivered_trades
            .lock()
            .insert((trade.order_id.clone(), trade.trade_id.clone()), trade);
    }

    pub(crate) fn take_undelivered_trades(&self) -> Vec<DeriveTrade> {
        let mut trades: Vec<_> = self
            .undelivered_trades
            .lock()
            .drain()
            .map(|(_, trade)| trade)
            .collect();
        trades.sort_unstable_by(|a, b| {
            (a.timestamp, &a.order_id, &a.trade_id).cmp(&(b.timestamp, &b.order_id, &b.trade_id))
        });

        trades
    }

    pub(crate) fn record_replace_completion(
        &self,
        client_order_id: ClientOrderId,
        completion: ReplaceCompletion,
    ) {
        self.replace_completions
            .lock()
            .insert(client_order_id, completion);
    }

    pub(crate) fn replace_completions(&self) -> Vec<(ClientOrderId, ReplaceCompletion)> {
        let mut completions: Vec<_> = self
            .replace_completions
            .lock()
            .iter()
            .map(|(cid, completion)| (*cid, completion.clone()))
            .collect();
        completions.sort_unstable_by_key(|(cid, _)| *cid);
        completions
    }

    pub(crate) fn clear_replace_completion(&self, client_order_id: &ClientOrderId) {
        self.replace_completions.lock().remove(client_order_id);
    }

    /// Returns the venue order id currently bound to a tracked client order.
    #[must_use]
    pub fn bound_venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.bound_venue_order_ids
            .lock()
            .get(client_order_id)
            .copied()
    }

    /// Records the old venue order id of an in-flight `private/replace`, set
    /// before the request so the cancel leg is suppressed.
    pub fn mark_pending_modify(
        &self,
        client_order_id: ClientOrderId,
        old_venue_order_id: VenueOrderId,
    ) -> bool {
        let mut pending = self.pending_modifies.lock();
        if pending.contains_key(&client_order_id) {
            return false;
        }

        pending.insert(client_order_id, old_venue_order_id);
        true
    }

    /// Clears the in-flight modify marker once the replace resolves.
    pub fn clear_pending_modify(&self, client_order_id: &ClientOrderId) {
        self.pending_modifies.lock().remove(client_order_id);
        self.modify_nonces.lock().remove(client_order_id);
        self.modify_targets.lock().remove(client_order_id);
    }

    pub(crate) fn record_modify_nonce(&self, client_order_id: ClientOrderId, nonce: u64) {
        self.modify_nonces.lock().insert(client_order_id, nonce);
    }

    pub(crate) fn modify_nonce(&self, client_order_id: &ClientOrderId) -> Option<u64> {
        self.modify_nonces.lock().get(client_order_id).copied()
    }

    pub(crate) fn record_modify_target(
        &self,
        client_order_id: ClientOrderId,
        quantity: Quantity,
        price: Option<Price>,
    ) {
        self.modify_targets
            .lock()
            .insert(client_order_id, (quantity, price));
    }

    pub(crate) fn modify_target(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(Quantity, Option<Price>)> {
        self.modify_targets.lock().get(client_order_id).copied()
    }

    pub(crate) fn take_modify_target(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(Quantity, Option<Price>)> {
        self.modify_targets.lock().remove(client_order_id)
    }

    /// Returns the old venue order id of an in-flight modify, when one is set.
    #[must_use]
    pub fn pending_modify(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.pending_modifies.lock().get(client_order_id).copied()
    }

    /// Rebinds an in-flight modify when a frame for its replacement arrives
    /// before the `private/replace` response.
    pub fn bind_incoming_modify(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        terminal: bool,
    ) -> bool {
        let mut pending = self.pending_modifies.lock();

        let Some(old_venue_order_id) = pending.get(&client_order_id).copied() else {
            return false;
        };

        if old_venue_order_id == venue_order_id {
            return false;
        }

        let mut bound = self.bound_venue_order_ids.lock();
        if bound
            .get(&client_order_id)
            .is_some_and(|current| *current != old_venue_order_id)
        {
            return false;
        }

        bound.insert(client_order_id, venue_order_id);
        if terminal {
            pending.remove(&client_order_id);
        }

        true
    }

    /// Atomically claims a pending modify for its RPC response, optionally
    /// rebinding the replacement venue order id before clearing the marker.
    /// Returns `false` when an incoming terminal frame already resolved it.
    pub fn take_pending_modify(
        &self,
        client_order_id: &ClientOrderId,
        old_venue_order_id: VenueOrderId,
        new_venue_order_id: Option<VenueOrderId>,
    ) -> bool {
        let mut pending = self.pending_modifies.lock();
        if pending.get(client_order_id) != Some(&old_venue_order_id) {
            return false;
        }

        if let Some(new_venue_order_id) = new_venue_order_id {
            self.record_venue_leg(*client_order_id, new_venue_order_id);
            self.bound_venue_order_ids
                .lock()
                .insert(*client_order_id, new_venue_order_id);
        }

        pending.remove(client_order_id);
        self.modify_nonces.lock().remove(client_order_id);
        true
    }

    /// Returns `true` when an `OrderAccepted` has already been emitted for
    /// this client order in the current process lifetime.
    #[must_use]
    pub fn contains_accepted(&self, client_order_id: &ClientOrderId) -> bool {
        self.emitted_accepted.lock().contains(client_order_id)
    }

    /// Records that `OrderAccepted` has been emitted for this client order.
    /// Returns `true` when the marker was already present (duplicate).
    pub fn mark_accepted(&self, client_order_id: ClientOrderId) -> bool {
        !self.emitted_accepted.lock().insert(client_order_id)
    }

    pub(crate) fn contains_canceled(&self, client_order_id: &ClientOrderId) -> bool {
        self.emitted_canceled.lock().contains(client_order_id)
    }

    /// Records that `OrderCanceled` has been emitted for this client order.
    /// Returns `true` when the marker was already present (duplicate).
    pub fn mark_canceled(&self, client_order_id: ClientOrderId) -> bool {
        let mut cache = self.emitted_canceled.lock();
        if cache.contains(&client_order_id) {
            return true;
        }

        cache.add(client_order_id);
        false
    }

    /// Returns `true` when this client order has reached a terminal filled
    /// state, used to suppress stale Accepted frames replayed on reconnect.
    #[must_use]
    pub fn contains_filled(&self, client_order_id: &ClientOrderId) -> bool {
        self.filled_orders.lock().contains(client_order_id)
    }

    /// Marks the client order as terminally filled. Idempotent.
    pub fn mark_filled(&self, client_order_id: ClientOrderId) {
        let mut cache = self.filled_orders.lock();
        if !cache.contains(&client_order_id) {
            cache.add(client_order_id);
        }
    }

    /// Inserts the trade id atomically. Returns `true` when the id was
    /// already present (i.e., this fill should be skipped as a duplicate).
    pub fn check_and_insert_trade(&self, trade_id: TradeId) -> bool {
        let mut cache = self.emitted_trades.lock();
        if cache.contains(&trade_id) {
            return true;
        }

        cache.add(trade_id);
        false
    }

    /// Returns `true` when this trade id has already been seen, without
    /// mutating state.
    #[must_use]
    pub fn contains_trade(&self, trade_id: &TradeId) -> bool {
        self.emitted_trades.lock().contains(trade_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderBinding {
    pub identity: OrderIdentity,
    pub venue_order_id: Option<VenueOrderId>,
    pub venue_order_legs: Vec<VenueOrderId>,
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    };
    use rstest::rstest;

    use super::*;

    fn sample_identity() -> OrderIdentity {
        OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        }
    }

    #[rstest]
    fn test_instrument_precision_roundtrip() {
        let state = WsDispatchState::new(42);
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

        assert_eq!(state.instrument_precision(&instrument_id), None);
        state.register_instrument_precision(instrument_id, 2, 3);
        assert_eq!(state.instrument_precision(&instrument_id), Some((2, 3)));
    }

    #[rstest]
    fn test_register_and_identity_roundtrip() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");
        let identity = sample_identity();

        assert!(state.identity(&cid).is_none());
        state.register_identity(cid, identity);
        assert_eq!(state.identity(&cid), Some(identity));

        state.forget(&cid);
        assert!(state.identity(&cid).is_none());
    }

    #[rstest]
    fn test_mark_accepted_dedupes_second_call() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");

        assert!(!state.mark_accepted(cid));
        assert!(state.contains_accepted(&cid));
        assert!(state.mark_accepted(cid));
    }

    #[rstest]
    fn test_active_acceptance_survives_replay_cache_eviction() {
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("ACTIVE-0");
        state.register_identity(client_order_id, sample_identity());
        state.mark_accepted(client_order_id);

        for n in 1..=ORDER_DEDUP_CAPACITY {
            let other = ClientOrderId::new(format!("ACTIVE-{n}"));
            state.register_identity(other, sample_identity());
            state.mark_accepted(other);
        }

        assert_eq!(state.identity(&client_order_id), Some(sample_identity()));
        assert!(state.contains_accepted(&client_order_id));
        assert!(state.mark_accepted(client_order_id));
    }

    #[rstest]
    fn test_check_and_insert_trade_returns_true_on_duplicate() {
        let state = WsDispatchState::new(42);
        let trade_id = TradeId::new("T-1");

        assert!(!state.check_and_insert_trade(trade_id));
        assert!(state.contains_trade(&trade_id));
        assert!(state.check_and_insert_trade(trade_id));
    }

    #[rstest]
    fn test_forget_clears_accepted_marker() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");

        state.mark_accepted(cid);
        state.forget(&cid);
        assert!(!state.contains_accepted(&cid));
    }

    #[rstest]
    fn test_bound_venue_order_id_records_and_advances() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");
        let voi1 = VenueOrderId::from("voi-1");
        let voi2 = VenueOrderId::from("voi-2");

        assert!(state.bound_venue_order_id(&cid).is_none());
        state.record_venue_order_id(cid, voi1);
        assert_eq!(state.bound_venue_order_id(&cid), Some(voi1));
        // A modify rebinds the order to the replacement venue order id.
        state.record_venue_order_id(cid, voi2);
        assert_eq!(state.bound_venue_order_id(&cid), Some(voi2));
    }

    #[rstest]
    fn test_pending_modify_admission_does_not_replace_existing_operation() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("MODIFY-SERIAL");
        let old_id = VenueOrderId::from("original-leg");
        state.mark_pending_modify(cid, old_id);
        state.record_modify_target(cid, Quantity::from("2.000"), Some(Price::from("3505.00")));
        state.mark_pending_modify(cid, VenueOrderId::from("overlapping-leg"));
        assert_eq!(state.pending_modify(&cid), Some(old_id));
        assert_eq!(
            state.modify_target(&cid),
            Some((Quantity::from("2.000"), Some(Price::from("3505.00"))))
        );
    }

    #[rstest]
    fn test_pending_modify_marker_set_and_cleared() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");
        let old_voi = VenueOrderId::from("voi-1");

        assert!(state.pending_modify(&cid).is_none());
        state.mark_pending_modify(cid, old_voi);
        assert_eq!(state.pending_modify(&cid), Some(old_voi));
        state.clear_pending_modify(&cid);
        assert!(state.pending_modify(&cid).is_none());
    }

    #[rstest]
    fn test_bind_incoming_modify_advances_bound_venue_order_id() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");
        let old_voi = VenueOrderId::from("voi-old");
        let new_voi = VenueOrderId::from("voi-new");
        state.record_venue_order_id(cid, old_voi);
        state.mark_pending_modify(cid, old_voi);

        assert!(state.bind_incoming_modify(cid, new_voi, false));
        assert_eq!(state.bound_venue_order_id(&cid), Some(new_voi));
        assert!(!state.bind_incoming_modify(cid, VenueOrderId::from("voi-other"), true));
        assert_eq!(state.bound_venue_order_id(&cid), Some(new_voi));
        assert!(state.take_pending_modify(&cid, old_voi, None));
        assert!(!state.take_pending_modify(&cid, old_voi, None));
    }

    #[rstest]
    fn test_terminal_incoming_modify_claims_pending_response() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");
        let old_voi = VenueOrderId::from("voi-old");
        state.record_venue_order_id(cid, old_voi);
        state.mark_pending_modify(cid, old_voi);

        assert!(state.bind_incoming_modify(cid, VenueOrderId::from("voi-rejected"), true));
        assert!(state.pending_modify(&cid).is_none());
        assert!(!state.take_pending_modify(&cid, old_voi, None));
    }

    #[rstest]
    fn test_response_claim_rebinds_before_clearing_pending_modify() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");
        let old_voi = VenueOrderId::from("voi-old");
        let new_voi = VenueOrderId::from("voi-new");
        state.record_venue_order_id(cid, old_voi);
        state.mark_pending_modify(cid, old_voi);

        assert!(state.take_pending_modify(&cid, old_voi, Some(new_voi)));
        assert_eq!(state.bound_venue_order_id(&cid), Some(new_voi));
        assert!(state.pending_modify(&cid).is_none());
    }

    #[rstest]
    fn test_forget_clears_bound_and_pending() {
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("STRAT-O-1");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(cid, identity);
        state.record_venue_order_id(cid, VenueOrderId::from("voi-0"));
        state.record_venue_order_id(cid, VenueOrderId::from("voi-1"));
        state.mark_pending_modify(cid, VenueOrderId::from("voi-1"));
        state.forget(&cid);
        assert!(state.bound_venue_order_id(&cid).is_none());
        assert!(state.pending_modify(&cid).is_none());
        assert_eq!(state.identity(&cid), None);
        assert_eq!(state.known_instrument(&cid), Some(identity.instrument_id));
        assert!(state.is_terminal(&cid, identity.instrument_id));
        assert_eq!(
            state.order_binding(&cid),
            Some(OrderBinding {
                identity,
                venue_order_id: Some(VenueOrderId::from("voi-1")),
                venue_order_legs: vec![VenueOrderId::from("voi-0"), VenueOrderId::from("voi-1")]
            })
        );
    }
}
