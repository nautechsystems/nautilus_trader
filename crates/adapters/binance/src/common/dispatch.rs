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

//! WebSocket dispatch state for tracked/external order routing.
//!
//! Orders submitted through this client and active local orders restored from the cache have their
//! identity registered in [`WsDispatchState`]. User data stream messages route by registered identity:
//! - Tracked orders produce typed order events (OrderAccepted, OrderFilled, etc.).
//! - Untracked orders fall back to execution reports for reconciliation.

use ahash::AHashSet;
use dashmap::{DashMap, mapref::entry::Entry};
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::{OrderAccepted, OrderCanceled, OrderEventAny},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, VenueOrderId,
    },
    orders::{Order, OrderAny},
    reports::OrderStatusReport,
    types::{Price, Quantity},
};
use parking_lot::Mutex;
use ustr::Ustr;

/// The type of operation a pending WS API request represents.
#[derive(Debug, Clone, Copy)]
pub enum PendingOperation {
    Place,
    Cancel,
    Modify,
}

/// A pending WS API request awaiting a response.
///
/// Stored in [`WsDispatchState::pending_requests`] after the WS client
/// returns a request ID. When the venue responds (accepted or rejected),
/// the pending request is removed and used to emit the correct order event.
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub operation: PendingOperation,
}

/// Outcome of a cancel-replace request, tracked by its `cancelNewClientOrderId`.
///
/// The cancel half's `CANCELED` report and the venue's response arrive in either
/// order, so whichever comes second completes the picture.
#[derive(Debug, Clone)]
enum CancelReplaceOutcome {
    Pending,
    /// The cancel report arrived; withheld while the replacement is pending or succeeded.
    ///
    /// Parsed at that point so it can be replayed even for an order without a
    /// dispatch identity, such as an external order.
    Canceled(Box<OrderStatusReport>),
    /// The venue rejected the request before any cancel report arrived.
    Rejected,
}

/// Order identity context registered on submission or restored for an active local order.
///
/// Provides the strategy and instrument metadata needed to construct
/// typed order events without accessing the cache from the async dispatch task.
/// The quantity and quote-sizing flag retain the order's sizing intent.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub price: Option<Price>,
    pub quantity: Quantity,
    pub is_quote_quantity: bool,
    pub venue_position_id: Option<PositionId>,
}

#[derive(Debug, Clone, Copy)]
struct AlgoOrderIds {
    algo: VenueOrderId,
    current: VenueOrderId,
}

/// Tracks order lifecycle state for dispatch routing.
///
/// Orders with a registered identity produce typed order events. Untracked orders
/// fall back to execution reports for reconciliation. For restored orders, accepted and filled
/// state, plus the trade IDs known at restoration, remain available until terminal cleanup,
/// independent of the bounded deduplication caches.
#[derive(Debug)]
pub struct WsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    pub pending_requests: DashMap<String, PendingRequest>,
    pub(crate) order_lists: DashMap<ClientOrderId, OrderListIdentity>,
    algo_order_ids: DashMap<ClientOrderId, AlgoOrderIds>,
    order_updates: DashMap<ClientOrderId, OrderBinding>,
    orders_restored: DashMap<ClientOrderId, RestoredOrderState>,
    replacements: DashMap<ClientOrderId, PendingReplacement>,
    emitted_accepted: Mutex<FifoCache<ClientOrderId, 10_000>>,
    filled_orders: Mutex<FifoCache<ClientOrderId, 10_000>>,
    /// Cancel-replace request IDs mapped to their `cancelNewClientOrderId`.
    pub cancel_replace_request_ids: DashMap<String, String>,
    cancel_replace_outcomes: Mutex<FifoCacheMap<String, CancelReplaceOutcome, 10_000>>,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            pending_requests: DashMap::new(),
            order_lists: DashMap::new(),
            algo_order_ids: DashMap::new(),
            order_updates: DashMap::new(),
            orders_restored: DashMap::new(),
            replacements: DashMap::new(),
            emitted_accepted: Mutex::new(FifoCache::new()),
            filled_orders: Mutex::new(FifoCache::new()),
            cancel_replace_request_ids: DashMap::new(),
            cancel_replace_outcomes: Mutex::new(FifoCacheMap::new()),
        }
    }
}

