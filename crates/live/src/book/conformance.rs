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

//! Output checks for the order book stream contract.
//!
//! [`BookStreamChecker`] consumes the [`OrderBookDeltas`] an adapter emits and rejects the first
//! batch that breaks the contract:
//!
//! - Every batch ends with `F_LAST`; an earlier `F_LAST` ends an event group inside the batch, and
//!   each group is checked on its own, as the `DataEngine` publishes them.
//! - A snapshot group starts with `Clear`, continues with `Add` deltas, and flags every delta
//!   `F_SNAPSHOT`; an empty snapshot is a lone `Clear`.
//! - An incremental group carries no `F_SNAPSHOT` and no `Clear`, and follows a snapshot.
//! - Sequences increase within a snapshot episode when the checker requires it.
//! - The maintained book stays consistent, and a closed book emits nothing.
//!
//! Harnesses compare the maintained book with venue truth through [`BookStreamChecker::verify`],
//! which also counts verified episodes so a run can require every recovery to be checked.

use std::{collections::BTreeMap, fmt::Display};

use ahash::AHashMap;
use nautilus_model::{
    data::{OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, RecordFlag},
    identifiers::InstrumentId,
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use rust_decimal::Decimal;

/// A batch or book state that breaks the order book stream contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookContractViolation {
    /// The batch does not end with an `F_LAST` delta.
    MissingLast,
    /// `F_SNAPSHOT` disagrees with the event group kind on at least one delta.
    SnapshotFlagMismatch,
    /// A snapshot delta after the leading `Clear` is not an `Add`.
    SnapshotAction(BookAction),
    /// An incremental event group contains a `Clear`.
    ClearInIncremental,
    /// An incremental event group arrived before the episode's snapshot.
    IncrementalBeforeSnapshot,
    /// An incremental sequence does not exceed the previous one.
    SequenceNotIncreasing { previous: u64, sequence: u64 },
    /// Output arrived for a book that is not open.
    OutputWhileClosed,
    /// The maintained book failed an integrity check.
    Integrity(String),
    /// The maintained book differs from venue truth.
    TruthMismatch { sequence: u64 },
    /// Verification was requested before any snapshot.
    Unsynced,
}

impl Display for BookContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingLast => write!(f, "batch does not end with F_LAST"),
            Self::SnapshotFlagMismatch => {
                write!(f, "F_SNAPSHOT disagrees with the event group kind")
            }
            Self::SnapshotAction(action) => {
                write!(f, "snapshot contains {action:?} after its Clear")
            }
            Self::ClearInIncremental => write!(f, "Clear inside an incremental event group"),
            Self::IncrementalBeforeSnapshot => {
                write!(f, "incremental event group before a snapshot")
            }
            Self::SequenceNotIncreasing { previous, sequence } => {
                write!(f, "sequence {sequence} does not follow {previous}")
            }
            Self::OutputWhileClosed => write!(f, "output while the book is closed"),
            Self::Integrity(e) => write!(f, "book integrity: {e}"),
            Self::TruthMismatch { sequence } => {
                write!(f, "book differs from venue truth at sequence {sequence}")
            }
            Self::Unsynced => write!(f, "no snapshot to verify"),
        }
    }
}

impl std::error::Error for BookContractViolation {}

/// Checks adapter order book output against the book stream contract.
#[derive(Debug)]
pub struct BookStreamChecker {
    book_type: BookType,
    sequenced: bool,
    books: AHashMap<InstrumentId, CheckedBook>,
}

impl BookStreamChecker {
    /// Creates a checker; `sequenced` requires each incremental event group's sequence to exceed
    /// the previous one.
    ///
    /// Emitted deltas do not carry a venue's linkage fields, so the adapter's tracker validates
    /// linkage. Pass `false` for a venue whose sequence can reset within an episode, such as OKX,
    /// and rely on [`Self::verify`] against venue truth.
    #[must_use]
    pub fn new(book_type: BookType, sequenced: bool) -> Self {
        Self {
            book_type,
            sequenced,
            books: AHashMap::new(),
        }
    }

