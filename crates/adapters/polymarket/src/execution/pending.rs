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

//! Trackers for in-flight submit and cancel commands whose venue order ID is not yet known.

use std::sync::Arc;

use ahash::AHashMap;
use nautilus_core::UUID4;
use nautilus_model::identifiers::{ClientOrderId, VenueOrderId};
use parking_lot::Mutex;

/// Maps an in-flight submit's expected venue order ID to its local client order ID, so the
/// cache-free WS dispatch can resolve a tracked own order before the submit response lands.
#[derive(Clone, Debug, Default)]
pub(crate) struct PendingSubmitTracker {
    venue_to_client: Arc<Mutex<AHashMap<VenueOrderId, ClientOrderId>>>,
}

impl PendingSubmitTracker {
    pub(crate) fn insert(&self, venue_order_id: VenueOrderId, client_order_id: ClientOrderId) {
        self.venue_to_client
            .lock()
            .insert(venue_order_id, client_order_id);
    }

    pub(crate) fn client_order_id(&self, venue_order_id: &VenueOrderId) -> Option<ClientOrderId> {
        self.venue_to_client.lock().get(venue_order_id).copied()
    }

    pub(crate) fn venue_order_id(&self, client_order_id: ClientOrderId) -> Option<VenueOrderId> {
        let guard = self.venue_to_client.lock();
        let mut matches = guard
            .iter()
            .filter(|(_, client)| **client == client_order_id);
        let (venue_order_id, _) = matches.next()?;
        matches.next().is_none().then_some(*venue_order_id)
    }
}

/// Tracks client order IDs whose cancel was deferred because the venue order ID was not yet
/// known, so the cancel can be issued once the submit response lands.
#[derive(Clone, Debug, Default)]
pub(crate) struct PendingCancelTracker {
    client_order_ids: Arc<Mutex<AHashMap<ClientOrderId, Vec<Option<UUID4>>>>>,
}

impl PendingCancelTracker {
    pub(super) fn venue_order_id_or_defer(
        &self,
        client_order_id: ClientOrderId,
        causation_id: Option<UUID4>,
        lookup: impl FnOnce() -> Option<VenueOrderId>,
    ) -> Option<VenueOrderId> {
        // Submit registration must not check pending cancels between lookup and insertion
        let mut guard = self.client_order_ids.lock();
        let venue_order_id = lookup();

        if venue_order_id.is_none() {
            Self::record_command(&mut guard, client_order_id, causation_id);
        }

        venue_order_id
    }

    pub(super) fn insert(
        &self,
        client_order_id: ClientOrderId,
        causation_id: Option<UUID4>,
    ) -> bool {
        Self::record_command(
            &mut self.client_order_ids.lock(),
            client_order_id,
            causation_id,
        )
    }

    fn record_command(
        commands: &mut AHashMap<ClientOrderId, Vec<Option<UUID4>>>,
        client_order_id: ClientOrderId,
        causation_id: Option<UUID4>,
    ) -> bool {
        let first = !commands.contains_key(&client_order_id);
        let causes = commands.entry(client_order_id).or_default();
        if !causes.contains(&causation_id) {
            causes.push(causation_id);
        }

        first
    }

    pub(super) fn take(&self, client_order_id: &ClientOrderId) -> Vec<Option<UUID4>> {
        self.client_order_ids
            .lock()
            .remove(client_order_id)
            .unwrap_or_default()
    }

    pub(crate) fn remove(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_ids
            .lock()
            .remove(client_order_id)
            .is_some()
    }

    pub(crate) fn contains(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_ids.lock().contains_key(client_order_id)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread};

