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

//! Each wire channel owns one shared book state and one ingress generation.
//! Full snapshots advance only when their timestamp increases; publish IDs can reset on resubscribe.

use std::sync::Arc;

use ahash::AHashMap;
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_live::book::{
    BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind,
    snapshot::SnapshotGate,
    sync::{BookPhase, BookSync},
};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::recovery::BookRecovery;
use crate::websocket::DeriveWsError;

type Book = BookSync<Arc<DeriveWsError>, i64>;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    books: Arc<Mutex<AHashMap<String, TrackedBook>>>,
}

impl BookSyncTracker {
    pub(crate) fn record_subscription(&self, channel: &str, instrument_id: InstrumentId) {
        let mut books = self.books.lock();
        books.entry(channel.to_owned()).or_insert_with(|| {
            let gate = SnapshotGate::default();
            gate.lock().close();
            let mut sync = Book::new(Instant::now());
            sync.expect_snapshot(gate.clone());

            TrackedBook {
                instrument_id,
                sync,
                timestamp: None,
                write: BookWrite {
                    gate,
                    generation: CancellationToken::new(),
                },
            }
        });
    }

    pub(crate) fn remove(&self, channel: &str) {
        self.books.lock().remove(channel);
    }

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
    }

    pub(crate) fn write(&self, channel: &str) -> Option<BookWrite> {
        self.books
            .lock()
            .get(channel)
            .map(|book| book.write.clone())
    }

    pub(crate) fn confirm_subscription(&self, write: &BookWrite, pending: impl FnOnce() -> bool) {
        // Recovery attempts share a gate, so admission must serialize with generation retirement
        let _guard = self.books.lock();

        if !write.generation.is_cancelled() && pending() {
            write.gate.open();
        }
    }

    pub(crate) fn subscription_written(&self, channel: &str, timeout: Duration) {
        let mut books = self.books.lock();

        let Some(book) = books.get_mut(channel) else {
            return;
        };

        if matches!(book.sync.phase(), BookPhase::Waiting)
            && !book.write.gate.lock().is_closed()
            && !timeout.is_zero()
        {
            book.arm_deadline(Instant::now() + timeout);
        }
    }

    pub(crate) fn record_snapshot(
        &self,
        channel: &str,
        timestamp: i64,
        generation: Option<&CancellationToken>,
        now: Instant,
    ) -> bool {
        let mut books = self.books.lock();

        let Some(book) = books.get_mut(channel) else {
            return false;
        };

        if generation.is_some_and(CancellationToken::is_cancelled)
            || book.write.gate.lock().is_closed()
        {
            return false;
        }

        if book.timestamp.is_some_and(|last| timestamp <= last) || timestamp < 0 {
            return false;
        }

        if !book.sync.accept_snapshot(timestamp, now) {
            return false;
        }

        book.timestamp = Some(timestamp);
        true
    }

    pub(crate) fn reject_snapshot(
        &self,
        channel: &str,
        error: Arc<DeriveWsError>,
        generation: Option<&CancellationToken>,
    ) {
        let mut books = self.books.lock();

        let Some(book) = books.get_mut(channel) else {
            return;
        };

        if generation.is_some_and(CancellationToken::is_cancelled)
            || book.write.gate.lock().is_closed()
        {
            return;
        }

        if book.sync.gap() == BookSequenceOutcome::Recover {
            book.arm_deadline(Instant::now());
        }

        book.sync.reject(error);
    }

    pub(crate) fn reset_on_reconnect(&self, timeout: Duration) {
        for book in self.books.lock().values_mut() {
            book.write.generation.cancel();
            book.write.generation = CancellationToken::new();
            book.write.gate.lock().close();
            book.sync.reset_on_reconnect();
            if !timeout.is_zero() {
                book.arm_deadline(Instant::now() + timeout);
            }
        }
    }

    pub(crate) fn begin_recovery_write(
        &self,
        channel: &str,
        recovery: &Arc<BookRecovery>,
        gate: SnapshotGate,
    ) -> Option<BookWrite> {
        let mut books = self.books.lock();
        let book = books.get_mut(channel)?;
        if !book.sync.is_current(recovery) || !recovery.is_running() {
            return None;
        }

        gate.lock().close();
        book.write.generation.cancel();

        book.write = BookWrite {
            gate,
            generation: CancellationToken::new(),
        };

        Some(book.write.clone())
    }

    pub(crate) fn recoveries(
        &self,
        stale_timeout: Duration,
        now: Instant,
    ) -> Vec<(String, BookSyncSignal, Arc<BookRecovery>)> {
        let mut books = self.books.lock();
        let mut recoveries = Vec::new();

        for (channel, book) in books.iter_mut() {
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
                if book.sync.is_send_pending() {
                    continue;
                }

                if let Some(recovery) = book.sync.claim() {
                    recoveries.push((
                        channel.clone(),
                        BookSyncSignal {
                            instrument_id: book.instrument_id,
                            kind,
                        },
                        recovery,
                    ));
                }
            }
        }

        recoveries.sort_by(|a, b| a.0.cmp(&b.0));
        recoveries
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BookWrite {
    pub(crate) gate: SnapshotGate,
    pub(crate) generation: CancellationToken,
}