    /// Accepts output for a subscribed book; its next batch must be a snapshot.
    pub fn open(&mut self, instrument_id: InstrumentId) {
        let book_type = self.book_type;

        let checked = self
            .books
            .entry(instrument_id)
            .or_insert_with(|| CheckedBook {
                book: OrderBook::new(instrument_id, book_type),
                open: false,
                sequence: None,
                episodes: 0,
                verified: 0,
                episode_verified: false,
            });

        checked.open = true;
        checked.sequence = None;
    }

    /// Rejects later output for a book after a settled unsubscribe or client stop.
    pub fn close(&mut self, instrument_id: InstrumentId) {
        if let Some(checked) = self.books.get_mut(&instrument_id) {
            checked.open = false;
        }
    }

    /// Applies one emitted batch.
    ///
    /// # Errors
    ///
    /// Returns the first contract violation the batch causes.
    pub fn apply(&mut self, deltas: &OrderBookDeltas) -> Result<(), BookContractViolation> {
        let checked = self
            .books
            .get_mut(&deltas.instrument_id)
            .filter(|checked| checked.open)
            .ok_or(BookContractViolation::OutputWhileClosed)?;

        if !deltas
            .deltas
            .last()
            .is_some_and(|last| RecordFlag::F_LAST.matches(last.flags))
        {
            return Err(BookContractViolation::MissingLast);
        }

        // Consumers see the book at every event boundary, so each group must leave it valid
        for group in deltas
            .deltas
            .split_inclusive(|delta| RecordFlag::F_LAST.matches(delta.flags))
        {
            checked.check_group(group, self.sequenced)?;

            let group = OrderBookDeltas::new(deltas.instrument_id, group.to_vec());
            checked
                .book
                .apply_deltas(&group)
                .map_err(|e| BookContractViolation::Integrity(e.to_string()))?;
            book_check_integrity(&checked.book)
                .map_err(|e| BookContractViolation::Integrity(e.to_string()))?;
        }

        Ok(())
    }

    /// Compares the top `depth` levels with venue truth, counting the current episode verified.
    ///
    /// # Errors
    ///
    /// Returns [`BookContractViolation::TruthMismatch`] when either side differs, or
    /// [`BookContractViolation::Unsynced`] before a snapshot since the book opened.
    pub fn verify(
        &mut self,
        instrument_id: InstrumentId,
        depth: usize,
        bids: &BTreeMap<Decimal, Decimal>,
        asks: &BTreeMap<Decimal, Decimal>,
    ) -> Result<(), BookContractViolation> {
        let checked = self
            .books
            .get_mut(&instrument_id)
            .ok_or(BookContractViolation::Unsynced)?;
        let sequence = checked.sequence.ok_or(BookContractViolation::Unsynced)?;

        let book_bids: BTreeMap<_, _> = checked.book.bids_as_map(Some(depth)).into_iter().collect();
        let book_asks: BTreeMap<_, _> = checked.book.asks_as_map(Some(depth)).into_iter().collect();

        if book_bids != *bids || book_asks != *asks {
            return Err(BookContractViolation::TruthMismatch { sequence });
        }

        if !checked.episode_verified {
            checked.episode_verified = true;
            checked.verified += 1;
        }

        Ok(())
    }

    /// Returns the maintained book.
    #[must_use]
    pub fn book(&self, instrument_id: InstrumentId) -> Option<&OrderBook> {
        self.books.get(&instrument_id).map(|checked| &checked.book)
    }

    /// Returns snapshot episodes and verified episodes across all books.
    #[must_use]
    pub fn coverage(&self) -> (u64, u64) {
        self.books.values().fold((0, 0), |(episodes, verified), c| {
            (episodes + c.episodes, verified + c.verified)
        })
    }
}

