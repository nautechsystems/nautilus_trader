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

//! Adapter-local order book synchronization state for OKX.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use nautilus_core::{AtomicMap, MUTEX_POISONED};
use nautilus_model::identifiers::InstrumentId;

use crate::common::enums::OKXBookChannel;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    state: Arc<Mutex<BookSyncState>>,
}

#[derive(Debug, Default)]
struct BookSyncState {
    last_book_ts: AHashMap<InstrumentId, Instant>,
    pending_snapshots: AHashMap<InstrumentId, Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookChannelScope {
    Public,
    Business,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookSyncSignalKind {
    Stale { elapsed: Duration },
    SnapshotMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BookSyncSignal {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) kind: BookSyncSignalKind,
}

impl BookSyncTracker {
    pub(crate) fn record_subscription(&self, instrument_id: InstrumentId, now: Instant) {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        state.last_book_ts.insert(instrument_id, now);
        state.pending_snapshots.remove(&instrument_id);
    }

    pub(crate) fn record_update_if_subscribed(
        &self,
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        instrument_id: InstrumentId,
        is_snapshot: bool,
        now: Instant,
    ) {
        if book_channels.contains_key(&instrument_id) {
            self.record_update(instrument_id, is_snapshot, now);
        }
    }

    fn record_update(&self, instrument_id: InstrumentId, is_snapshot: bool, now: Instant) {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        state.last_book_ts.insert(instrument_id, now);

        if is_snapshot {
            state.pending_snapshots.remove(&instrument_id);
        }
    }

    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        state.last_book_ts.remove(&instrument_id);
        state.pending_snapshots.remove(&instrument_id);
    }

    pub(crate) fn clear(&self) {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        state.last_book_ts.clear();
        state.pending_snapshots.clear();
    }

    pub(crate) fn seed_pending_snapshots(
        &self,
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        scope: BookChannelScope,
        timeout: Duration,
        now: Instant,
    ) -> usize {
        let deadline = now + timeout;
        let instrument_ids = book_channels
            .load()
            .iter()
            .filter_map(|(instrument_id, channel)| {
                book_channel_matches_scope(*channel, scope).then_some(*instrument_id)
            })
            .collect::<Vec<_>>();

        if instrument_ids.is_empty() {
            return 0;
        }

        let mut state = self.state.lock().expect(MUTEX_POISONED);
        for instrument_id in &instrument_ids {
            state.pending_snapshots.insert(*instrument_id, deadline);
        }
        instrument_ids.len()
    }

    pub(crate) fn stale_books(&self, threshold: Duration, now: Instant) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        let stale = state
            .last_book_ts
            .iter()
            .filter_map(|(instrument_id, last_update)| {
                let elapsed = now.checked_duration_since(*last_update)?;
                (elapsed > threshold).then_some(BookSyncSignal {
                    instrument_id: *instrument_id,
                    kind: BookSyncSignalKind::Stale { elapsed },
                })
            })
            .collect::<Vec<_>>();

        for signal in &stale {
            state.last_book_ts.remove(&signal.instrument_id);
            state.pending_snapshots.remove(&signal.instrument_id);
        }

        stale
    }

    pub(crate) fn expired_pending_snapshots(&self, now: Instant) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        let expired = state
            .pending_snapshots
            .iter()
            .filter_map(|(instrument_id, deadline)| {
                (*deadline <= now).then_some(BookSyncSignal {
                    instrument_id: *instrument_id,
                    kind: BookSyncSignalKind::SnapshotMissing,
                })
            })
            .collect::<Vec<_>>();

        for signal in &expired {
            state.pending_snapshots.remove(&signal.instrument_id);
            state.last_book_ts.remove(&signal.instrument_id);
        }

        expired
    }
}

