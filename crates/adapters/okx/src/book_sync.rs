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
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use nautilus_core::AtomicMap;
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;

use crate::common::enums::OKXBookChannel;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    state: Arc<Mutex<BookSyncState>>,
}

#[derive(Debug, Default)]
struct BookSyncState {
    last_book_ts: AHashMap<InstrumentId, Instant>,
    last_sequences: AHashMap<InstrumentId, u64>,
    recovering: AHashSet<InstrumentId>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookSequenceOutcome {
    Accept,
    Suppress,
    Recover {
        last_seq_id: Option<u64>,
        prev_seq_id: Option<i64>,
        seq_id: u64,
    },
}

impl BookSyncTracker {
    pub(crate) fn record_subscription(&self, instrument_id: InstrumentId, now: Instant) {
        let mut state = self.state.lock();
        state.last_book_ts.insert(instrument_id, now);
        state.last_sequences.remove(&instrument_id);
        state.recovering.remove(&instrument_id);
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

    pub(crate) fn validate_sequence_if_subscribed(
        &self,
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        instrument_id: InstrumentId,
        is_snapshot: bool,
        sequences: &[(Option<i64>, u64)],
        timeout: Duration,
        now: Instant,
    ) -> BookSequenceOutcome {
        if !book_channels.contains_key(&instrument_id) || sequences.is_empty() {
            return BookSequenceOutcome::Suppress;
        }

        self.validate_sequence(instrument_id, is_snapshot, sequences, timeout, now)
    }

    pub(crate) fn validate_sequence(
        &self,
        instrument_id: InstrumentId,
        is_snapshot: bool,
        sequences: &[(Option<i64>, u64)],
        timeout: Duration,
        now: Instant,
    ) -> BookSequenceOutcome {
        if sequences.is_empty() {
            return BookSequenceOutcome::Suppress;
        }

        let mut state = self.state.lock();

        if is_snapshot {
            let invalid = sequences
                .iter()
                .find(|(prev_seq_id, _)| prev_seq_id.is_some_and(|value| value != -1));

            if let Some((prev_seq_id, seq_id)) = invalid {
                return begin_recovery(
                    &mut state,
                    instrument_id,
                    *prev_seq_id,
                    *seq_id,
                    timeout,
                    now,
                );
            }

            let seq_id = sequences.last().expect("sequences are non-empty").1;
            state.last_sequences.insert(instrument_id, seq_id);
            state.recovering.remove(&instrument_id);
            state.pending_snapshots.remove(&instrument_id);
            state.last_book_ts.insert(instrument_id, now);
            return BookSequenceOutcome::Accept;
        }

        if state.recovering.contains(&instrument_id) {
            return BookSequenceOutcome::Suppress;
        }

        let mut expected = state.last_sequences.get(&instrument_id).copied();
        for (prev_seq_id, seq_id) in sequences {
            let linked = match (expected, prev_seq_id) {
                (Some(expected), Some(previous)) => *previous >= 0 && *previous as u64 == expected,
                _ => false,
            };

            if !linked {
                return begin_recovery(
                    &mut state,
                    instrument_id,
                    *prev_seq_id,
                    *seq_id,
                    timeout,
                    now,
                );
            }
            expected = Some(*seq_id);
        }

        state.last_sequences.insert(
            instrument_id,
            expected.expect("an accepted sequence batch has a final sequence"),
        );
        state.last_book_ts.insert(instrument_id, now);
        BookSequenceOutcome::Accept
    }

    fn record_update(&self, instrument_id: InstrumentId, is_snapshot: bool, now: Instant) {
        let mut state = self.state.lock();
        state.last_book_ts.insert(instrument_id, now);

        if is_snapshot {
            state.pending_snapshots.remove(&instrument_id);
        }
    }

    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        let mut state = self.state.lock();
        state.last_book_ts.remove(&instrument_id);
        state.last_sequences.remove(&instrument_id);
        state.recovering.remove(&instrument_id);
        state.pending_snapshots.remove(&instrument_id);
    }

    pub(crate) fn clear(&self) {
        let mut state = self.state.lock();
        state.last_book_ts.clear();
        state.last_sequences.clear();
        state.recovering.clear();
        state.pending_snapshots.clear();
    }

    pub(crate) fn reset_sequences(
        &self,
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        scope: BookChannelScope,
    ) {
        let instrument_ids = book_channels
            .load()
            .iter()
            .filter_map(|(instrument_id, channel)| {
                book_channel_matches_scope(*channel, scope).then_some(*instrument_id)
            })
            .collect::<Vec<_>>();
        let mut state = self.state.lock();

        for instrument_id in instrument_ids {
            state.last_sequences.remove(&instrument_id);
            state.recovering.insert(instrument_id);
        }
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

        let mut state = self.state.lock();
        for instrument_id in &instrument_ids {
            state.pending_snapshots.insert(*instrument_id, deadline);
        }
        instrument_ids.len()
    }

