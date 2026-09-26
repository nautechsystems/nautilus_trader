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

//! Adapter-local order book synchronization for Binance diff depth streams.
//!
//! [`BookSyncTracker`] buffers diffs until a REST snapshot bridges them and validates each later
//! diff against the previous update ID. Each book's lifecycle and recovery episode live in the
//! shared [`BookSync`], whose position is the last accepted update ID; the diff buffer stays
//! local. Validation, snapshot acceptance, and emission share the tracker lock, so a replayed
//! snapshot cannot interleave with diffs from the stream task.
//!
//! # Sequencing
//!
//! A book waits for its first diff on the current connection before claiming recovery, since a
//! snapshot can only be bridged once the stream is live. Spot diffs continue while
//! `U <= previous u + 1`. Futures diffs bridge the snapshot with `U <= lastUpdateId <= u`, then
//! link through `pu == previous u`. Any other diff discards the book, buffers the diff, and
//! claims a fresh snapshot.

use std::{collections::VecDeque, fmt::Display, sync::Arc};

use ahash::AHashMap;
use nautilus_common::{
    live::{dst::time::Instant, sender::EventSender},
    messages::DataEvent,
};
use nautilus_core::UnixNanos;
use nautilus_live::book::{
    BookSequenceOutcome, recovery::BookRecovery, snapshot::SnapshotGate, sync::BookSync,
};
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    identifiers::InstrumentId,
};
use parking_lot::Mutex;

use super::{BinanceBookError, pacing::SnapshotPacer};

const MAX_BUFFERED_DEPTH_UPDATES: usize = 10_000;

/// Binance rule linking consecutive diffs of one book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepthSequencing {
    /// Each diff continues at `U <= previous u + 1`.
    Spot,
    /// The first diff bridges the snapshot, then each diff links through `pu == previous u`.
    Futures,
}

/// One diff depth frame with the update IDs that order it.
///
/// Spot diffs carry no previous update ID. A diff without deltas changes no levels but still
/// advances the sequence.
#[derive(Debug, Clone)]
pub(crate) struct DepthUpdate {
    pub(crate) first_update_id: u64,
    pub(crate) final_update_id: u64,
    pub(crate) prev_final_update_id: Option<u64>,
    pub(crate) deltas: Option<OrderBookDeltas>,
}

/// A REST depth snapshot ready to seed a book.
///
/// Snapshot deltas without a venue event time take the first replayed diff's event time.
#[derive(Debug)]
pub(crate) struct DepthSnapshot {
    pub(crate) last_update_id: u64,
    pub(crate) deltas: OrderBookDeltas,
    pub(crate) has_event_time: bool,
}

/// Book synchronization and recovery state for the diff depth books of one data client.
///
/// Recoveries share the client's snapshot pacer.
#[derive(Debug, Clone)]
pub(crate) struct BookSyncTracker {
    sequencing: DepthSequencing,
    sender: EventSender<DataEvent>,
    pacer: Arc<SnapshotPacer>,
    books: Arc<Mutex<AHashMap<InstrumentId, BookState>>>,
}

impl BookSyncTracker {
    pub(crate) fn new(
        sequencing: DepthSequencing,
        sender: EventSender<DataEvent>,
        pacer: Arc<SnapshotPacer>,
    ) -> Self {
        Self {
            sequencing,
            sender,
            pacer,
            books: Arc::default(),
        }
    }

    pub(crate) fn pacer(&self) -> &SnapshotPacer {
        &self.pacer
    }

    /// Starts synchronizing a book, cancelling the recovery of any earlier sync.
    pub(crate) fn subscribe(&self, instrument_id: InstrumentId) {
        self.books
            .lock()
            .insert(instrument_id, BookState::new(Instant::now()));
    }

    pub(crate) fn remove(&self, instrument_id: InstrumentId) {
        self.books.lock().remove(&instrument_id);
    }

    pub(crate) fn clear(&self) {
        self.books.lock().clear();
    }

