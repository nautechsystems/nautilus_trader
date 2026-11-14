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

//! Adapter-local order book synchronization state for Coinbase.
//!
//! [`BookSyncTracker`] applies Coinbase connection gaps to the shared per-book [`BookSync`]
//! lifecycle and decides whether to accept a book event, suppress it, or request recovery. Books
//! carry no venue position: `level2` events have no per-book sequence, and connection continuity
//! is checked once per envelope by the feed handler.
//!
//! A snapshot replaces the whole book, so any snapshot that passes the write gates is accepted.
//! Claiming, accepting, and resetting recovery stay under the tracker's state lock so competing
//! events cannot independently change ownership. This module performs state transitions;
//! [`super::recovery`] runs the asynchronous subscription and retry work.
//!
//! Every update for an unsynced book goes through [`BookSync::gap`] once the initial subscription
//! write completes, so a book that no running recovery or armed deadline owns keeps requesting
//! recovery. The data client claims only after it has a product and socket to recover with; an
//! early return leaves the book unowned for the next update.

use std::sync::Arc;

use ahash::AHashMap;
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_core::AtomicMap;
use nautilus_live::book::{snapshot::SnapshotGate, sync::BookSync};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind, recovery::BookRecovery};
use crate::websocket::error::CoinbaseWsError;

type Book = BookSync<CoinbaseWsError>;

#[derive(Debug, Clone, Default)]
pub(crate) struct BookSyncTracker {
    books: Arc<Mutex<AHashMap<InstrumentId, Book>>>,
}

impl BookSyncTracker {
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

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
    }

    pub(crate) fn record_snapshot(&self, instrument_id: InstrumentId, now: Instant) -> bool {
        self.books
            .lock()
            .entry(instrument_id)
            .or_insert_with(|| Book::new(now))
            .accept_snapshot((), now)
    }

    pub(crate) fn validate_update(
        &self,
        instrument_id: InstrumentId,
        now: Instant,
    ) -> BookSequenceOutcome {
        let mut books = self.books.lock();
        let book = books.entry(instrument_id).or_insert_with(|| Book::new(now));

        // Updates before the initial write completes belong to an earlier subscription
        if book.is_send_pending() {
            return BookSequenceOutcome::Suppress;
        }

        if book.advance((), now) {
            return BookSequenceOutcome::Accept;
        }

        book.gap()
    }

    /// Marks every subscribed book out of sync after a connection gap, returning the books that
    /// need a recovery started, sorted by instrument.
    ///
    /// A book whose initial subscription write is still in flight is skipped: its snapshot
    /// follows the write, after the lost envelope.
    pub(crate) fn mark_connection_gap(
        &self,
        book_products: &AtomicMap<InstrumentId, Ustr>,
        now: Instant,
    ) -> Vec<InstrumentId> {
        let instrument_ids = subscribed_instrument_ids(book_products);
        let mut books = self.books.lock();

        let mut recover = instrument_ids
            .into_iter()
            .filter(|instrument_id| {
                let book = books
                    .entry(*instrument_id)
                    .or_insert_with(|| Book::new(now));
                !book.is_send_pending() && book.gap() == BookSequenceOutcome::Recover
            })
            .collect::<Vec<_>>();

        recover.sort();
        recover
    }

    pub(crate) fn reset_on_reconnect(&self, book_products: &AtomicMap<InstrumentId, Ustr>) {
        let instrument_ids = subscribed_instrument_ids(book_products);
        let mut books = self.books.lock();

        for instrument_id in instrument_ids {
            if let Some(book) = books.get_mut(&instrument_id) {
                book.reset_on_reconnect();
            }
        }
    }

    pub(crate) fn seed_pending_snapshots(
        &self,
        book_products: &AtomicMap<InstrumentId, Ustr>,
        timeout: Duration,
        now: Instant,
    ) -> usize {
        let deadline = now + timeout;
        let instrument_ids = subscribed_instrument_ids(book_products);
        let mut books = self.books.lock();

        instrument_ids
            .into_iter()
            .filter(|instrument_id| {
                books
                    .entry(*instrument_id)
                    .or_insert_with(|| Book::new(now))
                    .arm_deadline(deadline)
            })
            .count()
    }

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

    pub(crate) fn claim_recovery(&self, instrument_id: InstrumentId) -> Option<Arc<BookRecovery>> {
        self.books
            .lock()
            .get_mut(&instrument_id)
            .filter(|book| !book.is_send_pending())?
            .claim()
    }

    pub(crate) fn take_expired_snapshots(
        &self,
        book_products: &AtomicMap<InstrumentId, Ustr>,
        now: Instant,
    ) -> Vec<BookSyncSignal> {
        let instrument_ids = subscribed_instrument_ids(book_products);
        let mut books = self.books.lock();

        let mut expired = instrument_ids
            .into_iter()
            .filter(|instrument_id| {
                books
                    .get_mut(instrument_id)
                    .is_some_and(|book| book.take_expired(now))
            })
            .map(|instrument_id| BookSyncSignal {
                instrument_id,
                kind: BookSyncSignalKind::SnapshotMissing,
            })
            .collect::<Vec<_>>();

        // Sort by instrument; the product map iterates in per-process hash order
        expired.sort_by_key(|signal| signal.instrument_id);
        expired
    }
}

