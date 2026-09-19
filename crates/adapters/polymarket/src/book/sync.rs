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
//! [`BookSyncTracker`] decides whether to accept a book snapshot, suppress it, or request
//! recovery. It tracks pending snapshots, recovering books, and recovery ownership per
//! instrument. Polymarket books carry no sequence numbers, so incremental `price_change`
//! deltas are gated on an accepted snapshot rather than linkage validated.
//!
//! Claiming, accepting, failing, and resetting recovery stay under the tracker's state lock so
//! competing events cannot independently change ownership. Snapshot gates coordinate acceptance
//! with transport sends. This module performs state transitions; [`super::recovery`] runs the
//! asynchronous subscription and retry work.
//!
//! The `*_if_subscribed` checks run under the state lock. Clearing paths remove the
//! subscription before acquiring the lock, so a message that observed a live subscription
//! cannot insert tracking state after the clearing path's locked removal.

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_core::AtomicSet;
use nautilus_live::book::{
    recovery::BookRecoveryState,
    snapshot::{PendingSnapshot, SnapshotGate},
};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{BookRecoveryOutcome, BookSequenceOutcome, BookSyncSignalKind, recovery::BookRecovery};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BookSyncSignal {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) kind: BookSyncSignalKind,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    state: Arc<Mutex<BookSyncState>>,
}