    /// Emits or buffers a diff, returning a claimed recovery the caller must start.
    pub(crate) fn handle_update(
        &self,
        instrument_id: InstrumentId,
        update: DepthUpdate,
    ) -> Option<Arc<BookRecovery<BinanceBookError>>> {
        let mut books = self.books.lock();
        let book = books.get_mut(&instrument_id)?;

        let Some(mut position) = book.sync.position().copied() else {
            // An unsynced book buffers diffs and claims a snapshot whenever nothing owns it
            buffer_update(&mut book.buffer, update);
            let recovery = book.sync.claim();

            if recovery.is_some() {
                log::debug!("OrderBook snapshot rebuild for {instrument_id} starting");
            }

            return recovery;
        };

        let last_update_id = position.last_update_id;

        match self.sequencing.validate(&mut position, &update) {
            BookSequenceOutcome::Accept => {
                book.sync.advance(position, Instant::now());

                if let Some(deltas) = update.deltas {
                    self.send(deltas);
                }

                None
            }
            BookSequenceOutcome::Suppress => {
                // A Futures seam diff links the snapshot without changing its levels
                book.sync.advance(position, Instant::now());
                None
            }
            BookSequenceOutcome::Recover => {
                log::warn!(
                    "Book sequence gap for {instrument_id}: \
                     last_update_id={last_update_id}, {update}; requesting a fresh snapshot"
                );
                book.buffer.clear();
                buffer_update(&mut book.buffer, update);
                book.sync.claim()
            }
        }
    }

    /// Seeds a book from `snapshot` and replays the buffered diffs that follow it.
    ///
    /// # Errors
    ///
    /// Returns a retryable error when the snapshot does not bridge the buffered diffs. Returns a
    /// permanent error when `recovery` no longer owns the book.
    pub(crate) fn accept_snapshot(
        &self,
        instrument_id: InstrumentId,
        recovery: &Arc<BookRecovery<BinanceBookError>>,
        gate: &SnapshotGate,
        snapshot: DepthSnapshot,
    ) -> Result<(), BinanceBookError> {
        let last_update_id = snapshot.last_update_id;
        let mut books = self.books.lock();

        let Some(book) = books
            .get_mut(&instrument_id)
            .filter(|book| book.sync.is_current(recovery))
        else {
            return Err(BinanceBookError::Permanent(format!(
                "book recovery for {instrument_id} was superseded"
            )));
        };

        // A recovery kept across a reconnect can finish before the new stream delivers a diff,
        // so the first diff must then continue from the snapshot
        let mut position = DepthPosition::new(last_update_id);
        let mut replayed = Vec::new();

        for (index, update) in book.buffer.iter().enumerate() {
            match self.sequencing.validate(&mut position, update) {
                BookSequenceOutcome::Accept => replayed.push(index),
                BookSequenceOutcome::Suppress => {}
                BookSequenceOutcome::Recover => {
                    return Err(BinanceBookError::Retryable(format!(
                        "depth snapshot lastUpdateId={last_update_id} for {instrument_id} \
                         does not bridge buffered diff {update}"
                    )));
                }
            }
        }

        gate.open();

        if !recovery.is_running() || !book.sync.accept_snapshot(position, Instant::now()) {
            return Err(BinanceBookError::Permanent(format!(
                "book recovery for {instrument_id} was cancelled"
            )));
        }

        let mut updates = std::mem::take(&mut book.buffer);

        let replay = replayed
            .into_iter()
            .filter_map(|index| updates[index].deltas.take())
            .collect::<Vec<_>>();
        let mut snapshot_deltas = snapshot.deltas;

        if !snapshot.has_event_time
            && let Some(first) = replay.first()
        {
            set_event_time(&mut snapshot_deltas, first.ts_event);
        }

        self.send(snapshot_deltas);
        let replayed_count = replay.len();

        for deltas in replay {
            self.send(deltas);
        }

        log::debug!(
            "OrderBook snapshot rebuild for {instrument_id} completed \
             (lastUpdateId={last_update_id}, replayed={replayed_count})"
        );
        Ok(())
    }

    /// Discards book state after a reconnect so each book resyncs from the new stream.
    ///
    /// A running recovery keeps running, so the reconnect can neither replenish its retry budget
    /// nor abandon its snapshot fetch; any other is reset.
    pub(crate) fn reset_on_reconnect(&self) {
        let mut books = self.books.lock();

        for book in books.values_mut() {
            book.sync.reset_on_reconnect();
            book.buffer.clear();
        }
    }

    fn send(&self, deltas: OrderBookDeltas) {
        if let Err(e) = self
            .sender
            .send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
        {
            log::error!("Failed to emit order book deltas: {e}");
        }
    }
}

impl Display for DepthUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "U={}, u={}", self.first_update_id, self.final_update_id)?;

        if let Some(prev_final_update_id) = self.prev_final_update_id {
            write!(f, ", pu={prev_final_update_id}")?;
        }

        Ok(())
    }
}

