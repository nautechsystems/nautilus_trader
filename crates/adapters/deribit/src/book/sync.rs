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

//! Per-book synchronization state for Deribit order books.
//!
//! Raw delta channels (`book.{instrument}.{interval}`) share one `change_id` chain per
//! instrument, so they track by instrument name. Grouped channels
//! (`book.{instrument}.{group}.{depth}.{interval}`) stream standalone snapshots, so they track
//! by exact channel with delta and depth consumers sharing one entry.

use std::{fmt::Display, sync::Arc};

use ahash::{AHashMap, AHashSet};
use futures_util::stream::FuturesUnordered;
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_live::book::{
    BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind,
    snapshot::SnapshotGate,
    sync::{BookPhase, BookSync},
};
use nautilus_model::identifiers::InstrumentId;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::recovery::{BookRecovery, BookWork};
use crate::websocket::{enums::DeribitBookMsgType, error::DeribitWsError};

type Book = BookSync<DeribitWsError, u64>;

/// Key of one tracked Deribit book.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum BookKey {
    /// Raw delta channels of one instrument, sharing its `change_id` chain.
    Raw(Ustr),
    /// One grouped channel, streaming standalone snapshots.
    Grouped(String),
}

impl Display for BookKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Raw(instrument) => write!(f, "book.{instrument}"),
            Self::Grouped(channel) => write!(f, "{channel}"),
        }
    }
}

impl BookKey {
    /// Returns the tracked book for a channel, or `None` when it carries no book.
    #[must_use]
    pub(crate) fn for_channel(channel: &str) -> Option<Self> {
        if !channel.starts_with("book.") {
            return None;
        }

        if channel.split('.').count() == 5 {
            return Some(Self::Grouped(channel.to_owned()));
        }

        let instrument = channel.split('.').nth(1).filter(|s| !s.is_empty())?;
        Some(Self::Raw(Ustr::from(instrument)))
    }

    /// Decomposes the key into recovery command parts: the instrument name plus the grouped
    /// channel when the book tracks one exact channel.
    pub(crate) fn command_parts(&self) -> (Ustr, Option<String>) {
        match self {
            Self::Raw(instrument) => (*instrument, None),
            Self::Grouped(channel) => {
                let instrument = channel.split('.').nth(1).unwrap_or("");
                (Ustr::from(instrument), Some(channel.clone()))
            }
        }
    }
}

/// Outcome of a subscribe acknowledgement for a tracked book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubscribedOutcome {
    /// The initial subscription write completed.
    Initial,
    /// A recovery replacement completed its resubscribe.
    RecoveryComplete,
    /// The acknowledgement belongs to no tracked write.
    Ignored,
}

/// Outcome of an unsubscribe acknowledgement for a tracked book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnsubscribedOutcome {
    /// Every replacement channel is unsubscribed; resubscribe these.
    Resubscribe(Vec<String>),
    /// A running recovery lost its replacement; its attempt must fail so it retries.
    RejectRecovery,
    /// The acknowledgement belongs to no tracked replacement.
    Ignored,
}

/// Handler-owned synchronization state for Deribit books.
#[derive(Debug, Default)]
pub(crate) struct BookSyncTracker {
    books: AHashMap<BookKey, TrackedBook>,
    pub(crate) work: FuturesUnordered<BookWork>,
}

impl BookSyncTracker {
    /// Records a book subscription, keeping existing state across resubscribes.
    pub(crate) fn record_subscription(&mut self, key: BookKey, instrument_id: InstrumentId) {
        self.books.entry(key).or_insert_with_key(|_| {
            let now = Instant::now();
            let gate = SnapshotGate::default();
            gate.lock().close();
            let mut sync = Book::new(now);
            sync.expect_snapshot(gate.clone());

            TrackedBook {
                instrument_id,
                sync,
                write: TrackedWrite { gate },
                pending: None,
                subscribed_at: Some(now),
            }
        });
    }

    /// Removes a book, cancelling its recovery episode.
    pub(crate) fn remove(&mut self, key: &BookKey) {
        self.books.remove(key);
    }

