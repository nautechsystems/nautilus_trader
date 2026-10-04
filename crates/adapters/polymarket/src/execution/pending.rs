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

use ahash::{AHashMap, AHashSet};
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
    client_order_ids: Arc<Mutex<AHashSet<ClientOrderId>>>,
}

impl PendingCancelTracker {
    pub(crate) fn insert(&self, client_order_id: ClientOrderId) -> bool {
        self.client_order_ids.lock().insert(client_order_id)
    }

    pub(crate) fn remove(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_ids.lock().remove(client_order_id)
    }

    pub(crate) fn contains(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_ids.lock().contains(client_order_id)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