// Diffs are buffered while the book is unsynced; a snapshot that bridges them drains the buffer
#[derive(Debug)]
struct BookState {
    sync: BookSync<BinanceBookError, DepthPosition>,
    buffer: VecDeque<DepthUpdate>,
}

impl BookState {
    fn new(now: Instant) -> Self {
        Self {
            sync: BookSync::new(now),
            buffer: VecDeque::new(),
        }
    }
}

// `linked` records whether a Futures diff has bridged the snapshot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DepthPosition {
    last_update_id: u64,
    linked: bool,
}

impl DepthPosition {
    fn new(last_update_id: u64) -> Self {
        Self {
            last_update_id,
            linked: false,
        }
    }
}

impl DepthSequencing {
    // Advances `position` past an accepted diff or a Futures diff ending at the snapshot
    fn validate(self, position: &mut DepthPosition, update: &DepthUpdate) -> BookSequenceOutcome {
        let last_update_id = position.last_update_id;

        match self {
            Self::Spot => {
                if update.final_update_id <= last_update_id {
                    return BookSequenceOutcome::Suppress;
                }

                if update.first_update_id > last_update_id.saturating_add(1) {
                    return BookSequenceOutcome::Recover;
                }
            }
            Self::Futures if position.linked => {
                if update.prev_final_update_id != Some(last_update_id) {
                    return BookSequenceOutcome::Recover;
                }
            }
            Self::Futures => {
                if update.final_update_id < last_update_id {
                    return BookSequenceOutcome::Suppress;
                }

                if update.first_update_id > last_update_id {
                    return BookSequenceOutcome::Recover;
                }

                position.linked = true;

                // The snapshot already contains the diff that ends at its update ID
                if update.final_update_id == last_update_id {
                    return BookSequenceOutcome::Suppress;
                }
            }
        }

        position.last_update_id = update.final_update_id;
        BookSequenceOutcome::Accept
    }
}

fn buffer_update(updates: &mut VecDeque<DepthUpdate>, update: DepthUpdate) {
    if updates.len() == MAX_BUFFERED_DEPTH_UPDATES {
        updates.pop_front();
    }

    updates.push_back(update);
}

