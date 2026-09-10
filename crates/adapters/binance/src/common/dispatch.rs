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
//! Orders submitted through this client have their identity registered in
//! [`WsDispatchState`]. When user data stream messages arrive, the dispatch
//! function checks for a registered identity:
//! - Tracked orders produce proper order events (OrderAccepted, OrderFilled, etc.).
//! - Untracked orders fall back to execution reports for reconciliation.

use dashmap::DashMap;
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::{OrderAccepted, OrderCanceled, OrderEventAny},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, VenueOrderId},
    reports::OrderStatusReport,
    types::{Price, Quantity},
};
use parking_lot::Mutex;

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
    /// dispatch identity, such as one recovered after restart.
    Canceled(Box<OrderStatusReport>),
    /// The venue rejected the request before any cancel report arrived.
    Rejected,
}

/// Order identity context stored at submission time.
///
/// Provides the strategy and instrument metadata needed to construct proper
/// order events without accessing the cache from the async dispatch task.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub price: Option<Price>,
    pub quantity: Quantity,
    pub venue_position_id: Option<PositionId>,
}

#[derive(Debug, Clone, Copy)]
struct AlgoOrderIds {
    algo: VenueOrderId,
    current: VenueOrderId,
}

/// Tracks order lifecycle state for dispatch routing.
///
/// Orders with a registered identity (submitted through this client) produce
/// proper order events. Orders without identity (external or pre-existing)
/// fall back to execution reports for reconciliation.
#[derive(Debug)]
pub struct WsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    pub pending_requests: DashMap<String, PendingRequest>,
    algo_order_ids: DashMap<ClientOrderId, AlgoOrderIds>,
    order_updates: DashMap<ClientOrderId, OrderUpdate>,
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
            algo_order_ids: DashMap::new(),
            order_updates: DashMap::new(),
            replacements: DashMap::new(),
            emitted_accepted: Mutex::new(FifoCache::new()),
            filled_orders: Mutex::new(FifoCache::new()),
            cancel_replace_request_ids: DashMap::new(),
            cancel_replace_outcomes: Mutex::new(FifoCacheMap::new()),
        }
    }
}

impl WsDispatchState {
    pub fn has_emitted_accepted(&self, cid: &ClientOrderId) -> bool {
        self.emitted_accepted.lock().contains(cid)
    }

    /// Marks an order as having emitted an OrderAccepted event.
    pub fn insert_accepted(&self, cid: ClientOrderId) {
        self.emitted_accepted.lock().add(cid);
    }

    pub fn has_filled(&self, cid: &ClientOrderId) -> bool {
        self.filled_orders.lock().contains(cid)
    }

    /// Marks an order as having received a fill.
    pub fn insert_filled(&self, cid: ClientOrderId) {
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

    pub(crate) fn record_order_update(
        &self,
        cid: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        trigger_price: Option<Price>,
    ) -> bool {
        let update = OrderUpdate {
            venue_order_id,
            quantity,
            price,
            trigger_price,
        };
        let changed = self
            .order_updates
            .insert(cid, update)
            .is_none_or(|previous| previous != update);
        self.replacements
            .remove_if(&cid, |_, pending| pending.venue_order_id != venue_order_id);
        changed
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

        if self
            .order_updates
            .get(&cid)
            .is_some_and(|update| Some(update.venue_order_id) != canceled.venue_order_id)
        {
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
        self.algo_order_ids.remove(&cid);
        self.order_updates.remove(&cid);
        self.replacements.remove(&cid);
        self.emitted_accepted.lock().remove(&cid);
        self.filled_orders.lock().remove(&cid);
    }
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
