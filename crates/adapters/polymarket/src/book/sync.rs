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

//! Adapter-local order book synchronization state for Polymarket.
//!
//! [`BookSyncTracker`] maps Polymarket book events onto the shared per-book [`BookSync`] lifecycle
//! and decides whether to accept a snapshot or incremental, suppress it, or request recovery.
//! Polymarket books carry no sequence numbers, so incremental `price_change` deltas are gated on
//! an accepted `book` snapshot rather than linkage validated, and every snapshot replaces the book.
//!
//! Claiming, accepting, and resetting recovery stay under the tracker's state lock so competing
//! events cannot independently change ownership. Snapshot gates coordinate acceptance with
//! transport sends. This module performs state transitions; [`super::recovery`] runs the
//! asynchronous subscription and retry work.
//!
//! The `*_if_subscribed` checks run under the state lock. Clearing paths remove the
//! subscription before acquiring the lock, so a message that observed a live subscription
//! cannot insert tracking state after the clearing path's locked removal.

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_core::AtomicSet;
use nautilus_live::book::sync::{BookPhase, BookSync};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;

use super::{BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind, recovery::BookRecovery};
use crate::websocket::error::PolymarketWsError;

type Book = BookSync<PolymarketWsError>;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    books: Arc<Mutex<AHashMap<InstrumentId, Book>>>,
}

impl BookSyncTracker {
    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        self.books.lock().remove(&instrument_id);
    }

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
    }

    pub(crate) fn record_snapshot_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
        now: Instant,
    ) -> bool {
        // Checked under the state lock: clearing paths drop the subscription
        // first, so a racing unsubscribe cannot land between this check and
        // the insert below. `AtomicSet::contains` is lock-free.
        let mut books = self.books.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return false;
        }

        books
            .entry(instrument_id)
            .or_insert_with(|| Book::new(now))
            .accept_snapshot((), now)
    }

    pub(crate) fn validate_incremental_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
        now: Instant,
    ) -> BookSequenceOutcome {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut books = self.books.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return BookSequenceOutcome::Suppress;
        }

        validate_incremental(&mut books, instrument_id, now)
    }

    #[cfg(test)]
    pub(crate) fn validate_incremental(
        &self,
        instrument_id: InstrumentId,
        now: Instant,
    ) -> BookSequenceOutcome {
        validate_incremental(&mut self.books.lock(), instrument_id, now)
    }

    /// Marks the book gated after an invalid snapshot, requesting recovery when nothing owns it.
    #[cfg(test)]
    pub(crate) fn request_recovery(
        &self,
        instrument_id: InstrumentId,
        now: Instant,
    ) -> BookSequenceOutcome {
        request_recovery(&mut self.books.lock(), instrument_id, now)
    }

    pub(crate) fn request_recovery_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
        now: Instant,
    ) -> BookSequenceOutcome {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut books = self.books.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return BookSequenceOutcome::Suppress;
        }

        request_recovery(&mut books, instrument_id, now)
    }

    pub(crate) fn reset_for_instruments(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_ids: &[InstrumentId],
        now: Instant,
    ) {
        let mut books = self.books.lock();

        for instrument_id in instrument_ids {
            // Re-checked under the state lock: the caller filters first, but
            // an unsubscribe can land between that filter and this reset.
            if !active_delta_subs.contains(instrument_id) {
                continue;
            }

            books
                .entry(*instrument_id)
                .or_insert_with(|| Book::new(now))
                .reset_on_reconnect();
        }
    }

    pub(crate) fn seed_pending_snapshots(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_ids: &[InstrumentId],
        timeout: Duration,
        now: Instant,
    ) -> usize {
        let deadline = now + timeout;
        let mut books = self.books.lock();

        instrument_ids
            .iter()
            // Re-checked under the state lock, as in `reset_for_instruments`.
            .filter(|instrument_id| active_delta_subs.contains(instrument_id))
            .filter(|instrument_id| {
                books
                    .entry(**instrument_id)
                    .or_insert_with(|| Book::new(now))
                    .arm_deadline(deadline)
            })
            .count()
    }

    #[cfg(test)]
    pub(crate) fn claim_recovery(&self, instrument_id: InstrumentId) -> Option<Arc<BookRecovery>> {
        self.books
            .lock()
            .entry(instrument_id)
            .or_insert_with(|| Book::new(Instant::now()))
            .claim()
    }

    pub(crate) fn claim_recovery_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
    ) -> Option<Arc<BookRecovery>> {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut books = self.books.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return None;
        }

        books
            .entry(instrument_id)
            .or_insert_with(|| Book::new(Instant::now()))
            .claim()
    }

    /// Reports whether book delta output is gated pending a valid snapshot.
    pub(crate) fn book_gated(&self, instrument_id: InstrumentId) -> bool {
        self.books
            .lock()
            .get(&instrument_id)
            .is_some_and(|book| book.position().is_none() || book.has_pending_snapshot())
    }

    /// Reports books without a running recovery whose feed has exceeded `threshold` since the
    /// last update, re-arming each reported window so a still-dead feed keeps being reported at
    /// most once per threshold window.
    pub(crate) fn stale_books(&self, threshold: Duration, now: Instant) -> Vec<BookSyncSignal> {
        let mut stale = self
            .books
            .lock()
            .iter_mut()
            .filter_map(|(instrument_id, book)| {
                book.stale(threshold, now).map(|elapsed| BookSyncSignal {
                    instrument_id: *instrument_id,
                    kind: BookSyncSignalKind::Stale { elapsed },
                })
            })
            .collect::<Vec<_>>();

        // Sort by instrument; the book map iterates in per-process hash order
        stale.sort_by_key(|signal| signal.instrument_id);
        stale
    }

    pub(crate) fn take_expired_snapshots(
        &self,
        instrument_ids: &AHashSet<InstrumentId>,
        now: Instant,
    ) -> Vec<BookSyncSignal> {
        let mut expired = self
            .books
            .lock()
            .iter_mut()
            .filter_map(|(instrument_id, book)| {
                (instrument_ids.contains(instrument_id) && book.take_expired(now)).then_some(
                    BookSyncSignal {
                        instrument_id: *instrument_id,
                        kind: BookSyncSignalKind::SnapshotMissing,
                    },
                )
            })
            .collect::<Vec<_>>();

        // Sort by instrument; the book map iterates in per-process hash order
        expired.sort_by_key(|signal| signal.instrument_id);
        expired
    }
}