    /// Returns the number of tracked books.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.books.len()
    }

    /// Returns whether no book is tracked.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    /// Returns whether a book is tracked.
    #[must_use]
    pub(crate) fn is_tracked(&self, key: &BookKey) -> bool {
        self.books.contains_key(key)
    }

    /// Validates a raw or grouped book frame before parsing.
    pub(crate) fn validate(
        &mut self,
        key: &BookKey,
        msg_type: DeribitBookMsgType,
        change_id: u64,
        prev_change_id: Option<u64>,
        now: Instant,
    ) -> BookSequenceOutcome {
        let Some(book) = self.books.get_mut(key) else {
            return BookSequenceOutcome::Suppress;
        };

        if msg_type == DeribitBookMsgType::Snapshot {
            return if book.sync.accept_snapshot(change_id, now) {
                BookSequenceOutcome::Accept
            } else {
                BookSequenceOutcome::Suppress
            };
        }

        let contiguous = book
            .sync
            .position()
            .is_some_and(|&last| prev_change_id == Some(last));

        if contiguous {
            book.sync.advance(change_id, now);
            return BookSequenceOutcome::Accept;
        }

        if !matches!(book.sync.phase(), BookPhase::Recovering) {
            match book.sync.position() {
                Some(&last) => log::warn!(
                    "Book sequence gap for {key}: expected prev_change_id={last}, \
                    was {prev_change_id:?} - dropping delta, requesting recovery"
                ),
                None => log::warn!(
                    "Book delta before snapshot for {key}: dropping delta, requesting recovery"
                ),
            }
        }

        book.sync.gap()
    }

    /// Marks the gap for an accepted frame that failed to parse, delivering the error to the
    /// running recovery.
    ///
    /// A frame the gate would drop never reaches the attempt: it is suppressed without
    /// marking a gap.
    pub(crate) fn reject_snapshot(
        &mut self,
        key: &BookKey,
        error: DeribitWsError,
    ) -> BookSequenceOutcome {
        let Some(book) = self.books.get_mut(key) else {
            return BookSequenceOutcome::Suppress;
        };

        if book.sync.is_send_pending() || book.write.gate.lock().is_closed() {
            return BookSequenceOutcome::Suppress;
        }

        let outcome = book.sync.gap();
        book.sync.reject(error);
        outcome
    }

    /// Marks the gap for a validated delta that failed to parse, delivering the error to the
    /// running recovery.
    ///
    /// Unlike snapshots, a delta reaches here only after validation accepted it, so its gap
    /// is always marked even while a replacement runs.
    pub(crate) fn reject_delta(
        &mut self,
        key: &BookKey,
        error: DeribitWsError,
    ) -> BookSequenceOutcome {
        let Some(book) = self.books.get_mut(key) else {
            return BookSequenceOutcome::Suppress;
        };

        let outcome = book.sync.gap();
        book.sync.reject(error);
        outcome
    }

    /// Claims a recovery episode, refusing while another is running.
    pub(crate) fn claim(&mut self, key: &BookKey) -> Option<Arc<BookRecovery>> {
        self.books.get_mut(key)?.sync.claim()
    }

    /// Starts a replacement write for a claimed episode.
    pub(crate) fn begin_recovery_write(
        &mut self,
        key: &BookKey,
        recovery: &Arc<BookRecovery>,
        gate: SnapshotGate,
        completion: tokio::sync::oneshot::Sender<Result<(), DeribitWsError>>,
        channels: Vec<String>,
    ) -> bool {
        let Some(book) = self.books.get_mut(key) else {
            return false;
        };

        if !book.sync.is_current(recovery) || !recovery.is_running() {
            return false;
        }

        gate.lock().close();
        book.write = TrackedWrite { gate };
        book.subscribed_at = None;

        // A replacement supersedes any reconnect wait; retire it so the
        // replacement's own gates own acceptance from here on
        if book.sync.is_send_pending() {
            book.sync.expect_snapshot(SnapshotGate::default()).cancel();
        }

        book.pending = Some(PendingReplacement {
            generation: CancellationToken::new(),
            completion,
            channels,
            unsubscribed: AHashSet::new(),
            resubscribed: false,
        });

        true
    }

    /// Fails the in-flight replacement write, if any, so its attempt retries.
    pub(crate) fn abort_recovery_write(&mut self, key: &BookKey, reason: DeribitWsError) {
        if let Some(book) = self.books.get_mut(key)
            && let Some(pending) = book.pending.take()
        {
            pending.generation.cancel();
            let _ = pending.completion.send(Err(reason));
        }
    }

    /// Confirms a subscribe acknowledgement for a book channel.
    pub(crate) fn subscribed(
        &mut self,
        key: &BookKey,
        timeout: Duration,
        now: Instant,
    ) -> SubscribedOutcome {
        let Some(book) = self.books.get_mut(key) else {
            return SubscribedOutcome::Ignored;
        };

        // Any acknowledgement resolves the initial wait the monitor bounds
        book.subscribed_at = None;

        if let Some(pending) = book.pending.as_ref() {
            if pending.generation.is_cancelled() || !pending.awaiting_subscribe() {
                return SubscribedOutcome::Ignored;
            }

            book.write.gate.open();
            let pending = book.pending.take().expect("pending checked above");
            let _ = pending.completion.send(Ok(()));
            return SubscribedOutcome::RecoveryComplete;
        }

        book.write.gate.open();

        // Retire the initial wait on first confirmation (fresh subscribe or
        // reconnect replay); re-subscribe acknowledgements re-arm while waiting
        if book.sync.is_send_pending() || matches!(book.sync.phase(), BookPhase::Waiting) {
            book.sync.expect_snapshot(SnapshotGate::default()).cancel();
            if !timeout.is_zero()
                && let Some(deadline) = now.checked_add(timeout)
            {
                book.sync.arm_deadline(deadline);
            }
        }

        SubscribedOutcome::Initial
    }

    /// Confirms an unsubscribe acknowledgement for a book channel.
    ///
    /// Without an in-flight replacement, the acknowledgement ends a write the running recovery
    /// lost (such as a reordered acknowledgement pair), so `lost_error` fails its attempt and
    /// the next attempt replaces the subscription again.
    pub(crate) fn unsubscribed(
        &mut self,
        key: &BookKey,
        channel: &str,
        lost_error: DeribitWsError,
    ) -> UnsubscribedOutcome {
        let Some(book) = self.books.get_mut(key) else {
            return UnsubscribedOutcome::Ignored;
        };

        if let Some(pending) = book.pending.as_mut() {
            if pending.generation.is_cancelled() || !pending.channels.iter().any(|c| c == channel) {
                return UnsubscribedOutcome::Ignored;
            }

            pending.unsubscribed.insert(channel.to_owned());
            if pending.awaiting_subscribe() && !pending.resubscribed {
                // The transition fires once: late or duplicate acknowledgements
                // from superseded attempts must not resubscribe again
                pending.resubscribed = true;
                return UnsubscribedOutcome::Resubscribe(pending.channels.clone());
            }

            return UnsubscribedOutcome::Ignored;
        }

        if book.sync.reject(lost_error) {
            return UnsubscribedOutcome::RejectRecovery;
        }

        UnsubscribedOutcome::Ignored
    }

    /// Collects stale-feed and missing-snapshot signals without claiming recoveries.
    pub(crate) fn recoveries(
        &mut self,
        stale_timeout: Duration,
        snapshot_timeout: Duration,
        now: Instant,
    ) -> Vec<(BookKey, BookSyncSignal)> {
        let mut recoveries = Vec::new();

        for (key, book) in &mut self.books {
            // The initial wait carries no shared deadline; bound it here so a missing
            // acknowledgement still owns the book
            if book.sync.is_send_pending() {
                let expired = !snapshot_timeout.is_zero()
                    && book.subscribed_at.is_some_and(|at| {
                        now.checked_duration_since(at)
                            .is_some_and(|waited| waited >= snapshot_timeout)
                    });

                if expired {
                    // One-shot like an expired deadline: a book that cannot
                    // start recovery must not warn on every monitor pass
                    book.subscribed_at = None;
                    recoveries.push((
                        key.clone(),
                        BookSyncSignal {
                            instrument_id: book.instrument_id,
                            kind: BookSyncSignalKind::SnapshotMissing,
                        },
                    ));
                } else if !stale_timeout.is_zero() {
                    // Starved while waiting: no ack and no frames since subscribing, so the
                    // baseline is the subscribe time; begun replacements clear it and skip this
                    if let Some(at) = book.subscribed_at
                        && let Some(elapsed) = now.checked_duration_since(at)
                        && elapsed > stale_timeout
                    {
                        book.subscribed_at = Some(now);
                        recoveries.push((
                            key.clone(),
                            BookSyncSignal {
                                instrument_id: book.instrument_id,
                                kind: BookSyncSignalKind::Stale { elapsed },
                            },
                        ));
                    }
                }

                continue;
            }

            let kind = if book.sync.take_expired(now) {
                Some(BookSyncSignalKind::SnapshotMissing)
            } else if !stale_timeout.is_zero() {
                book.sync
                    .stale(stale_timeout, now)
                    .map(|elapsed| BookSyncSignalKind::Stale { elapsed })
            } else {
                None
            };

            if let Some(kind) = kind {
                recoveries.push((
                    key.clone(),
                    BookSyncSignal {
                        instrument_id: book.instrument_id,
                        kind,
                    },
                ));
            }
        }

        recoveries.sort_by(|a, b| a.0.cmp(&b.0));
        recoveries
    }

    /// Returns whether the book still waits for its initial subscribe acknowledgement.
    pub(crate) fn has_unacked_subscription(&self, key: &BookKey) -> bool {
        self.books
            .get(key)
            .is_some_and(|book| book.sync.is_send_pending())
    }

    /// Restarts synchronization after the connection reconnects.
    ///
    /// Running recoveries keep their budget; their in-flight replacement writes fail so the
    /// next attempt uses the new connection. Every book waits for the reconnect replay to
    /// confirm it, bounded by the monitor like the initial wait.
    pub(crate) fn reset_on_reconnect(&mut self) {
        let now = Instant::now();

        for book in self.books.values_mut() {
            if let Some(pending) = book.pending.take() {
                pending.generation.cancel();
                let _ = pending.completion.send(Err(DeribitWsError::Transport(
                    "socket reconnected during book recovery".to_owned(),
                )));
            }

            book.sync.reset_on_reconnect();

            // Suppress snapshots until the replay confirms; the shared deadline
            // cannot arm during an initial wait, so the monitor bounds this one
            let gate = SnapshotGate::default();
            gate.lock().close();
            book.sync.expect_snapshot(gate);
            book.subscribed_at = Some(now);
        }
    }
}