impl WsDispatchState {
    pub(crate) fn restore_order(
        &self,
        order: &OrderAny,
        venue_position_id: Option<PositionId>,
        price_precision: u8,
    ) {
        let cid = order.client_order_id();

        let Entry::Vacant(entry) = self.order_identities.entry(cid) else {
            return;
        };

        let accepted = order.ts_accepted().is_some();
        self.orders_restored.insert(
            cid,
            RestoredOrderState {
                trade_ids: order.trade_ids().into_iter().copied().collect(),
                accepted,
                filled: order.filled_qty().is_positive(),
            },
        );

        if let Some(venue_order_id) = order.venue_order_id()
            && !order.is_quote_quantity()
        {
            let current = OrderUpdate {
                venue_order_id,
                quantity: order.quantity(),
                price: order
                    .price()
                    .unwrap_or_else(|| Price::zero(price_precision)),
                trigger_price: order.trigger_price(),
            };

            let superseded = order
                .venue_order_ids()
                .into_iter()
                .copied()
                .filter(|id| *id != venue_order_id)
                .collect();
            self.order_updates.insert(
                cid,
                OrderBinding {
                    current,
                    superseded,
                },
            );
        }

        entry.insert(OrderIdentity {
            instrument_id: order.instrument_id(),
            strategy_id: order.strategy_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
            price: order.price(),
            quantity: order.quantity(),
            is_quote_quantity: order.is_quote_quantity(),
            venue_position_id,
        });
    }

    pub fn has_emitted_accepted(&self, cid: &ClientOrderId) -> bool {
        self.orders_restored
            .get(cid)
            .is_some_and(|order| order.accepted)
            || self.order_lists.contains_key(cid)
            || self.emitted_accepted.lock().contains(cid)
    }

    /// Marks an order as having emitted an OrderAccepted event.
    pub fn insert_accepted(&self, cid: ClientOrderId) {
        if let Some(mut order) = self.orders_restored.get_mut(&cid) {
            order.accepted = true;
        }

        self.emitted_accepted.lock().add(cid);
    }

    pub fn has_filled(&self, cid: &ClientOrderId) -> bool {
        self.orders_restored
            .get(cid)
            .is_some_and(|order| order.filled)
            || self.filled_orders.lock().contains(cid)
    }