impl BookSyncTracker {
    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        let mut state = self.state.lock();
        state.last_book_ts.remove(&instrument_id);
        state.recovering.remove(&instrument_id);
        state.pending_snapshots.remove(&instrument_id);
        reset_recovery(&mut state, instrument_id);
    }

    pub(crate) fn clear(&self) {
        let mut state = self.state.lock();
        state.last_book_ts.clear();
        state.recovering.clear();
        state.pending_snapshots.clear();
        state.recoveries.clear();
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
        let mut state = self.state.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return false;
        }

        record_snapshot(&mut state, instrument_id, now)
    }

    pub(crate) fn validate_incremental_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
        timeout: Duration,
        now: Instant,
    ) -> BookSequenceOutcome {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut state = self.state.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return BookSequenceOutcome::Suppress;
        }

        validate_incremental(&mut state, instrument_id, timeout, now)
    }

    #[cfg(test)]
    pub(crate) fn validate_incremental(
        &self,
        instrument_id: InstrumentId,
        timeout: Duration,
        now: Instant,
    ) -> BookSequenceOutcome {
        let mut state = self.state.lock();
        validate_incremental(&mut state, instrument_id, timeout, now)
    }

    /// Arms a snapshot deadline and marks the book recovering, requesting one recovery.
    #[cfg(test)]
    pub(crate) fn request_recovery(
        &self,
        instrument_id: InstrumentId,
        timeout: Duration,
        now: Instant,
    ) -> BookSequenceOutcome {
        let mut state = self.state.lock();
        request_recovery(&mut state, instrument_id, timeout, now)
    }

    pub(crate) fn request_recovery_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
        timeout: Duration,
        now: Instant,
    ) -> BookSequenceOutcome {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut state = self.state.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return BookSequenceOutcome::Suppress;
        }

        request_recovery(&mut state, instrument_id, timeout, now)
    }

    pub(crate) fn reset_for_instruments(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_ids: &[InstrumentId],
    ) {
        let mut state = self.state.lock();

        for instrument_id in instrument_ids {
            // Re-checked under the state lock: the caller filters first, but
            // an unsubscribe can land between that filter and this reset.
            if !active_delta_subs.contains(instrument_id) {
                continue;
            }

            state.pending_snapshots.remove(instrument_id);
            state.recovering.insert(*instrument_id);

            // Cancellation between replacement sends can leave the book unsubscribed
            // even after reconnect replay restores it.
            let recovery_active = state
                .recoveries
                .get(instrument_id)
                .and_then(BookRecoveryState::current)
                .is_some_and(|recovery| {
                    !recovery.cancellation.is_cancelled()
                        && !matches!(*recovery.outcome.borrow(), BookRecoveryOutcome::Accepted)
                });

            if !recovery_active {
                reset_recovery(&mut state, *instrument_id);
            }
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

        if instrument_ids.is_empty() {
            return 0;
        }

        let mut state = self.state.lock();
        let mut seeded = 0;

        for instrument_id in instrument_ids {
            // Re-checked under the state lock, as in `reset_for_instruments`.
            if !active_delta_subs.contains(instrument_id) {
                continue;
            }

            state.pending_snapshots.insert(
                *instrument_id,
                PendingSnapshot {
                    deadline: Some(deadline),
                    cancel: CancellationToken::new(),
                    gate: SnapshotGate::default(),
                },
            );

            seeded += 1;
        }

        seeded
    }

    #[cfg(test)]
    pub(crate) fn claim_recovery(&self, instrument_id: InstrumentId) -> Option<Arc<BookRecovery>> {
        let mut state = self.state.lock();
        claim_recovery(&mut state, instrument_id)
    }

    pub(crate) fn claim_recovery_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
    ) -> Option<Arc<BookRecovery>> {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut state = self.state.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return None;
        }

        claim_recovery(&mut state, instrument_id)
    }

    pub(crate) fn fail_recovery(
        &self,
        instrument_id: InstrumentId,
        recovery: Option<&Arc<BookRecovery>>,
    ) {
        let mut state = self.state.lock();
        fail_recovery(&mut state, instrument_id, recovery);
    }

    pub(crate) fn fail_recovery_if_subscribed(
        &self,
        active_delta_subs: &AtomicSet<InstrumentId>,
        instrument_id: InstrumentId,
        recovery: Option<&Arc<BookRecovery>>,
    ) {
        // Checked under the state lock, as in `record_snapshot_if_subscribed`.
        let mut state = self.state.lock();

        if !active_delta_subs.contains(&instrument_id) {
            return;
        }

        fail_recovery(&mut state, instrument_id, recovery);
    }

    /// Reports whether book delta output is gated pending a valid snapshot.
    pub(crate) fn book_gated(&self, instrument_id: InstrumentId) -> bool {
        let state = self.state.lock();
        state
            .recoveries
            .get(&instrument_id)
            .is_some_and(BookRecoveryState::is_failed)
            || state.recovering.contains(&instrument_id)
            || state.pending_snapshots.contains_key(&instrument_id)
    }

    /// Reports instruments whose book feed has exceeded `threshold` since the
    /// last update, re-arming each reported window so a still-dead feed keeps
    /// being reported at most once per threshold window. Terminally failed
    /// books are skipped until reconnect or resubscribe clears them.
    pub(crate) fn stale_books(&self, threshold: Duration, now: Instant) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock();
        let mut stale = Vec::new();
        let BookSyncState {
            last_book_ts,
            recoveries,
            ..
        } = &mut *state;

        for (instrument_id, last_update) in last_book_ts {
            // Terminally failed books stay suppressed until reconnect or
            // resubscribe; reporting them every window would mask live feeds.
            if recoveries
                .get(instrument_id)
                .is_some_and(BookRecoveryState::is_failed)
            {
                continue;
            }

            let Some(elapsed) = now.checked_duration_since(*last_update) else {
                continue;
            };

            if elapsed <= threshold {
                continue;
            }

            *last_update = now;
            stale.push(BookSyncSignal {
                instrument_id: *instrument_id,
                kind: BookSyncSignalKind::Stale { elapsed },
            });
        }

        stale
    }

    pub(crate) fn take_expired_snapshots(
        &self,
        instrument_ids: &AHashSet<InstrumentId>,
        now: Instant,
    ) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock();

        let expired = state
            .pending_snapshots
            .iter()
            .filter_map(|(instrument_id, pending)| {
                (pending.deadline.is_some_and(|deadline| deadline <= now)
                    && instrument_ids.contains(instrument_id))
                .then_some(BookSyncSignal {
                    instrument_id: *instrument_id,
                    kind: BookSyncSignalKind::SnapshotMissing,
                })
            })
            .collect::<Vec<_>>();

        for signal in &expired {
            state.pending_snapshots.remove(&signal.instrument_id);
        }

        expired
    }
}

pub(crate) fn log_sync_signals(signals: &[BookSyncSignal]) {
    for signal in signals {
        match signal.kind {
            BookSyncSignalKind::Stale { elapsed } => {
                log::warn!(
                    "Book feed stale for {}: no update for {:.3}s",
                    signal.instrument_id,
                    elapsed.as_secs_f64()
                );
            }
            BookSyncSignalKind::SnapshotMissing => {
                log::warn!(
                    "Book snapshot not received for {} after recovery request",
                    signal.instrument_id
                );
            }
        }
    }
}