#[derive(Debug)]
struct TrackedBook {
    instrument_id: InstrumentId,
    sync: Book,
    timestamp: Option<i64>,
    write: BookWrite,
}

impl TrackedBook {
    fn arm_deadline(&mut self, deadline: Instant) {
        // BookSync refuses deadlines during an initial wait, which this write no longer needs
        self.sync.expect_snapshot(SnapshotGate::default()).cancel();
        self.sync.arm_deadline(deadline);
    }
}

impl Drop for TrackedBook {
    fn drop(&mut self) {
        self.write.generation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use nautilus_live::book::recovery::BookRecoveryOutcome;
    use rstest::rstest;

    use super::*;

    const CHANNEL: &str = "orderbook.ETH-PERP.1.10";

    fn tracker() -> BookSyncTracker {
        let tracker = BookSyncTracker::default();
        tracker.record_subscription(CHANNEL, InstrumentId::from("ETH-PERP.DERIVE"));
        tracker
    }

    #[rstest]
    fn retired_ack_cannot_reopen_replacement_gate() {
        let tracker = tracker();
        let now = Instant::now();
        tracker.write(CHANNEL).unwrap().gate.open();
        assert!(tracker.record_snapshot(CHANNEL, 100, None, now));
        let recovery = tracker
            .recoveries(Duration::from_secs(1), now + Duration::from_secs(2))
            .pop()
            .unwrap()
            .2;
        let old = tracker
            .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
            .unwrap();
        let (checked_tx, checked_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();

        let ack = {
            let tracker = tracker.clone();
            let old = old.clone();
            std::thread::spawn(move || {
                tracker.confirm_subscription(&old, || {
                    checked_tx.send(()).unwrap();
                    resume_rx.recv().unwrap();
                    true
                });
            })
        };

        checked_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let can_retire = tracker.books.try_lock().is_some();
        let replacement = can_retire.then(|| {
            tracker
                .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
                .unwrap()
        });

        resume_tx.send(()).unwrap();
        ack.join().unwrap();

        let replacement = replacement.unwrap_or_else(|| {
            tracker
                .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
                .unwrap()
        });

        assert!(old.generation.is_cancelled());
        assert!(!replacement.generation.is_cancelled());
        assert!(replacement.gate.lock().is_closed());
        assert!(!tracker.record_snapshot(CHANNEL, 101, Some(&replacement.generation), now));

        tracker.confirm_subscription(&replacement, || true);

        assert!(tracker.record_snapshot(CHANNEL, 101, Some(&replacement.generation), now));
        assert!(recovery.is_accepted());
    }

    #[rstest]
    fn initial_gate_and_missing_snapshot_deadline() {
        let tracker = tracker();
        let now = Instant::now();
        assert!(!tracker.record_snapshot(CHANNEL, 100, None, now));
        tracker.write(CHANNEL).unwrap().gate.open();
        tracker.subscription_written(CHANNEL, Duration::from_secs(1));
        let recoveries = tracker.recoveries(Duration::ZERO, now + Duration::from_secs(2));
        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].1.kind, BookSyncSignalKind::SnapshotMissing);
        assert!(recoveries[0].2.is_running());
        assert!(
            tracker
                .recoveries(Duration::ZERO, now + Duration::from_secs(3))
                .is_empty()
        );
    }

