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

//! Synchronization lifecycle of one subscribed book.
//!
//! [`BookSync`] holds the state an adapter tracks per book: its phase, the time of its last
//! accepted frame, a pending snapshot wait, and its recovery episode. It performs no I/O and takes
//! `&mut self`, so an adapter keeps it under its existing state lock or owning task.
//!
//! # Sync Contract
//!
//! - Output happens only in [`BookPhase::Synced`], which starts with an accepted snapshot.
//! - A book out of sync has one owner: a running recovery or an armed snapshot deadline.
//!   [`BookSync::gap`] returns [`BookSequenceOutcome::Recover`] when it has neither, and the adapter
//!   then claims a recovery.
//! - [`BookSync::stale`] reports a book that stops advancing without a running recovery.
//!
//! # Adapters
//!
//! The adapter keeps its venue sequence rule, wire commands, snapshot parsing, and buffers, and
//! stores the venue position of the last accepted frame as `P`.

use std::sync::Arc;

use nautilus_common::live::dst::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use super::{
    BookSequenceOutcome,
    recovery::{BookRecovery, BookRecoveryOutcome, BookRecoveryState},
    snapshot::{PendingSnapshot, SnapshotGate},
};

/// Synchronization phase of one book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookPhase<P> {
    /// Subscribed and waiting for the first snapshot.
    Waiting,
    /// A snapshot was accepted; holds the venue position of the last accepted frame.
    Synced(P),
    /// Output is suppressed until a snapshot restores the book.
    Recovering,
}

/// Synchronization state of one subscribed book.
#[derive(Debug)]
pub struct BookSync<E, P = ()> {
    phase: BookPhase<P>,
    last_update: Instant,
    pending: Option<PendingSnapshot>,
    recovery: BookRecoveryState<E>,
}