#[derive(Debug, Default)]
struct BookSyncState {
    last_book_ts: AHashMap<InstrumentId, Instant>,
    recovering: AHashSet<InstrumentId>,
    pending_snapshots: AHashMap<InstrumentId, PendingSnapshot>,
    recoveries:
        AHashMap<InstrumentId, BookRecoveryState<crate::websocket::error::PolymarketWsError>>,
}

fn record_snapshot(state: &mut BookSyncState, instrument_id: InstrumentId, now: Instant) -> bool {
    if state
        .recoveries
        .get(&instrument_id)
        .is_some_and(BookRecoveryState::is_failed)
    {
        return false;
    }

    if !accept_recovery(state, instrument_id) {
        return false;
    }

    state.last_book_ts.insert(instrument_id, now);
    state.pending_snapshots.remove(&instrument_id);
    state.recovering.remove(&instrument_id);

    true
}

fn validate_incremental(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    timeout: Duration,
    now: Instant,
) -> BookSequenceOutcome {
    if state
        .recoveries
        .get(&instrument_id)
        .is_some_and(BookRecoveryState::is_failed)
    {
        return BookSequenceOutcome::Suppress;
    }

    if state.recovering.contains(&instrument_id)
        || state.pending_snapshots.contains_key(&instrument_id)
    {
        return BookSequenceOutcome::Suppress;
    }

    if !state.last_book_ts.contains_key(&instrument_id) {
        return handle_missing_snapshot(state, instrument_id, timeout, now);
    }

    state.last_book_ts.insert(instrument_id, now);
    BookSequenceOutcome::Accept
}

fn fail_recovery(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    recovery: Option<&Arc<BookRecovery>>,
) {
    // A stale report must not mint an entry: only an unconditional failure
    // creates one.
    let recorded = match recovery {
        Some(owner) => state
            .recoveries
            .get_mut(&instrument_id)
            .is_some_and(|current| current.fail(Some(owner))),
        None => state
            .recoveries
            .entry(instrument_id)
            .or_default()
            .fail(None),
    };

    if !recorded {
        return;
    }

    state.recovering.insert(instrument_id);
    state.pending_snapshots.remove(&instrument_id);
}

fn claim_recovery(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
) -> Option<Arc<BookRecovery>> {
    let recovery = state.recoveries.entry(instrument_id).or_default().claim()?;

    state.recovering.insert(instrument_id);
    state.pending_snapshots.remove(&instrument_id);
    Some(recovery)
}

fn reset_recovery(state: &mut BookSyncState, instrument_id: InstrumentId) {
    state.recoveries.remove(&instrument_id);
}

fn accept_recovery(state: &BookSyncState, instrument_id: InstrumentId) -> bool {
    state
        .recoveries
        .get(&instrument_id)
        .and_then(BookRecoveryState::current)
        .is_none_or(|recovery| recovery.accept())
}

fn handle_missing_snapshot(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    timeout: Duration,
    now: Instant,
) -> BookSequenceOutcome {
    arm_snapshot_deadline(state, instrument_id, timeout, now);

    // The caller only reaches here when the instrument is not recovering, so
    // this insert always wins the request.
    debug_assert!(!state.recovering.contains(&instrument_id));
    state.recovering.insert(instrument_id);
    log::warn!("Book update before snapshot for {instrument_id}; requesting a fresh snapshot");
    BookSequenceOutcome::Recover
}

fn request_recovery(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    timeout: Duration,
    now: Instant,
) -> BookSequenceOutcome {
    arm_snapshot_deadline(state, instrument_id, timeout, now);

    let fresh = state.recovering.insert(instrument_id);

    // A gated book with no recovery owner (e.g. a reconnect that armed no
    // monitor) must still be able to claim one; claiming serializes owners.
    let unowned = !fresh
        && !state
            .recoveries
            .get(&instrument_id)
            .is_some_and(|recovery| recovery.is_failed() || recovery.current().is_some());

    if fresh || unowned {
        log::warn!("Book snapshot invalid for {instrument_id}; requesting a fresh snapshot");
        BookSequenceOutcome::Recover
    } else {
        BookSequenceOutcome::Suppress
    }
}