fn book_channel_matches_scope(channel: OKXBookChannel, scope: BookChannelScope) -> bool {
    match scope {
        BookChannelScope::Public => matches!(
            channel,
            OKXBookChannel::Book | OKXBookChannel::BookL2Tbt | OKXBookChannel::Books50L2Tbt
        ),
        BookChannelScope::Business => matches!(channel, OKXBookChannel::SprdBooks5),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use nautilus_core::{AtomicMap, MUTEX_POISONED};
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{BookChannelScope, BookSyncSignalKind, BookSyncTracker};
    use crate::common::enums::OKXBookChannel;

    #[rstest]
    fn record_update_if_subscribed_removes_pending_snapshot() {
        let tracker = BookSyncTracker::default();
        let book_channels = AtomicMap::new();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now,
        );

        tracker.record_update_if_subscribed(&book_channels, instrument_id, true, now);

        assert!(has_last_book_ts(&tracker, instrument_id));
        assert!(!has_pending_snapshot(&tracker, instrument_id));
    }

    #[rstest]
    fn record_update_ignores_unsubscribed_instrument() {
        let tracker = BookSyncTracker::default();
        let book_channels = AtomicMap::new();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        tracker.record_update_if_subscribed(&book_channels, instrument_id, true, now);

        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn stale_books_emits_once() {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        tracker.record_subscription(
            instrument_id,
            now.checked_sub(Duration::from_secs(6)).unwrap(),
        );
        let first = tracker.stale_books(Duration::from_secs(5), now);
        let second = tracker.stale_books(Duration::from_secs(5), now);

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].instrument_id, instrument_id);
        assert_eq!(
            first[0].kind,
            BookSyncSignalKind::Stale {
                elapsed: Duration::from_secs(6)
            }
        );
        assert!(second.is_empty());
        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn seed_pending_snapshots_filters_by_socket_scope() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let public_instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let spread_instrument_id = InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX");
        let now = Instant::now();

        book_channels.insert(public_instrument_id, OKXBookChannel::Book);
        book_channels.insert(spread_instrument_id, OKXBookChannel::SprdBooks5);

        let public_count = tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now,
        );
        assert_eq!(public_count, 1);
        assert!(has_pending_snapshot(&tracker, public_instrument_id));
        assert!(!has_pending_snapshot(&tracker, spread_instrument_id));

        let business_count = tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Business,
            Duration::from_secs(3),
            now,
        );
        assert_eq!(business_count, 1);
        assert!(has_pending_snapshot(&tracker, spread_instrument_id));
    }

    #[rstest]
    fn expired_pending_snapshots_emits_once() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(
            instrument_id,
            now.checked_sub(Duration::from_secs(6)).unwrap(),
        );
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now.checked_sub(Duration::from_secs(4)).unwrap(),
        );

        let first = tracker.expired_pending_snapshots(now);
        let second = tracker.expired_pending_snapshots(now);

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].instrument_id, instrument_id);
        assert_eq!(first[0].kind, BookSyncSignalKind::SnapshotMissing);
        assert!(second.is_empty());
        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn remove_clears_tracking_state() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, now);
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now,
        );

        tracker.remove(instrument_id);

        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn clear_removes_all_tracking_state() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let public_instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let spread_instrument_id = InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX");
        let now = Instant::now();

        book_channels.insert(public_instrument_id, OKXBookChannel::Book);
        book_channels.insert(spread_instrument_id, OKXBookChannel::SprdBooks5);
        tracker.record_subscription(public_instrument_id, now);
        tracker.record_subscription(spread_instrument_id, now);
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now,
        );
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Business,
            Duration::from_secs(3),
            now,
        );

        tracker.clear();

        assert!(is_empty(&tracker));
    }

    fn has_last_book_ts(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker
            .state
            .lock()
            .expect(MUTEX_POISONED)
            .last_book_ts
            .contains_key(&instrument_id)
    }

    fn has_pending_snapshot(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker
            .state
            .lock()
            .expect(MUTEX_POISONED)
            .pending_snapshots
            .contains_key(&instrument_id)
    }

    fn is_empty(tracker: &BookSyncTracker) -> bool {
        let state = tracker.state.lock().expect(MUTEX_POISONED);
        state.last_book_ts.is_empty() && state.pending_snapshots.is_empty()
    }
}
