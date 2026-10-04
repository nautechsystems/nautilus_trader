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

//! Adapter-local order book synchronization state for Hyperliquid.
//!
//! [`BookSyncTracker`] keeps one shared [`BookSync`] per order book delta subscription and maps
//! `l2Book` frames onto its lifecycle. Each frame replaces the whole book, so every frame is a
//! snapshot: the tracker accepts it unless a subscription write gate blocks it.
//!
//! A subscribed book is present in the tracker from its subscribe until its unsubscribe, so a
//! frame for an absent book is suppressed. Claiming and accepting recovery stay under the tracker's
//! lock so competing events cannot independently change ownership. This module performs state
//! transitions; [`super::recovery`] runs the asynchronous subscription and retry work.

use std::sync::Arc;

use ahash::AHashMap;
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_live::book::{snapshot::SnapshotGate, sync::BookSync};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind, recovery::BookRecovery};
use crate::websocket::error::HyperliquidWsError;

type Book = BookSync<HyperliquidWsError>;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    books: Arc<Mutex<AHashMap<InstrumentId, Book>>>,
}

impl BookSyncTracker {
    /// Tracks a new subscription whose write `gate` guards its initial snapshot.
    ///
    /// Accepting a snapshot, claiming a recovery, or removing the book cancels the returned token.
    pub(crate) fn record_subscription(
        &self,
        instrument_id: InstrumentId,
        now: Instant,
        gate: SnapshotGate,
    ) -> CancellationToken {
        let mut book = Book::new(now);
        let cancel = book.expect_snapshot(gate);
        self.books.lock().insert(instrument_id, book);
        cancel
    }

    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        self.books.lock().remove(&instrument_id);
    }

    /// Removes a book whose subscription was rejected, unless a newer subscription replaced it.
    pub(crate) fn remove_subscription(
        &self,
        instrument_id: InstrumentId,
        cancel: &CancellationToken,
    ) {
        let mut books = self.books.lock();

        if !cancel.is_cancelled() {
            books.remove(&instrument_id);
        }
    }

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
    }

    /// Accepts an `l2Book` frame as a snapshot, returning whether the book may emit it.
    pub(crate) fn record_snapshot(&self, instrument_id: InstrumentId, now: Instant) -> bool {
        self.books
            .lock()
            .get_mut(&instrument_id)
            .is_some_and(|book| book.accept_snapshot((), now))
    }

    /// Restarts synchronization for every book after the connection reconnects.
    pub(crate) fn reset_on_reconnect(&self) {
        for book in self.books.lock().values_mut() {
            book.reset_on_reconnect();
        }
    }

    /// Arms a snapshot deadline for every book, returning how many were armed.
    pub(crate) fn seed_pending_snapshots(&self, timeout: Duration, now: Instant) -> usize {
        let deadline = now + timeout;

        self.books
            .lock()
            .values_mut()
            .map(|book| book.arm_deadline(deadline))
            .filter(|armed| *armed)
            .count()
    }

    pub(crate) fn take_expired_snapshots(&self, now: Instant) -> Vec<BookSyncSignal> {
        let mut expired = self
            .books
            .lock()
            .iter_mut()
            .filter_map(|(instrument_id, book)| {
                book.take_expired(now).then_some(BookSyncSignal {
                    instrument_id: *instrument_id,
                    kind: BookSyncSignalKind::SnapshotMissing,
                })
            })
            .collect::<Vec<_>>();

        // Sort by instrument; the book map iterates in per-process hash order
        expired.sort_by_key(|signal| signal.instrument_id);
        expired
    }

    /// Claims recovery for a subscription whose initial snapshot did not arrive, unless the
    /// subscription was replaced or removed first.
    pub(crate) fn claim_subscription_recovery(
        &self,
        instrument_id: InstrumentId,
        cancel: &CancellationToken,
    ) -> Option<Arc<BookRecovery>> {
        let mut books = self.books.lock();

        if cancel.is_cancelled() {
            return None;
        }

        books.get_mut(&instrument_id)?.claim()
    }

    /// Marks the book out of sync and claims recovery when nothing owns it.
    ///
    /// A book whose subscription write is still in flight belongs to that write.
    pub(crate) fn claim_recovery(&self, instrument_id: InstrumentId) -> Option<Arc<BookRecovery>> {
        let mut books = self.books.lock();
        let book = books
            .get_mut(&instrument_id)
            .filter(|book| !book.is_send_pending())?;

        if book.gap() == BookSequenceOutcome::Recover {
            book.claim()
        } else {
            None
        }
    }

    /// Marks the book out of sync after an invalid frame and claims recovery when nothing owns it.
    ///
    /// A running recovery receives `error` instead, so its current attempt fails without waiting
    /// for the snapshot deadline.
    pub(crate) fn reject_snapshot(
        &self,
        instrument_id: InstrumentId,
        error: HyperliquidWsError,
    ) -> Option<Arc<BookRecovery>> {
        let mut books = self.books.lock();
        let book = books
            .get_mut(&instrument_id)
            .filter(|book| !book.is_send_pending())?;

        if book.gap() == BookSequenceOutcome::Recover {
            return book.claim();
        }

        book.reject(error);
        None
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::dst::time::{Duration, Instant};
    use nautilus_live::book::{
        recovery::BookRecoveryOutcome, snapshot::SnapshotGate, sync::BookPhase,
    };
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{BookSyncSignalKind, BookSyncTracker};
    use crate::websocket::error::HyperliquidWsError;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTC-USD-PERP.HYPERLIQUID")
    }

    fn closed_gate() -> SnapshotGate {
        let gate = SnapshotGate::default();
        gate.lock().close();
        gate
    }

    fn phase(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> Option<BookPhase<()>> {
        tracker
            .books
            .lock()
            .get(&instrument_id)
            .map(|book| *book.phase())
    }

    #[rstest]
    fn untracked_book_suppresses_frames() {
        let tracker = BookSyncTracker::default();

        let accepted = tracker.record_snapshot(instrument_id(), Instant::now());

        assert!(!accepted);
        assert_eq!(phase(&tracker, instrument_id()), None);
    }

    #[rstest]
    fn subscription_gate_blocks_frames_until_write_confirmed() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let gate = closed_gate();
        let cancel = tracker.record_subscription(instrument_id, now, gate.clone());

        let while_writing = tracker.record_snapshot(instrument_id, now);
        gate.open();
        let after_write = tracker.record_snapshot(instrument_id, now);
        let next_frame = tracker.record_snapshot(instrument_id, now);

        assert!(!while_writing);
        assert!(after_write);
        assert!(next_frame);
        assert!(cancel.is_cancelled());
        assert_eq!(phase(&tracker, instrument_id), Some(BookPhase::Synced(())));
    }

    #[rstest]
    fn removal_cancels_initial_wait_and_suppresses_frames() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let cancel = tracker.record_subscription(instrument_id, now, SnapshotGate::default());

        tracker.remove(instrument_id);
        let accepted = tracker.record_snapshot(instrument_id, now);

        assert!(cancel.is_cancelled());
        assert!(!accepted);
        assert!(!tracker.books.lock().contains_key(&instrument_id));
    }

    #[rstest]
    fn rejected_subscription_removal_keeps_newer_subscription() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let first = tracker.record_subscription(instrument_id, now, closed_gate());
        let second = tracker.record_subscription(instrument_id, now, closed_gate());

        tracker.remove_subscription(instrument_id, &first);
        let kept = tracker.books.lock().contains_key(&instrument_id);
        tracker.remove_subscription(instrument_id, &second);
        let removed = !tracker.books.lock().contains_key(&instrument_id);

        assert!(kept);
        assert!(removed);
    }

    #[rstest]
    fn subscription_recovery_refused_after_resubscribe() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let first = tracker.record_subscription(instrument_id, now, closed_gate());
        let second = tracker.record_subscription(instrument_id, now, closed_gate());

        let stale = tracker.claim_subscription_recovery(instrument_id, &first);
        let current = tracker.claim_subscription_recovery(instrument_id, &second);

        assert!(first.is_cancelled());
        assert!(stale.is_none());
        assert!(current.is_some_and(|recovery| recovery.is_running()));
        assert!(second.is_cancelled(), "claim retires the initial wait");
    }

    #[rstest]
    fn claim_recovery_waits_for_initial_write_then_admits_one_owner() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let gate = closed_gate();
        tracker.record_subscription(instrument_id, now, gate.clone());

        let during_write = tracker.claim_recovery(instrument_id);
        gate.open();
        let first = tracker.claim_recovery(instrument_id);
        let second = tracker.claim_recovery(instrument_id);

        assert!(during_write.is_none());
        assert!(first.as_ref().is_some_and(|recovery| recovery.is_running()));
        assert!(second.is_none());
        assert_eq!(phase(&tracker, instrument_id), Some(BookPhase::Recovering));
    }

    #[rstest]
    fn recovery_gate_blocks_frames_until_replacement_written() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        assert!(tracker.record_snapshot(instrument_id, now));
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(recovery.begin_replacement());

        let during_write = tracker.record_snapshot(instrument_id, now);
        recovery.gate.open();
        let after_write = tracker.record_snapshot(instrument_id, now);

        assert!(!during_write);
        assert!(after_write);
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
        assert!(tracker.claim_recovery(instrument_id).is_some());
    }

    #[rstest]
    fn rejected_snapshot_claims_unowned_book_then_fails_running_attempt() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let error = || HyperliquidWsError::InvalidSnapshot("time overflow".into());
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        assert!(tracker.record_snapshot(instrument_id, now));

        let claimed = tracker.reject_snapshot(instrument_id, error()).unwrap();
        let running = tracker.reject_snapshot(instrument_id, error());

        assert!(running.is_none());
        assert!(claimed.is_running());
        assert!(matches!(
            &*claimed.outcome.borrow(),
            BookRecoveryOutcome::Rejected(HyperliquidWsError::InvalidSnapshot(message))
            if message == "time overflow"
        ));
        assert_eq!(phase(&tracker, instrument_id), Some(BookPhase::Recovering));
    }

    #[rstest]
    fn rejected_snapshot_ignored_while_initial_write_in_flight() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let cancel = tracker.record_subscription(instrument_id, now, closed_gate());

        let claimed = tracker.reject_snapshot(
            instrument_id,
            HyperliquidWsError::InvalidSnapshot("time overflow".into()),
        );

        assert!(claimed.is_none());
        assert!(!cancel.is_cancelled());
        assert_eq!(phase(&tracker, instrument_id), Some(BookPhase::Waiting));
    }

    #[rstest]
    fn reconnect_deadline_expires_once_without_snapshot() {
        let tracker = BookSyncTracker::default();
        let synced = instrument_id();
        let recovered = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
        let now = Instant::now();
        let timeout = Duration::from_secs(10);

        for instrument_id in [synced, recovered] {
            tracker.record_subscription(instrument_id, now, SnapshotGate::default());
            assert!(tracker.record_snapshot(instrument_id, now));
        }

        tracker.reset_on_reconnect();
        let armed = tracker.seed_pending_snapshots(timeout, now);
        let owned = tracker.claim_recovery(synced);
        assert!(tracker.record_snapshot(recovered, now));
        let early = tracker.take_expired_snapshots(now + Duration::from_secs(9));
        let expired = tracker.take_expired_snapshots(now + timeout);
        let again = tracker.take_expired_snapshots(now + timeout);

        assert_eq!(armed, 2);
        assert!(owned.is_none(), "an armed deadline owns the book");
        assert!(early.is_empty());
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].instrument_id, synced);
        assert_eq!(expired[0].kind, BookSyncSignalKind::SnapshotMissing);
        assert!(again.is_empty());
        assert_eq!(phase(&tracker, synced), Some(BookPhase::Recovering));
        assert_eq!(phase(&tracker, recovered), Some(BookPhase::Synced(())));
    }

    #[rstest]
    fn reconnect_keeps_running_recovery() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        tracker.reset_on_reconnect();
        tracker.seed_pending_snapshots(Duration::from_secs(10), now);
        let accepted = tracker.record_snapshot(instrument_id, now);

        assert!(accepted);
        assert!(recovery.is_accepted());
    }

    #[rstest]
    fn reconnect_keeps_initial_write_wait() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let cancel = tracker.record_subscription(instrument_id, now, closed_gate());

        tracker.reset_on_reconnect();
        let armed = tracker.seed_pending_snapshots(Duration::from_secs(10), now);

        assert_eq!(armed, 0);
        assert!(!cancel.is_cancelled());
    }

    #[rstest]
    fn clear_cancels_every_episode() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        tracker.clear();

        assert!(recovery.cancellation.is_cancelled());
        assert!(!tracker.books.lock().contains_key(&instrument_id));
    }
}