    pub(crate) fn is_order_new_stale(
        &self,
        cid: &ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> bool {
        self.has_filled(cid)
            && self
                .order_updates
                .get(cid)
                .is_none_or(|binding| binding.current.venue_order_id == venue_order_id)
    }

    pub(crate) fn has_trade(
        &self,
        cid: &ClientOrderId,
        key: &(Ustr, i64),
        seen_trade_ids: &Mutex<FifoCache<(Ustr, i64), 10_000>>,
    ) -> bool {
        self.orders_restored.get(cid).is_some_and(|order| {
            let trade_id = TradeId::new(key.1.to_string());
            order.trade_ids.contains(&trade_id)
        }) || seen_trade_ids.lock().contains(key)
    }

    /// Marks an order as having received a fill.
    pub fn insert_filled(&self, cid: ClientOrderId) {
        if let Some(mut order) = self.orders_restored.get_mut(&cid) {
            order.filled = true;
        }

        self.filled_orders.lock().add(cid);
    }

    /// Records the `cancelNewClientOrderId` sent with a cancel-replace request.
    pub fn insert_cancel_replace(&self, cancel_id: String) {
        self.cancel_replace_outcomes
            .lock()
            .insert(cancel_id, CancelReplaceOutcome::Pending);
    }

    /// Returns `true` when `cancel_id` belongs to a cancel-replace this client issued.
    pub fn has_cancel_replace(&self, cancel_id: &str) -> bool {
        self.cancel_replace_outcomes
            .lock()
            .contains_key(&cancel_id.to_string())
    }

    /// Records the cancel half's `CANCELED` report for a cancel-replace request.
    ///
    /// Returns `true` when the report must be withheld because the replacement is
    /// pending or succeeded, and `false` when it should dispatch as a standalone
    /// cancel because the venue already rejected the replacement or the ID is not
    /// one this client issued.
    pub fn on_cancel_replace_canceled(&self, cancel_id: &str, report: OrderStatusReport) -> bool {
        let mut outcomes = self.cancel_replace_outcomes.lock();
        let Some(outcome) = outcomes.get_mut(&cancel_id.to_string()) else {
            return false;
        };

        match outcome {
            CancelReplaceOutcome::Pending => {
                *outcome = CancelReplaceOutcome::Canceled(Box::new(report));
                true
            }
            CancelReplaceOutcome::Canceled(_) => true,
            CancelReplaceOutcome::Rejected => false,
        }
    }

    /// Records a rejected cancel-replace request.
    ///
    /// Returns the withheld cancel report when it already arrived, so the caller
    /// can emit the confirmed cancellation for the original order.
    pub fn on_cancel_replace_rejected(&self, cancel_id: &str) -> Option<OrderStatusReport> {
        let mut outcomes = self.cancel_replace_outcomes.lock();
        let outcome = outcomes.get_mut(&cancel_id.to_string())?;

        match outcome {
            CancelReplaceOutcome::Pending => {
                *outcome = CancelReplaceOutcome::Rejected;
                None
            }
            CancelReplaceOutcome::Canceled(report) => Some((**report).clone()),
            CancelReplaceOutcome::Rejected => None,
        }
    }

    pub fn insert_algo_order_id(&self, cid: ClientOrderId, venue_order_id: VenueOrderId) {
        self.algo_order_ids.entry(cid).or_insert(AlgoOrderIds {
            algo: venue_order_id,
            current: venue_order_id,
        });
    }

    /// Promotes a known Algo order to its matching-engine venue order ID.
    ///
    /// Returns `None` for an unknown Algo order, `Some(true)` for a new ID, and
    /// `Some(false)` when the ID was already current.
    pub fn promote_algo_order_id(
        &self,
        cid: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> Option<bool> {
        let mut ids = self.algo_order_ids.get_mut(&cid)?;
        let changed = ids.current != venue_order_id;
        ids.current = venue_order_id;
        Some(changed)
    }

    /// Returns the matching-engine venue order ID for a promoted Algo order.
    pub fn promoted_algo_order_id(&self, cid: &ClientOrderId) -> Option<VenueOrderId> {
        self.algo_order_ids
            .get(cid)
            .and_then(|ids| (ids.current != ids.algo).then_some(ids.current))
    }

    /// Returns `None` for a superseded venue order ID.
    ///
    /// Otherwise, returns `Some(true)` for the first update or a changed venue order ID,
    /// quantity, price, or trigger price, and `Some(false)` for unchanged values.
    pub(crate) fn record_order_update(
        &self,
        cid: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        trigger_price: Option<Price>,
    ) -> Option<bool> {
        let update = OrderUpdate {
            venue_order_id,
            quantity,
            price,
            trigger_price,
        };

        let changed = match self.order_updates.entry(cid) {
            Entry::Occupied(mut entry) => {
                let binding = entry.get_mut();
                if binding.superseded.contains(&venue_order_id) {
                    return None;
                }

                let changed = binding.current != update;
                if binding.current.venue_order_id != venue_order_id {
                    binding.superseded.push(binding.current.venue_order_id);
                }

                binding.current = update;
                changed
            }
            Entry::Vacant(entry) => {
                entry.insert(OrderBinding {
                    current: update,
                    superseded: Vec::new(),
                });

                true
            }
        };

        self.replacements
            .remove_if(&cid, |_, pending| pending.venue_order_id != venue_order_id);
        Some(changed)
    }

    pub(crate) fn record_order_update_from_fill(
        &self,
        cid: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        trigger_price: Option<Price>,
        identity: &OrderIdentity,
    ) -> Option<bool> {
        if !identity.is_quote_quantity {
            self.order_updates
                .entry(cid)
                .or_insert_with(|| OrderBinding {
                    current: OrderUpdate {
                        venue_order_id,
                        quantity: identity.quantity,
                        price: identity.price.unwrap_or(price),
                        trigger_price,
                    },
                    superseded: Vec::new(),
                });
        }

        self.record_order_update(cid, venue_order_id, quantity, price, trigger_price)
    }

    pub(crate) fn begin_replace(&self, cid: ClientOrderId, venue_order_id: VenueOrderId) {
        self.replacements.insert(
            cid,
            PendingReplacement {
                venue_order_id,
                canceled: None,
            },
        );
    }

    pub(crate) fn defer_replace_cancel(&self, canceled: OrderCanceled) -> bool {
        let cid = canceled.client_order_id;

        if self.order_updates.get(&cid).is_some_and(|binding| {
            canceled
                .venue_order_id
                .is_some_and(|venue_order_id| binding.superseded.contains(&venue_order_id))
        }) {
            return true;
        }

        if let Some(mut pending) = self.replacements.get_mut(&cid)
            && Some(pending.venue_order_id) == canceled.venue_order_id
        {
            pending.canceled = Some(canceled);
            return true;
        }
        false
    }

    pub(crate) fn reject_replace(&self, cid: ClientOrderId) -> Option<OrderCanceled> {
        self.replacements
            .remove(&cid)
            .and_then(|(_, pending)| pending.canceled)
    }

    /// Removes all tracking state for a terminal order.
    pub fn cleanup_terminal(&self, cid: ClientOrderId) {
        self.order_identities.remove(&cid);
        self.order_lists.remove(&cid);
        self.algo_order_ids.remove(&cid);
        self.order_updates.remove(&cid);
        self.orders_restored.remove(&cid);
        self.replacements.remove(&cid);
        self.emitted_accepted.lock().remove(&cid);
        self.filled_orders.lock().remove(&cid);
    }
}

#[derive(Debug)]
struct RestoredOrderState {
    trade_ids: AHashSet<TradeId>,
    accepted: bool,
    filled: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OrderListIdentity {
    pub order_list_id: i64,
    pub venue_order_id: VenueOrderId,
    pub execution_report_seen: bool,
}

#[derive(Debug)]
struct PendingReplacement {
    venue_order_id: VenueOrderId,
    canceled: Option<OrderCanceled>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrderUpdate {
    venue_order_id: VenueOrderId,
    quantity: Quantity,
    price: Price,
    trigger_price: Option<Price>,
}

#[derive(Debug)]
struct OrderBinding {
    current: OrderUpdate,
    superseded: Vec<VenueOrderId>,
}

/// Synthesizes and emits OrderAccepted if one has not yet been emitted.
///
/// Handles fast-filling orders that skip the New state on Binance.
pub fn ensure_accepted_emitted(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if state.has_emitted_accepted(&client_order_id) {
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

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderType,
        events::order::spec::{OrderFilledSpec, OrderUpdatedSpec},
        identifiers::AccountId,
        orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::submitted(false)]
    #[case::accepted(true)]
    fn test_restore_order_retains_identity_and_lifecycle_beyond_replay_capacity(
        #[case] accepted: bool,
        #[values(false, true)] quote_quantity: bool,
    ) {
        let order_type = if quote_quantity {
            OrderType::Market
        } else {
            OrderType::Limit
        };

        let mut order = OrderTestBuilder::new(order_type)
            .client_order_id(ClientOrderId::from("restored-active"))
            .strategy_id(StrategyId::from("RETAINED-007"))
            .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("2.003"))
            .price(Price::from("3456.78"))
            .quote_quantity(quote_quantity)
            .build();
        let account_id = AccountId::from("BINANCE-009");
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        order.apply(submitted).unwrap();

        if accepted {
            let event =
                TestOrderEventStubs::accepted(&order, account_id, VenueOrderId::from("9182"));
            order.apply(event).unwrap();
        }

        let cid = order.client_order_id();
        let position_id = Some(PositionId::from("ETHUSDT.BINANCE-SHORT"));
        let state = WsDispatchState::default();
        state.restore_order(&order, position_id, 2);

        assert_eq!(state.has_emitted_accepted(&cid), accepted);
        assert!(!state.has_filled(&cid));
        state.insert_accepted(cid);
        state.insert_filled(cid);

        for index in 0..10_001 {
            let other = ClientOrderId::from(format!("other-{index}"));
            state.insert_accepted(other);
            state.insert_filled(other);
        }

        state.restore_order(&order, None, 2);

        let identity = state.order_identities.get(&cid).unwrap().clone();
        assert_eq!(identity.instrument_id, order.instrument_id());
        assert_eq!(identity.strategy_id, order.strategy_id());
        assert_eq!(identity.order_side, OrderSide::Sell);
        assert_eq!(identity.order_type, order_type);
        assert_eq!(
            identity.price,
            (!quote_quantity).then(|| Price::from("3456.78"))
        );
        assert_eq!(identity.quantity, Quantity::from("2.003"));
        assert_eq!(identity.is_quote_quantity, quote_quantity);
        assert_eq!(identity.venue_position_id, position_id);
        assert!(state.has_emitted_accepted(&cid));
        assert!(state.has_filled(&cid));

        state.cleanup_terminal(cid);
        assert!(!state.order_identities.contains_key(&cid));
        assert!(!state.orders_restored.contains_key(&cid));
        assert!(!state.has_emitted_accepted(&cid));
        assert!(!state.has_filled(&cid));
    }

    #[rstest]
    fn test_restore_market_order_seeds_zero_price_update(
        #[values(false, true)] quote_quantity: bool,
    ) {
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .quantity(Quantity::from("0.002"))
            .quote_quantity(quote_quantity)
            .build();
        let event = TestOrderEventStubs::accepted(
            &order,
            AccountId::from("BINANCE-001"),
            VenueOrderId::from("312"),
        );
        order.apply(event).unwrap();
        let state = WsDispatchState::default();
        state.restore_order(&order, None, 2);

        assert_eq!(
            state.record_order_update(
                order.client_order_id(),
                VenueOrderId::from("312"),
                order.quantity(),
                Price::zero(2),
                None
            ),
            Some(quote_quantity)
        );
    }

    #[rstest]
    fn test_restore_order_retains_partial_fills() {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .quantity(Quantity::from("3"))
            .price(Price::from("50000.00"))
            .build();
        let accepted = TestOrderEventStubs::accepted(
            &order,
            AccountId::from("BINANCE-001"),
            VenueOrderId::from("312"),
        );
        order.apply(accepted).unwrap();
        let fill = OrderFilledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("312"))
            .account_id(AccountId::from("BINANCE-001"))
            .trade_id(TradeId::from("9182"))
            .last_qty(Quantity::from("1"))
            .last_px(Price::from("50000.00"))
            .build();
        order.apply(OrderEventAny::Filled(fill)).unwrap();
        let state = WsDispatchState::default();
        state.restore_order(&order, None, 2);

        assert!(state.has_emitted_accepted(&order.client_order_id()));
        assert!(state.has_filled(&order.client_order_id()));
        let seen_trade_ids = Mutex::new(FifoCache::new());
        assert!(state.has_trade(
            &order.client_order_id(),
            &(Ustr::from("BTCUSDT"), 9182),
            &seen_trade_ids
        ));
        assert!(!state.has_trade(
            &order.client_order_id(),
            &(Ustr::from("BTCUSDT"), 9183),
            &seen_trade_ids
        ));
    }

    #[rstest]
    fn test_restore_order_same_shape_replacement_updates_venue_id() {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .quantity(Quantity::from("3"))
            .price(Price::from("50000.00"))
            .build();
        let account_id = AccountId::from("BINANCE-001");
        let old_id = VenueOrderId::from("312");
        let current_id = VenueOrderId::from("313");
        let accepted = TestOrderEventStubs::accepted(&order, account_id, old_id);
        order.apply(accepted).unwrap();
        let state = WsDispatchState::default();
        state.restore_order(&order, None, 2);

        let canceled = OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(current_id),
            Some(account_id),
            None,
        );
        assert!(!state.defer_replace_cancel(canceled));
        assert_eq!(
            state.record_order_update(
                order.client_order_id(),
                current_id,
                order.quantity(),
                order.price().unwrap(),
                None
            ),
            Some(true)
        );
        assert_eq!(
            state.record_order_update(
                order.client_order_id(),
                current_id,
                order.quantity(),
                order.price().unwrap(),
                None
            ),
            Some(false)
        );

        let stale_canceled = OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(old_id),
            Some(account_id),
            None,
        );
        assert!(state.defer_replace_cancel(stale_canceled));
        assert!(!state.defer_replace_cancel(canceled));
    }

