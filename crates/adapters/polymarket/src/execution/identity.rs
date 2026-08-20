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

use ahash::{AHashMap, AHashSet};
use nautilus_core::MUTEX_POISONED;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, VenueOrderId},
    orders::{Order, OrderAny},
    reports::OrderStatusReport,
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
    pub time_in_force: TimeInForce,
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
            time_in_force: order.time_in_force(),
        }
    }

    /// Validates that a provider report belongs to this tracked order before it can mutate state.
    pub(crate) fn validate_order_report(
        &self,
        report: &OrderStatusReport,
        venue_order_id: VenueOrderId,
        expected_client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            report.venue_order_id == venue_order_id,
            "order report venue order ID {} does not match expected venue order ID {venue_order_id}",
            report.venue_order_id
        );
        anyhow::ensure!(
            self.instrument_id == report.instrument_id,
            "order report instrument {} does not match tracked instrument {}",
            report.instrument_id,
            self.instrument_id
        );
        anyhow::ensure!(
            self.order_side == report.order_side,
            "order report side {} does not match tracked side {}",
            report.order_side,
            self.order_side
        );
        anyhow::ensure!(
            self.time_in_force == report.time_in_force,
            "order report time in force {} does not match tracked time in force {}",
            report.time_in_force,
            self.time_in_force
        );

        if let Some(client_order_id) = report.client_order_id {
            anyhow::ensure!(
                self.client_order_id == client_order_id,
                "order report client order ID {client_order_id} does not match tracked client order ID {}",
                self.client_order_id
            );
        }

        if let Some(client_order_id) = expected_client_order_id {
            anyhow::ensure!(
                self.client_order_id == client_order_id,
                "pending submit client order ID {client_order_id} does not match tracked client order ID {}",
                self.client_order_id
            );
        }
        Ok(())
    }

    /// Returns true when any taker fill implies full completion.
    ///
    /// FOK is atomic, so a sub-cent difference between its registered and filled quantities is
    /// normalization. IOC maps to venue FAK: every positive remainder is canceled.
    pub(crate) fn requires_terminal_quantity_normalization(&self) -> bool {
        self.time_in_force == TimeInForce::Fok
    }
}

/// Shared registry of tracked own-order identities, keyed by venue order ID.
///
/// Populated by the submit path (which holds the `OrderAny`) and consulted by the WS dispatch
/// and buffer-drain paths. Active identity and the accepted marker stay in unbounded maps so FIFO
/// replay eviction cannot reclassify a still-owned update as external or emit a second
/// `OrderAccepted`.
#[derive(Debug, Default)]
pub(crate) struct OrderIdentityRegistry {
    inner: Mutex<RegistryInner>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    identities: AHashMap<VenueOrderId, OrderIdentity>,
    client_to_venue: AHashMap<ClientOrderId, VenueOrderId>,
    accepted: AHashSet<VenueOrderId>,
}

impl OrderIdentityRegistry {
    pub(crate) fn clear(&self) {
        *self.inner.lock().expect(MUTEX_POISONED) = RegistryInner::default();
    }

    /// Records the identity for a tracked order under its venue order ID.
    pub(crate) fn register_order_identity(
        &self,
        venue_order_id: VenueOrderId,
        identity: OrderIdentity,
    ) {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        guard.identities.insert(venue_order_id, identity);
        guard
            .client_to_venue
            .insert(identity.client_order_id, venue_order_id);
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

    pub(crate) fn remove(&self, venue_order_id: &VenueOrderId) -> Option<OrderIdentity> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let identity = guard.identities.remove(venue_order_id)?;
        if guard.client_to_venue.get(&identity.client_order_id) == Some(venue_order_id) {
            guard.client_to_venue.remove(&identity.client_order_id);
        }
        guard.accepted.remove(venue_order_id);
        Some(identity)
    }

    /// Returns the latest venue order ID captured for a tracked client order.
    pub(crate) fn venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .client_to_venue
            .get(client_order_id)
            .copied()
    }

    pub(crate) fn venue_order_ids_for_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> Vec<VenueOrderId> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .identities
            .iter()
            .filter_map(|(venue_order_id, identity)| {
                (identity.instrument_id == *instrument_id).then_some(*venue_order_id)
            })
            .collect()
    }

    /// Marks acceptance as emitted, returning `true` only when this call newly marks it.
    ///
    /// Callers emit `OrderAccepted` only on a `true` result, so acceptance is emitted once
    /// across the submit confirmation and the WS stream.
    pub(crate) fn mark_accepted(&self, venue_order_id: VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .accepted
            .insert(venue_order_id)
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
            time_in_force: TimeInForce::Gtc,
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
        assert_eq!(
            registry.venue_order_id(&ClientOrderId::from("O-1")),
            Some(vid)
        );
    }

    #[rstest]
    fn test_mark_accepted_is_idempotent() {
        let registry = OrderIdentityRegistry::default();
        let vid = VenueOrderId::from("V-1");

        assert!(registry.mark_accepted(vid), "first mark is new");
        assert!(!registry.mark_accepted(vid), "second mark is a no-op");
    }

    #[rstest]
    fn test_mark_accepted_retains_flag_after_later_capacity_flood() {
        let registry = OrderIdentityRegistry::default();
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
        let registry = OrderIdentityRegistry::default();
        let retained = VenueOrderId::from("V-RETAIN");
        registry.register_order_identity(retained, test_identity());

        for index in 0..10_000 {
            registry.register_order_identity(
                VenueOrderId::from(format!("V-FLOOD-{index}").as_str()),
                OrderIdentity {
                    client_order_id: ClientOrderId::from(format!("O-FLOOD-{index}").as_str()),
                    ..test_identity()
                },
            );
        }

        let identity = registry
            .get(&retained)
            .expect("active identity must survive later registrations");
        assert_eq!(identity.client_order_id, ClientOrderId::from("O-1"));
        assert_eq!(
            registry.venue_order_id(&ClientOrderId::from("O-1")),
            Some(retained)
        );
    }
}
