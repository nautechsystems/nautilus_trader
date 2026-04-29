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

use std::sync::Mutex;

use dashmap::DashMap;
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::{OrderAccepted, OrderEventAny},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, VenueOrderId},
    types::Price,
};

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
    emitted_accepted: Mutex<FifoCache<ClientOrderId, 10_000>>,
    filled_orders: Mutex<FifoCache<ClientOrderId, 10_000>>,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            pending_requests: DashMap::new(),
            emitted_accepted: Mutex::new(FifoCache::new()),
            filled_orders: Mutex::new(FifoCache::new()),
        }
    }
}

#[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
impl WsDispatchState {
    pub fn has_emitted_accepted(&self, cid: &ClientOrderId) -> bool {
        self.emitted_accepted
            .lock()
            .expect(MUTEX_POISONED)
            .contains(cid)
    }

    /// Marks an order as having emitted an OrderAccepted event.
    pub fn insert_accepted(&self, cid: ClientOrderId) {
        self.emitted_accepted.lock().expect(MUTEX_POISONED).add(cid);
    }

    pub fn has_filled(&self, cid: &ClientOrderId) -> bool {
        self.filled_orders
            .lock()
            .expect(MUTEX_POISONED)
            .contains(cid)
    }

    /// Marks an order as having received a fill.
    pub fn insert_filled(&self, cid: ClientOrderId) {
        self.filled_orders.lock().expect(MUTEX_POISONED).add(cid);
    }

    /// Removes all tracking state for a terminal order.
    pub fn cleanup_terminal(&self, cid: ClientOrderId) {
        self.order_identities.remove(&cid);
        self.emitted_accepted
            .lock()
            .expect(MUTEX_POISONED)
            .remove(&cid);
        self.filled_orders
            .lock()
            .expect(MUTEX_POISONED)
            .remove(&cid);
    }
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
