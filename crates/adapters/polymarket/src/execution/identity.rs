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

//! Tracked own-order identity registry for the Polymarket execution client.
//!
//! The user WebSocket dispatch runs on a spawned task without cache access, so it cannot
//! resolve an [`OrderAny`](nautilus_model::orders::OrderAny) to build order events. The submit
//! path captures the identity fields needed to construct `OrderAccepted` / `OrderFilled` /
//! `OrderCanceled` / `OrderRejected` / `OrderExpired` directly, keyed by venue order ID, and the
//! dispatch consults this registry to emit events for tracked orders (reserving reports for
//! externally-managed orders and reconciliation).

use std::sync::Mutex;

use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::MUTEX_POISONED;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, VenueOrderId},
    orders::{Order, OrderAny},
};

/// Identity fields captured at submit so the cache-free WS dispatch can build order events.
///
/// `trader_id` and `account_id` are client-wide constants threaded from the dispatch context,
/// so they are not stored here. Fill-specific values (`last_qty`, `last_px`, `trade_id`,
/// `commission`) come from the venue trade payload.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OrderIdentity {
    pub client_order_id: ClientOrderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

impl OrderIdentity {
    /// Captures the identity from an order held by the submit path.
    pub(crate) fn from_order(order: &OrderAny) -> Self {
        Self {
            client_order_id: order.client_order_id(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
        }
    }
}

/// Shared registry of tracked own-order identities, keyed by venue order ID.
///
/// Populated by the submit path (which holds the `OrderAny`) and consulted by the WS dispatch
/// and buffer-drain paths. The `accepted` set deduplicates `OrderAccepted` so acceptance is
/// emitted exactly once across the submit confirmation and the WS stream, including when a fill
/// or cancel races ahead of the acceptance message.
#[derive(Debug, Default)]
pub(crate) struct OrderIdentityRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    identities: FifoCacheMap<VenueOrderId, OrderIdentity, 10_000>,
    accepted: FifoCache<VenueOrderId, 10_000>,
}

impl OrderIdentityRegistry {
    /// Records the identity for a tracked order under its venue order ID.
    pub(crate) fn register_order_identity(
        &self,
        venue_order_id: VenueOrderId,
        identity: OrderIdentity,
    ) {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .identities
            .insert(venue_order_id, identity);
    }

    /// Returns the identity for a tracked order, if known.
    pub(crate) fn get(&self, venue_order_id: &VenueOrderId) -> Option<OrderIdentity> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .identities
            .get(venue_order_id)
            .copied()
    }

    /// Marks acceptance as emitted, returning `true` only when this call newly marks it.
    ///
    /// Callers emit `OrderAccepted` only on a `true` result, so acceptance is emitted once
    /// across the submit confirmation and the WS stream.
    pub(crate) fn mark_accepted(&self, venue_order_id: VenueOrderId) -> bool {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if guard.accepted.contains(&venue_order_id) {
            false
        } else {
            guard.accepted.add(venue_order_id);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn test_identity() -> OrderIdentity {
        OrderIdentity {
            client_order_id: ClientOrderId::from("O-1"),
            strategy_id: StrategyId::from("S-1"),
            instrument_id: InstrumentId::from("TEST.POLYMARKET"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        }
    }

    #[rstest]
    fn test_register_and_get() {
        let registry = OrderIdentityRegistry::default();
        let vid = VenueOrderId::from("V-1");
        assert!(registry.get(&vid).is_none());

        registry.register_order_identity(vid, test_identity());
        let identity = registry.get(&vid).expect("identity registered");
        assert_eq!(identity.client_order_id, ClientOrderId::from("O-1"));
        assert_eq!(identity.order_side, OrderSide::Buy);
    }

    #[rstest]
    fn test_mark_accepted_is_idempotent() {
        let registry = OrderIdentityRegistry::default();
        let vid = VenueOrderId::from("V-1");

        assert!(registry.mark_accepted(vid), "first mark is new");
        assert!(!registry.mark_accepted(vid), "second mark is a no-op");
    }
}
