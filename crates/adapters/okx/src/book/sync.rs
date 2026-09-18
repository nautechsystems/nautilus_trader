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
//!
//! [`BookSyncTracker`] decides whether to accept a book batch, suppress it, or request recovery.
//! It tracks sequence linkage, pending snapshots, stale feeds, and recovery ownership per instrument.
//!
//! Claiming, accepting, failing, and resetting recovery stay under the tracker's state lock so
//! competing events cannot independently change ownership. Snapshot gates coordinate acceptance
//! with transport sends. This module performs state transitions; [`super::recovery`] runs the
//! asynchronous subscription and retry work.

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_core::AtomicMap;
use nautilus_live::book::{recovery::BookRecoveryState, snapshot::PendingSnapshot};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    BookChannelScope, BookRecoveryOutcome, BookSequenceOutcome, BookSyncSignalKind,
    recovery::BookRecovery,
};
use crate::{
    common::enums::OKXBookChannel,
    websocket::{error::OKXWsError, handler::SnapshotGate},
};

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
    pub(crate) fn record_subscription(
        &self,
        instrument_id: InstrumentId,
        now: Instant,
        gate: SnapshotGate,
    ) -> CancellationToken {
        let mut state = self.state.lock();
        state.last_book_ts.insert(instrument_id, now);
        state.last_sequences.remove(&instrument_id);
        state.recovering.remove(&instrument_id);
        reset_recovery(&mut state, instrument_id);

        let cancel = CancellationToken::new();

        state.pending_snapshots.insert(
            instrument_id,
            PendingSnapshot {
                deadline: None,
                cancel: cancel.clone(),
                gate,
            },
        );

        cancel
    }

    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        let mut state = self.state.lock();
        state.last_book_ts.remove(&instrument_id);
        state.last_sequences.remove(&instrument_id);
        state.recovering.remove(&instrument_id);
        state.pending_snapshots.remove(&instrument_id);
        reset_recovery(&mut state, instrument_id);
    }

    pub(crate) fn clear(&self) {
        let mut state = self.state.lock();
        state.last_book_ts.clear();
        state.last_sequences.clear();
        state.recovering.clear();
        state.pending_snapshots.clear();
        state.recoveries.clear();
    }

    pub(crate) fn record_update_if_subscribed(
        &self,
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        instrument_id: InstrumentId,
        is_snapshot: bool,
        now: Instant,
    ) -> bool {
        book_channels.contains_key(&instrument_id)
            && self.record_update(instrument_id, is_snapshot, now)
    }

    fn record_update(&self, instrument_id: InstrumentId, is_snapshot: bool, now: Instant) -> bool {
        let mut state = self.state.lock();
        if state
            .recoveries
            .get(&instrument_id)
            .is_some_and(BookRecoveryState::is_failed)
            || subscription_send_pending(&state, instrument_id)
        {
            return false;
        }

        if is_snapshot && !accept_recovery(&state, instrument_id) {
            return false;
        }

        state.last_book_ts.insert(instrument_id, now);

        if is_snapshot {
            state.pending_snapshots.remove(&instrument_id);
            state.recovering.remove(&instrument_id);
        }

        true
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

        if state
            .recoveries
            .get(&instrument_id)
            .is_some_and(BookRecoveryState::is_failed)
            || subscription_send_pending(&state, instrument_id)
        {
            return BookSequenceOutcome::Suppress;
        }

        if is_snapshot {
            if state.last_sequences.contains_key(&instrument_id) {
                return BookSequenceOutcome::Suppress;
            }

            let invalid = sequences
                .iter()
                .find(|(prev_seq_id, _)| prev_seq_id.is_some_and(|value| value != -1));

            if let Some((prev_seq_id, seq_id)) = invalid {
                return handle_sequence_gap(
                    &mut state,
                    instrument_id,
                    *prev_seq_id,
                    *seq_id,
                    timeout,
                    now,
                );
            }

            if !accept_recovery(&state, instrument_id) {
                return BookSequenceOutcome::Suppress;
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
                return handle_sequence_gap(
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

    pub(crate) fn reset_sequences(
        &self,
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        scope: BookChannelScope,
    ) {
        let instrument_ids = book_channels
            .load()
            .iter()
            .filter_map(|(instrument_id, channel)| {
                channel_matches_scope(*channel, scope).then_some(*instrument_id)
            })
            .collect::<Vec<_>>();

        let mut state = self.state.lock();

        for instrument_id in instrument_ids {
            // An in-flight initial send may not yet be registered for reconnect replay
            if state
                .pending_snapshots
                .get(&instrument_id)
                .is_some_and(|pending| pending.deadline.is_some() || pending.cancel.is_cancelled())
            {
                state.pending_snapshots.remove(&instrument_id);
            }

            state.last_sequences.remove(&instrument_id);
            state.recovering.insert(instrument_id);

            // Cancellation between replacement sends can leave the book unsubscribed
            // even after reconnect replay restores it.
            let recovery_active = state
                .recoveries
                .get(&instrument_id)
                .and_then(BookRecoveryState::current)
                .is_some_and(|recovery| {
                    !recovery.cancellation.is_cancelled()
                        && !matches!(*recovery.outcome.borrow(), BookRecoveryOutcome::Accepted)
                });

            if !recovery_active {
                reset_recovery(&mut state, instrument_id);
            }
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
                channel_matches_scope(*channel, scope).then_some(*instrument_id)
            })
            .collect::<Vec<_>>();

        if instrument_ids.is_empty() {
            return 0;
        }

        let mut state = self.state.lock();
        let mut seeded = 0;

        for instrument_id in &instrument_ids {
            if state
                .pending_snapshots
                .get(instrument_id)
                .is_some_and(|pending| pending.deadline.is_none() && !pending.cancel.is_cancelled())
            {
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

    pub(crate) fn claim_subscription_recovery(
        &self,
        instrument_id: InstrumentId,
        cancel: &CancellationToken,
    ) -> Option<Arc<BookRecovery>> {
        let mut state = self.state.lock();

        if cancel.is_cancelled() {
            return None;
        }

        claim_recovery(&mut state, instrument_id)
    }

    pub(crate) fn claim_recovery(&self, instrument_id: InstrumentId) -> Option<Arc<BookRecovery>> {
        let mut state = self.state.lock();
        if subscription_send_pending(&state, instrument_id) {
            return None;
        }

        claim_recovery(&mut state, instrument_id)
    }

    pub(crate) fn reject_recovery(&self, instrument_id: InstrumentId, error: OKXWsError) -> bool {
        let state = self.state.lock();
        if subscription_send_pending(&state, instrument_id) {
            // A rejection while the send gate is closed belongs to the previous subscription
            return true;
        }

        if let Some(recovery) = state
            .recoveries
            .get(&instrument_id)
            .and_then(BookRecoveryState::current)
            && !matches!(*recovery.outcome.borrow(), BookRecoveryOutcome::Accepted)
        {
            recovery
                .outcome
                .send_replace(BookRecoveryOutcome::Rejected(error));
            true
        } else {
            false
        }
    }

    pub(crate) fn fail_recovery(
        &self,
        instrument_id: InstrumentId,
        recovery: Option<&Arc<BookRecovery>>,
    ) {
        let mut state = self.state.lock();
        if recovery.is_none() && subscription_send_pending(&state, instrument_id) {
            return;
        }

        if !state
            .recoveries
            .entry(instrument_id)
            .or_default()
            .fail(recovery)
        {
            return;
        }

        state.recovering.insert(instrument_id);
        state.last_sequences.remove(&instrument_id);
        state.pending_snapshots.remove(&instrument_id);
    }

    /// Reports instruments whose book feed has exceeded `threshold` since the
    /// last update, re-arming each reported window so a still-dead feed keeps
    /// being reported at most once per threshold window.
    pub(crate) fn stale_books(&self, threshold: Duration, now: Instant) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock();
        let mut stale = Vec::new();

        for (instrument_id, last_update) in &mut state.last_book_ts {
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
        book_channels: &AtomicMap<InstrumentId, OKXBookChannel>,
        scope: BookChannelScope,
        now: Instant,
    ) -> Vec<BookSyncSignal> {
        let mut state = self.state.lock();

        let mut expired = state
            .pending_snapshots
            .iter()
            .filter_map(|(instrument_id, pending)| {
                (pending.deadline.is_some_and(|deadline| deadline <= now)
                    && book_channels
                        .get_cloned(instrument_id)
                        .is_some_and(|channel| channel_matches_scope(channel, scope)))
                .then_some(BookSyncSignal {
                    instrument_id: *instrument_id,
                    kind: BookSyncSignalKind::SnapshotMissing,
                })
            })
            .collect::<Vec<_>>();

        // Sort by instrument; the pending map iterates in per-process hash order
        expired.sort_by_key(|signal| signal.instrument_id);

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
    last_sequences: AHashMap<InstrumentId, u64>,
    recovering: AHashSet<InstrumentId>,
    pending_snapshots: AHashMap<InstrumentId, PendingSnapshot>,
    recoveries: AHashMap<InstrumentId, BookRecoveryState<OKXWsError>>,
}

fn subscription_send_pending(state: &BookSyncState, instrument_id: InstrumentId) -> bool {
    state
        .pending_snapshots
        .get(&instrument_id)
        .is_some_and(|pending| pending.gate.lock().is_closed())
}

fn claim_recovery(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
) -> Option<Arc<BookRecovery>> {
    if !state.last_book_ts.contains_key(&instrument_id) {
        return None;
    }

    let recovery = state.recoveries.entry(instrument_id).or_default().claim()?;

    state.recovering.insert(instrument_id);
    state.last_sequences.remove(&instrument_id);
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

fn handle_sequence_gap(
    state: &mut BookSyncState,
    instrument_id: InstrumentId,
    prev_seq_id: Option<i64>,
    seq_id: u64,
    timeout: Duration,
    now: Instant,
) -> BookSequenceOutcome {
    let last_seq_id = state.last_sequences.remove(&instrument_id);
    arm_snapshot_deadline(state, instrument_id, timeout, now);

    if state.recovering.insert(instrument_id) {
        log::warn!(
            "Book sequence gap for {instrument_id}: last_seq_id={last_seq_id:?}, \
             prev_seq_id={prev_seq_id:?}, seq_id={seq_id}; requesting a fresh snapshot"
        );
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
        state.pending_snapshots.insert(
            instrument_id,
            PendingSnapshot {
                deadline: Some(now + timeout),
                cancel: CancellationToken::new(),
                gate: SnapshotGate::default(),
            },
        );
    }
}

fn channel_matches_scope(channel: OKXBookChannel, scope: BookChannelScope) -> bool {
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
    use std::sync::Arc;

    use nautilus_common::live::dst::time::{Duration, Instant};
    use nautilus_core::AtomicMap;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::{
        BookChannelScope, BookRecoveryOutcome, BookRecoveryState, BookSequenceOutcome,
        BookSyncSignalKind, BookSyncTracker,
    };
    use crate::{
        common::enums::OKXBookChannel,
        websocket::{error::OKXWsError, handler::SnapshotGate},
    };

    #[rstest]
    fn stale_rejection_fallback_cannot_claim_new_initial_send() {
        let tracker = BookSyncTracker::default();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let old = tracker.record_subscription(id, Instant::now(), SnapshotGate::default());
        assert!(!tracker.reject_recovery(
            id,
            OKXWsError::OkxError {
                error_code: "60014".to_string(),
                message: "Temporary subscription failure".to_string(),
            }
        ));
        let gate = SnapshotGate::default();
        gate.lock().close();
        let current = tracker.record_subscription(id, Instant::now(), gate);
        assert!(old.is_cancelled());
        assert!(tracker.claim_recovery(id).is_none());
        assert!(!current.is_cancelled());
        let recovery = tracker.claim_subscription_recovery(id, &current).unwrap();
        assert!(current.is_cancelled());
        assert!(!recovery.cancellation.is_cancelled());
    }

    #[rstest]
    #[case::incremental(false)]
    #[case::full_snapshot(true)]
    fn initial_send_gate_suppresses_old_frames_without_canceling_subscription(
        #[case] full_snapshot: bool,
    ) {
        let tracker = BookSyncTracker::default();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = tracker.record_subscription(id, now, gate.clone());

        if full_snapshot {
            assert!(!tracker.record_update(id, true, now));
        } else {
            assert_eq!(
                tracker.validate_sequence(id, true, &[(Some(-1), 10)], Duration::from_secs(3), now),
                BookSequenceOutcome::Suppress
            );
            assert_eq!(
                tracker.validate_sequence(
                    id,
                    false,
                    &[(Some(10), 11)],
                    Duration::from_secs(3),
                    now
                ),
                BookSequenceOutcome::Suppress
            );
        }

        assert!(!cancel.is_cancelled());
        assert!(tracker.state.lock().recoveries.is_empty());
        gate.open();

        if full_snapshot {
            assert!(tracker.record_update(id, true, now));
        } else {
            assert_eq!(
                tracker.validate_sequence(id, true, &[(Some(-1), 20)], Duration::from_secs(3), now),
                BookSequenceOutcome::Accept
            );
        }

        assert!(cancel.is_cancelled());
        assert!(tracker.claim_subscription_recovery(id, &cancel).is_none());
    }

    #[rstest]
    #[case::public(OKXBookChannel::Book, BookChannelScope::Public)]
    #[case::business(OKXBookChannel::SprdBooks5, BookChannelScope::Business)]
    fn reconnect_preserves_initial_send_until_snapshot(
        #[case] channel: OKXBookChannel,
        #[case] scope: BookChannelScope,
    ) {
        let tracker = BookSyncTracker::default();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let cancel = tracker.record_subscription(id, now, SnapshotGate::default());
        let channels = AtomicMap::new();
        channels.insert(id, channel);
        tracker.reset_sequences(&channels, scope);
        assert_eq!(
            tracker.seed_pending_snapshots(&channels, scope, Duration::from_secs(3), now),
            0
        );
        assert!(!cancel.is_cancelled());
        assert!(
            tracker
                .take_expired_snapshots(&channels, scope, now + Duration::from_secs(4))
                .is_empty()
        );
        assert!(tracker.record_update(id, true, now));
        assert!(cancel.is_cancelled());
        assert!(tracker.claim_subscription_recovery(id, &cancel).is_none());
    }

    #[rstest]
    #[case::snapshot(0)]
    #[case::unsubscribe(1)]
    #[case::resubscribe(2)]
    #[case::shutdown(3)]
    #[case::reconnect(4)]
    #[case::recovery(5)]
    fn initial_subscription_completion_cannot_claim_recovery_after_reset(#[case] boundary: u8) {
        let tracker = BookSyncTracker::default();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let cancel = tracker.record_subscription(id, now, SnapshotGate::default());
        let channels = AtomicMap::new();
        channels.insert(id, OKXBookChannel::Book);
        assert!(
            tracker
                .take_expired_snapshots(
                    &channels,
                    BookChannelScope::Public,
                    now + Duration::from_secs(60)
                )
                .is_empty()
        );

        match boundary {
            0 => {
                assert!(tracker.record_update(id, true, now));
            }
            1 => tracker.remove(id),
            2 => {
                tracker.record_subscription(id, now, SnapshotGate::default());
            }
            3 => tracker.clear(),
            4 => {
                tracker.reset_sequences(&channels, BookChannelScope::Public);
                assert!(tracker.record_update(id, true, now));
            }
            5 => {
                assert!(tracker.claim_recovery(id).is_some());
            }
            _ => unreachable!(),
        }

        assert!(cancel.is_cancelled());
        assert!(tracker.claim_subscription_recovery(id, &cancel).is_none());
    }

    #[rstest]
    fn initial_subscription_timeout_claims_one_recovery_owner() {
        let tracker = BookSyncTracker::default();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let cancel = tracker.record_subscription(id, Instant::now(), SnapshotGate::default());
        let recovery = tracker.claim_subscription_recovery(id, &cancel).unwrap();
        assert!(cancel.is_cancelled());
        assert!(!recovery.cancellation.is_cancelled());
        assert!(tracker.claim_subscription_recovery(id, &cancel).is_none());
        assert!(tracker.claim_recovery(id).is_none());
    }

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
    fn stale_books_rearms_window_and_keeps_tracking() {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let threshold = Duration::from_secs(5);

        tracker.record_subscription(
            instrument_id,
            now.checked_sub(Duration::from_secs(6)).unwrap(),
            SnapshotGate::default(),
        );
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
    fn take_expired_snapshots_emits_once_and_keeps_staleness_tracking() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        assert!(tracker.record_update(instrument_id, true, now));
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now.checked_sub(Duration::from_secs(4)).unwrap(),
        );

        let first = tracker.take_expired_snapshots(&book_channels, BookChannelScope::Public, now);
        let second = tracker.take_expired_snapshots(&book_channels, BookChannelScope::Public, now);

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
    fn take_expired_snapshots_does_not_drain_other_scope() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let public_id = InstrumentId::from("BTC-USDT.OKX");
        let spread_id = InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX");
        let now = Instant::now();

        book_channels.insert(public_id, OKXBookChannel::Book);
        book_channels.insert(spread_id, OKXBookChannel::SprdBooks5);
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now.checked_sub(Duration::from_secs(4)).unwrap(),
        );
        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Business,
            Duration::from_secs(3),
            now.checked_sub(Duration::from_secs(4)).unwrap(),
        );

        let public = tracker.take_expired_snapshots(&book_channels, BookChannelScope::Public, now);

        assert_eq!(public.len(), 1);
        assert_eq!(public[0].instrument_id, public_id);
        assert!(has_pending_snapshot(&tracker, spread_id));
        assert!(!has_pending_snapshot(&tracker, public_id));
    }

    #[rstest]
    fn take_expired_snapshots_returns_instruments_in_sorted_order() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let now = Instant::now();

        // Insert in non-sorted order; expiry must still report sorted by instrument
        for id in [
            "ETH-USDT.OKX",
            "BTC-USDT.OKX",
            "SOL-USDT.OKX",
            "DOGE-USDT.OKX",
            "XRP-USDT.OKX",
        ] {
            book_channels.insert(InstrumentId::from(id), OKXBookChannel::Book);
        }

        tracker.seed_pending_snapshots(
            &book_channels,
            BookChannelScope::Public,
            Duration::from_secs(3),
            now.checked_sub(Duration::from_secs(4)).unwrap(),
        );

        let expired = tracker.take_expired_snapshots(&book_channels, BookChannelScope::Public, now);

        let instrument_ids: Vec<InstrumentId> =
            expired.iter().map(|signal| signal.instrument_id).collect();
        assert_eq!(
            instrument_ids,
            [
                InstrumentId::from("BTC-USDT.OKX"),
                InstrumentId::from("DOGE-USDT.OKX"),
                InstrumentId::from("ETH-USDT.OKX"),
                InstrumentId::from("SOL-USDT.OKX"),
                InstrumentId::from("XRP-USDT.OKX"),
            ]
        );
    }

    #[rstest]
    fn recovery_has_one_owner_and_requires_snapshot() {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(tracker.claim_recovery(instrument_id).is_none());
        assert_eq!(
            tracker.validate_sequence(instrument_id, false, &[(Some(1), 2)], Duration::ZERO, now),
            BookSequenceOutcome::Suppress
        );
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 10)], Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
        tracker.fail_recovery(instrument_id, Some(&recovery));
        assert_eq!(
            tracker.validate_sequence(instrument_id, false, &[(Some(10), 11)], Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn rejection_after_completed_recovery_starts_new_episode() {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 10)], Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );

        let error = OKXWsError::OkxError {
            error_code: "50011".into(),
            message: "Rate limited".into(),
        };

        assert!(!tracker.reject_recovery(instrument_id, error));
        let replacement = tracker
            .claim_recovery(instrument_id)
            .expect("new recovery owner");
        assert!(recovery.cancellation.is_cancelled());
        assert!(!replacement.cancellation.is_cancelled());
    }

    #[rstest]
    fn failed_recovery_suppresses_late_snapshot_until_reset() {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        tracker.fail_recovery(instrument_id, Some(&recovery));
        assert!(recovery.cancellation.is_cancelled());
        assert!(tracker.claim_recovery(instrument_id).is_none());
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 10)], Duration::ZERO, now),
            BookSequenceOutcome::Suppress
        );
        assert!(!tracker.record_update(instrument_id, true, now));
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 20)], Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    #[case::unsubscribe(0)]
    #[case::completed_reconnect(1)]
    #[case::shutdown(2)]
    #[case::resubscribe(3)]
    fn recovery_cancellation_cannot_fail_replacement(#[case] boundary: u8) {
        let tracker = BookSyncTracker::default();
        let channels = AtomicMap::new();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let obsolete = tracker.claim_recovery(instrument_id).unwrap();

        match boundary {
            0 => tracker.remove(instrument_id),
            1 => {
                obsolete.outcome.send_replace(BookRecoveryOutcome::Accepted);
                tracker.reset_sequences(&channels, BookChannelScope::Public);
            }
            2 => tracker.clear(),
            3 => {
                tracker.record_subscription(instrument_id, now, SnapshotGate::default());
            }
            _ => unreachable!(),
        }

        assert!(obsolete.cancellation.is_cancelled());
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let current = tracker.claim_recovery(instrument_id).unwrap();
        tracker.fail_recovery(instrument_id, Some(&obsolete));
        assert!(!current.cancellation.is_cancelled());
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 20)], Duration::ZERO, now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    #[case::public(OKXBookChannel::Book, BookChannelScope::Public)]
    #[case::rpi(OKXBookChannel::BooksRpi, BookChannelScope::Public)]
    #[case::business(OKXBookChannel::SprdBooks5, BookChannelScope::Business)]
    fn reconnect_preserves_active_recovery(
        #[case] channel: OKXBookChannel,
        #[case] scope: BookChannelScope,
        #[values(0, 1, 2, 3)] phase: u8,
        #[values(Duration::ZERO, Duration::from_secs(3))] timeout: Duration,
    ) {
        let tracker = BookSyncTracker::default();
        let channels = AtomicMap::new();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        channels.insert(id, channel);
        tracker.record_subscription(id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(id).unwrap();

        if phase > 0 {
            assert!(recovery.begin_replacement());
        }

        if phase > 1 {
            recovery.gate.open();
        }

        if phase == 3 {
            assert!(
                tracker.reject_recovery(id, OKXWsError::OperationTimeout { timeout_ms: 3_000 })
            );
        }

        tracker.reset_sequences(&channels, scope);

        if !timeout.is_zero() {
            assert_eq!(
                tracker.seed_pending_snapshots(&channels, scope, timeout, now),
                1
            );
        }

        assert!(!recovery.cancellation.is_cancelled());
        assert!(tracker.claim_recovery(id).is_none());
        assert_eq!(recovery.gate.lock().is_closed(), phase == 1);
        assert_eq!(
            tracker.validate_sequence(id, true, &[(Some(-1), 100)], timeout, now),
            if phase == 1 {
                BookSequenceOutcome::Suppress
            } else {
                BookSequenceOutcome::Accept
            },
        );

        if phase == 1 {
            recovery.gate.open();
            assert_eq!(
                tracker.validate_sequence(id, true, &[(Some(-1), 100)], timeout, now),
                BookSequenceOutcome::Accept,
            );
        }

        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
        assert_eq!(
            tracker.validate_sequence(id, false, &[(Some(100), 101)], timeout, now),
            BookSequenceOutcome::Accept,
        );
    }

    #[rstest]
    #[case::accepted(0)]
    #[case::cancelled(1)]
    #[case::failed(2)]
    fn reconnect_retires_inactive_recovery(#[case] phase: u8) {
        let tracker = BookSyncTracker::default();
        let channels = AtomicMap::new();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        channels.insert(id, OKXBookChannel::Book);
        tracker.record_subscription(id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(id).unwrap();

        match phase {
            0 => assert_eq!(
                tracker.validate_sequence(id, true, &[(Some(-1), 100)], Duration::ZERO, now),
                BookSequenceOutcome::Accept,
            ),
            1 => recovery.cancellation.cancel(),
            2 => tracker.fail_recovery(id, Some(&recovery)),
            _ => unreachable!(),
        }

        tracker.reset_sequences(&channels, BookChannelScope::Public);
        let replacement = tracker.claim_recovery(id).expect("fresh recovery owner");

        assert!(recovery.cancellation.is_cancelled());
        assert!(!replacement.cancellation.is_cancelled());
        assert_eq!(
            tracker.validate_sequence(id, true, &[(Some(-1), 200)], Duration::ZERO, now),
            BookSequenceOutcome::Accept,
        );
    }

    #[rstest]
    fn remove_clears_tracking_state() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();

        book_channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
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
        tracker.record_subscription(public_instrument_id, now, SnapshotGate::default());
        tracker.record_subscription(spread_instrument_id, now, SnapshotGate::default());
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
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());

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
    #[case::stale(Some(-1), 90)]
    #[case::duplicate(Some(-1), 105)]
    #[case::newer(Some(-1), 110)]
    #[case::malformed(Some(105), 110)]
    fn sequence_suppresses_unsolicited_snapshot_without_mutating_state(
        #[case] previous: Option<i64>,
        #[case] sequence: u64,
        #[values(Duration::ZERO, Duration::from_secs(3))] timeout: Duration,
        #[values(false, true)] recovered: bool,
    ) {
        let tracker = BookSyncTracker::default();
        let id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        tracker.record_subscription(id, now, SnapshotGate::default());
        let owner = recovered.then(|| tracker.claim_recovery(id).unwrap());

        assert_eq!(
            tracker.validate_sequence(id, true, &[(Some(-1), 100)], timeout, now),
            BookSequenceOutcome::Accept,
        );
        let updated_at = now + Duration::from_secs(1);
        assert_eq!(
            tracker.validate_sequence(id, false, &[(Some(100), 105)], timeout, updated_at),
            BookSequenceOutcome::Accept,
        );

        let outcome = tracker.validate_sequence(
            id,
            true,
            &[(previous, sequence)],
            timeout,
            now + Duration::from_secs(2),
        );

        assert_eq!(outcome, BookSequenceOutcome::Suppress);
        {
            let state = tracker.state.lock();
            assert_eq!(state.last_sequences.get(&id), Some(&105));
            assert_eq!(state.last_book_ts.get(&id), Some(&updated_at));
            assert!(!state.pending_snapshots.contains_key(&id));
            assert!(!state.recovering.contains(&id));
            let current = state
                .recoveries
                .get(&id)
                .and_then(BookRecoveryState::current);
            assert_eq!(current.is_some(), recovered);

            if let Some(owner) = &owner {
                assert!(Arc::ptr_eq(current.unwrap(), owner));
                assert!(owner.is_accepted());
                assert!(!owner.cancellation.is_cancelled());
            }
        }

        assert_eq!(
            tracker.validate_sequence(
                id,
                false,
                &[(Some(105), 110)],
                timeout,
                now + Duration::from_secs(3),
            ),
            BookSequenceOutcome::Accept,
        );
        assert_eq!(last_sequence(&tracker, id), Some(110));
    }

    #[rstest]
    fn recurring_snapshots_remain_accepted() {
        let tracker = BookSyncTracker::default();
        let channels = AtomicMap::new();
        let id = InstrumentId::from("BTC-USDT-SPREAD.OKX");
        let now = Instant::now();
        channels.insert(id, OKXBookChannel::SprdBooks5);
        tracker.record_subscription(id, now, SnapshotGate::default());

        let first = tracker.record_update_if_subscribed(&channels, id, true, now);
        let updated_at = now + Duration::from_secs(1);
        let next = tracker.record_update_if_subscribed(&channels, id, true, updated_at);

        assert!(first);
        assert!(next);
        assert_eq!(
            tracker.state.lock().last_book_ts.get(&id),
            Some(&updated_at)
        );
        assert!(!has_pending_snapshot(&tracker, id));
    }

    #[rstest]
    fn sequence_gap_requests_one_recovery_and_waits_for_snapshot() {
        let book_channels = AtomicMap::new();
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        let timeout = Duration::from_secs(3);

        book_channels.insert(instrument_id, OKXBookChannel::BooksRpi);
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
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

        assert_eq!(gap, BookSequenceOutcome::Recover);
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
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
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
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
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

        assert_eq!(update, BookSequenceOutcome::Recover);
    }

    #[rstest]
    #[case::incremental(false)]
    #[case::full_snapshot(true)]
    fn replacement_blocks_snapshot_acceptance_until_sends_complete(#[case] full_snapshot: bool) {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();

        let snapshot = || {
            if full_snapshot {
                tracker.record_update(instrument_id, true, now)
            } else {
                tracker.validate_sequence(
                    instrument_id,
                    true,
                    &[(Some(-1), 42)],
                    Duration::ZERO,
                    now,
                ) == BookSequenceOutcome::Accept
            }
        };

        assert!(recovery.begin_replacement());
        assert!(!snapshot());
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        recovery.gate.open();
        assert!(snapshot());
        assert!(!recovery.begin_replacement());
        assert!(!recovery.gate.lock().is_closed());
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Accepted
        ));
    }

    #[rstest]
    fn failed_replacement_cannot_be_rescued_by_late_snapshot() {
        let tracker = BookSyncTracker::default();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let now = Instant::now();
        tracker.record_subscription(instrument_id, now, SnapshotGate::default());
        let recovery = tracker.claim_recovery(instrument_id).unwrap();
        assert!(recovery.begin_replacement());
        assert!(tracker.reject_recovery(
            instrument_id,
            OKXWsError::SendFailed("subscribe stalled".into())
        ));
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 42)], Duration::ZERO, now),
            BookSequenceOutcome::Suppress
        );
        tracker.fail_recovery(instrument_id, Some(&recovery));

        assert!(recovery.cancellation.is_cancelled());
        assert!(!recovery.begin_replacement());
        assert_eq!(
            tracker.validate_sequence(instrument_id, true, &[(Some(-1), 43)], Duration::ZERO, now),
            BookSequenceOutcome::Suppress
        );
        assert!(tracker.claim_recovery(instrument_id).is_none());
    }

    #[rstest]
    fn recovery_event_order_preserves_snapshot_and_owner_guards() {
        // Covers every ordering of five tracker events
        for trace in 0_u32..6_u32.pow(5) {
            let tracker = BookSyncTracker::default();
            let id = InstrumentId::from("BTC-USDT.OKX");
            let now = Instant::now();
            tracker.record_subscription(id, now, SnapshotGate::default());
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
                        let expected = if failed || pending || accepted {
                            BookSequenceOutcome::Suppress
                        } else {
                            BookSequenceOutcome::Accept
                        };

                        assert_eq!(
                            tracker.validate_sequence(
                                id,
                                true,
                                &[(Some(-1), 10)],
                                Duration::ZERO,
                                now
                            ),
                            expected,
                            "trace={trace}, step={step}"
                        );
                        accepted |= expected == BookSequenceOutcome::Accept;
                    }
                    3 => {
                        tracker.fail_recovery(id, Some(&owner));
                        failed |= !accepted;
                    }
                    4 => {
                        tracker.record_subscription(id, now, SnapshotGate::default());
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
            && state.recoveries.is_empty()
    }
}
