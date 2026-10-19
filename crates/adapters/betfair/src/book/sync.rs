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

//! Adapter-local order book synchronization state for Betfair.
//!
//! [`BookSyncTracker`] applies Betfair market images to the shared per-book [`BookSync`]
//! lifecycle and decides whether to accept a market change's book data, suppress it, or request
//! recovery. Betfair images whole markets, so the tracker keeps one book per market ID and an
//! image replaces the books of every runner in it. Market changes carry no per-market sequence, so
//! a book has no venue position.
//!
//! A market change with `img` set replaces its market, as the venue requires: Betfair sends one
//! for every market in a subscription image, for a market that a resubscription cannot patch, and
//! on rare occasions during normal streaming. Any other change for an unsynced market is a gap, as
//! is a change the adapter cannot parse; an unparsable image also fails the recovery waiting on it.
//! A subscription image replaces every market, so every book falls out of sync when one starts and
//! resyncs from its own image; a book the image omits recovers on its next change.
//!
//! Betfair permits one market subscription per connection, and each subscription message replaces
//! the last and images every market in it. The tracker records when a written subscription's image
//! was requested, until that image starts. A recovery joins a request younger than its snapshot
//! timeout rather than writing another subscription that would discard the pending image, so
//! markets that need recovery together share one image; an older request's image is presumed
//! lost. Requests are written one at a time, and the stream handler only clears the record, so it
//! never waits on a request.
//!
//! A reconnect replays the subscription with its last clocks, so Betfair patches synced books in
//! place with `RESUB_DELTA` changes. The tracker therefore restarts only unsynced books on
//! reconnect, which wakes a recovery waiting between attempts after its budget.
//!
//! Claiming, accepting, and resetting recovery stay under the tracker's state lock so competing
//! events cannot independently change ownership. This module performs state transitions;
//! [`super::recovery`] runs the asynchronous subscription and retry work.

use std::sync::Arc;

use ahash::AHashMap;
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_live::book::{snapshot::SnapshotGate, sync::BookSync};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{BookSequenceOutcome, recovery::BookRecovery};
use crate::stream::error::BetfairStreamError;

type Book = BookSync<BetfairStreamError>;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    books: Arc<Mutex<AHashMap<String, Book>>>,
    image_requested: Arc<Mutex<Option<Instant>>>,
    image_requesting: Arc<Mutex<()>>,
}

impl BookSyncTracker {
    pub(crate) fn record_subscription(
        &self,
        market_id: &str,
        now: Instant,
        gate: SnapshotGate,
    ) -> CancellationToken {
        let mut book = Book::new(now);
        let cancel = book.expect_snapshot(gate);
        self.books.lock().insert(market_id.to_string(), book);
        cancel
    }