    pub(crate) fn stale_books(&self, threshold: Duration, now: Instant) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock();
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
        let mut state = self.state.lock();
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

fn begin_recovery(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    prev_seq_id: Option<i64>,
    seq_id: u64,
    timeout: Duration,
    now: Instant,
) -> BookSequenceOutcome {
    let last_seq_id = state.last_sequences.remove(&instrument_id);
    if !timeout.is_zero() {
        state.pending_snapshots.insert(instrument_id, now + timeout);
    }

    if state.recovering.insert(instrument_id) {
        BookSequenceOutcome::Recover {
            last_seq_id,
            prev_seq_id,
            seq_id,
        }
    } else {
        BookSequenceOutcome::Suppress
    }
}

fn book_channel_matches_scope(channel: OKXBookChannel, scope: BookChannelScope) -> bool {
    match scope {
        BookChannelScope::Public => matches!(
            channel,
            OKXBookChannel::Book
                | OKXBookChannel::BookL2Tbt
                | OKXBookChannel::Books50L2Tbt
                | OKXBookChannel::BooksRpi
        ),
        BookChannelScope::Business => matches!(channel, OKXBookChannel::SprdBooks5),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use nautilus_core::AtomicMap;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{BookChannelScope, BookSequenceOutcome, BookSyncSignalKind, BookSyncTracker};
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

    #[rstest]
    fn sequence_accepts_snapshot_and_linked_update_with_skipped_sequence_ids() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        book_channels.insert(instrument_id, OKXBookChannel::BooksRpi);
        tracker.record_subscription(instrument_id, now);

        let snapshot = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            true,
            &[(Some(-1), 1_226)],
            Duration::from_secs(3),
            now,
        );
        let update = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            false,
            &[(Some(1_226), 1_230)],
            Duration::from_secs(3),
            now,
        );

        assert_eq!(snapshot, BookSequenceOutcome::Accept);
        assert_eq!(update, BookSequenceOutcome::Accept);
        assert_eq!(last_sequence(&tracker, instrument_id), Some(1_230));
    }

    #[rstest]
    fn sequence_gap_requests_one_recovery_and_waits_for_snapshot() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        book_channels.insert(instrument_id, OKXBookChannel::BooksRpi);
        tracker.record_subscription(instrument_id, now);
        assert_eq!(
            tracker.validate_sequence_if_subscribed(
                &book_channels,
                instrument_id,
                true,
                &[(Some(-1), 1_226)],
                timeout,
                now,
            ),
            BookSequenceOutcome::Accept
        );

        let gap = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            false,
            &[(Some(1_225), 1_230)],
            timeout,
            now,
        );
        let repeated = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            false,
            &[(Some(1_230), 1_231)],
            timeout,
            now,
        );
        let snapshot = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            true,
            &[(Some(-1), 2_000)],
            timeout,
            now,
        );
        let linked = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            false,
            &[(Some(2_000), 2_004)],
            timeout,
            now,
        );

        assert_eq!(
            gap,
            BookSequenceOutcome::Recover {
                last_seq_id: Some(1_226),
                prev_seq_id: Some(1_225),
                seq_id: 1_230,
            }
        );
        assert_eq!(repeated, BookSequenceOutcome::Suppress);
        assert_eq!(snapshot, BookSequenceOutcome::Accept);
        assert_eq!(linked, BookSequenceOutcome::Accept);
        assert_eq!(last_sequence(&tracker, instrument_id), Some(2_004));
        assert!(!has_pending_snapshot(&tracker, instrument_id));
    }

    #[rstest]
    fn sequence_reset_suppresses_updates_until_fresh_snapshot() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, now);
        assert_eq!(
            tracker.validate_sequence_if_subscribed(
                &book_channels,
                instrument_id,
                true,
                &[(Some(-1), 100)],
                timeout,
                now,
            ),
            BookSequenceOutcome::Accept
        );

        tracker.reset_sequences(&book_channels, BookChannelScope::Public);
        let update = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            false,
            &[(Some(100), 101)],
            timeout,
            now,
        );
        let snapshot = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            true,
            &[(Some(-1), 200)],
            timeout,
            now,
        );

        assert_eq!(update, BookSequenceOutcome::Suppress);
        assert_eq!(snapshot, BookSequenceOutcome::Accept);
        assert_eq!(last_sequence(&tracker, instrument_id), Some(200));
    }

    #[rstest]
    fn sequence_missing_previous_id_requests_recovery() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, now);
        assert_eq!(
            tracker.validate_sequence_if_subscribed(
                &book_channels,
                instrument_id,
                true,
                &[(Some(-1), 100)],
                timeout,
                now,
            ),
            BookSequenceOutcome::Accept
        );

        let update = tracker.validate_sequence_if_subscribed(
            &book_channels,
            instrument_id,
            false,
            &[(None, 101)],
            timeout,
            now,
        );

        assert_eq!(
            update,
            BookSequenceOutcome::Recover {
                last_seq_id: Some(100),
                prev_seq_id: None,
                seq_id: 101,
            }
        );
    }

    fn has_last_book_ts(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker
            .state
            .lock()
            .last_book_ts
            .contains_key(&instrument_id)
    }

    fn has_pending_snapshot(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker
            .state
            .lock()
            .pending_snapshots
            .contains_key(&instrument_id)
    }

    fn last_sequence(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> Option<u64> {
        tracker
            .state
            .lock()
            .last_sequences
            .get(&instrument_id)
            .copied()
    }

    fn is_empty(tracker: &BookSyncTracker) -> bool {
        let state = tracker.state.lock();
        state.last_book_ts.is_empty()
            && state.last_sequences.is_empty()
            && state.recovering.is_empty()
            && state.pending_snapshots.is_empty()
    }
}