fn set_event_time(deltas: &mut OrderBookDeltas, ts_event: UnixNanos) {
    deltas.ts_event = ts_event;

    for delta in &mut deltas.deltas {
        delta.ts_event = ts_event;
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use nautilus_model::{
        data::{BookOrder, OrderBookDelta},
        enums::{BookAction, OrderSide, RecordFlag},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTCUSDT.BINANCE")
    }

    fn tracker(
        sequencing: DepthSequencing,
    ) -> (
        BookSyncTracker,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pacer = SnapshotPacer::new(NonZeroU32::new(60_000).unwrap());
        let tracker = BookSyncTracker::new(sequencing, EventSender::from(tx), Arc::new(pacer));
        (tracker, rx)
    }

    fn diff_deltas(final_update_id: u64) -> OrderBookDeltas {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from("1.000"),
            0,
        );

        let delta = OrderBookDelta::new(
            instrument_id(),
            BookAction::Update,
            order,
            RecordFlag::F_LAST as u8,
            final_update_id,
            UnixNanos::from(final_update_id * 1_000),
            UnixNanos::from(final_update_id * 1_000 + 1),
        );
        OrderBookDeltas::new(instrument_id(), vec![delta])
    }

    fn spot_update(first_update_id: u64, final_update_id: u64) -> DepthUpdate {
        DepthUpdate {
            first_update_id,
            final_update_id,
            prev_final_update_id: None,
            deltas: Some(diff_deltas(final_update_id)),
        }
    }

    fn futures_update(
        first_update_id: u64,
        final_update_id: u64,
        prev_final_update_id: u64,
    ) -> DepthUpdate {
        DepthUpdate {
            first_update_id,
            final_update_id,
            prev_final_update_id: Some(prev_final_update_id),
            deltas: Some(diff_deltas(final_update_id)),
        }
    }

    fn snapshot(last_update_id: u64, has_event_time: bool) -> DepthSnapshot {
        let clear = OrderBookDelta::clear(
            instrument_id(),
            last_update_id,
            UnixNanos::from(7_u64),
            UnixNanos::from(9_u64),
        );

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from("2.000"),
            0,
        );

        let add = OrderBookDelta::new(
            instrument_id(),
            BookAction::Add,
            order,
            RecordFlag::F_LAST as u8,
            last_update_id,
            UnixNanos::from(7_u64),
            UnixNanos::from(9_u64),
        );

        DepthSnapshot {
            last_update_id,
            deltas: OrderBookDeltas::new(instrument_id(), vec![clear, add]),
            has_event_time,
        }
    }

    fn accept(
        tracker: &BookSyncTracker,
        recovery: &Arc<BookRecovery<BinanceBookError>>,
        snapshot: DepthSnapshot,
    ) -> Result<(), BinanceBookError> {
        // Mirrors the runner closing the gate before each replacement attempt
        recovery.gate.lock().close();
        tracker.accept_snapshot(instrument_id(), recovery, &recovery.gate, snapshot)
    }

    fn received(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Vec<OrderBookDeltas> {
        let mut received = Vec::new();

        while let Ok(event) = rx.try_recv() {
            let DataEvent::Data(Data::BookDeltas(deltas)) = event else {
                panic!("expected order book deltas");
            };

            received.push(*deltas);
        }

        received
    }

    fn sequences(deltas: &[OrderBookDeltas]) -> Vec<u64> {
        deltas.iter().map(|deltas| deltas.sequence).collect()
    }

    #[rstest]
    #[case::stale_final_equals_last(95, 100, BookSequenceOutcome::Suppress, 100)]
    #[case::stale_final_below_last(90, 99, BookSequenceOutcome::Suppress, 100)]
    #[case::contiguous(101, 101, BookSequenceOutcome::Accept, 101)]
    #[case::straddles_last(99, 105, BookSequenceOutcome::Accept, 105)]
    #[case::gap(102, 103, BookSequenceOutcome::Recover, 100)]
    fn spot_sequencing_continues_at_next_update_id(
        #[case] first_update_id: u64,
        #[case] final_update_id: u64,
        #[case] expected: BookSequenceOutcome,
        #[case] expected_last_update_id: u64,
    ) {
        let mut position = DepthPosition::new(100);
        let update = spot_update(first_update_id, final_update_id);

        let outcome = DepthSequencing::Spot.validate(&mut position, &update);

        assert_eq!(outcome, expected);
        assert_eq!(position.last_update_id, expected_last_update_id);
    }

    #[rstest]
    #[case::stale(false, 95, 99, 90, BookSequenceOutcome::Suppress, 100, false)]
    #[case::seam_ends_at_snapshot(false, 95, 100, 94, BookSequenceOutcome::Suppress, 100, true)]
    #[case::bridges_with_older_pu(false, 98, 105, 97, BookSequenceOutcome::Accept, 105, true)]
    #[case::starts_after_snapshot(false, 101, 105, 100, BookSequenceOutcome::Recover, 100, false)]
    #[case::links_through_pu(true, 101, 105, 100, BookSequenceOutcome::Accept, 105, true)]
    #[case::pu_mismatch(true, 101, 105, 99, BookSequenceOutcome::Recover, 100, true)]
    fn futures_sequencing_bridges_then_links_through_pu(
        #[case] linked: bool,
        #[case] first_update_id: u64,
        #[case] final_update_id: u64,
        #[case] prev_final_update_id: u64,
        #[case] expected: BookSequenceOutcome,
        #[case] expected_last_update_id: u64,
        #[case] expected_linked: bool,
    ) {
        let mut position = DepthPosition {
            last_update_id: 100,
            linked,
        };

        let update = futures_update(first_update_id, final_update_id, prev_final_update_id);

        let outcome = DepthSequencing::Futures.validate(&mut position, &update);

        assert_eq!(outcome, expected);
        assert_eq!(
            position,
            DepthPosition {
                last_update_id: expected_last_update_id,
                linked: expected_linked,
            }
        );
    }

    #[rstest]
    fn first_diff_claims_one_recovery_and_buffers_later_diffs() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());

        let recovery = tracker.handle_update(instrument_id(), spot_update(101, 101));
        let second = tracker.handle_update(instrument_id(), spot_update(102, 102));

        assert!(recovery.is_some());
        assert!(second.is_none());
        assert!(received(&mut rx).is_empty());
    }

    #[rstest]
    fn unsubscribed_book_ignores_diffs() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);

        let recovery = tracker.handle_update(instrument_id(), spot_update(101, 101));

        assert!(recovery.is_none());
        assert!(received(&mut rx).is_empty());
    }

    #[rstest]
    fn accepted_snapshot_replays_bridging_diffs_then_streams_live() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(95, 100))
            .unwrap();
        tracker.handle_update(instrument_id(), spot_update(101, 101));
        tracker.handle_update(instrument_id(), spot_update(102, 103));

        accept(&tracker, &recovery, snapshot(100, false)).unwrap();
        let live = tracker.handle_update(instrument_id(), spot_update(104, 104));

        let received = received(&mut rx);
        assert!(recovery.is_accepted());
        assert!(live.is_none());
        assert_eq!(sequences(&received), vec![100, 101, 103, 104]);
        assert_eq!(received[0].deltas[0].action, BookAction::Clear);
        assert_eq!(received[0].ts_event, received[1].ts_event);
        assert!(
            received[0]
                .deltas
                .iter()
                .all(|delta| delta.ts_event == received[1].ts_event)
        );
        assert_eq!(received[0].ts_init, UnixNanos::from(9_u64));
    }

    #[rstest]
    fn snapshot_with_event_time_keeps_it_when_replaying() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();

        accept(&tracker, &recovery, snapshot(100, true)).unwrap();

        let received = received(&mut rx);
        assert_eq!(sequences(&received), vec![100, 101]);
        assert_eq!(received[0].ts_event, UnixNanos::from(7_u64));
    }

    #[rstest]
    fn snapshot_without_replay_keeps_its_timestamps() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(95, 100))
            .unwrap();

        accept(&tracker, &recovery, snapshot(100, false)).unwrap();

        let received = received(&mut rx);
        assert_eq!(sequences(&received), vec![100]);
        assert_eq!(received[0].ts_event, UnixNanos::from(7_u64));
    }

    #[rstest]
    fn snapshot_that_does_not_bridge_keeps_buffer_for_retry() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();
        tracker.handle_update(instrument_id(), spot_update(102, 102));

        let stale = accept(&tracker, &recovery, snapshot(99, false));
        let bridged = accept(&tracker, &recovery, snapshot(100, false));

        assert_eq!(
            stale,
            Err(BinanceBookError::Retryable(
                "depth snapshot lastUpdateId=99 for BTCUSDT.BINANCE does not bridge \
                 buffered diff U=101, u=101"
                    .to_string()
            ))
        );
        assert_eq!(bridged, Ok(()));
        assert_eq!(sequences(&received(&mut rx)), vec![100, 101, 102]);
    }

    #[rstest]
    fn gap_inside_buffer_rejects_snapshot() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();
        tracker.handle_update(instrument_id(), spot_update(103, 103));

        let result = accept(&tracker, &recovery, snapshot(100, false));

        assert!(matches!(result, Err(BinanceBookError::Retryable(_))));
        assert!(!recovery.is_accepted());
        assert!(received(&mut rx).is_empty());
    }

    #[rstest]
    fn live_gap_buffers_gap_diff_and_claims_new_recovery() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let first = tracker
            .handle_update(instrument_id(), spot_update(100, 100))
            .unwrap();
        accept(&tracker, &first, snapshot(100, false)).unwrap();
        tracker.handle_update(instrument_id(), spot_update(101, 101));

        let second = tracker
            .handle_update(instrument_id(), spot_update(103, 103))
            .unwrap();
        let buffered = tracker.handle_update(instrument_id(), spot_update(104, 104));
        let before_resync = sequences(&received(&mut rx));
        accept(&tracker, &second, snapshot(103, false)).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
        assert!(buffered.is_none());
        assert_eq!(before_resync, vec![100, 101]);
        assert_eq!(sequences(&received(&mut rx)), vec![103, 104]);
    }

    #[rstest]
    fn diff_without_deltas_advances_sequence_without_emission() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(100, 100))
            .unwrap();
        accept(&tracker, &recovery, snapshot(100, false)).unwrap();

        let empty = DepthUpdate {
            deltas: None,
            ..spot_update(101, 101)
        };

        let empty_recovery = tracker.handle_update(instrument_id(), empty);
        let next_recovery = tracker.handle_update(instrument_id(), spot_update(102, 102));

        assert!(empty_recovery.is_none());
        assert!(next_recovery.is_none());
        assert_eq!(sequences(&received(&mut rx)), vec![100, 102]);
    }

    #[rstest]
    fn futures_seam_diff_links_without_emission() {
        let (tracker, mut rx) = tracker(DepthSequencing::Futures);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), futures_update(95, 100, 94))
            .unwrap();
        tracker.handle_update(instrument_id(), futures_update(101, 105, 100));

        accept(&tracker, &recovery, snapshot(100, true)).unwrap();
        let linked = tracker.handle_update(instrument_id(), futures_update(106, 110, 105));
        let gap = tracker.handle_update(instrument_id(), futures_update(111, 115, 108));

        let received = received(&mut rx);
        assert!(linked.is_none());
        assert!(gap.is_some());
        assert_eq!(sequences(&received), vec![100, 105, 110]);
        assert_eq!(received[0].ts_event, UnixNanos::from(7_u64));
    }

    #[rstest]
    fn cancelled_recovery_is_replaced_by_next_diff() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let first = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();
        first.cancellation.cancel();

        let second = tracker
            .handle_update(instrument_id(), spot_update(102, 102))
            .unwrap();
        let accepted = accept(&tracker, &second, snapshot(100, false));

        // The unsynced book never waits on an owner that can no longer run
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(accepted, Ok(()));
        assert_eq!(sequences(&received(&mut rx)), vec![100, 101, 102]);
    }

    #[rstest]
    fn superseded_recovery_cannot_accept() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let old = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();

        tracker.subscribe(instrument_id());
        let current = tracker
            .handle_update(instrument_id(), spot_update(102, 102))
            .unwrap();
        let accepted =
            tracker.accept_snapshot(instrument_id(), &old, &old.gate, snapshot(101, false));

        assert!(old.cancellation.is_cancelled());
        assert_eq!(
            accepted,
            Err(BinanceBookError::Permanent(
                "book recovery for BTCUSDT.BINANCE was superseded".to_string()
            ))
        );
        assert!(!current.cancellation.is_cancelled());
        assert!(received(&mut rx).is_empty());
    }

    #[rstest]
    fn remove_cancels_recovery_and_ignores_diffs() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();

        tracker.remove(instrument_id());
        let after = tracker.handle_update(instrument_id(), spot_update(102, 102));

        assert!(recovery.cancellation.is_cancelled());
        assert!(after.is_none());
        assert!(received(&mut rx).is_empty());
    }

    #[rstest]
    fn reconnect_retains_active_recovery_and_drops_buffered_diffs() {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let recovery = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();
        tracker.handle_update(instrument_id(), spot_update(102, 102));

        tracker.reset_on_reconnect();
        let accepted = accept(&tracker, &recovery, snapshot(100, false));
        let resumed = tracker.handle_update(instrument_id(), spot_update(101, 101));

        assert_eq!(accepted, Ok(()));
        assert!(resumed.is_none());
        assert!(!recovery.cancellation.is_cancelled());
        assert_eq!(sequences(&received(&mut rx)), vec![100, 101]);
    }

    #[rstest]
    #[case::synced(false)]
    #[case::cancelled(true)]
    fn reconnect_resets_settled_recovery(#[case] cancelled: bool) {
        let (tracker, mut rx) = tracker(DepthSequencing::Spot);
        tracker.subscribe(instrument_id());
        let first = tracker
            .handle_update(instrument_id(), spot_update(101, 101))
            .unwrap();

        if cancelled {
            first.cancellation.cancel();
        } else {
            accept(&tracker, &first, snapshot(101, false)).unwrap();
        }

        let before_reconnect = received(&mut rx).len();

        tracker.reset_on_reconnect();
        let resumed = tracker.handle_update(instrument_id(), spot_update(102, 102));

        assert_eq!(before_reconnect, usize::from(!cancelled));
        assert!(resumed.is_some_and(|second| !Arc::ptr_eq(&first, &second)));
        assert!(received(&mut rx).is_empty());
    }

    #[rstest]
    fn buffer_keeps_newest_updates() {
        let mut updates = VecDeque::new();

        for final_update_id in 0..=MAX_BUFFERED_DEPTH_UPDATES as u64 {
            buffer_update(&mut updates, spot_update(final_update_id, final_update_id));
        }

        assert_eq!(updates.len(), MAX_BUFFERED_DEPTH_UPDATES);
        assert_eq!(updates.front().unwrap().final_update_id, 1);
        assert_eq!(
            updates.back().unwrap().final_update_id,
            MAX_BUFFERED_DEPTH_UPDATES as u64
        );
    }
}