    #[rstest]
    fn test_restore_order_retains_superseded_venue_ids() {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .quantity(Quantity::from("3"))
            .price(Price::from("50000.00"))
            .build();
        let account_id = AccountId::from("BINANCE-001");
        let old_id = VenueOrderId::from("312");
        let current_id = VenueOrderId::from("313");
        let unknown_id = VenueOrderId::from("314");
        let accepted = TestOrderEventStubs::accepted(&order, account_id, old_id);
        order.apply(accepted).unwrap();
        let updated = OrderUpdatedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .quantity(order.quantity())
            .price(order.price().unwrap())
            .venue_order_id(current_id)
            .account_id(account_id)
            .build();
        order.apply(OrderEventAny::Updated(updated)).unwrap();
        let state = WsDispatchState::default();
        state.restore_order(&order, None, 2);

        for (venue_order_id, suppressed) in
            [(old_id, true), (current_id, false), (unknown_id, false)]
        {
            let canceled = OrderCanceled::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
                false,
                Some(venue_order_id),
                Some(account_id),
                None,
            );
            assert_eq!(state.defer_replace_cancel(canceled), suppressed);
        }

        state.begin_replace(order.client_order_id(), current_id);

        let pending = OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(current_id),
            Some(account_id),
            None,
        );
        assert!(state.defer_replace_cancel(pending));
        assert_eq!(
            state.record_order_update(
                order.client_order_id(),
                old_id,
                Quantity::from("2"),
                Price::from("49000.00"),
                None
            ),
            None
        );
        assert_eq!(state.reject_replace(order.client_order_id()), Some(pending));
    }
}
