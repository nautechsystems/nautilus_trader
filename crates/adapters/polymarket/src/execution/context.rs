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

//! Tracked own-order context registry for the Polymarket execution client.
//!
//! The user WebSocket dispatch runs on a spawned task without cache access, so it cannot
//! resolve an [`OrderAny`](nautilus_model::orders::OrderAny) to build order events. The submit
//! path captures shared order context needed to construct `OrderAccepted` / `OrderFilled` /
//! `OrderCanceled` / `OrderRejected` / `OrderExpired` directly, keyed by venue order ID, and the
//! dispatch consults this registry to emit events for tracked orders (reserving reports for
//! externally-managed orders and reconciliation).

use ahash::{AHashMap, AHashSet};
use nautilus_live::execution::context::OrderContext;
use nautilus_model::identifiers::{ClientOrderId, VenueOrderId};
use parking_lot::Mutex;

/// Shared registry of tracked own-order contexts, keyed by venue order ID.
///
/// Populated by the submit path (which holds the `OrderAny`) and consulted by the WS dispatch
/// and buffer-drain paths. Active identity and the accepted marker stay in unbounded maps so FIFO
/// replay eviction cannot reclassify a still-owned update as external or emit a second
/// `OrderAccepted`.
#[derive(Debug, Default)]
pub(crate) struct OrderContextRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    contexts: AHashMap<VenueOrderId, OrderContext>,
    client_to_venue: AHashMap<ClientOrderId, VenueOrderId>,
    accepted: AHashSet<VenueOrderId>,
}

impl OrderContextRegistry {
    /// Records the context for a tracked order under its venue order ID.
    pub(crate) fn register_context(&self, venue_order_id: VenueOrderId, context: OrderContext) {
        let mut guard = self.inner.lock();
        guard.contexts.insert(venue_order_id, context);
        guard
            .client_to_venue
            .insert(context.identity.client_order_id, venue_order_id);
    }

    /// Returns the context for a tracked order, if known.
    pub(crate) fn get(&self, venue_order_id: &VenueOrderId) -> Option<OrderContext> {
        self.inner.lock().contexts.get(venue_order_id).copied()
    }

    /// Returns the latest venue order ID captured for a tracked client order.
    pub(crate) fn venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.inner
            .lock()
            .client_to_venue
            .get(client_order_id)
            .copied()
    }

    /// Marks acceptance as emitted, returning `true` only when this call newly marks it.
    ///
    /// Callers emit `OrderAccepted` only on a `true` result, so acceptance is emitted once
    /// across the submit confirmation and the WS stream.
    pub(crate) fn mark_accepted(&self, venue_order_id: VenueOrderId) -> bool {
        self.inner.lock().accepted.insert(venue_order_id)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_live::execution::context::OrderIdentity;
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce},
        identifiers::{InstrumentId, StrategyId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn test_context() -> OrderContext {
        OrderContext {
            identity: OrderIdentity {
                client_order_id: ClientOrderId::from("O-1"),
                strategy_id: StrategyId::from("S-1"),
                instrument_id: InstrumentId::from("TEST.POLYMARKET"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
            quantity: Quantity::from("12.34"),
            price: Some(Price::from("0.5678")),
            trigger_price: None,
            trigger_type: None,
            time_in_force: TimeInForce::Gtc,
            is_post_only: true,
            is_reduce_only: false,
            is_quote_quantity: false,
        }
    }

    #[rstest]
    #[case(TimeInForce::Gtc)]
    #[case(TimeInForce::Fok)]
    #[case(TimeInForce::Ioc)]
    fn test_register_and_get(#[case] time_in_force: TimeInForce) {
        let registry = OrderContextRegistry::default();
        let vid = VenueOrderId::from("V-1");
        assert!(registry.get(&vid).is_none());

        let expected = OrderContext {
            time_in_force,
            ..test_context()
        };
        registry.register_context(vid, expected);
        let context = registry.get(&vid).expect("identity registered");
        assert_eq!(context, expected);
        assert_eq!(
            registry.venue_order_id(&ClientOrderId::from("O-1")),
            Some(vid)
        );
    }

    #[rstest]
    fn test_mark_accepted_is_idempotent() {
        let registry = OrderContextRegistry::default();
        let vid = VenueOrderId::from("V-1");

        assert!(registry.mark_accepted(vid), "first mark is new");
        assert!(!registry.mark_accepted(vid), "second mark is a no-op");
    }

    #[rstest]
    fn test_mark_accepted_retains_flag_after_later_capacity_flood() {
        let registry = OrderContextRegistry::default();
        let retained = VenueOrderId::from("V-RETAIN");
        assert!(registry.mark_accepted(retained));

        for index in 0..10_000 {
            assert!(
                registry.mark_accepted(VenueOrderId::from(format!("V-FLOOD-{index}").as_str()))
            );
        }

        assert!(!registry.mark_accepted(retained));
    }

    #[rstest]
    fn test_register_retains_identity_after_later_capacity_flood() {
        let registry = OrderContextRegistry::default();
        let retained = VenueOrderId::from("V-RETAIN");
        registry.register_context(retained, test_context());

        for index in 0..10_000 {
            registry.register_context(
                VenueOrderId::from(format!("V-FLOOD-{index}").as_str()),
                OrderContext {
                    identity: OrderIdentity {
                        client_order_id: ClientOrderId::from(format!("O-FLOOD-{index}").as_str()),
                        ..test_context().identity
                    },
                    ..test_context()
                },
            );
        }

        let context = registry
            .get(&retained)
            .expect("active identity must survive later registrations");
        assert_eq!(context, test_context());
        assert_eq!(
            registry.venue_order_id(&ClientOrderId::from("O-1")),
            Some(retained)
        );
    }
}