fn validate_incremental(
    books: &mut AHashMap<InstrumentId, Book>,
    instrument_id: InstrumentId,
    now: Instant,
) -> BookSequenceOutcome {
    let book = books.entry(instrument_id).or_insert_with(|| Book::new(now));

    if book.advance((), now) {
        return BookSequenceOutcome::Accept;
    }

    if *book.phase() == BookPhase::Recovering || book.has_pending_snapshot() {
        return BookSequenceOutcome::Suppress;
    }

    let outcome = book.gap();

    if outcome == BookSequenceOutcome::Recover {
        log::warn!("Book update before snapshot for {instrument_id}; requesting a fresh snapshot");
    }

    outcome
}

fn request_recovery(
    books: &mut AHashMap<InstrumentId, Book>,
    instrument_id: InstrumentId,
    now: Instant,
) -> BookSequenceOutcome {
    let outcome = books
        .entry(instrument_id)
        .or_insert_with(|| Book::new(now))
        .gap();

    if outcome == BookSequenceOutcome::Recover {
        log::warn!("Book snapshot invalid for {instrument_id}; requesting a fresh snapshot");
    }

    outcome
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use nautilus_common::live::dst::time::{Duration, Instant};
    use nautilus_core::AtomicSet;
    use nautilus_live::book::recovery::BookRecoveryOutcome;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{Book, BookSequenceOutcome, BookSyncSignalKind, BookSyncTracker};
    use crate::websocket::error::PolymarketWsError;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
    }

    fn subscribed_tracker() -> (BookSyncTracker, AtomicSet<InstrumentId>) {
        let tracker = BookSyncTracker::default();
        let subs = AtomicSet::new();
        subs.insert(instrument_id());
        (tracker, subs)
    }

    #[rstest]
    fn incremental_requires_snapshot_and_requests_one_recovery() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        let first = tracker.validate_incremental_if_subscribed(&subs, instrument_id, now);
        let repeated = tracker.validate_incremental_if_subscribed(&subs, instrument_id, now);
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        let steady = tracker.validate_incremental_if_subscribed(&subs, instrument_id, now);

        assert_eq!(first, BookSequenceOutcome::Recover);
        assert_eq!(repeated, BookSequenceOutcome::Suppress);
        assert_eq!(steady, BookSequenceOutcome::Accept);
        assert!(!tracker.book_gated(instrument_id));
    }

    #[rstest]
    fn snapshot_acceptance_completes_claimed_recovery() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        assert!(tracker.claim_recovery(instrument_id).is_none());
        assert_eq!(
            tracker.validate_incremental(instrument_id, now),
            BookSequenceOutcome::Suppress
        );
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
        assert_eq!(
            tracker.validate_incremental(instrument_id, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn rejected_recovery_stays_owned_until_next_write_confirms() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(recovery.begin_replacement());
        recovery
            .outcome
            .send_replace(BookRecoveryOutcome::Rejected(PolymarketWsError::Client(
                "subscribe rejected".into(),
            )));

        let during_write = tracker.record_snapshot_if_subscribed(&subs, instrument_id, now);
        let second_owner = tracker.claim_recovery(instrument_id);
        let gated = tracker.book_gated(instrument_id);
        recovery.gate.open();
        let after_write = tracker.record_snapshot_if_subscribed(&subs, instrument_id, now);

        assert!(!during_write);
        assert!(second_owner.is_none());
        assert!(gated);
        assert!(after_write);
        assert!(recovery.is_accepted());
        assert!(!tracker.book_gated(instrument_id));
    }

    #[rstest]
    fn replacement_send_blocks_snapshot_acceptance_until_open() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        assert!(recovery.begin_replacement());
        assert!(!tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        recovery.gate.open();
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
        assert_eq!(
            tracker.validate_incremental(instrument_id, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn request_recovery_recovers_until_owned() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();

        let first = tracker.request_recovery(instrument_id, now);

        // Still gated but unowned: a repeated request may claim an owner
        // instead of suppressing forever.
        let repeated = tracker.request_recovery(instrument_id, now);
        let gated = tracker.book_gated(instrument_id);
        let _recovery = tracker.claim_recovery(instrument_id).unwrap();
        let owned = tracker.request_recovery(instrument_id, now);

        assert_eq!(first, BookSequenceOutcome::Recover);
        assert_eq!(repeated, BookSequenceOutcome::Recover);
        assert!(gated);
        assert_eq!(owned, BookSequenceOutcome::Suppress);
    }

    #[rstest]
    fn request_recovery_leaves_reconnect_deadline_to_its_monitor() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let subs = AtomicSet::new();
        subs.insert(instrument_id);
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        // An invalid snapshot inside the reconnect window defers to the armed
        // deadline, whose monitor starts recovery when it expires.
        tracker.seed_pending_snapshots(&subs, &[instrument_id], timeout, now);
        let inside_window = tracker.request_recovery(
            instrument_id,
            now.checked_add(Duration::from_secs(1)).unwrap(),
        );
        let filter = AHashSet::from_iter([instrument_id]);
        let expired = tracker
            .take_expired_snapshots(&filter, now.checked_add(Duration::from_secs(3)).unwrap());
        let after_expiry = tracker.request_recovery(instrument_id, now);

        assert_eq!(inside_window, BookSequenceOutcome::Suppress);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].instrument_id, instrument_id);
        assert_eq!(expired[0].kind, BookSyncSignalKind::SnapshotMissing);
        assert_eq!(after_expiry, BookSequenceOutcome::Recover);
    }

    #[rstest]
    fn request_recovery_marks_gated_without_arming_deadline() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert_eq!(
            tracker.request_recovery(instrument_id, now),
            BookSequenceOutcome::Recover
        );

        assert!(tracker.book_gated(instrument_id));
        assert!(!has_pending_snapshot(&tracker, instrument_id));

        let filter = AHashSet::from_iter([instrument_id]);
        assert!(
            tracker
                .take_expired_snapshots(&filter, now + Duration::from_secs(60))
                .is_empty()
        );
    }

    #[rstest]
    fn record_and_validate_ignore_unsubscribed_instrument() {
        let tracker = BookSyncTracker::default();
        let subs = AtomicSet::new();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(!tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert_eq!(
            tracker.validate_incremental_if_subscribed(&subs, instrument_id, now),
            BookSequenceOutcome::Suppress
        );
        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn stale_books_rearms_window_and_keeps_tracking() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let threshold = Duration::from_secs(5);

        assert!(tracker.record_snapshot_if_subscribed(
            &subs,
            instrument_id,
            now.checked_sub(Duration::from_secs(6)).unwrap()
        ));
        let first = tracker.stale_books(threshold, now);
        let same_window = tracker.stale_books(threshold, now);
        let next_window =
            tracker.stale_books(threshold, now.checked_add(Duration::from_secs(6)).unwrap());

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].instrument_id, instrument_id);
        assert_eq!(
            first[0].kind,
            BookSyncSignalKind::Stale {
                elapsed: Duration::from_secs(6)
            }
        );
        assert!(
            same_window.is_empty(),
            "window must not re-report after firing"
        );
        assert_eq!(next_window.len(), 1, "a still-dead feed must report again");
        assert!(
            is_tracked(&tracker, instrument_id),
            "tracking must persist so staleness stays observable"
        );
    }

    #[rstest]
    fn seed_pending_snapshots_arms_listed_instruments_only() {
        let tracker = BookSyncTracker::default();
        let first = InstrumentId::from("0xCOND-A-0xTOKEN-A.POLYMARKET");
        let second = InstrumentId::from("0xCOND-B-0xTOKEN-B.POLYMARKET");
        let subs = AtomicSet::new();
        subs.insert(first);
        subs.insert(second);
        let now = Instant::now();

        assert_eq!(
            tracker.seed_pending_snapshots(&subs, &[first, second], Duration::from_secs(3), now),
            2
        );
        assert_eq!(
            tracker.seed_pending_snapshots(&subs, &[], Duration::from_secs(3), now),
            0
        );

        let first_only = AHashSet::from_iter([first]);
        let expired = tracker.take_expired_snapshots(&first_only, now + Duration::from_secs(4));

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].instrument_id, first);
        assert!(has_pending_snapshot(&tracker, second));
        assert!(!has_pending_snapshot(&tracker, first));
    }

    #[rstest]
    fn take_expired_snapshots_emits_once_and_keeps_staleness_tracking() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        tracker.seed_pending_snapshots(
            &subs,
            &[instrument_id],
            Duration::from_secs(3),
            now.checked_sub(Duration::from_secs(4)).unwrap(),
        );

        let filter = AHashSet::from_iter([instrument_id]);
        let first = tracker.take_expired_snapshots(&filter, now);
        let second = tracker.take_expired_snapshots(&filter, now);

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].instrument_id, instrument_id);
        assert_eq!(first[0].kind, BookSyncSignalKind::SnapshotMissing);
        assert!(second.is_empty());
        assert!(
            is_tracked(&tracker, instrument_id),
            "snapshot expiry must keep the stale window armed"
        );
        assert!(!has_pending_snapshot(&tracker, instrument_id));
    }

    #[rstest]
    fn reset_for_instruments_gates_until_fresh_snapshot() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        tracker.seed_pending_snapshots(&subs, &[instrument_id], Duration::from_secs(3), now);
        tracker.reset_for_instruments(&subs, &[instrument_id], now);

        // The reset drops the armed deadline and gates on the replayed snapshot.
        let filter = AHashSet::from_iter([instrument_id]);
        assert!(
            tracker
                .take_expired_snapshots(&filter, now + Duration::from_secs(60))
                .is_empty()
        );
        assert!(tracker.book_gated(instrument_id));
        assert_eq!(
            tracker.validate_incremental(instrument_id, now),
            BookSequenceOutcome::Suppress
        );

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert_eq!(
            tracker.validate_incremental(instrument_id, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn reset_for_instruments_preserves_active_recovery() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        assert!(recovery.begin_replacement());
        tracker.reset_for_instruments(&subs, &[instrument_id], now);

        assert!(!recovery.cancellation.is_cancelled());
        assert!(tracker.claim_recovery(instrument_id).is_none());
        assert!(recovery.gate.lock().is_closed());
        assert!(!tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));

        recovery.gate.open();

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
        assert_eq!(
            tracker.validate_incremental(instrument_id, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    #[case::accepted(0)]
    #[case::cancelled(1)]
    fn reset_for_instruments_retires_inactive_recovery(#[case] phase: u8) {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        match phase {
            0 => {
                assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
            }
            1 => recovery.cancellation.cancel(),
            _ => unreachable!(),
        }

        tracker.reset_for_instruments(&subs, &[instrument_id], now);
        let replacement = tracker
            .claim_recovery(instrument_id)
            .expect("fresh recovery owner");

        assert!(recovery.cancellation.is_cancelled());
        assert!(!replacement.cancellation.is_cancelled());
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(replacement.is_accepted());
    }

    #[rstest]
    #[case::remove(0)]
    #[case::accept_then_reset(1)]
    #[case::shutdown(2)]
    fn obsolete_recovery_cannot_affect_replacement(#[case] boundary: u8) {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let obsolete = tracker.claim_recovery(instrument_id).unwrap();

        match boundary {
            0 => tracker.remove(instrument_id),
            1 => {
                assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
                tracker.reset_for_instruments(&subs, &[instrument_id], now);
            }
            2 => tracker.clear(),
            _ => unreachable!(),
        }

        assert!(obsolete.cancellation.is_cancelled());
        let current = tracker.claim_recovery(instrument_id).unwrap();
        assert!(!obsolete.begin_replacement());
        assert!(!current.cancellation.is_cancelled());
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(current.is_accepted());
    }

    #[rstest]
    fn book_gated_reports_pending_and_recovering() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(!tracker.book_gated(instrument_id));

        tracker.seed_pending_snapshots(&subs, &[instrument_id], Duration::from_secs(3), now);
        assert!(tracker.book_gated(instrument_id));

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(!tracker.book_gated(instrument_id));

        assert_eq!(
            tracker.request_recovery(instrument_id, now),
            BookSequenceOutcome::Recover
        );
        assert!(tracker.book_gated(instrument_id));

        let _recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(tracker.book_gated(instrument_id));

        tracker.remove(instrument_id);
        assert!(!tracker.book_gated(instrument_id));
    }

    #[rstest]
    fn remove_clears_tracking_state() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        tracker.seed_pending_snapshots(&subs, &[instrument_id], Duration::from_secs(3), now);
        assert!(tracker.claim_recovery(instrument_id).is_some());

        tracker.remove(instrument_id);

        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn clear_removes_all_tracking_state() {
        let tracker = BookSyncTracker::default();
        let first = InstrumentId::from("0xCOND-A-0xTOKEN-A.POLYMARKET");
        let second = InstrumentId::from("0xCOND-B-0xTOKEN-B.POLYMARKET");
        let now = Instant::now();

        let subs = AtomicSet::new();
        subs.insert(first);
        subs.insert(second);
        assert!(tracker.record_snapshot_if_subscribed(&subs, first, now));
        tracker.seed_pending_snapshots(&subs, &[first, second], Duration::from_secs(3), now);
        assert!(tracker.claim_recovery(second).is_some());

        tracker.clear();

        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn recovery_event_order_preserves_snapshot_and_owner_guards() {
        // Covers every ordering of five tracker events
        for trace in 0_u32..6_u32.pow(5) {
            let tracker = BookSyncTracker::default();
            let id = instrument_id();
            let subs = AtomicSet::new();
            subs.insert(id);
            let mut owner = tracker.claim_recovery(id).unwrap();
            let mut obsolete = Vec::new();
            let mut pending = false;
            let mut accepted = false;
            let mut steps = trace;

            for step in 0..5 {
                let event = steps % 6;
                steps /= 6;

                match event {
                    0 => {
                        let expected = !accepted;
                        assert_eq!(
                            owner.begin_replacement(),
                            expected,
                            "trace={trace}, step={step}"
                        );
                        pending |= expected;
                    }
                    1 => {
                        owner.gate.open();
                        pending = false;
                    }
                    2 => {
                        // Snapshot acceptance is idempotent: only a closed send
                        // gate suppresses it.
                        let expected = !pending;

                        assert_eq!(
                            tracker.record_snapshot_if_subscribed(&subs, id, Instant::now()),
                            expected,
                            "trace={trace}, step={step}"
                        );
                        accepted |= expected;
                    }
                    3 => {
                        // A rejection reaches only a running owner
                        let running = !accepted;
                        owner.outcome.send_if_modified(|outcome| {
                            if running {
                                *outcome = BookRecoveryOutcome::Rejected(
                                    PolymarketWsError::Client("rejected".into()),
                                );
                            }

                            running
                        });
                    }
                    4 => {
                        tracker.remove(id);
                        assert!(owner.cancellation.is_cancelled());
                        obsolete.push(owner);
                        owner = tracker.claim_recovery(id).unwrap();
                        pending = false;
                        accepted = false;
                    }
                    5 => {
                        for previous in &obsolete {
                            assert!(!previous.begin_replacement(), "trace={trace}, step={step}");
                        }
                    }
                    _ => unreachable!(),
                }

                assert!(
                    !owner.cancellation.is_cancelled(),
                    "trace={trace}, step={step}"
                );
                assert_eq!(
                    matches!(*owner.outcome.borrow(), BookRecoveryOutcome::Accepted),
                    accepted,
                    "trace={trace}, step={step}"
                );
            }
        }
    }

    #[rstest]
    fn steady_deltas_refresh_staleness_window() {
        let tracker = BookSyncTracker::default();
        let stale_id = InstrumentId::from("0xCOND-A-0xTOKEN-A.POLYMARKET");
        let steady_id = InstrumentId::from("0xCOND-B-0xTOKEN-B.POLYMARKET");
        let subs = AtomicSet::new();
        subs.insert(stale_id);
        subs.insert(steady_id);
        let now = Instant::now();
        let past = now.checked_sub(Duration::from_secs(6)).unwrap();
        let threshold = Duration::from_secs(5);

        assert!(tracker.record_snapshot_if_subscribed(&subs, stale_id, past));
        assert!(tracker.record_snapshot_if_subscribed(&subs, steady_id, past));
        assert_eq!(
            tracker.validate_incremental_if_subscribed(&subs, steady_id, now),
            BookSequenceOutcome::Accept
        );

        let stale = tracker.stale_books(threshold, now);

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].instrument_id, stale_id);
    }

    #[rstest]
    fn stale_books_ignores_threshold_boundary_and_clock_skew() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let threshold = Duration::from_secs(5);

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));

        // Elapsed exactly at the threshold is not stale.
        assert!(
            tracker
                .stale_books(threshold, now.checked_add(threshold).unwrap())
                .is_empty()
        );
        // A last update in the future (clock skew) is skipped, not reported.
        assert!(
            tracker
                .stale_books(threshold, now.checked_sub(Duration::from_secs(1)).unwrap())
                .is_empty()
        );

        // Neither probe re-armed the window: the original elapsed still reports.
        let stale =
            tracker.stale_books(threshold, now.checked_add(Duration::from_secs(6)).unwrap());

        assert_eq!(stale.len(), 1);
        assert_eq!(
            stale[0].kind,
            BookSyncSignalKind::Stale {
                elapsed: Duration::from_secs(6)
            }
        );
    }

    #[rstest]
    fn expired_snapshot_deadline_is_inclusive_and_rearm_overwrites() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let subs = AtomicSet::new();
        subs.insert(instrument_id);
        let now = Instant::now();
        let timeout = Duration::from_secs(3);
        let filter = AHashSet::from_iter([instrument_id]);

        tracker.seed_pending_snapshots(&subs, &[instrument_id], timeout, now);
        let deadline = now.checked_add(timeout).unwrap();

        assert!(
            tracker
                .take_expired_snapshots(
                    &filter,
                    deadline.checked_sub(Duration::from_millis(1)).unwrap()
                )
                .is_empty()
        );
        let expired = tracker.take_expired_snapshots(&filter, deadline);

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].kind, BookSyncSignalKind::SnapshotMissing);

        // Re-seeding before expiry moves the deadline instead of keeping the old one.
        tracker.seed_pending_snapshots(&subs, &[instrument_id], timeout, now);
        tracker.seed_pending_snapshots(
            &subs,
            &[instrument_id],
            timeout,
            now.checked_add(Duration::from_secs(1)).unwrap(),
        );

        assert!(
            tracker
                .take_expired_snapshots(&filter, now.checked_add(timeout).unwrap())
                .is_empty(),
            "re-arm must overwrite the earlier deadline"
        );
        let rearmed = tracker
            .take_expired_snapshots(&filter, now.checked_add(Duration::from_secs(4)).unwrap());

        assert_eq!(rearmed.len(), 1);
    }

    #[rstest]
    fn claim_recovery_if_subscribed_requires_subscription() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let subs = AtomicSet::new();

        assert!(
            tracker
                .claim_recovery_if_subscribed(&subs, instrument_id)
                .is_none()
        );
        assert!(is_empty(&tracker));

        subs.insert(instrument_id);

        assert!(
            tracker
                .claim_recovery_if_subscribed(&subs, instrument_id)
                .is_some()
        );
    }

    #[rstest]
    fn stale_books_skips_running_recovery() {
        let tracker = BookSyncTracker::default();
        let recovering_id = InstrumentId::from("0xCOND-A-0xTOKEN-A.POLYMARKET");
        let steady_id = InstrumentId::from("0xCOND-B-0xTOKEN-B.POLYMARKET");
        let subs = AtomicSet::new();
        subs.insert(recovering_id);
        subs.insert(steady_id);
        let now = Instant::now();
        let past = now.checked_sub(Duration::from_secs(6)).unwrap();
        let threshold = Duration::from_secs(5);

        assert!(tracker.record_snapshot_if_subscribed(&subs, recovering_id, past));
        assert!(tracker.record_snapshot_if_subscribed(&subs, steady_id, past));

        let _recovery = tracker.claim_recovery(recovering_id).unwrap();

        let stale = tracker.stale_books(threshold, now);

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].instrument_id, steady_id);
    }

    #[rstest]
    fn record_after_remove_stays_cleared_when_unsubscribed() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));

        // Mirror `unsubscribe_book_deltas`: drop the subscription, then clear.
        subs.remove(&instrument_id);
        tracker.remove(instrument_id);

        assert!(!tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert_eq!(
            tracker.validate_incremental_if_subscribed(&subs, instrument_id, now),
            BookSequenceOutcome::Suppress
        );
        assert!(is_empty(&tracker));
    }

    #[rstest]
    fn request_recovery_if_subscribed_requires_subscription() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let subs = AtomicSet::new();
        let now = Instant::now();

        assert_eq!(
            tracker.request_recovery_if_subscribed(&subs, instrument_id, now),
            BookSequenceOutcome::Suppress
        );
        assert!(is_empty(&tracker));

        subs.insert(instrument_id);

        assert_eq!(
            tracker.request_recovery_if_subscribed(&subs, instrument_id, now),
            BookSequenceOutcome::Recover
        );
        assert!(tracker.book_gated(instrument_id));
    }

    #[rstest]
    fn reset_and_seed_ignore_unsubscribed_instruments() {
        let tracker = BookSyncTracker::default();
        let subscribed_id = InstrumentId::from("0xCOND-A-0xTOKEN-A.POLYMARKET");
        let retired_id = InstrumentId::from("0xCOND-B-0xTOKEN-B.POLYMARKET");
        let subs = AtomicSet::new();
        subs.insert(subscribed_id);
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        tracker.reset_for_instruments(&subs, &[subscribed_id, retired_id], now);

        assert!(tracker.book_gated(subscribed_id));
        assert!(!tracker.book_gated(retired_id));

        let seeded =
            tracker.seed_pending_snapshots(&subs, &[subscribed_id, retired_id], timeout, now);

        assert_eq!(seeded, 1);
        assert!(has_pending_snapshot(&tracker, subscribed_id));
        assert!(!has_pending_snapshot(&tracker, retired_id));
    }

    fn is_tracked(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker.books.lock().contains_key(&instrument_id)
    }

    fn has_pending_snapshot(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker
            .books
            .lock()
            .get(&instrument_id)
            .is_some_and(Book::has_pending_snapshot)
    }

    fn is_empty(tracker: &BookSyncTracker) -> bool {
        tracker.books.lock().is_empty()
    }
}