    #[rstest]
    #[case::enabled(Duration::from_secs(1), 1)]
    #[case::disabled(Duration::ZERO, 0)]
    fn reconnect_before_initial_deadline_keeps_timeout_policy(
        #[case] timeout: Duration,
        #[case] expected_recoveries: usize,
    ) {
        let tracker = tracker();
        let now = Instant::now();
        let initial = tracker.write(CHANNEL).unwrap();
        tracker.confirm_subscription(&initial, || true);

        tracker.reset_on_reconnect(timeout);
        tracker.subscription_written(CHANNEL, timeout);
        let replay = tracker.write(CHANNEL).unwrap();
        tracker.confirm_subscription(&replay, || true);
        let recoveries = tracker.recoveries(Duration::ZERO, now + Duration::from_secs(2));

        assert!(initial.generation.is_cancelled());
        assert!(!replay.generation.is_cancelled());
        assert!(!replay.gate.lock().is_closed());
        assert_eq!(recoveries.len(), expected_recoveries);

        if let Some((channel, signal, recovery)) = recoveries.first() {
            assert_eq!(channel, CHANNEL);
            assert_eq!(signal.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
            assert_eq!(signal.kind, BookSyncSignalKind::SnapshotMissing);
            assert!(recovery.is_running());
        }
    }

    #[rstest]
    fn retired_snapshot_cannot_reject_current_recovery() {
        let tracker = tracker();
        let now = Instant::now();
        let initial = tracker.write(CHANNEL).unwrap();
        tracker.confirm_subscription(&initial, || true);
        assert!(tracker.record_snapshot(CHANNEL, 100, Some(&initial.generation), now));
        let recovery = tracker
            .recoveries(Duration::from_secs(1), now + Duration::from_secs(2))
            .pop()
            .unwrap()
            .2;
        let replacement = tracker
            .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
            .unwrap();
        tracker.confirm_subscription(&replacement, || true);

        tracker.reject_snapshot(
            CHANNEL,
            Arc::new(DeriveWsError::NotConnected),
            Some(&initial.generation),
        );

        assert!(initial.generation.is_cancelled());
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        assert!(tracker.record_snapshot(CHANNEL, 101, Some(&replacement.generation), now));
        assert!(recovery.is_accepted());
    }

    #[rstest]
    fn invalid_initial_snapshot_keeps_immediate_deadline_after_subscribe_returns() {
        let tracker = tracker();
        let initial = tracker.write(CHANNEL).unwrap();
        tracker.confirm_subscription(&initial, || true);
        tracker.reject_snapshot(
            CHANNEL,
            Arc::new(DeriveWsError::NotConnected),
            Some(&initial.generation),
        );

        tracker.subscription_written(CHANNEL, Duration::from_secs(10));
        let recoveries = tracker.recoveries(Duration::ZERO, Instant::now());

        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].1.kind, BookSyncSignalKind::SnapshotMissing);
        assert!(recoveries[0].2.is_running());
    }

    #[rstest]
    fn duplicate_and_obsolete_snapshots_do_not_mask_staleness() {
        let tracker = tracker();
        let now = Instant::now();
        tracker.write(CHANNEL).unwrap().gate.open();
        assert!(tracker.record_snapshot(CHANNEL, 100, None, now));
        assert!(!tracker.record_snapshot(CHANNEL, 99, None, now + Duration::from_secs(1)));
        assert!(!tracker.record_snapshot(CHANNEL, 100, None, now + Duration::from_secs(2)));
        let recoveries = tracker.recoveries(Duration::from_secs(1), now + Duration::from_secs(3));
        assert_eq!(recoveries.len(), 1);
        assert_eq!(
            recoveries[0].1.kind,
            BookSyncSignalKind::Stale {
                elapsed: Duration::from_secs(3)
            }
        );
    }

    #[rstest]
    fn reconnect_preserves_running_recovery_and_obsoletes_queued_frames() {
        let tracker = tracker();
        let now = Instant::now();
        let initial = tracker.write(CHANNEL).unwrap();
        initial.gate.open();
        assert!(tracker.record_snapshot(CHANNEL, 100, Some(&initial.generation), now));
        let recovery = tracker
            .recoveries(Duration::from_secs(1), now + Duration::from_secs(2))
            .pop()
            .unwrap()
            .2;
        let write = tracker
            .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
            .unwrap();
        assert!(initial.generation.is_cancelled());
        assert!(!tracker.record_snapshot(CHANNEL, 101, Some(&initial.generation), now));
        tracker.reset_on_reconnect(Duration::from_secs(1));
        assert!(write.generation.is_cancelled());
        assert!(recovery.is_running());
        assert!(
            tracker
                .recoveries(Duration::ZERO, now + Duration::from_secs(5))
                .is_empty()
        );
        let current = tracker.write(CHANNEL).unwrap();
        current.gate.open();
        assert!(!tracker.record_snapshot(CHANNEL, 100, Some(&current.generation), now));
        assert!(tracker.record_snapshot(CHANNEL, 102, Some(&current.generation), now));
        assert!(recovery.is_accepted());
    }

    #[rstest]
    fn shared_channel_registration_and_separate_channels_keep_ownership() {
        let tracker = tracker();
        let original = tracker.write(CHANNEL).unwrap();
        tracker.record_subscription(CHANNEL, InstrumentId::from("ETH-PERP.DERIVE"));
        let other = "orderbook.ETH-PERP.10.10";
        tracker.record_subscription(other, InstrumentId::from("ETH-PERP.DERIVE"));
        original.gate.open();
        assert!(!original.generation.is_cancelled());
        assert!(tracker.record_snapshot(CHANNEL, 100, Some(&original.generation), Instant::now()));
        tracker.remove(other);
        assert!(!original.generation.is_cancelled());
        tracker.clear();
        assert!(original.generation.is_cancelled());
        assert!(!tracker.record_snapshot(CHANNEL, 101, None, Instant::now()));
    }

    #[rstest]
    fn zero_timeouts_disable_deadlines_and_staleness() {
        let tracker = tracker();
        tracker.write(CHANNEL).unwrap().gate.open();
        tracker.subscription_written(CHANNEL, Duration::ZERO);
        assert!(
            tracker
                .recoveries(Duration::ZERO, Instant::now() + Duration::from_secs(3600))
                .is_empty()
        );
        let now = Instant::now();
        assert!(tracker.record_snapshot(CHANNEL, 100, None, now));
        tracker.reset_on_reconnect(Duration::ZERO);
        tracker.write(CHANNEL).unwrap().gate.open();
        tracker.subscription_written(CHANNEL, Duration::ZERO);
        assert!(
            tracker
                .recoveries(Duration::ZERO, now + Duration::from_secs(7200))
                .is_empty()
        );
        assert!(tracker.record_snapshot(CHANNEL, 101, None, now));
    }

    #[rstest]
    #[case::unsubscribe(false)]
    #[case::shutdown(true)]
    fn removal_cancels_pending_recovery_and_frames(#[case] shutdown: bool) {
        let tracker = tracker();
        let now = Instant::now();
        tracker.write(CHANNEL).unwrap().gate.open();
        assert!(tracker.record_snapshot(CHANNEL, 100, None, now));
        let recovery = tracker
            .recoveries(Duration::from_secs(1), now + Duration::from_secs(2))
            .pop()
            .unwrap()
            .2;
        let write = tracker
            .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
            .unwrap();

        if shutdown {
            tracker.clear();
        } else {
            tracker.remove(CHANNEL);
        }

        write.gate.open();

        assert!(recovery.cancellation.is_cancelled());
        assert!(write.generation.is_cancelled());
        assert!(tracker.write(CHANNEL).is_none());
        assert!(
            tracker
                .begin_recovery_write(CHANNEL, &recovery, recovery.gate.clone())
                .is_none()
        );
        assert!(!tracker.record_snapshot(CHANNEL, 101, Some(&write.generation), now));
        assert!(tracker.recoveries(Duration::ZERO, now).is_empty());
    }

    #[rstest]
    fn valid_full_snapshot_heals_rejection_before_monitor_claim() {
        let tracker = tracker();
        let now = Instant::now();
        tracker.write(CHANNEL).unwrap().gate.open();
        assert!(tracker.record_snapshot(CHANNEL, 100, None, now));

        tracker.reject_snapshot(CHANNEL, Arc::new(DeriveWsError::NotConnected), None);
        let accepted = tracker.record_snapshot(CHANNEL, 101, None, now);
        let recoveries = tracker.recoveries(Duration::ZERO, now + Duration::from_secs(1));

        assert!(accepted);
        assert!(recoveries.is_empty());
    }

    #[rstest]
    fn invalid_snapshot_recovers_with_timeouts_disabled() {
        let tracker = tracker();
        tracker.write(CHANNEL).unwrap().gate.open();
        tracker.subscription_written(CHANNEL, Duration::ZERO);

        tracker.reject_snapshot(CHANNEL, Arc::new(DeriveWsError::NotConnected), None);
        let recoveries = tracker.recoveries(Duration::ZERO, Instant::now());

        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].1.kind, BookSyncSignalKind::SnapshotMissing);
        assert!(recoveries[0].2.is_running());
        assert!(
            tracker
                .recoveries(Duration::ZERO, Instant::now())
                .is_empty()
        );
    }
}