#[derive(Debug)]
struct CheckedBook {
    book: OrderBook,
    open: bool,
    sequence: Option<u64>,
    episodes: u64,
    verified: u64,
    episode_verified: bool,
}

impl CheckedBook {
    fn check_group(
        &mut self,
        group: &[OrderBookDelta],
        sequenced: bool,
    ) -> Result<(), BookContractViolation> {
        let snapshot = group[0].action == BookAction::Clear;
        let sequence = group[group.len() - 1].sequence;

        if group
            .iter()
            .any(|delta| RecordFlag::F_SNAPSHOT.matches(delta.flags) != snapshot)
        {
            return Err(BookContractViolation::SnapshotFlagMismatch);
        }

        if snapshot {
            if let Some(delta) = group[1..]
                .iter()
                .find(|delta| delta.action != BookAction::Add)
            {
                return Err(BookContractViolation::SnapshotAction(delta.action));
            }

            self.episodes += 1;
            self.episode_verified = false;
        } else {
            if group.iter().any(|delta| delta.action == BookAction::Clear) {
                return Err(BookContractViolation::ClearInIncremental);
            }

            let Some(previous) = self.sequence else {
                return Err(BookContractViolation::IncrementalBeforeSnapshot);
            };

            if sequenced && sequence <= previous {
                return Err(BookContractViolation::SequenceNotIncreasing { previous, sequence });
            }
        }

        self.sequence = Some(sequence);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{BookOrder, OrderBookDelta},
        enums::OrderSide,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    const LAST: u8 = RecordFlag::F_LAST as u8;
    const SNAPSHOT: u8 = RecordFlag::F_SNAPSHOT as u8;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTCUSDT.VENUE")
    }

    fn delta(
        action: BookAction,
        side: OrderSide,
        price: &str,
        size: &str,
        flags: u8,
        sequence: u64,
    ) -> OrderBookDelta {
        let order = BookOrder::new(side, Price::from(price), Quantity::from(size), 0);

        OrderBookDelta::new(
            instrument_id(),
            action,
            order,
            flags,
            sequence,
            UnixNanos::from(sequence),
            UnixNanos::from(sequence),
        )
    }

    fn batch(deltas: Vec<OrderBookDelta>) -> OrderBookDeltas {
        OrderBookDeltas::new(instrument_id(), deltas)
    }

    fn clear(flags: u8, sequence: u64) -> OrderBookDelta {
        let mut delta = OrderBookDelta::clear(
            instrument_id(),
            sequence,
            UnixNanos::from(sequence),
            UnixNanos::from(sequence),
        );
        delta.flags = flags;
        delta
    }

    fn snapshot(sequence: u64) -> OrderBookDeltas {
        batch(vec![
            clear(SNAPSHOT, sequence),
            delta(
                BookAction::Add,
                OrderSide::Buy,
                "100.0",
                "1.0",
                SNAPSHOT,
                sequence,
            ),
            delta(
                BookAction::Add,
                OrderSide::Sell,
                "101.0",
                "2.0",
                SNAPSHOT | LAST,
                sequence,
            ),
        ])
    }

    fn update(price: &str, size: &str, sequence: u64) -> OrderBookDeltas {
        batch(vec![delta(
            BookAction::Update,
            OrderSide::Buy,
            price,
            size,
            LAST,
            sequence,
        )])
    }

    fn checker() -> BookStreamChecker {
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(instrument_id());
        checker
    }

    #[rstest]
    fn conforming_stream_passes_and_counts_verified_episode() {
        let mut checker = checker();

        checker.apply(&snapshot(10)).unwrap();
        checker.apply(&update("100.0", "3.0", 11)).unwrap();
        let verified = checker.verify(
            instrument_id(),
            10,
            &BTreeMap::from([(dec!(100.0), dec!(3.0))]),
            &BTreeMap::from([(dec!(101.0), dec!(2.0))]),
        );

        assert_eq!(verified, Ok(()));
        assert_eq!(checker.coverage(), (1, 1));
    }