    pub(crate) fn remove(&self, market_id: &str) {
        self.books.lock().remove(market_id);
    }

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
        self.clear_image_request();
    }

    pub(crate) fn validate(
        &self,
        market_id: &str,
        is_image: bool,
        now: Instant,
    ) -> BookSequenceOutcome {
        let mut books = self.books.lock();

        let Some(book) = books.get_mut(market_id) else {
            return BookSequenceOutcome::Suppress;
        };

        if book.is_send_pending() {
            return BookSequenceOutcome::Suppress;
        }

        if is_image {
            return if book.accept_snapshot((), now) {
                BookSequenceOutcome::Accept
            } else {
                BookSequenceOutcome::Suppress
            };
        }

        if book.advance((), now) {
            return BookSequenceOutcome::Accept;
        }

        let outcome = book.gap();

        if outcome == BookSequenceOutcome::Recover {
            log::warn!(
                "Market change for {market_id} arrived before its market image; \
                 requesting a fresh image"
            );
        }

        outcome
    }

    pub(crate) fn reject_change(
        &self,
        market_id: &str,
        is_image: bool,
        error: BetfairStreamError,
    ) -> BookSequenceOutcome {
        let mut books = self.books.lock();

        let Some(book) = books.get_mut(market_id) else {
            return BookSequenceOutcome::Suppress;
        };

        if book.is_send_pending() {
            return BookSequenceOutcome::Suppress;
        }

        let outcome = book.gap();

        if is_image {
            book.reject(error);
        }

        outcome
    }

    pub(crate) fn begin_image(&self) {
        self.clear_image_request();

        for book in self.books.lock().values_mut() {
            book.gap();
        }
    }

    pub(crate) fn reset_on_reconnect(&self) {
        for book in self.books.lock().values_mut() {
            if book.position().is_none() {
                book.reset_on_reconnect();
            }
        }
    }

    pub(crate) fn claim_subscription_recovery(
        &self,
        market_id: &str,
        cancel: &CancellationToken,
    ) -> Option<Arc<BookRecovery>> {
        let mut books = self.books.lock();

        if cancel.is_cancelled() {
            return None;
        }

        books.get_mut(market_id)?.claim()
    }

    pub(crate) fn claim_recovery(&self, market_id: &str) -> Option<Arc<BookRecovery>> {
        self.books
            .lock()
            .get_mut(market_id)
            .filter(|book| !book.is_send_pending())?
            .claim()
    }

    /// Writes an image request through `write` unless a written request younger than
    /// `join_within` awaits its image. A zero `join_within` always writes.
    ///
    /// A joining request opens `gate` while it holds the request record, which an image start
    /// clears first, so the gate opens before the joined image is handled. A writing request
    /// records itself before `write`, so its image cannot start before the record exists.
    pub(crate) fn request_image(
        &self,
        join_within: Duration,
        now: Instant,
        gate: &SnapshotGate,
        write: impl FnOnce() -> Result<(), BetfairStreamError>,
    ) -> Result<(), BetfairStreamError> {
        let _requesting = self.image_requesting.lock();

        let previous = {
            let mut requested = self.image_requested.lock();

            if requested
                .is_some_and(|requested| now.saturating_duration_since(requested) < join_within)
            {
                gate.open();
                return Ok(());
            }

            requested.replace(now)
        };

        write().inspect_err(|_| *self.image_requested.lock() = previous)
    }

    pub(crate) fn clear_image_request(&self) {
        *self.image_requested.lock() = None;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::dst::time::{Duration, Instant};
    use nautilus_live::book::{
        recovery::BookRecoveryOutcome, snapshot::SnapshotGate, sync::BookPhase,
    };
    use rstest::rstest;

    use super::{BookSequenceOutcome, BookSyncTracker};
    use crate::stream::error::BetfairStreamError;

    const MARKET: &str = "1.180737206";
    const OTHER_MARKET: &str = "1.176621195";

    fn synced(tracker: &BookSyncTracker, market_id: &str) {
        let now = Instant::now();
        tracker.record_subscription(market_id, now, SnapshotGate::default());
        let outcome = tracker.validate(market_id, true, now);
        assert_eq!(outcome, BookSequenceOutcome::Accept);
    }

    fn record_request(tracker: &BookSyncTracker, now: Instant) {
        tracker
            .request_image(Duration::ZERO, now, &SnapshotGate::default(), || Ok(()))
            .unwrap();
    }

    fn phase(tracker: &BookSyncTracker, market_id: &str) -> BookPhase<()> {
        *tracker.books.lock()[market_id].phase()
    }

    #[rstest]
    fn image_syncs_book_and_changes_advance() {
        let tracker = BookSyncTracker::default();
        tracker.record_subscription(MARKET, Instant::now(), SnapshotGate::default());

        let image = tracker.validate(MARKET, true, Instant::now());
        let first = tracker.validate(MARKET, false, Instant::now());
        let second = tracker.validate(MARKET, false, Instant::now());

        assert_eq!(image, BookSequenceOutcome::Accept);
        assert_eq!(first, BookSequenceOutcome::Accept);
        assert_eq!(second, BookSequenceOutcome::Accept);
        assert_eq!(phase(&tracker, MARKET), BookPhase::Synced(()));
    }

    #[rstest]
    fn change_before_image_requests_recovery_until_claimed() {
        let tracker = BookSyncTracker::default();
        tracker.record_subscription(MARKET, Instant::now(), SnapshotGate::default());

        let first = tracker.validate(MARKET, false, Instant::now());
        let again = tracker.validate(MARKET, false, Instant::now());
        let recovery = tracker.claim_recovery(MARKET).unwrap();
        let owned = tracker.validate(MARKET, false, Instant::now());

        assert_eq!(first, BookSequenceOutcome::Recover);
        assert_eq!(again, BookSequenceOutcome::Recover);
        assert_eq!(owned, BookSequenceOutcome::Suppress);
        assert!(recovery.is_running());
        assert_eq!(phase(&tracker, MARKET), BookPhase::Recovering);
    }

    #[rstest]
    fn image_replaces_synced_book() {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);

        let image = tracker.validate(MARKET, true, Instant::now());
        let change = tracker.validate(MARKET, false, Instant::now());

        assert_eq!(image, BookSequenceOutcome::Accept);
        assert_eq!(change, BookSequenceOutcome::Accept);
        assert_eq!(phase(&tracker, MARKET), BookPhase::Synced(()));
    }

    #[rstest]
    fn image_completes_running_recovery_once_write_confirmed() {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);
        let recovery = tracker.claim_recovery(MARKET).unwrap();
        assert!(recovery.begin_replacement());

        let during_write = tracker.validate(MARKET, true, Instant::now());
        recovery.gate.open();
        let after_write = tracker.validate(MARKET, true, Instant::now());

        assert_eq!(during_write, BookSequenceOutcome::Suppress);
        assert_eq!(after_write, BookSequenceOutcome::Accept);
        assert!(recovery.is_accepted());
        assert_eq!(phase(&tracker, MARKET), BookPhase::Synced(()));
    }

    #[rstest]
    #[case::image(true)]
    #[case::change(false)]
    fn initial_send_gate_suppresses_changes_until_write_completes(#[case] is_image: bool) {
        let tracker = BookSyncTracker::default();
        let now = Instant::now();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = tracker.record_subscription(MARKET, now, gate.clone());

        let while_sending = tracker.validate(MARKET, is_image, now);
        let claimed_while_sending = tracker.claim_recovery(MARKET).is_some();
        gate.open();
        let image = tracker.validate(MARKET, true, now);

        assert_eq!(while_sending, BookSequenceOutcome::Suppress);
        assert!(!claimed_while_sending);
        assert_eq!(image, BookSequenceOutcome::Accept);
        assert!(cancel.is_cancelled());
        assert!(
            tracker
                .claim_subscription_recovery(MARKET, &cancel)
                .is_none()
        );
    }

    #[rstest]
    fn untracked_market_suppresses_changes() {
        let tracker = BookSyncTracker::default();

        let image = tracker.validate(MARKET, true, Instant::now());
        let change = tracker.validate(MARKET, false, Instant::now());

        assert_eq!(image, BookSequenceOutcome::Suppress);
        assert_eq!(change, BookSequenceOutcome::Suppress);
        assert!(tracker.books.lock().is_empty());
    }

    #[rstest]
    fn reconnect_keeps_synced_books_and_running_recovery() {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);
        synced(&tracker, OTHER_MARKET);
        let recovery = tracker.claim_recovery(OTHER_MARKET).unwrap();

        tracker.reset_on_reconnect();
        let change = tracker.validate(MARKET, false, Instant::now());
        let recovering = tracker.validate(OTHER_MARKET, false, Instant::now());

        assert_eq!(change, BookSequenceOutcome::Accept);
        assert_eq!(recovering, BookSequenceOutcome::Suppress);
        assert!(recovery.is_running());
        assert!(tracker.claim_recovery(OTHER_MARKET).is_none());
        assert_eq!(phase(&tracker, MARKET), BookPhase::Synced(()));
        assert_eq!(phase(&tracker, OTHER_MARKET), BookPhase::Recovering);
    }

    #[rstest]
    fn reconnect_keeps_initial_send_wait() {
        let tracker = BookSyncTracker::default();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = tracker.record_subscription(MARKET, Instant::now(), gate);

        tracker.reset_on_reconnect();

        assert!(!cancel.is_cancelled());
        assert!(tracker.claim_recovery(MARKET).is_none());
        assert!(
            tracker
                .claim_subscription_recovery(MARKET, &cancel)
                .is_some()
        );
    }

    #[rstest]
    #[case::removed(false)]
    #[case::cleared(true)]
    fn removing_book_cancels_its_recovery(#[case] clear: bool) {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);
        let recovery = tracker.claim_recovery(MARKET).unwrap();

        if clear {
            tracker.clear();
        } else {
            tracker.remove(MARKET);
        }

        assert!(recovery.cancellation.is_cancelled());
        assert!(tracker.books.lock().is_empty());
    }

    #[rstest]
    #[case::recent(Some(1), 10, 0)]
    #[case::stale(Some(10), 10, 1)]
    #[case::none(None, 10, 1)]
    #[case::joins_disabled(Some(0), 0, 2)]
    fn request_image_joins_only_a_recent_request(
        #[case] age_secs: Option<u64>,
        #[case] join_within_secs: u64,
        #[case] expected_writes: usize,
    ) {
        let tracker = BookSyncTracker::default();
        let requested = Instant::now();
        let now = requested + Duration::from_secs(age_secs.unwrap_or(0));
        let join_within = Duration::from_secs(join_within_secs);

        if age_secs.is_some() {
            record_request(&tracker, requested);
        }

        let gate = SnapshotGate::default();
        gate.lock().close();
        let mut writes = 0;

        let mut write = || {
            writes += 1;
            Ok(())
        };

        // A second request joins the first one's write unless joins are disabled
        tracker
            .request_image(join_within, now, &gate, &mut write)
            .unwrap();
        tracker
            .request_image(join_within, now, &gate, &mut write)
            .unwrap();

        // Only a join opens the gate here, since the test writes queue nothing
        assert_eq!(writes, expected_writes);
        assert_eq!(gate.lock().is_closed(), expected_writes == 2);
    }

    #[rstest]
    fn request_image_failure_records_no_request() {
        let tracker = BookSyncTracker::default();
        let now = Instant::now();
        let join_within = Duration::from_secs(10);

        let failed = tracker.request_image(join_within, now, &SnapshotGate::default(), || {
            Err(BetfairStreamError::Disconnected("closed".to_string()))
        });

        let mut writes = 0;
        tracker
            .request_image(join_within, now, &SnapshotGate::default(), || {
                writes += 1;
                Ok(())
            })
            .unwrap();

        assert!(matches!(failed, Err(BetfairStreamError::Disconnected(_))));
        assert_eq!(writes, 1);
    }

    #[rstest]
    #[case::image_started(false)]
    #[case::tracker_cleared(true)]
    fn cleared_image_request_is_not_joined(#[case] clear_tracker: bool) {
        let tracker = BookSyncTracker::default();
        let now = Instant::now();
        record_request(&tracker, now);

        if clear_tracker {
            tracker.clear();
        } else {
            tracker.clear_image_request();
        }

        let mut writes = 0;
        tracker
            .request_image(
                Duration::from_secs(10),
                now,
                &SnapshotGate::default(),
                || {
                    writes += 1;
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(writes, 1);
    }

    #[rstest]
    fn image_start_unsyncs_every_book_until_its_image() {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);
        synced(&tracker, OTHER_MARKET);

        tracker.begin_image();
        let imaged = tracker.validate(MARKET, true, Instant::now());
        let omitted = tracker.validate(OTHER_MARKET, false, Instant::now());

        assert_eq!(imaged, BookSequenceOutcome::Accept);
        assert_eq!(omitted, BookSequenceOutcome::Recover);
        assert_eq!(phase(&tracker, MARKET), BookPhase::Synced(()));
        assert_eq!(phase(&tracker, OTHER_MARKET), BookPhase::Recovering);
    }

    #[rstest]
    #[case::image(true)]
    #[case::update(false)]
    fn unparsable_change_fails_only_an_image_recovery(#[case] is_image: bool) {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);
        let recovery = tracker.claim_recovery(MARKET).unwrap();
        assert!(recovery.begin_replacement());
        recovery.gate.open();

        let outcome = tracker.reject_change(
            MARKET,
            is_image,
            BetfairStreamError::ProtocolError("negative size".to_string()),
        );

        assert_eq!(outcome, BookSequenceOutcome::Suppress);
        assert_eq!(
            matches!(
                *recovery.outcome.borrow(),
                BookRecoveryOutcome::Rejected(BetfairStreamError::ProtocolError(_))
            ),
            is_image
        );
        assert!(recovery.is_running());
    }

    #[rstest]
    fn unparsable_change_unsyncs_book_without_recovery() {
        let tracker = BookSyncTracker::default();
        synced(&tracker, MARKET);

        let outcome = tracker.reject_change(
            MARKET,
            false,
            BetfairStreamError::ProtocolError("negative size".to_string()),
        );
        let change = tracker.validate(MARKET, false, Instant::now());

        assert_eq!(outcome, BookSequenceOutcome::Recover);
        assert_eq!(change, BookSequenceOutcome::Recover);
        assert_eq!(phase(&tracker, MARKET), BookPhase::Recovering);
    }

    // The stream handler can start the requested image before the write returns
    #[rstest]
    fn image_started_during_write_leaves_no_request_to_join() {
        let tracker = BookSyncTracker::default();
        let now = Instant::now();
        let join_within = Duration::from_secs(10);
        let gate = SnapshotGate::default();

        tracker
            .request_image(join_within, now, &gate, || {
                tracker.clear_image_request();
                Ok(())
            })
            .unwrap();

        let mut writes = 0;
        tracker
            .request_image(join_within, now, &gate, || {
                writes += 1;
                Ok(())
            })
            .unwrap();

        assert_eq!(writes, 1);
    }
}