    use nautilus_live::execution::context::OrderContext;
    use nautilus_model::{
        enums::OrderType,
        identifiers::InstrumentId,
        orders::{Order, OrderTestBuilder},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;
    use crate::execution::context::OrderContextRegistry;

    #[rstest]
    #[case::unknown(None, true)]
    #[case::known(Some(VenueOrderId::from("V-CANCEL")), false)]
    fn test_cancel_uses_known_venue_id_or_defers(
        #[case] venue_order_id: Option<VenueOrderId>,
        #[case] deferred: bool,
    ) {
        let tracker = PendingCancelTracker::default();
        let client_order_id = ClientOrderId::from("O-CANCEL");
        let other_client_order_id = ClientOrderId::from("O-OTHER");
        tracker.insert(other_client_order_id, None);

        let result = tracker.venue_order_id_or_defer(client_order_id, None, || venue_order_id);

        assert_eq!(result, venue_order_id);
        assert_eq!(tracker.contains(&client_order_id), deferred);
        assert!(tracker.contains(&other_client_order_id));
    }

    #[rstest]
    fn test_merged_cancel_deferrals_preserve_each_command_once() {
        let tracker = PendingCancelTracker::default();
        let client_order_id = ClientOrderId::from("O-MERGED-CANCEL");
        let first = UUID4::new();
        let second = UUID4::new();
        assert_eq!(
            tracker.venue_order_id_or_defer(client_order_id, Some(first), || None),
            None
        );
        assert!(!tracker.insert(client_order_id, Some(second)));
        assert!(!tracker.insert(client_order_id, Some(first)));
        assert_eq!(
            tracker.take(&client_order_id),
            vec![Some(first), Some(second)]
        );
        assert!(!tracker.contains(&client_order_id));
        assert!(tracker.take(&client_order_id).is_empty());
    }

    #[rstest]
    fn test_cancel_deferral_cannot_be_missed_by_submit_registration() {
        let tracker = PendingCancelTracker::default();
        let contexts = OrderContextRegistry::default();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("TEST.POLYMARKET"))
            .client_order_id(ClientOrderId::from("O-RACING-CANCEL"))
            .quantity(Quantity::from("7"))
            .build();
        let client_order_id = order.client_order_id();
        let venue_order_id = VenueOrderId::from("V-RACING-CANCEL");
        let (lookup_tx, lookup_rx) = mpsc::channel();
        let (registered_tx, registered_rx) = mpsc::channel();

        thread::scope(|scope| {
            let tracker = &tracker;
            let contexts = &contexts;
            let order = &order;

            let submit = scope.spawn(move || {
                lookup_rx.recv().unwrap();
                contexts.recover_context(venue_order_id, OrderContext::from(order));
                let pending_check_blocked = tracker.client_order_ids.try_lock().is_none();
                registered_tx.send(pending_check_blocked).unwrap();
                tracker.contains(&client_order_id)
            });

            let result = tracker.venue_order_id_or_defer(client_order_id, None, || {
                let stale_venue_order_id = contexts.venue_order_id(&client_order_id);
                assert_eq!(stale_venue_order_id, None);
                lookup_tx.send(()).unwrap();
                assert!(registered_rx.recv().unwrap());
                stale_venue_order_id
            });

            let submit_saw_cancel = submit.join().unwrap();

            assert_eq!(result, None);
            assert!(submit_saw_cancel);
            assert_eq!(
                contexts.venue_order_id(&client_order_id),
                Some(venue_order_id)
            );
            assert!(tracker.remove(&client_order_id));
            assert!(!tracker.contains(&client_order_id));
        });
    }

    #[rstest]
    #[case::missing(0, None)]
    #[case::unique(1, Some(VenueOrderId::from("V-1")))]
    #[case::ambiguous(2, None)]
    fn venue_order_id_requires_unique_client_mapping(
        #[case] matching_orders: usize,
        #[case] expected: Option<VenueOrderId>,
    ) {
        let tracker = PendingSubmitTracker::default();
        let client_order_id = ClientOrderId::from("O-1");
        tracker.insert(
            VenueOrderId::from("V-OTHER"),
            ClientOrderId::from("O-OTHER"),
        );

        for venue_order_id in [VenueOrderId::from("V-1"), VenueOrderId::from("V-2")]
            .into_iter()
            .take(matching_orders)
        {
            tracker.insert(venue_order_id, client_order_id);
        }

        assert_eq!(tracker.venue_order_id(client_order_id), expected);
        assert_eq!(
            tracker.venue_order_id(ClientOrderId::from("O-OTHER")),
            Some(VenueOrderId::from("V-OTHER"))
        );
    }
}