fn arm_snapshot_deadline(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    timeout: Duration,
    now: Instant,
) {
    if !timeout.is_zero() {
        // Preserve an existing deadline: the one-shot monitor waits for the
        // first armed deadline, so moving it forward would let the monitor
        // exit early and strand the book gated with no recovery scheduled.
        state
            .pending_snapshots
            .entry(instrument_id)
            .or_insert_with(|| PendingSnapshot {
                deadline: Some(now + timeout),
                cancel: CancellationToken::new(),
                gate: SnapshotGate::default(),
            });
    }
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use nautilus_common::live::dst::time::{Duration, Instant};
    use nautilus_core::AtomicSet;
    use nautilus_live::book::recovery::BookRecoveryState;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{BookRecoveryOutcome, BookSequenceOutcome, BookSyncSignalKind, BookSyncTracker};

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
        let timeout = Duration::from_secs(3);

        let first = tracker.validate_incremental_if_subscribed(&subs, instrument_id, timeout, now);
        let repeated =
            tracker.validate_incremental_if_subscribed(&subs, instrument_id, timeout, now);
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        let steady = tracker.validate_incremental_if_subscribed(&subs, instrument_id, timeout, now);

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
            tracker.validate_incremental(instrument_id, Duration::ZERO, now),
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

        // Failure after acceptance is a no-op; the steady state holds.
        tracker.fail_recovery(instrument_id, Some(&recovery));
        assert_eq!(
            tracker.validate_incremental(instrument_id, Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn failed_recovery_suppresses_until_remove() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        tracker.fail_recovery(instrument_id, Some(&recovery));

        assert!(recovery.cancellation.is_cancelled());
        assert!(is_failed(&tracker, instrument_id));
        assert!(tracker.claim_recovery(instrument_id).is_none());
        assert_eq!(
            tracker.validate_incremental(instrument_id, Duration::ZERO, now),
            BookSequenceOutcome::Suppress
        );
        assert!(!tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(tracker.book_gated(instrument_id));

        tracker.remove(instrument_id);

        assert!(!is_failed(&tracker, instrument_id));

        assert_eq!(
            tracker.validate_incremental(instrument_id, Duration::ZERO, now),
            BookSequenceOutcome::Recover
        );
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
            tracker.validate_incremental(instrument_id, Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn failed_replacement_cannot_be_rescued_by_late_snapshot() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        assert!(recovery.begin_replacement());
        tracker.fail_recovery(instrument_id, Some(&recovery));

        assert!(recovery.cancellation.is_cancelled());
        assert!(!recovery.begin_replacement());
        assert!(!tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(tracker.claim_recovery(instrument_id).is_none());
    }

    #[rstest]
    fn request_recovery_recovers_until_owned_or_failed() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        assert_eq!(
            tracker.request_recovery(instrument_id, timeout, now),
            BookSequenceOutcome::Recover
        );

        // Still gated but unowned: a repeated request may claim an owner
        // instead of suppressing forever.
        assert_eq!(
            tracker.request_recovery(instrument_id, timeout, now),
            BookSequenceOutcome::Recover
        );
        assert!(tracker.book_gated(instrument_id));

        // The armed deadline still expires while unowned.
        let filter = AHashSet::from_iter([instrument_id]);
        let expired = tracker.take_expired_snapshots(&filter, now + timeout);

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].instrument_id, instrument_id);
        assert_eq!(expired[0].kind, BookSyncSignalKind::SnapshotMissing);

        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert_eq!(
            tracker.request_recovery(instrument_id, timeout, now),
            BookSequenceOutcome::Suppress
        );

        tracker.fail_recovery(instrument_id, Some(&recovery));
        assert_eq!(
            tracker.request_recovery(instrument_id, timeout, now),
            BookSequenceOutcome::Suppress
        );
    }

    #[rstest]
    fn request_recovery_preserves_existing_snapshot_deadline() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let subs = AtomicSet::new();
        subs.insert(instrument_id);
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        // Seed the reconnect deadline, then simulate an invalid snapshot
        // arriving inside the window: the re-arm must not move the deadline
        // past the waiting one-shot monitor.
        tracker.seed_pending_snapshots(&subs, &[instrument_id], timeout, now);
        assert_eq!(
            tracker.request_recovery(
                instrument_id,
                timeout,
                now.checked_add(Duration::from_secs(1)).unwrap(),
            ),
            BookSequenceOutcome::Recover
        );

        // Probe at the seeded deadline: preservation reports it, while the
        // old overwrite (now + 4s) would still be pending.
        let filter = AHashSet::from_iter([instrument_id]);
        let expired = tracker
            .take_expired_snapshots(&filter, now.checked_add(Duration::from_secs(3)).unwrap());

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].instrument_id, instrument_id);
        assert_eq!(expired[0].kind, BookSyncSignalKind::SnapshotMissing);
    }

    #[rstest]
    fn request_recovery_without_timeout_marks_recovering_only() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert_eq!(
            tracker.request_recovery(instrument_id, Duration::ZERO, now),
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
            tracker.validate_incremental_if_subscribed(
                &subs,
                instrument_id,
                Duration::from_secs(3),
                now
            ),
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
            has_last_book_ts(&tracker, instrument_id),
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
            has_last_book_ts(&tracker, instrument_id),
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
        tracker.reset_for_instruments(&subs, &[instrument_id]);

        // The reset drops the armed deadline and gates on the replayed snapshot.
        let filter = AHashSet::from_iter([instrument_id]);
        assert!(
            tracker
                .take_expired_snapshots(&filter, now + Duration::from_secs(60))
                .is_empty()
        );
        assert!(tracker.book_gated(instrument_id));
        assert_eq!(
            tracker.validate_incremental(instrument_id, Duration::from_secs(3), now),
            BookSequenceOutcome::Suppress
        );

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert_eq!(
            tracker.validate_incremental(instrument_id, Duration::from_secs(3), now),
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
        tracker.reset_for_instruments(&subs, &[instrument_id]);

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
            tracker.validate_incremental(instrument_id, Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    #[case::accepted(0)]
    #[case::cancelled(1)]
    #[case::failed(2)]
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
            2 => tracker.fail_recovery(instrument_id, Some(&recovery)),
            _ => unreachable!(),
        }

        tracker.reset_for_instruments(&subs, &[instrument_id]);
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
    fn recovery_cancellation_cannot_fail_replacement(#[case] boundary: u8) {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();
        let obsolete = tracker.claim_recovery(instrument_id).unwrap();

        match boundary {
            0 => tracker.remove(instrument_id),
            1 => {
                assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
                tracker.reset_for_instruments(&subs, &[instrument_id]);
            }
            2 => tracker.clear(),
            _ => unreachable!(),
        }

        assert!(obsolete.cancellation.is_cancelled());
        let current = tracker.claim_recovery(instrument_id).unwrap();
        tracker.fail_recovery(instrument_id, Some(&obsolete));
        assert!(!current.cancellation.is_cancelled());
        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(current.is_accepted());
    }

    #[rstest]
    fn book_gated_reports_pending_recovering_and_failed() {
        let (tracker, subs) = subscribed_tracker();
        let instrument_id = instrument_id();
        let now = Instant::now();

        assert!(!tracker.book_gated(instrument_id));

        tracker.seed_pending_snapshots(&subs, &[instrument_id], Duration::from_secs(3), now);
        assert!(tracker.book_gated(instrument_id));

        assert!(tracker.record_snapshot_if_subscribed(&subs, instrument_id, now));
        assert!(!tracker.book_gated(instrument_id));

        assert_eq!(
            tracker.request_recovery(instrument_id, Duration::ZERO, now),
            BookSequenceOutcome::Recover
        );
        assert!(tracker.book_gated(instrument_id));

        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        tracker.fail_recovery(instrument_id, Some(&recovery));
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
            let mut failed = false;
            let mut steps = trace;

            for step in 0..5 {
                let event = steps % 6;
                steps /= 6;

                match event {
                    0 => {
                        let expected = !accepted && !failed;
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
                        // gate or terminal failure suppresses it.
                        let expected = !failed && !pending;

                        assert_eq!(
                            tracker.record_snapshot_if_subscribed(&subs, id, Instant::now()),
                            expected,
                            "trace={trace}, step={step}"
                        );
                        accepted |= expected;
                    }
                    3 => {
                        tracker.fail_recovery(id, Some(&owner));
                        failed |= !accepted;
                    }
                    4 => {
                        tracker.remove(id);
                        assert!(owner.cancellation.is_cancelled());
                        obsolete.push(owner);
                        owner = tracker.claim_recovery(id).unwrap();
                        pending = false;
                        accepted = false;
                        failed = false;
                    }
                    5 => {
                        for previous in &obsolete {
                            tracker.fail_recovery(id, Some(previous));
                        }
                    }
                    _ => unreachable!(),
                }

                assert_eq!(
                    owner.cancellation.is_cancelled(),
                    failed,
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
            tracker.validate_incremental_if_subscribed(
                &subs,
                steady_id,
                Duration::from_secs(3),
                now
            ),
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
    fn fail_recovery_if_subscribed_ignores_unsubscribed() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let subs = AtomicSet::new();

        tracker.fail_recovery_if_subscribed(&subs, instrument_id, None);

        assert!(!is_failed(&tracker, instrument_id));
        assert!(tracker.claim_recovery(instrument_id).is_some());

        subs.insert(instrument_id);
        tracker.fail_recovery_if_subscribed(&subs, instrument_id, None);

        assert!(is_failed(&tracker, instrument_id));
    }

    #[rstest]
    fn stale_books_skips_failed_recovery() {
        let tracker = BookSyncTracker::default();
        let failed_id = InstrumentId::from("0xCOND-A-0xTOKEN-A.POLYMARKET");
        let steady_id = InstrumentId::from("0xCOND-B-0xTOKEN-B.POLYMARKET");
        let subs = AtomicSet::new();
        subs.insert(failed_id);
        subs.insert(steady_id);
        let now = Instant::now();
        let past = now.checked_sub(Duration::from_secs(6)).unwrap();
        let threshold = Duration::from_secs(5);

        assert!(tracker.record_snapshot_if_subscribed(&subs, failed_id, past));
        assert!(tracker.record_snapshot_if_subscribed(&subs, steady_id, past));

        let recovery = tracker.claim_recovery(failed_id).unwrap();
        tracker.fail_recovery(failed_id, Some(&recovery));

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
            tracker.validate_incremental_if_subscribed(
                &subs,
                instrument_id,
                Duration::from_secs(3),
                now
            ),
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
        let timeout = Duration::from_secs(3);

        assert_eq!(
            tracker.request_recovery_if_subscribed(&subs, instrument_id, timeout, now),
            BookSequenceOutcome::Suppress
        );
        assert!(is_empty(&tracker));

        subs.insert(instrument_id);

        assert_eq!(
            tracker.request_recovery_if_subscribed(&subs, instrument_id, timeout, now),
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

        tracker.reset_for_instruments(&subs, &[subscribed_id, retired_id]);

        assert!(tracker.book_gated(subscribed_id));
        assert!(!tracker.book_gated(retired_id));

        let seeded =
            tracker.seed_pending_snapshots(&subs, &[subscribed_id, retired_id], timeout, now);

        assert_eq!(seeded, 1);
        assert!(has_pending_snapshot(&tracker, subscribed_id));
        assert!(!has_pending_snapshot(&tracker, retired_id));
    }

    #[rstest]
    fn stale_failure_report_leaves_no_entry() {
        let tracker = BookSyncTracker::default();
        let instrument_id = instrument_id();
        let obsolete = tracker.claim_recovery(instrument_id).unwrap();

        tracker.remove(instrument_id);
        tracker.fail_recovery(instrument_id, Some(&obsolete));

        assert!(is_empty(&tracker));
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
            .get(&instrument_id)
            .is_some_and(|pending| pending.deadline.is_some())
    }

    fn is_empty(tracker: &BookSyncTracker) -> bool {
        let state = tracker.state.lock();
        state.last_book_ts.is_empty()
            && state.recovering.is_empty()
            && state.pending_snapshots.is_empty()
            && state.recoveries.is_empty()
    }

    fn is_failed(tracker: &BookSyncTracker, instrument_id: InstrumentId) -> bool {
        tracker
            .state
            .lock()
            .recoveries
            .get(&instrument_id)
            .is_some_and(BookRecoveryState::is_failed)
    }
}