fn subscribed_instrument_ids(book_products: &AtomicMap<InstrumentId, Ustr>) -> Vec<InstrumentId> {
    book_products.load().keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::dst::time::{Duration, Instant};
    use nautilus_core::AtomicMap;
    use nautilus_live::book::{snapshot::SnapshotGate, sync::BookPhase};
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;
    use ustr::Ustr;

    use super::{BookSequenceOutcome, BookSyncTracker};

    fn btc() -> InstrumentId {
        InstrumentId::from("BTC-USD.COINBASE")
    }

    fn eth() -> InstrumentId {
        InstrumentId::from("ETH-USD.COINBASE")
    }

    fn products(ids: &[InstrumentId]) -> AtomicMap<InstrumentId, Ustr> {
        let products = AtomicMap::new();
        for id in ids {
            products.insert(*id, id.symbol.inner());
        }
        products
    }

    fn synced(ids: &[InstrumentId], now: Instant) -> BookSyncTracker {
        let tracker = BookSyncTracker::default();
        for id in ids {
            tracker.record_subscription(*id, now, SnapshotGate::default());
            assert!(tracker.record_snapshot(*id, now));
        }
        tracker
    }

    #[rstest]
    fn connection_gap_requests_one_recovery_per_book() {
        let now = Instant::now();
        let books = products(&[btc(), eth()]);
        let tracker = synced(&[btc(), eth()], now);

        let first = tracker.mark_connection_gap(&books, now);
        let btc_recovery = tracker.claim_recovery(btc()).unwrap();
        let eth_recovery = tracker.claim_recovery(eth()).unwrap();
        let second = tracker.mark_connection_gap(&books, now);

        assert_eq!(first, [btc(), eth()]);
        assert!(second.is_empty());
        assert!(btc_recovery.is_running());
        assert!(eth_recovery.is_running());
        assert!(tracker.claim_recovery(btc()).is_none());
    }

    #[rstest]
    fn gap_suppresses_each_book_until_its_own_snapshot() {
        let now = Instant::now();
        let books = products(&[btc(), eth()]);
        let tracker = synced(&[btc(), eth()], now);
        tracker.mark_connection_gap(&books, now);
        let btc_recovery = tracker.claim_recovery(btc()).unwrap();
        let eth_recovery = tracker.claim_recovery(eth()).unwrap();
        assert!(btc_recovery.begin_replacement());
        assert!(eth_recovery.begin_replacement());

        // Updates and replayed snapshots during the replacement write stay suppressed
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Suppress
        );
        assert!(!tracker.record_snapshot(btc(), now));

        btc_recovery.gate.open();
        assert!(tracker.record_snapshot(btc(), now));

        // BTC-USD resumes on its snapshot while ETH-USD still waits for its own
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Accept
        );
        assert_eq!(
            tracker.validate_update(eth(), now),
            BookSequenceOutcome::Suppress
        );
        assert!(btc_recovery.is_accepted());
        assert!(eth_recovery.is_running());

        eth_recovery.gate.open();
        assert!(tracker.record_snapshot(eth(), now));
        assert_eq!(
            tracker.validate_update(eth(), now),
            BookSequenceOutcome::Accept
        );
    }

    #[rstest]
    fn update_for_unowned_unsynced_book_requests_recovery_again() {
        let now = Instant::now();
        let books = products(&[btc()]);
        let tracker = synced(&[btc()], now);
        tracker.mark_connection_gap(&books, now);

        // Recovery could not start, so the next update asks again
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Recover
        );
        let recovery = tracker.claim_recovery(btc()).unwrap();
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Suppress
        );
        recovery.cancellation.cancel();
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Recover
        );
    }

    #[rstest]
    fn initial_send_gate_suppresses_frames_and_skips_gap() {
        let now = Instant::now();
        let books = products(&[btc()]);
        let tracker = BookSyncTracker::default();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = tracker.record_subscription(btc(), now, gate.clone());

        assert!(!tracker.record_snapshot(btc(), now));
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Suppress
        );
        assert!(tracker.mark_connection_gap(&books, now).is_empty());
        assert!(tracker.claim_recovery(btc()).is_none());
        assert!(!cancel.is_cancelled());

        gate.open();
        assert!(tracker.record_snapshot(btc(), now));
        assert!(cancel.is_cancelled());
        assert!(
            tracker
                .claim_subscription_recovery(btc(), &cancel)
                .is_none()
        );
    }

    #[rstest]
    fn gap_after_initial_write_retires_initial_wait() {
        let now = Instant::now();
        let books = products(&[btc()]);
        let tracker = BookSyncTracker::default();
        let cancel = tracker.record_subscription(btc(), now, SnapshotGate::default());

        // The lost envelope may have been this book's initial snapshot
        assert_eq!(tracker.mark_connection_gap(&books, now), [btc()]);
        let recovery = tracker.claim_recovery(btc()).unwrap();

        assert!(cancel.is_cancelled());
        assert!(recovery.is_running());
        assert!(
            tracker
                .claim_subscription_recovery(btc(), &cancel)
                .is_none()
        );
    }

    #[rstest]
    fn reconnect_arms_deadlines_and_reports_missing_snapshots() {
        let now = Instant::now();
        let books = products(&[btc(), eth()]);
        let tracker = synced(&[btc(), eth()], now);

        tracker.reset_on_reconnect(&books);
        let armed = tracker.seed_pending_snapshots(&books, Duration::from_secs(10), now);

        // Armed deadlines own the books, so updates and gaps start nothing
        assert_eq!(armed, 2);
        assert_eq!(
            tracker.validate_update(btc(), now),
            BookSequenceOutcome::Suppress
        );
        assert!(tracker.mark_connection_gap(&books, now).is_empty());

        assert!(tracker.record_snapshot(btc(), now));
        let early = tracker.take_expired_snapshots(&books, now + Duration::from_secs(9));
        let expired = tracker.take_expired_snapshots(&books, now + Duration::from_secs(10));

        assert!(early.is_empty());
        assert_eq!(
            expired
                .iter()
                .map(|signal| signal.instrument_id)
                .collect::<Vec<_>>(),
            [eth()]
        );
        assert!(tracker.claim_recovery(eth()).is_some());
    }

    #[rstest]
    fn reconnect_keeps_running_recovery() {
        let now = Instant::now();
        let books = products(&[btc()]);
        let tracker = synced(&[btc()], now);
        tracker.mark_connection_gap(&books, now);
        let recovery = tracker.claim_recovery(btc()).unwrap();

        tracker.reset_on_reconnect(&books);

        assert!(recovery.is_running());
        assert!(tracker.claim_recovery(btc()).is_none());
        assert_eq!(*tracker.books.lock()[&btc()].phase(), BookPhase::Recovering);
    }

    #[rstest]
    #[case::unsubscribe(false)]
    #[case::shutdown(true)]
    fn removing_book_cancels_recovery(#[case] clear: bool) {
        let now = Instant::now();
        let books = products(&[btc()]);
        let tracker = synced(&[btc()], now);
        tracker.mark_connection_gap(&books, now);
        let recovery = tracker.claim_recovery(btc()).unwrap();

        if clear {
            tracker.clear();
        } else {
            tracker.remove(btc());
        }

        assert!(recovery.cancellation.is_cancelled());
        assert!(tracker.claim_recovery(btc()).is_none());
    }
}