#[derive(Debug)]
struct TrackedBook {
    instrument_id: InstrumentId,
    sync: Book,
    write: TrackedWrite,
    pending: Option<PendingReplacement>,
    subscribed_at: Option<Instant>,
}

#[derive(Debug)]
struct TrackedWrite {
    gate: SnapshotGate,
}

#[derive(Debug)]
struct PendingReplacement {
    generation: CancellationToken,
    completion: tokio::sync::oneshot::Sender<Result<(), DeribitWsError>>,
    channels: Vec<String>,
    unsubscribed: AHashSet<String>,
    resubscribed: bool,
}

impl PendingReplacement {
    fn awaiting_subscribe(&self) -> bool {
        !self.channels.is_empty() && self.channels.iter().all(|c| self.unsubscribed.contains(c))
    }
}

#[cfg(test)]
mod tests {
    use nautilus_live::book::recovery::BookRecoveryOutcome;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;

    const RAW_CHANNEL: &str = "book.BTC-PERPETUAL.raw";
    const GROUPED_CHANNEL: &str = "book.BTC-PERPETUAL.none.10.100ms";

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTC-PERPETUAL.DERIBIT")
    }

    fn raw_key() -> BookKey {
        BookKey::Raw(Ustr::from("BTC-PERPETUAL"))
    }

    fn grouped_key() -> BookKey {
        BookKey::Grouped(GROUPED_CHANNEL.to_owned())
    }

    fn subscribed_tracker() -> BookSyncTracker {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::Initial
        );
        tracker
    }

    fn accept_snapshot(tracker: &mut BookSyncTracker, key: &BookKey, change_id: u64) {
        assert_eq!(
            tracker.validate(
                key,
                DeribitBookMsgType::Snapshot,
                change_id,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn book_key_classifies_channels() {
        assert_eq!(BookKey::for_channel(RAW_CHANNEL), Some(raw_key()),);
        assert_eq!(
            BookKey::for_channel("book.BTC-PERPETUAL.100ms"),
            Some(raw_key()),
        );
        assert_eq!(BookKey::for_channel(GROUPED_CHANNEL), Some(grouped_key()),);
        assert_eq!(BookKey::for_channel("trades.BTC-PERPETUAL.raw"), None);
        assert_eq!(BookKey::for_channel("book."), None);
    }

    #[rstest]
    fn unsubscribed_book_suppresses_frames() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Snapshot,
                100,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                101,
                Some(100),
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );
    }

    #[rstest]
    fn snapshot_before_write_confirmation_is_suppressed() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Snapshot,
                100,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );
    }

    #[rstest]
    fn missing_acknowledgement_is_bounded_by_snapshot_timeout() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());

        // No acknowledgement arrives; the expired wait still owns the book
        let now = Instant::now() + Duration::from_secs(11);
        let recoveries = tracker.recoveries(Duration::ZERO, Duration::from_secs(10), now);

        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].0, key);
        assert_eq!(recoveries[0].1.kind, BookSyncSignalKind::SnapshotMissing);

        // ... unless the snapshot timer is disabled
        let mut tracker = BookSyncTracker::default();
        tracker.record_subscription(key, instrument_id());
        assert!(
            tracker
                .recoveries(Duration::ZERO, Duration::ZERO, now)
                .is_empty()
        );

        // The bound reports once: a book that cannot start recovery stays quiet
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key, instrument_id());
        let later = Instant::now() + Duration::from_secs(12);
        assert_eq!(
            tracker
                .recoveries(Duration::ZERO, Duration::from_secs(10), later)
                .len(),
            1
        );
        assert!(
            tracker
                .recoveries(
                    Duration::ZERO,
                    Duration::from_secs(10),
                    later + Duration::from_secs(1)
                )
                .is_empty()
        );
    }

    #[rstest]
    fn unacked_subscription_is_bounded_by_stale_threshold() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());

        // A recent subscribe reports nothing even with the snapshot timer off
        let now = Instant::now();
        assert!(
            tracker
                .recoveries(Duration::from_secs(120), Duration::ZERO, now)
                .is_empty()
        );

        // No acknowledgement and no frames for the stale threshold owns the book
        let later = now + Duration::from_secs(121);
        let recoveries = tracker.recoveries(Duration::from_secs(120), Duration::ZERO, later);
        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].0, key);
        assert!(matches!(
            recoveries[0].1.kind,
            BookSyncSignalKind::Stale { .. }
        ));

        // The stale report re-arms like the shared check: quiet until the next window
        assert!(
            tracker
                .recoveries(
                    Duration::from_secs(120),
                    Duration::ZERO,
                    later + Duration::from_secs(1)
                )
                .is_empty()
        );
        assert_eq!(
            tracker
                .recoveries(
                    Duration::from_secs(120),
                    Duration::ZERO,
                    later + Duration::from_secs(121)
                )
                .len(),
            1
        );
    }

    #[rstest]
    fn unacked_subscription_skips_stale_while_replacement_runs() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());

        // Beginning a replacement clears the subscribe baseline it owns
        let recovery = tracker.claim(&key).expect("episode");
        let gate = recovery.gate.clone();
        let (completion, _rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            gate,
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        let later = Instant::now() + Duration::from_secs(121);
        assert!(
            tracker
                .recoveries(Duration::from_secs(120), Duration::ZERO, later)
                .is_empty()
        );
    }

    #[rstest]
    fn rejected_frame_with_closed_gate_never_reaches_attempt() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        // A begun replacement closes the gate until the resubscribe confirms
        let recovery = tracker.claim(&key).expect("episode");
        let gate = recovery.gate.clone();
        let (completion, _rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            gate,
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        assert_eq!(
            tracker.reject_snapshot(&key, DeribitWsError::Transport("corrupt".into())),
            BookSequenceOutcome::Suppress
        );
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
    }

    #[rstest]
    fn rejected_delta_with_closed_gate_still_marks_gap() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        let recovery = tracker.claim(&key).expect("episode");
        let gate = recovery.gate.clone();
        let (completion, _rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            gate,
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        // A validated delta always delivers its error, even with the gate closed
        tracker.reject_delta(&key, DeribitWsError::Transport("corrupt".into()));
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Rejected(_)
        ));
    }

    #[rstest]
    fn replacement_beginning_after_reconnect_retires_replay_wait() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        // A recovery is running when the socket reconnects
        let recovery = tracker.claim(&key).expect("episode");
        tracker.reset_on_reconnect();

        // The retry begins before the replay acknowledgement arrives
        let gate = recovery.gate.clone();
        let (completion, _rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            gate,
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        // The replay acknowledgement arrives while the replacement still waits
        // for its unsubscribe acks, so it is ignored without retiring anything
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::Ignored
        );

        // The replacement then runs its own ack flow to completion
        assert_eq!(
            tracker.unsubscribed(&key, RAW_CHANNEL, DeribitWsError::Transport("lost".into())),
            UnsubscribedOutcome::Resubscribe(vec![RAW_CHANNEL.to_owned()])
        );
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::RecoveryComplete
        );

        // Snapshots flow again instead of suppressing forever on the retired wait
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Snapshot,
                200,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn reconnect_suppresses_snapshots_until_replay_confirms() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        tracker.reset_on_reconnect();

        // A snapshot from the old connection cannot start an emitted chain
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Snapshot,
                200,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );

        // The replay acknowledgement re-arms and re-opens the book
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::Initial
        );
        accept_snapshot(&mut tracker, &key, 200);
    }

    #[rstest]
    fn first_delta_without_snapshot_requests_recovery() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());
        assert_eq!(
            tracker.subscribed(&key, Duration::ZERO, Instant::now()),
            SubscribedOutcome::Initial
        );

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                101,
                Some(100),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );
    }

    #[rstest]
    fn first_delta_with_armed_deadline_is_suppressed() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                101,
                Some(100),
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );
    }

    #[rstest]
    fn contiguous_delta_advances_synced_book() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                101,
                Some(100),
                Instant::now()
            ),
            BookSequenceOutcome::Accept
        );
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                102,
                Some(100),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );
    }

    #[rstest]
    fn change_without_prev_change_id_requests_recovery() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        assert_eq!(
            tracker.validate(&key, DeribitBookMsgType::Change, 101, None, Instant::now()),
            BookSequenceOutcome::Recover
        );
    }

    #[rstest]
    fn gap_while_recovering_suppresses_once_owned() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                105,
                Some(104),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );

        // Unowned books ask again on the next incremental
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                106,
                Some(105),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );

        tracker.claim(&key).expect("claim admits one episode");

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                107,
                Some(106),
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );
    }

    #[rstest]
    fn grouped_book_tracks_by_channel() {
        let mut tracker = BookSyncTracker::default();
        let key = grouped_key();
        tracker.record_subscription(key.clone(), instrument_id());
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::Initial
        );
        accept_snapshot(&mut tracker, &key, 999);

        // Another grouped channel for the same instrument tracks independently
        let other = BookKey::Grouped("book.BTC-PERPETUAL.1.10.100ms".to_owned());
        assert_eq!(
            tracker.validate(
                &other,
                DeribitBookMsgType::Snapshot,
                999,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );
    }

    #[rstest]
    fn replacement_ack_flow_completes() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                105,
                Some(104),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );

        let recovery = tracker.claim(&key).expect("claim admits one episode");
        let gate = recovery.gate.clone();
        let (completion, mut rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            gate,
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        // Snapshots stay gated until the venue confirms the resubscribe
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Snapshot,
                200,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Suppress
        );

        assert_eq!(
            tracker.unsubscribed(&key, RAW_CHANNEL, DeribitWsError::Transport("lost".into())),
            UnsubscribedOutcome::Resubscribe(vec![RAW_CHANNEL.to_owned()])
        );
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::RecoveryComplete
        );
        assert!(rx.try_recv().unwrap().is_ok());

        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Snapshot,
                200,
                None,
                Instant::now()
            ),
            BookSequenceOutcome::Accept
        );
        assert!(recovery.is_accepted());
    }

    #[rstest]
    fn superseded_replacement_ack_is_ignored() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                105,
                Some(104),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );

        let recovery = tracker.claim(&key).expect("claim admits one episode");
        let (first_tx, mut first_rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            recovery.gate.clone(),
            first_tx,
            vec![RAW_CHANNEL.to_owned()]
        ));
        let (second_tx, mut second_rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            recovery.gate.clone(),
            second_tx,
            vec![RAW_CHANNEL.to_owned()]
        ));

        // The first attempt's write is abandoned
        assert!(first_rx.try_recv().is_err());

        assert_eq!(
            tracker.unsubscribed(&key, RAW_CHANNEL, DeribitWsError::Transport("lost".into())),
            UnsubscribedOutcome::Resubscribe(vec![RAW_CHANNEL.to_owned()])
        );
        assert_eq!(
            tracker.subscribed(&key, Duration::from_secs(10), Instant::now()),
            SubscribedOutcome::RecoveryComplete
        );
        assert!(second_rx.try_recv().unwrap().is_ok());
    }

    #[rstest]
    fn duplicate_unsubscribe_ack_resubscribes_once() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        let recovery = tracker.claim(&key).expect("episode");
        let (completion, _rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            recovery.gate.clone(),
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        // A late or duplicate acknowledgement must not resubscribe again
        assert_eq!(
            tracker.unsubscribed(&key, RAW_CHANNEL, DeribitWsError::Transport("lost".into())),
            UnsubscribedOutcome::Resubscribe(vec![RAW_CHANNEL.to_owned()])
        );
        assert_eq!(
            tracker.unsubscribed(&key, RAW_CHANNEL, DeribitWsError::Transport("lost".into())),
            UnsubscribedOutcome::Ignored
        );
    }

    #[rstest]
    fn remove_cancels_running_recovery() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                105,
                Some(104),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );

        let recovery = tracker.claim(&key).expect("claim admits one episode");
        assert!(recovery.is_running());

        tracker.remove(&key);

        assert!(!recovery.is_running());
        assert!(tracker.is_empty());
    }

    #[rstest]
    fn reset_on_reconnect_keeps_running_recovery_and_fails_its_write() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);
        assert_eq!(
            tracker.validate(
                &key,
                DeribitBookMsgType::Change,
                105,
                Some(104),
                Instant::now()
            ),
            BookSequenceOutcome::Recover
        );

        let recovery = tracker.claim(&key).expect("claim admits one episode");
        let (completion, mut rx) = tokio::sync::oneshot::channel();
        assert!(tracker.begin_recovery_write(
            &key,
            &recovery,
            recovery.gate.clone(),
            completion,
            vec![RAW_CHANNEL.to_owned()]
        ));

        tracker.reset_on_reconnect();

        assert!(recovery.is_running());
        assert!(rx.try_recv().unwrap().is_err());
        // A second claim is refused while the kept episode runs
        assert!(tracker.claim(&key).is_none());
    }

    #[rstest]
    fn reset_on_reconnect_arms_deadline_for_idle_book() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        tracker.reset_on_reconnect();

        let now = Instant::now() + Duration::from_secs(11);
        let recoveries = tracker.recoveries(Duration::from_secs(120), Duration::from_secs(10), now);
        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].0, key);
        assert_eq!(recoveries[0].1.kind, BookSyncSignalKind::SnapshotMissing);
    }

    #[rstest]
    fn zero_timeout_disables_deadlines() {
        let mut tracker = BookSyncTracker::default();
        let key = raw_key();
        tracker.record_subscription(key.clone(), instrument_id());
        assert_eq!(
            tracker.subscribed(&key, Duration::ZERO, Instant::now()),
            SubscribedOutcome::Initial
        );

        let now = Instant::now() + Duration::from_secs(3600);
        assert!(
            tracker
                .recoveries(Duration::ZERO, Duration::ZERO, now)
                .is_empty()
        );

        tracker.reset_on_reconnect();
        assert!(
            tracker
                .recoveries(Duration::ZERO, Duration::ZERO, now)
                .is_empty()
        );
    }

    #[rstest]
    fn stale_book_reports_once_per_window() {
        let mut tracker = subscribed_tracker();
        let key = raw_key();
        accept_snapshot(&mut tracker, &key, 100);

        let now = Instant::now() + Duration::from_secs(121);
        let recoveries = tracker.recoveries(Duration::from_secs(120), Duration::from_secs(10), now);
        assert_eq!(recoveries.len(), 1);
        assert!(matches!(
            recoveries[0].1.kind,
            BookSyncSignalKind::Stale { .. }
        ));

        // Re-armed: no second report within the window
        let recoveries = tracker.recoveries(
            Duration::from_secs(120),
            Duration::from_secs(10),
            now + Duration::from_secs(120),
        );
        assert!(recoveries.is_empty());
    }
}