    #[rstest]
    fn empty_snapshot_is_a_lone_clear() {
        let mut checker = checker();

        let result = checker.apply(&batch(vec![clear(SNAPSHOT | LAST, 5)]));

        assert_eq!(result, Ok(()));
        assert_eq!(checker.book(instrument_id()).unwrap().bids(None).count(), 0);
    }

    // Each planted fault must fail the checker, proving the oracle can see it
    #[rstest]
    #[case::missing_last(
        vec![delta(BookAction::Update, OrderSide::Buy, "100.0", "1.0", 0, 11)],
        BookContractViolation::MissingLast,
    )]
    #[case::snapshot_ended_early(
        vec![
            clear(SNAPSHOT, 11),
            delta(BookAction::Add, OrderSide::Buy, "100.0", "1.0", SNAPSHOT | LAST, 11),
            delta(BookAction::Add, OrderSide::Buy, "99.0", "1.0", SNAPSHOT | LAST, 11),
        ],
        BookContractViolation::SnapshotFlagMismatch,
    )]
    #[case::incremental_flagged_snapshot(
        vec![delta(BookAction::Update, OrderSide::Buy, "100.0", "1.0", SNAPSHOT | LAST, 11)],
        BookContractViolation::SnapshotFlagMismatch,
    )]
    #[case::snapshot_missing_flag(
        vec![
            clear(SNAPSHOT, 11),
            delta(BookAction::Add, OrderSide::Buy, "100.0", "1.0", LAST, 11),
        ],
        BookContractViolation::SnapshotFlagMismatch,
    )]
    #[case::snapshot_update_action(
        vec![
            clear(SNAPSHOT, 11),
            delta(BookAction::Update, OrderSide::Buy, "100.0", "1.0", SNAPSHOT | LAST, 11),
        ],
        BookContractViolation::SnapshotAction(BookAction::Update),
    )]
    #[case::clear_in_incremental(
        vec![
            delta(BookAction::Update, OrderSide::Buy, "100.0", "1.0", 0, 11),
            clear(LAST, 11),
        ],
        BookContractViolation::ClearInIncremental,
    )]
    #[case::duplicate_sequence(
        vec![delta(BookAction::Update, OrderSide::Buy, "100.0", "1.0", LAST, 10)],
        BookContractViolation::SequenceNotIncreasing { previous: 10, sequence: 10 },
    )]
    #[case::crossed_book(
        vec![delta(BookAction::Add, OrderSide::Buy, "102.0", "1.0", LAST, 11)],
        BookContractViolation::Integrity(String::new()),
    )]
    fn planted_fault_fails_checker(
        #[case] deltas: Vec<OrderBookDelta>,
        #[case] expected: BookContractViolation,
    ) {
        let mut checker = checker();
        checker.apply(&snapshot(10)).unwrap();

        let result = checker.apply(&batch(deltas));

        match (result, expected) {
            (Err(BookContractViolation::Integrity(_)), BookContractViolation::Integrity(_)) => {}
            (result, expected) => assert_eq!(result, Err(expected)),
        }
    }

    #[rstest]
    fn batch_of_event_groups_checks_each_group() {
        let mut checker = checker();
        let deltas = batch(vec![
            clear(SNAPSHOT, 10),
            delta(
                BookAction::Add,
                OrderSide::Buy,
                "100.0",
                "1.0",
                SNAPSHOT,
                10,
            ),
            delta(
                BookAction::Add,
                OrderSide::Sell,
                "101.0",
                "2.0",
                SNAPSHOT | LAST,
                10,
            ),
            delta(BookAction::Update, OrderSide::Buy, "100.0", "3.0", LAST, 11),
            delta(BookAction::Update, OrderSide::Buy, "100.0", "4.0", LAST, 12),
        ]);

        let result = checker.apply(&deltas);

        assert_eq!(result, Ok(()));
        assert_eq!(checker.coverage(), (1, 0));
        assert_eq!(
            checker
                .book(instrument_id())
                .unwrap()
                .bids_as_map(None)
                .get(&dec!(100.0)),
            Some(&dec!(4.0))
        );
    }

    #[rstest]
    fn crossed_book_at_group_boundary_fails_even_if_later_repaired() {
        let mut checker = checker();
        checker.apply(&snapshot(10)).unwrap();

        let result = checker.apply(&batch(vec![
            delta(BookAction::Add, OrderSide::Buy, "102.0", "1.0", LAST, 11),
            delta(BookAction::Delete, OrderSide::Buy, "102.0", "1.0", LAST, 12),
        ]));

        assert!(matches!(result, Err(BookContractViolation::Integrity(_))));
    }

    #[rstest]
    fn incremental_before_snapshot_fails() {
        let mut checker = checker();

        let result = checker.apply(&update("100.0", "1.0", 11));

        assert_eq!(
            result,
            Err(BookContractViolation::IncrementalBeforeSnapshot)
        );
    }

    #[rstest]
    fn output_after_close_fails_until_reopened() {
        let mut checker = checker();
        checker.apply(&snapshot(10)).unwrap();
        checker.close(instrument_id());

        let closed = checker.apply(&update("100.0", "1.0", 11));
        checker.open(instrument_id());
        let reopened_incremental = checker.apply(&update("100.0", "1.0", 12));
        let reopened_snapshot = checker.apply(&snapshot(13));

        assert_eq!(closed, Err(BookContractViolation::OutputWhileClosed));
        assert_eq!(
            reopened_incremental,
            Err(BookContractViolation::IncrementalBeforeSnapshot)
        );
        assert_eq!(reopened_snapshot, Ok(()));
    }

    #[rstest]
    fn dropped_update_fails_truth_comparison() {
        let mut checker = checker();
        checker.apply(&snapshot(10)).unwrap();

        // The venue moved the bid to 3.0 at sequence 11, but only sequence 12 was emitted
        checker.apply(&update("99.0", "1.0", 12)).unwrap();
        let result = checker.verify(
            instrument_id(),
            10,
            &BTreeMap::from([(dec!(100.0), dec!(3.0)), (dec!(99.0), dec!(1.0))]),
            &BTreeMap::from([(dec!(101.0), dec!(2.0))]),
        );

        assert_eq!(
            result,
            Err(BookContractViolation::TruthMismatch { sequence: 12 })
        );
        assert_eq!(checker.coverage(), (1, 0));
    }

    #[rstest]
    fn ask_mismatch_fails_truth_comparison() {
        let mut checker = checker();
        checker.apply(&snapshot(10)).unwrap();

        let result = checker.verify(
            instrument_id(),
            10,
            &BTreeMap::from([(dec!(100.0), dec!(1.0))]),
            &BTreeMap::from([(dec!(101.0), dec!(5.0))]),
        );

        assert_eq!(
            result,
            Err(BookContractViolation::TruthMismatch { sequence: 10 })
        );
        assert_eq!(checker.coverage(), (1, 0));
    }

    #[rstest]
    #[case::never_synced(false)]
    #[case::reopened(true)]
    fn verify_requires_snapshot_since_open(#[case] reopened: bool) {
        let mut checker = checker();

        if reopened {
            checker.apply(&snapshot(10)).unwrap();
            checker.open(instrument_id());
        }

        let result = checker.verify(instrument_id(), 10, &BTreeMap::new(), &BTreeMap::new());

        assert_eq!(result, Err(BookContractViolation::Unsynced));
    }

    #[rstest]
    fn unsequenced_checker_accepts_repeated_sequence() {
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, false);
        checker.open(instrument_id());
        checker.apply(&snapshot(0)).unwrap();

        let result = checker.apply(&update("100.0", "1.0", 0));

        assert_eq!(result, Ok(()));
    }
}