impl<E: Clone, P> BookSync<E, P> {
    /// Creates a book waiting for its first snapshot.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            phase: BookPhase::Waiting,
            last_update: now,
            pending: None,
            recovery: BookRecoveryState::default(),
        }
    }

    /// Returns the synchronization phase.
    #[must_use]
    pub fn phase(&self) -> &BookPhase<P> {
        &self.phase
    }

    /// Returns the venue position of the last accepted frame while synced.
    #[must_use]
    pub fn position(&self) -> Option<&P> {
        match &self.phase {
            BookPhase::Synced(position) => Some(position),
            BookPhase::Waiting | BookPhase::Recovering => None,
        }
    }

    /// Returns whether `recovery` is the book's current episode.
    #[must_use]
    pub fn is_current(&self, recovery: &Arc<BookRecovery<E>>) -> bool {
        self.recovery
            .current()
            .is_some_and(|current| Arc::ptr_eq(current, recovery))
    }

    /// Returns whether a snapshot wait or deadline is pending.
    #[must_use]
    pub fn has_pending_snapshot(&self) -> bool {
        self.pending.is_some()
    }

    /// Returns whether a guarded subscription write is still in flight.
    #[must_use]
    pub fn is_send_pending(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.gate.lock().is_closed())
    }

    /// Records the initial snapshot wait for a subscription whose write `gate` guards.
    ///
    /// Accepting a snapshot, claiming a recovery, or removing the book cancels the returned token.
    pub fn expect_snapshot(&mut self, gate: SnapshotGate) -> CancellationToken {
        let cancel = CancellationToken::new();
        self.pending = Some(PendingSnapshot {
            deadline: None,
            cancel: cancel.clone(),
            gate,
        });

        cancel
    }

    /// Accepts a snapshot at `position` unless a subscription write or recovery gate blocks it.
    pub fn accept_snapshot(&mut self, position: P, now: Instant) -> bool {
        if self.is_send_pending() {
            return false;
        }

        if let Some(recovery) = self.recovery.current()
            && recovery.is_running()
            && !recovery.accept()
        {
            return false;
        }

        self.phase = BookPhase::Synced(position);
        self.pending = None;
        self.last_update = now;
        true
    }

    /// Advances a synced book to `position`, returning `false` when the book is not synced.
    pub fn advance(&mut self, position: P, now: Instant) -> bool {
        if !matches!(self.phase, BookPhase::Synced(_)) {
            return false;
        }

        self.phase = BookPhase::Synced(position);
        self.last_update = now;
        true
    }

    /// Marks the book out of sync after a gap, an incremental before any snapshot, or an invalid
    /// snapshot.
    ///
    /// Returns [`BookSequenceOutcome::Recover`] when neither a running recovery nor an armed
    /// snapshot deadline owns the book; the adapter then claims a recovery.
    pub fn gap(&mut self) -> BookSequenceOutcome {
        self.phase = BookPhase::Recovering;

        let deadline_armed = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.deadline.is_some());

        if self.recovery.is_running() || deadline_armed {
            BookSequenceOutcome::Suppress
        } else {
            BookSequenceOutcome::Recover
        }
    }

    /// Claims a recovery episode, refusing while another is running.
    pub fn claim(&mut self) -> Option<Arc<BookRecovery<E>>> {
        let recovery = self.recovery.claim()?;
        self.phase = BookPhase::Recovering;
        self.pending = None;
        Some(recovery)
    }

    /// Delivers `error` to the running recovery, returning `false` when none is running.
    pub fn reject(&self, error: E) -> bool {
        match self.recovery.current() {
            Some(recovery) if recovery.is_running() => {
                recovery
                    .outcome
                    .send_replace(BookRecoveryOutcome::Rejected(error));
                true
            }
            _ => false,
        }
    }

    /// Restarts synchronization after the book's connection reconnects.
    ///
    /// An initial subscription write still in flight keeps its snapshot wait, since reconnect
    /// replay may not include it yet. A running recovery keeps running.
    pub fn reset_on_reconnect(&mut self) {
        if !self.has_initial_wait() {
            self.pending = None;
        }

        self.phase = BookPhase::Recovering;
        self.recovery.reset_on_reconnect();
    }

    /// Arms a snapshot deadline, returning `false` while an initial snapshot wait is in flight.
    pub fn arm_deadline(&mut self, deadline: Instant) -> bool {
        if self.has_initial_wait() {
            return false;
        }

        self.pending = Some(PendingSnapshot {
            deadline: Some(deadline),
            cancel: CancellationToken::new(),
            gate: SnapshotGate::default(),
        });

        true
    }

    /// Removes an expired snapshot deadline, returning whether one expired.
    pub fn take_expired(&mut self, now: Instant) -> bool {
        let expired = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.deadline.is_some_and(|deadline| deadline <= now));

        if expired {
            self.pending = None;
        }

        expired
    }

    /// Returns how long the book has gone without an accepted frame once that exceeds
    /// `threshold`, re-arming the window so a dead feed is reported once per threshold.
    ///
    /// A book owned by a running recovery is not reported; the recovery logs its own attempts.
    pub fn stale(&mut self, threshold: Duration, now: Instant) -> Option<Duration> {
        if self.recovery.is_running() {
            return None;
        }

        let elapsed = now.checked_duration_since(self.last_update)?;

        if elapsed <= threshold {
            return None;
        }

        self.last_update = now;
        Some(elapsed)
    }

    fn has_initial_wait(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.deadline.is_none() && !pending.cancel.is_cancelled())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::error::SendError;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    type Book = BookSync<SendError, u64>;

    fn synced(now: Instant) -> Book {
        let mut book = Book::new(now);
        assert!(book.accept_snapshot(1, now));
        book
    }

    #[rstest]
    fn new_book_waits_and_rejects_incrementals() {
        let now = Instant::now();
        let mut book = Book::new(now);

        let advanced = book.advance(2, now);

        assert_eq!(*book.phase(), BookPhase::Waiting);
        assert!(!advanced);
        assert_eq!(book.position(), None);
    }

    #[rstest]
    fn snapshot_syncs_and_incrementals_advance() {
        let now = Instant::now();
        let mut book = Book::new(now);

        let accepted = book.accept_snapshot(10, now);
        let advanced = book.advance(11, now);

        assert!(accepted);
        assert!(advanced);
        assert_eq!(*book.phase(), BookPhase::Synced(11));
    }

    #[rstest]
    fn closed_subscription_gate_blocks_snapshot() {
        let now = Instant::now();
        let mut book = Book::new(now);
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = book.expect_snapshot(gate.clone());

        let while_sending = book.accept_snapshot(1, now);
        gate.open();
        let after_send = book.accept_snapshot(1, now);

        assert!(!while_sending);
        assert!(after_send);
        assert!(cancel.is_cancelled());
        assert!(!book.has_pending_snapshot());
    }

    #[rstest]
    fn gap_requests_recovery_until_claimed() {
        let now = Instant::now();
        let mut book = synced(now);

        let first = book.gap();
        let again = book.gap();
        let recovery = book.claim().unwrap();
        let owned = book.gap();

        assert_eq!(first, BookSequenceOutcome::Recover);
        assert_eq!(again, BookSequenceOutcome::Recover);
        assert_eq!(owned, BookSequenceOutcome::Suppress);
        assert!(recovery.is_running());
        assert_eq!(*book.phase(), BookPhase::Recovering);
    }

    #[rstest]
    fn armed_deadline_owns_book_after_reconnect() {
        let now = Instant::now();
        let mut book = synced(now);
        book.reset_on_reconnect();
        let armed = book.arm_deadline(now + Duration::from_secs(10));

        let outcome = book.gap();
        let early = book.take_expired(now + Duration::from_secs(9));
        let expired = book.take_expired(now + Duration::from_secs(10));
        let unowned = book.gap();

        assert!(armed);
        assert_eq!(outcome, BookSequenceOutcome::Suppress);
        assert!(!early);
        assert!(expired);
        assert_eq!(unowned, BookSequenceOutcome::Recover);
    }

    #[rstest]
    fn recovery_gate_blocks_snapshot_until_write_confirmed() {
        let now = Instant::now();
        let mut book = synced(now);
        let recovery = book.claim().unwrap();
        assert!(recovery.begin_replacement());

        let during_write = book.accept_snapshot(5, now);
        recovery.gate.open();
        let after_write = book.accept_snapshot(5, now);

        assert!(!during_write);
        assert!(after_write);
        assert!(recovery.is_accepted());
        assert_eq!(*book.phase(), BookPhase::Synced(5));
    }

    #[rstest]
    fn reconnect_keeps_initial_send_wait() {
        let now = Instant::now();
        let mut book = Book::new(now);
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = book.expect_snapshot(gate);

        book.reset_on_reconnect();
        let armed = book.arm_deadline(now + Duration::from_secs(10));

        assert!(!armed);
        assert!(!cancel.is_cancelled());
        assert!(book.is_send_pending());
    }

    #[rstest]
    fn claim_makes_new_episode_current() {
        let now = Instant::now();
        let mut book = synced(now);
        let first = book.claim().unwrap();
        assert!(first.accept());
        let second = book.claim().unwrap();

        assert!(!book.is_current(&first));
        assert!(book.is_current(&second));
    }

    #[rstest]
    fn reject_reaches_only_running_recovery() {
        let now = Instant::now();
        let mut book = synced(now);
        let unowned = book.reject(SendError::Closed);
        let recovery = book.claim().unwrap();

        let delivered = book.reject(SendError::Closed);

        assert!(!unowned);
        assert!(delivered);
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Rejected(SendError::Closed)
        ));
    }

    #[rstest]
    fn stale_skips_running_recovery_and_rearms() {
        let now = Instant::now();
        let threshold = Duration::from_secs(5);
        let mut book = synced(now);

        let fresh = book.stale(threshold, now + Duration::from_secs(5));
        let first = book.stale(threshold, now + Duration::from_secs(6));
        let rearmed = book.stale(threshold, now + Duration::from_secs(7));
        let _recovery = book.claim().unwrap();
        let owned = book.stale(threshold, now + Duration::from_secs(60));

        assert_eq!(fresh, None);
        assert_eq!(first, Some(Duration::from_secs(6)));
        assert_eq!(rearmed, None);
        assert_eq!(owned, None);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Subscribe { sending: bool },
        ConfirmSend,
        Snapshot,
        Advance,
        Gap,
        Claim,
        BeginReplacement,
        OpenRecoveryGate,
        Reject,
        CancelRecovery,
        Reconnect,
        Arm(u8),
        Expire,
        Tick(u8),
        Stale(u8),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            any::<bool>().prop_map(|sending| Op::Subscribe { sending }),
            Just(Op::ConfirmSend),
            Just(Op::Snapshot),
            Just(Op::Advance),
            Just(Op::Gap),
            Just(Op::Claim),
            Just(Op::BeginReplacement),
            Just(Op::OpenRecoveryGate),
            Just(Op::Reject),
            Just(Op::CancelRecovery),
            Just(Op::Reconnect),
            (1u8..30).prop_map(Op::Arm),
            Just(Op::Expire),
            (0u8..30).prop_map(Op::Tick),
            (1u8..30).prop_map(Op::Stale),
        ]
    }

    // Tracks what the contract says the book must do, independent of its internal fields
    #[derive(Debug, Default)]
    struct Model {
        send_gate: Option<SnapshotGate>,
        initial_wait: Option<CancellationToken>,
        deadline: Option<Instant>,
        recovery: Option<Arc<BookRecovery<SendError>>>,
        synced: bool,
    }

    impl Model {
        fn running(&self) -> Option<&Arc<BookRecovery<SendError>>> {
            self.recovery.as_ref().filter(|r| r.is_running())
        }

        fn sending(&self) -> bool {
            self.initial_wait.is_some()
                && self
                    .send_gate
                    .as_ref()
                    .is_some_and(|g| g.lock().is_closed())
        }

        fn clear_pending(&mut self) {
            if let Some(cancel) = self.initial_wait.take() {
                assert!(cancel.is_cancelled(), "dropped initial wait must cancel");
            }

            self.deadline = None;
        }
    }

    proptest! {
        #[rstest]
        fn schedule_keeps_sync_contract(ops in prop::collection::vec(op(), 1..80)) {
            let mut now = Instant::now();
            let mut book = Book::new(now);
            let mut model = Model::default();
            let mut position = 0u64;
            let mut last_accepted = now;

            for op in ops {
                match op {
                    Op::Subscribe { sending } => {
                        // A resubscribe replaces the book, cancelling the previous episode
                        let previous = model.recovery.take();
                        let gate = SnapshotGate::default();
                        if sending {
                            gate.lock().close();
                        }
                        book = Book::new(now);
                        let cancel = book.expect_snapshot(gate.clone());
                        model = Model {
                            send_gate: Some(gate),
                            initial_wait: Some(cancel),
                            ..Model::default()
                        };
                        last_accepted = now;

                        if let Some(previous) = previous {
                            prop_assert!(previous.cancellation.is_cancelled());
                        }
                        prop_assert_eq!(book.is_send_pending(), sending);
                    }
                    Op::ConfirmSend => {
                        if let Some(gate) = &model.send_gate {
                            gate.open();
                        }
                    }
                    Op::Snapshot => {
                        position += 1;
                        let blocked = model.sending()
                            || model.running().is_some_and(|r| r.gate.lock().is_closed());
                        let accepted = book.accept_snapshot(position, now);

                        prop_assert_eq!(accepted, !blocked);
                        if accepted {
                            model.clear_pending();
                            model.synced = true;
                            last_accepted = now;
                            prop_assert!(!book.has_pending_snapshot());
                            prop_assert!(model.running().is_none());
                        }
                    }
                    Op::Advance => {
                        position += 1;
                        let advanced = book.advance(position, now);

                        // Incremental output only while synced
                        prop_assert_eq!(advanced, model.synced);
                        if advanced {
                            last_accepted = now;
                        }
                    }
                    Op::Gap => {
                        let owned = model.running().is_some() || model.deadline.is_some();
                        let outcome = book.gap();
                        model.synced = false;

                        // An out-of-sync book always has an owner or asks for one
                        prop_assert_eq!(*book.phase(), BookPhase::Recovering);
                        prop_assert_eq!(outcome == BookSequenceOutcome::Recover, !owned);
                    }
                    Op::Claim => {
                        let expected = model.running().is_none();
                        let claimed = book.claim();

                        // At most one running owner, and claiming retires any pending wait
                        prop_assert_eq!(claimed.is_some(), expected);
                        if let Some(recovery) = claimed {
                            if let Some(previous) = model.recovery.replace(recovery) {
                                prop_assert!(previous.cancellation.is_cancelled());
                            }
                            model.clear_pending();
                            model.synced = false;
                            prop_assert!(!book.has_pending_snapshot());
                        }
                        prop_assert!(model.running().is_some_and(|recovery| book.is_current(recovery)));
                    }
                    Op::BeginReplacement => {
                        if let Some(recovery) = model.running() {
                            prop_assert!(recovery.begin_replacement());
                        }
                    }
                    Op::OpenRecoveryGate => {
                        if let Some(recovery) = model.running() {
                            recovery.gate.open();
                        }
                    }
                    Op::Reject => {
                        let delivered = book.reject(SendError::Closed);

                        prop_assert_eq!(delivered, model.running().is_some());
                    }
                    Op::CancelRecovery => {
                        if let Some(recovery) = model.running() {
                            recovery.cancellation.cancel();
                        }

                        // Rejection reaches only a running recovery, so nothing receives it
                        prop_assert!(!book.reject(SendError::Closed));
                    }
                    Op::Reconnect => {
                        let running = model.running().cloned();
                        let waiting = model.initial_wait.clone();
                        book.reset_on_reconnect();
                        model.synced = false;
                        model.deadline = None;

                        // Reconnect keeps a running recovery and an initial send in flight
                        prop_assert_eq!(*book.phase(), BookPhase::Recovering);
                        match &running {
                            Some(recovery) => prop_assert!(recovery.is_running() && book.is_current(recovery)),
                            None => prop_assert!(!book.reject(SendError::Closed)),
                        }

                        if let Some(waiting) = waiting {
                            prop_assert!(!waiting.is_cancelled());
                            prop_assert!(book.has_pending_snapshot());
                        }
                    }
                    Op::Arm(secs) => {
                        let deadline = now + Duration::from_secs(u64::from(secs));
                        let armed = book.arm_deadline(deadline);

                        // An initial send in flight keeps its wait
                        prop_assert_eq!(armed, model.initial_wait.is_none());
                        if armed {
                            model.deadline = Some(deadline);
                        }
                    }
                    Op::Expire => {
                        let expected = model.deadline.is_some_and(|d| d <= now);
                        let expired = book.take_expired(now);

                        prop_assert_eq!(expired, expected);
                        if expired {
                            model.deadline = None;
                        }
                    }
                    Op::Tick(secs) => {
                        now += Duration::from_secs(u64::from(secs));
                    }
                    Op::Stale(secs) => {
                        let threshold = Duration::from_secs(u64::from(secs));
                        let reported = book.stale(threshold, now);

                        // Every book without a running owner is reported once past the threshold
                        if model.running().is_some() {
                            prop_assert_eq!(reported, None);
                        } else {
                            let elapsed = now.duration_since(last_accepted);
                            prop_assert_eq!(reported, (elapsed > threshold).then_some(elapsed));
                            if reported.is_some() {
                                last_accepted = now;
                            }
                        }
                    }
                }
            }
        }
    }
}
