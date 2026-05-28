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
//! Holds identity context for orders submitted through this client plus the
//! cross-stream deduplication gates that keep replay frames and concurrent
//! `.orders` / `.trades` updates from emitting duplicate events.
//!
//! Tracked orders (those whose identity was registered at submission time)
//! produce proper order events (`OrderAccepted`, `OrderFilled`, `OrderCanceled`,
//! `OrderExpired`, `OrderRejected`). Untracked frames fall back to execution
//! reports for downstream reconciliation.

use std::sync::Mutex;

use ahash::AHashMap;
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::MUTEX_POISONED;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId},
};

/// Capacity for the cross-source trade-id dedup cache. Sized to cover any
/// reconciliation lookback window plausible for live trading.
pub const TRADE_DEDUP_CAPACITY: usize = 4_096;

/// Capacity for the per-order accepted / filled dedup caches. Tracks active
/// and recently-terminal orders so reconnect replays do not re-emit lifecycle
/// events; need only span the live-stream replay window plus a margin.
pub const ORDER_DEDUP_CAPACITY: usize = 1_024;

/// Order identity captured at submission time so the dispatch task can build
/// proper order events without consulting the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

/// Shared dispatch state for the Derive WS execution loop.
///
/// `order_identities` populates on successful `submit_order` and is consulted
/// by both the `.orders` and `.trades` dispatch paths to decide whether a
/// frame belongs to a tracked or external order.
#[derive(Debug, Default)]
pub struct WsDispatchState {
    order_identities: Mutex<AHashMap<ClientOrderId, OrderIdentity>>,
    emitted_accepted: Mutex<FifoCache<ClientOrderId, ORDER_DEDUP_CAPACITY>>,
    filled_orders: Mutex<FifoCache<ClientOrderId, ORDER_DEDUP_CAPACITY>>,
    emitted_trades: Mutex<FifoCache<TradeId, TRADE_DEDUP_CAPACITY>>,
}

impl WsDispatchState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an order identity captured at submission so subsequent WS
    /// frames for the same client_order_id resolve to the tracked path.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn register_identity(&self, client_order_id: ClientOrderId, identity: OrderIdentity) {
        self.order_identities
            .lock()
            .expect(MUTEX_POISONED)
            .insert(client_order_id, identity);
    }

    /// Returns the registered identity for a client order, when one was
    /// captured at submission time.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    #[must_use]
    pub fn identity(&self, client_order_id: &ClientOrderId) -> Option<OrderIdentity> {
        self.order_identities
            .lock()
            .expect(MUTEX_POISONED)
            .get(client_order_id)
            .copied()
    }

    /// Drops identity and the accepted marker for a terminal order so future
    /// stale frames (post-cancel cleanup, history backfill) take the untracked
    /// report path.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn forget(&self, client_order_id: &ClientOrderId) {
        self.order_identities
            .lock()
            .expect(MUTEX_POISONED)
            .remove(client_order_id);
        self.emitted_accepted
            .lock()
            .expect(MUTEX_POISONED)
            .remove(client_order_id);
    }

    /// Returns `true` when an `OrderAccepted` has already been emitted for
    /// this client order in the current process lifetime.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    #[must_use]
    pub fn contains_accepted(&self, client_order_id: &ClientOrderId) -> bool {
        self.emitted_accepted
            .lock()
            .expect(MUTEX_POISONED)
            .contains(client_order_id)
    }

    /// Records that `OrderAccepted` has been emitted for this client order.
    /// Returns `true` when the marker was already present (duplicate).
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn mark_accepted(&self, client_order_id: ClientOrderId) -> bool {
        let mut cache = self.emitted_accepted.lock().expect(MUTEX_POISONED);
        if cache.contains(&client_order_id) {
            return true;
        }
        cache.add(client_order_id);
        false
    }

    /// Returns `true` when this client order has reached a terminal filled
    /// state, used to suppress stale Accepted frames replayed on reconnect.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    #[must_use]
    pub fn contains_filled(&self, client_order_id: &ClientOrderId) -> bool {
        self.filled_orders
            .lock()
            .expect(MUTEX_POISONED)
            .contains(client_order_id)
    }

    /// Marks the client order as terminally filled. Idempotent.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn mark_filled(&self, client_order_id: ClientOrderId) {
        let mut cache = self.filled_orders.lock().expect(MUTEX_POISONED);
        if !cache.contains(&client_order_id) {
            cache.add(client_order_id);
        }
    }

    /// Inserts the trade id atomically. Returns `true` when the id was
    /// already present (i.e., this fill should be skipped as a duplicate).
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn check_and_insert_trade(&self, trade_id: TradeId) -> bool {
        let mut cache = self.emitted_trades.lock().expect(MUTEX_POISONED);
        if cache.contains(&trade_id) {
            return true;
        }
        cache.add(trade_id);
        false
    }

    /// Returns `true` when this trade id has already been seen, without
    /// mutating state.
    #[allow(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    #[must_use]
    pub fn contains_trade(&self, trade_id: &TradeId) -> bool {
        self.emitted_trades
            .lock()
            .expect(MUTEX_POISONED)
            .contains(trade_id)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId},
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
    fn test_register_and_identity_roundtrip() {
        let state = WsDispatchState::new();
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
        let state = WsDispatchState::new();
        let cid = ClientOrderId::from("STRAT-O-1");

        assert!(!state.mark_accepted(cid));
        assert!(state.contains_accepted(&cid));
        assert!(state.mark_accepted(cid));
    }

    #[rstest]
    fn test_check_and_insert_trade_returns_true_on_duplicate() {
        let state = WsDispatchState::new();
        let trade_id = TradeId::new("T-1");

        assert!(!state.check_and_insert_trade(trade_id));
        assert!(state.contains_trade(&trade_id));
        assert!(state.check_and_insert_trade(trade_id));
    }

    #[rstest]
    fn test_forget_clears_accepted_marker() {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::from("STRAT-O-1");

        state.mark_accepted(cid);
        state.forget(&cid);
        assert!(!state.contains_accepted(&cid));
    }
}
