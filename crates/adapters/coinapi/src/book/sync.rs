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

//! Adapter-local order book synchronization state for CoinAPI.
//!
//! [`BookSyncTracker`] applies the CoinAPI sequence rule to the shared per-book [`BookSync`]
//! lifecycle and converts accepted messages into deltas. A snapshot lists the whole book, so it is
//! accepted at any sequence; an update is accepted only at the sequence after the last accepted
//! message. A book's position is that last sequence.
//!
//! CoinAPI restarts a pair's sequence at one whenever it restarts the pair or the connection, and
//! a managed `OrderBook` keeps its sequence as a high-water mark across snapshots. Emitted deltas
//! therefore carry the venue sequence plus a per-book offset, set at each accepted snapshot so the
//! emitted sequence keeps rising across restarts, unsubscribes, and reconnects. Gap detection uses
//! the venue sequence.
//!
//! Conversion happens under the tracker's lock, so an L3 book's `OrderIndex` changes only with
//! its accepted position. A message that fails conversion leaves the book out of sync, as a gap
//! does. This module performs state transitions; `super::recovery` runs the asynchronous
//! scope changes and retries.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_common::live::dst::time::Instant;
use nautilus_core::AtomicMap;
use nautilus_live::book::{
    BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind,
    snapshot::SnapshotGate,
    sync::{BookPhase, BookSync},
};
use nautilus_model::{
    data::OrderBookDeltas, identifiers::InstrumentId, instruments::InstrumentAny,
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{CoinApiBookError, orders::OrderIndex, recovery::BookRecovery};
use crate::websocket::{
    messages::{Book, BookL3, DataType, Subscription},
    parse::parse_book_deltas,
};

/// A `book` or `book_l3` message routed to its book.
#[derive(Debug, Clone, Copy)]
pub enum BookFrame<'a> {
    Levels(&'a Book),
    Orders(&'a BookL3),
}

impl BookFrame<'_> {
    fn sequence(self) -> u64 {
        match self {
            Self::Levels(book) => book.sequence,
            Self::Orders(book) => book.sequence,
        }
    }

    fn is_snapshot(self) -> bool {
        match self {
            Self::Levels(book) => book.is_snapshot == Some(true),
            Self::Orders(book) => book.is_snapshot,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BookSubscription {
    pub(crate) data_type: DataType,
    pub(crate) depth: Option<usize>,
    pub(crate) active: Arc<AtomicBool>,
}

// Registration and retirement share one writer lock; market-data reads use immutable snapshots
#[derive(Debug, Default)]
pub(crate) struct BookSubscriptions {
    pub(crate) entries: AtomicMap<InstrumentId, BookSubscription>,
    pub(crate) writes: Mutex<()>,
    pub(crate) sync: BookSyncTracker,
}

impl BookSubscriptions {
    pub(crate) fn clear(&self) {
        let _write = self.writes.lock();
        self.sync.clear();
        self.entries.store(AHashMap::new());
    }

    pub(crate) fn retire(&self) {
        let _write = self.writes.lock();
        let mut entries = (**self.entries.load()).clone();
        entries.retain(|instrument_id, registration| {
            if registration.active.load(Ordering::Relaxed) {
                self.sync.remove(*instrument_id);
                false
            } else {
                true
            }
        });

        self.entries.store(entries);
    }
}

#[derive(Debug)]
struct BookState {
    subscription: Subscription,
    sync: BookSync<CoinApiBookError, u64>,
    orders: OrderIndex,
    // Added to the venue sequence of each delta emitted in the current snapshot episode
    offset: u64,
}

/// Sequence, snapshot, and recovery state of each subscribed book.
///
/// A book accepts messages only after [`Self::record_subscription`] registers it; [`Self::apply`]
/// suppresses messages for any other book.
#[derive(Debug, Clone, Default)]
pub struct BookSyncTracker {
    books: Arc<Mutex<AHashMap<InstrumentId, BookState>>>,
    // Last emitted sequence of each book, kept across subscriptions and connections
    emitted: Arc<Mutex<AHashMap<InstrumentId, u64>>>,
    // Last venue and emitted sequence of each depth stream
    depths: Arc<Mutex<AHashMap<InstrumentId, (u64, u64)>>>,
}

impl BookSyncTracker {
    /// Replaces the book with one waiting for its first snapshot behind `gate`.
    pub fn record_subscription(
        &self,
        instrument_id: InstrumentId,
        subscription: Subscription,
        now: Instant,
        gate: SnapshotGate,
    ) -> CancellationToken {
        let mut sync = BookSync::new(now);
        let cancel = sync.expect_snapshot(gate);

        let state = BookState {
            subscription,
            sync,
            orders: OrderIndex::default(),
            offset: 0,
        };

        self.books.lock().insert(instrument_id, state);
        cancel
    }

    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        self.books.lock().remove(&instrument_id);
    }

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
    }

    /// Validates a message and converts it once accepted.
    ///
    /// Returns [`BookSequenceOutcome::Recover`] when the caller must start a recovery, and deltas
    /// only for an accepted message that changes the book.
    pub fn apply(
        &self,
        instrument_id: InstrumentId,
        frame: BookFrame<'_>,
        instrument: &InstrumentAny,
        now: Instant,
    ) -> (BookSequenceOutcome, Option<OrderBookDeltas>) {
        let mut books = self.books.lock();

        let Some(state) = books.get_mut(&instrument_id) else {
            return (BookSequenceOutcome::Suppress, None);
        };

        // Frames before the subscribing write belong to a scope that did not include the book
        if state.sync.is_send_pending() {
            return (BookSequenceOutcome::Suppress, None);
        }

        let sequence = frame.sequence();
        if frame.is_snapshot() {
            let converted = match frame {
                BookFrame::Levels(book) => parse_book_deltas(book, instrument).map(|d| (None, d)),
                BookFrame::Orders(book) => OrderIndex::snapshot(book, instrument)
                    .map(|(orders, deltas)| (Some(orders), Some(deltas))),
            };

            let (orders, deltas) = match converted {
                Ok(converted) => converted,
                Err(e) => return (invalid(state, instrument_id, sequence, &e), None),
            };

            if !state.sync.accept_snapshot(sequence, now) {
                return (BookSequenceOutcome::Suppress, None);
            }

            if let Some(orders) = orders {
                state.orders = orders;
            }

            // The snapshot's emitted sequence follows the book's last one, even after a restart
            let mut emitted = self.emitted.lock();
            let last = emitted.get(&instrument_id).copied().unwrap_or(0);
            state.offset = (last + 1).saturating_sub(sequence);
            emitted.insert(instrument_id, state.offset + sequence);
            return (
                BookSequenceOutcome::Accept,
                deltas.map(|deltas| offset(deltas, state.offset)),
            );
        }

        // An out-of-sync book suppresses updates while an owner restores it, and asks for one
        // otherwise
        match *state.sync.phase() {
            BookPhase::Synced(position) if position.checked_add(1) == Some(sequence) => {}
            BookPhase::Synced(_) | BookPhase::Waiting | BookPhase::Recovering => {
                return (gap(state, instrument_id, sequence), None);
            }
        }

        let converted = match frame {
            BookFrame::Levels(book) => parse_book_deltas(book, instrument),
            BookFrame::Orders(book) => state.orders.update(book, instrument),
        };

        match converted {
            Ok(deltas) => {
                state.sync.advance(sequence, now);
                self.emitted
                    .lock()
                    .insert(instrument_id, state.offset + sequence);
                (
                    BookSequenceOutcome::Accept,
                    deltas.map(|deltas| offset(deltas, state.offset)),
                )
            }
            Err(e) => (invalid(state, instrument_id, sequence, &e), None),
        }
    }

    /// Returns the emitted sequence of a depth snapshot at venue `sequence`.
    ///
    /// Each depth message lists complete top levels, so the stream advances by the venue step and
    /// continues from its last emitted sequence when CoinAPI restarts the venue sequence.
    pub fn depth_sequence(&self, instrument_id: InstrumentId, sequence: u64) -> u64 {
        let mut depths = self.depths.lock();

        let emitted = match depths.get(&instrument_id) {
            Some(&(venue, emitted)) if sequence > venue => emitted + (sequence - venue),
            Some(&(_, emitted)) => emitted + 1,
            None => sequence,
        };

        depths.insert(instrument_id, (sequence, emitted));
        emitted
    }

    /// Restarts every book after its connection reconnects, arming a snapshot `deadline` when
    /// given, and returns how many deadlines were armed.
    pub fn reset_on_reconnect(&self, deadline: Option<Instant>) -> usize {
        let mut armed = 0;

        for state in self.books.lock().values_mut() {
            state.sync.reset_on_reconnect();

            if deadline.is_some_and(|deadline| state.sync.arm_deadline(deadline)) {
                armed += 1;
            }
        }

        armed
    }

    pub(crate) fn take_expired_snapshots(&self, now: Instant) -> Vec<BookSyncSignal> {
        let mut expired = self
            .books
            .lock()
            .iter_mut()
            .filter_map(|(instrument_id, state)| {
                state.sync.take_expired(now).then_some(instrument_id)
            })
            .map(|instrument_id| BookSyncSignal {
                instrument_id: *instrument_id,
                kind: BookSyncSignalKind::SnapshotMissing,
            })
            .collect::<Vec<_>>();

        // Sort by instrument; the book map iterates in per-process hash order
        expired.sort_by_key(|signal| signal.instrument_id);
        expired
    }

    /// Claims a recovery for a subscription whose initial snapshot did not arrive, unless the
    /// subscription was replaced or removed.
    pub(crate) fn claim_subscription_recovery(
        &self,
        instrument_id: InstrumentId,
        cancel: &CancellationToken,
    ) -> Option<(Arc<BookRecovery>, Subscription)> {
        let mut books = self.books.lock();

        if cancel.is_cancelled() {
            return None;
        }

        let state = books.get_mut(&instrument_id)?;
        let recovery = state.sync.claim()?;
        Some((recovery, state.subscription))
    }

    pub(crate) fn claim_recovery(
        &self,
        instrument_id: InstrumentId,
    ) -> Option<(Arc<BookRecovery>, Subscription)> {
        let mut books = self.books.lock();
        let state = books
            .get_mut(&instrument_id)
            .filter(|state| !state.sync.is_send_pending())?;
        let recovery = state.sync.claim()?;
        Some((recovery, state.subscription))
    }
}

fn offset(mut deltas: OrderBookDeltas, offset: u64) -> OrderBookDeltas {
    for delta in &mut deltas.deltas {
        delta.sequence += offset;
    }

    deltas.sequence += offset;
    deltas
}

fn gap(state: &mut BookState, instrument_id: InstrumentId, sequence: u64) -> BookSequenceOutcome {
    let position = state.sync.position().copied();
    let outcome = state.sync.gap();

    if outcome == BookSequenceOutcome::Recover {
        log::warn!(
            "Book sequence gap for {instrument_id}: last sequence {position:?}, received \
             {sequence}; requesting a fresh snapshot"
        );
    }

    outcome
}

fn invalid(
    state: &mut BookState,
    instrument_id: InstrumentId,
    sequence: u64,
    error: &anyhow::Error,
) -> BookSequenceOutcome {
    log::warn!("Invalid book message for {instrument_id} at sequence {sequence}: {error:#}");
    state.sync.gap()
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::dst::time::Duration;
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::BookAction,
        identifiers::InstrumentId,
        instruments::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::websocket::messages::{Channel, DataType};

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTC-USD.COINBASE")
    }

    fn instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id())
                .raw_symbol("BTC-USD".into())
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .price_precision(2)
                .size_precision(8)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn subscription(data_type: DataType) -> Subscription {
        Subscription {
            channel: Channel {
                data_type,
                period_id: None,
            },
            symbol_id: Ustr::from("COINBASE_SPOT_BTC_USD"),
        }
    }

    fn levels(sequence: u64, is_snapshot: bool) -> Book {
        let mut book: Book =
            serde_json::from_str(include_str!("../../test_data/ws_book.json")).unwrap();
        book.sequence = sequence;
        book.is_snapshot = Some(is_snapshot);
        book
    }

    fn orders(sequence: u64, is_snapshot: bool) -> BookL3 {
        let mut book: BookL3 =
            serde_json::from_str(include_str!("../../test_data/ws_book_l3.json")).unwrap();
        book.sequence = sequence;
        book.is_snapshot = is_snapshot;
        book
    }

    // A tracker whose book's subscribing write has completed
    fn subscribed(data_type: DataType, now: Instant) -> BookSyncTracker {
        let tracker = BookSyncTracker::default();
        tracker.record_subscription(
            instrument_id(),
            subscription(data_type),
            now,
            SnapshotGate::default(),
        );
        tracker
    }

    fn apply(
        tracker: &BookSyncTracker,
        book: &Book,
        now: Instant,
    ) -> (BookSequenceOutcome, Option<Vec<BookAction>>) {
        let (outcome, deltas) =
            tracker.apply(instrument_id(), BookFrame::Levels(book), &instrument(), now);
        let actions = deltas.map(|deltas| deltas.deltas.iter().map(|d| d.action).collect());
        (outcome, actions)
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn retirement_removes_active_and_preserves_pending(#[case] active: bool) {
        let subscriptions = BookSubscriptions::default();
        subscriptions.entries.insert(
            instrument_id(),
            BookSubscription {
                data_type: DataType::Book,
                depth: None,
                active: Arc::new(AtomicBool::new(active)),
            },
        );

        let cancel = subscriptions.sync.record_subscription(
            instrument_id(),
            subscription(DataType::Book),
            Instant::now(),
            SnapshotGate::default(),
        );
        subscriptions.retire();

        assert_eq!(
            subscriptions.entries.contains_key(&instrument_id()),
            !active
        );
        assert_eq!(cancel.is_cancelled(), active);
    }

    #[rstest]
    fn retirement_keeps_replacement_registration_when_old_command_commits() {
        let subscriptions = BookSubscriptions::default();
        let old = Arc::new(AtomicBool::new(false));
        let replacement = Arc::new(AtomicBool::new(false));
        subscriptions.entries.insert(
            instrument_id(),
            BookSubscription {
                data_type: DataType::Book20,
                depth: Some(10),
                active: Arc::clone(&replacement),
            },
        );

        old.store(true, Ordering::Relaxed);
        subscriptions.retire();

        let registration = subscriptions.entries.get_cloned(&instrument_id()).unwrap();
        assert_eq!(registration.data_type, DataType::Book20);
        assert_eq!(registration.depth, Some(10));
        assert!(!registration.active.load(Ordering::Relaxed));
        assert!(Arc::ptr_eq(&registration.active, &replacement));
    }

    #[rstest]
    fn snapshot_then_linked_updates_are_accepted() {
        let now = Instant::now();
        let tracker = subscribed(DataType::Book, now);

        let snapshot = apply(&tracker, &levels(1, true), now);
        let update = apply(&tracker, &levels(2, false), now);

        assert_eq!(
            snapshot,
            (
                BookSequenceOutcome::Accept,
                Some(vec![
                    BookAction::Clear,
                    BookAction::Add,
                    BookAction::Add,
                    BookAction::Add,
                    BookAction::Add,
                ])
            )
        );
        assert_eq!(
            update,
            (
                BookSequenceOutcome::Accept,
                Some(vec![BookAction::Update; 4])
            )
        );
    }

    // A snapshot lists the whole book, so it resynchronizes at any sequence, including after a skip
    #[rstest]
    fn gap_recovers_once_claimed_and_snapshot_completes_recovery() {
        let now = Instant::now();
        let tracker = subscribed(DataType::Book, now);
        apply(&tracker, &levels(1, true), now);

        let gap = apply(&tracker, &levels(3, false), now);
        let unowned = apply(&tracker, &levels(4, false), now);
        let (recovery, claimed) = tracker.claim_recovery(instrument_id()).unwrap();
        assert!(recovery.begin_replacement());
        let owned = apply(&tracker, &levels(5, false), now);
        let during_write = apply(&tracker, &levels(1, true), now);
        recovery.gate.open();
        let restarted = apply(&tracker, &levels(1, true), now);
        let linked = apply(&tracker, &levels(2, false), now);

        assert_eq!(gap, (BookSequenceOutcome::Recover, None));
        assert_eq!(unowned, (BookSequenceOutcome::Recover, None));
        assert_eq!(claimed, subscription(DataType::Book));
        assert_eq!(owned, (BookSequenceOutcome::Suppress, None));
        assert_eq!(during_write, (BookSequenceOutcome::Suppress, None));
        assert_eq!(restarted.0, BookSequenceOutcome::Accept);
        assert!(recovery.is_accepted());
        assert_eq!(linked.0, BookSequenceOutcome::Accept);
    }

    // CoinAPI restarts sequences at one, so emitted sequences continue from the last emitted one,
    // including after the book is subscribed again
    #[rstest]
    fn emitted_sequences_keep_rising_across_restarts_and_resubscribes() {
        let now = Instant::now();
        let tracker = subscribed(DataType::Book, now);
        let emitted = |frame: &Book| {
            tracker
                .apply(
                    instrument_id(),
                    BookFrame::Levels(frame),
                    &instrument(),
                    now,
                )
                .1
                .map(|deltas| {
                    let sequences: Vec<u64> = deltas.deltas.iter().map(|d| d.sequence).collect();
                    assert!(
                        sequences
                            .iter()
                            .all(|sequence| *sequence == deltas.sequence)
                    );
                    deltas.sequence
                })
        };

        let first = emitted(&levels(1, true));
        let second = emitted(&levels(2, false));
        let doubled = emitted(&levels(2, true));
        let restarted = emitted(&levels(1, true));
        let linked = emitted(&levels(2, false));
        tracker.record_subscription(
            instrument_id(),
            subscription(DataType::Book),
            now,
            SnapshotGate::default(),
        );
        let resubscribed = emitted(&levels(1, true));

        assert_eq!(
            [first, second, doubled, restarted, linked, resubscribed],
            [Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)]
        );
    }

    #[rstest]
    fn depth_sequences_keep_rising_when_the_venue_restarts() {
        let tracker = BookSyncTracker::default();
        let emitted: Vec<u64> = [7, 9, 1, 2, 5]
            .into_iter()
            .map(|sequence| tracker.depth_sequence(instrument_id(), sequence))
            .collect();

        assert_eq!(emitted, [7, 9, 10, 11, 14]);
    }

    #[rstest]
    fn update_before_snapshot_requests_recovery() {
        let now = Instant::now();
        let tracker = subscribed(DataType::Book, now);

        let result = apply(&tracker, &levels(7, false), now);

        assert_eq!(result, (BookSequenceOutcome::Recover, None));
    }

    #[rstest]
    fn frames_wait_for_subscribing_write() {
        let now = Instant::now();
        let tracker = BookSyncTracker::default();
        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = tracker.record_subscription(
            instrument_id(),
            subscription(DataType::Book),
            now,
            gate.clone(),
        );

        let update = apply(&tracker, &levels(7, false), now);
        let snapshot = apply(&tracker, &levels(1, true), now);
        let refused = tracker.claim_recovery(instrument_id());
        gate.open();
        let accepted = apply(&tracker, &levels(1, true), now);

        assert_eq!(update, (BookSequenceOutcome::Suppress, None));
        assert_eq!(snapshot, (BookSequenceOutcome::Suppress, None));
        assert!(refused.is_none());
        assert_eq!(accepted.0, BookSequenceOutcome::Accept);
        assert!(cancel.is_cancelled());
    }

    #[rstest]
    fn invalid_messages_request_recovery_without_output() {
        let now = Instant::now();
        let tracker = subscribed(DataType::BookL3, now);
        let mut aggregated = orders(1, true);
        aggregated.bids[1].update_type = None;
        aggregated
            .asks
            .iter_mut()
            .for_each(|entry| entry.update_type = None);

        let (snapshot, snapshot_deltas) = tracker.apply(
            instrument_id(),
            BookFrame::Orders(&aggregated),
            &instrument(),
            now,
        );

        assert_eq!(snapshot, BookSequenceOutcome::Recover);
        assert_eq!(snapshot_deltas, None);
    }

    // The index follows the accepted snapshot, so a later ADD for a listed order is inconsistent
    #[rstest]
    fn l3_updates_apply_to_orders_from_snapshot() {
        let now = Instant::now();
        let tracker = subscribed(DataType::BookL3, now);
        let mut snapshot = orders(1, true);
        snapshot.bids.truncate(1);
        let mut delete = orders(2, false);
        delete.asks.remove(0);
        delete.bids.clear();
        let mut add_listed = orders(3, false);
        add_listed.asks.truncate(1);
        add_listed.bids.clear();

        let apply = |book: &BookL3| {
            let (outcome, deltas) =
                tracker.apply(instrument_id(), BookFrame::Orders(book), &instrument(), now);
            let actions: Option<Vec<BookAction>> =
                deltas.map(|deltas| deltas.deltas.iter().map(|d| d.action).collect());
            (outcome, actions)
        };

        let accepted = apply(&snapshot);
        let deleted = apply(&delete);
        let inconsistent = apply(&add_listed);

        assert_eq!(
            accepted,
            (
                BookSequenceOutcome::Accept,
                Some(vec![
                    BookAction::Clear,
                    BookAction::Add,
                    BookAction::Add,
                    BookAction::Add
                ])
            )
        );
        assert_eq!(
            deleted,
            (BookSequenceOutcome::Accept, Some(vec![BookAction::Delete]))
        );
        assert_eq!(inconsistent, (BookSequenceOutcome::Recover, None));
    }

    #[rstest]
    fn reconnect_waits_for_replayed_snapshot_until_deadline() {
        let now = Instant::now();
        let tracker = subscribed(DataType::Book, now);
        apply(&tracker, &levels(1, true), now);
        apply(&tracker, &levels(2, false), now);

        let armed = tracker.reset_on_reconnect(Some(now + Duration::from_secs(10)));
        let stale = apply(&tracker, &levels(3, false), now);
        let early = tracker.take_expired_snapshots(now + Duration::from_secs(9));
        let expired = tracker.take_expired_snapshots(now + Duration::from_secs(10));
        let unowned = apply(&tracker, &levels(3, false), now);

        assert_eq!(armed, 1);
        assert_eq!(stale, (BookSequenceOutcome::Suppress, None));
        assert!(early.is_empty());
        assert_eq!(
            expired,
            vec![BookSyncSignal {
                instrument_id: instrument_id(),
                kind: BookSyncSignalKind::SnapshotMissing,
            }]
        );
        assert_eq!(unowned, (BookSequenceOutcome::Recover, None));
    }

    #[rstest]
    fn removed_book_suppresses_frames_and_cancels_recovery() {
        let now = Instant::now();
        let tracker = subscribed(DataType::Book, now);
        let (recovery, _) = tracker.claim_recovery(instrument_id()).unwrap();

        tracker.remove(instrument_id());
        let result = apply(&tracker, &levels(1, true), now);

        assert!(recovery.cancellation.is_cancelled());
        assert_eq!(result, (BookSequenceOutcome::Suppress, None));
    }
}
