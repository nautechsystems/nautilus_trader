#![no_main]

//! Fuzz book sequencing and conversion across `book` and `book_l3` message sequences.
//!
//! The input drives snapshots, updates, reconnects, resubscriptions, and depth messages for one
//! book. A reference model of the resting orders predicts whether each L3 snapshot and contiguous
//! update is valid and which deltas it emits; level batches are checked against their messages.
//! Across the whole run:
//!
//! - Emitted sequences strictly rise, including across reconnects and resubscriptions.
//! - An update is accepted only when it continues the last accepted venue sequence.
//! - No update is accepted after a rejected message until a snapshot is accepted.
//! - An L3 snapshot or contiguous update is accepted exactly when the reference model can apply it.
//! - The reference model never holds a non-positive remaining size.

#[path = "../common/mod.rs"]
mod common;

use ahash::AHashMap;
use common::{assert_batch, assert_level_deltas, currency_pair, nanos};
use jiff::Timestamp;
use nautilus_coinapi::{
    book::sync::{BookFrame, BookSyncTracker},
    http::models::BookLevel,
    websocket::messages::{
        Book, BookL3, BookL3Entry, BookUpdateType, Channel, DataType, Subscription,
    },
};
use nautilus_common::live::dst::time::Instant;
use nautilus_live::{
    book::{BookSequenceOutcome, snapshot::SnapshotGate},
    fuzz::fuzz_target,
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    instruments::{Instrument, InstrumentAny},
};
use rust_decimal::Decimal;
use ustr::Ustr;

const SYMBOL_ID: &str = "COINBASE_SPOT_BTC_USD";
const PRICE_PRECISION: u8 = 2;
const SIZE_PRECISION: u8 = 4;
const ORDER_IDS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];
const ENTRIES_MAX: u8 = 6;

fuzz_target!(|data: &[u8]| {
    let mut input = Input(data);
    let Some(mode) = input.byte() else {
        return;
    };
    let data_type = if mode % 2 == 0 {
        DataType::Book
    } else {
        DataType::BookL3
    };

    let mut run = Run::new(data_type);

    while let Some(op) = input.byte() {
        match op % 8 {
            0 | 1 => run.message(&mut input, true),
            2..=5 => run.message(&mut input, false),
            6 => run.restart(op >= 128),
            _ => run.depth(&mut input),
        }
    }
});

struct Input<'a>(&'a [u8]);

impl Input<'_> {
    fn byte(&mut self) -> Option<u8> {
        let (&first, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(first)
    }

    fn byte_or_zero(&mut self) -> u8 {
        self.byte().unwrap_or(0)
    }
}

struct Run {
    data_type: DataType,
    instrument: InstrumentAny,
    tracker: BookSyncTracker,
    now: Instant,
    // Venue sequence of the last message sent to the tracker
    sent: u64,
    // Venue sequence of the last accepted message, while the book is synced
    synced: Option<u64>,
    emitted: Option<u64>,
    // Emitted minus venue sequence within the current snapshot episode
    offset: Option<u64>,
    depth_emitted: Option<u64>,
    // Resting L3 orders by venue identifier, as last emitted
    orders: AHashMap<&'static str, BookOrder>,
}

impl Run {
    fn new(data_type: DataType) -> Self {
        let run = Self {
            data_type,
            instrument: currency_pair("BTC-USD.COINBASE", PRICE_PRECISION, SIZE_PRECISION),
            tracker: BookSyncTracker::default(),
            now: Instant::now(),
            sent: 0,
            synced: None,
            emitted: None,
            offset: None,
            depth_emitted: None,
            orders: AHashMap::new(),
        };

        run.subscribe();
        run
    }

    fn subscribe(&self) {
        let subscription = Subscription {
            channel: Channel {
                data_type: self.data_type,
                period_id: None,
            },
            symbol_id: Ustr::from(SYMBOL_ID),
        };

        self.tracker.record_subscription(
            self.instrument.id(),
            subscription,
            self.now,
            SnapshotGate::default(),
        );
    }

    fn restart(&mut self, resubscribe: bool) {
        if resubscribe {
            self.subscribe();
        } else {
            self.tracker.reset_on_reconnect(None);
        }

        self.synced = None;
    }

    fn depth(&mut self, input: &mut Input) {
        let sequence = u64::from(input.byte_or_zero());
        let emitted = self.tracker.depth_sequence(self.instrument.id(), sequence);
        assert!(self.depth_emitted.is_none_or(|last| emitted > last));
        self.depth_emitted = Some(emitted);
    }

    fn message(&mut self, input: &mut Input, is_snapshot: bool) {
        self.sent = match input.byte_or_zero() % 8 {
            0..=4 => self.sent + 1,
            5 => self.sent,
            6 => self.sent + 2,
            _ => u64::from(input.byte_or_zero()),
        };

        let sequence = self.sent;

        let time_exchange = if input.byte_or_zero() == u8::MAX {
            Timestamp::MIN
        } else {
            Timestamp::UNIX_EPOCH
        };

        let count = input.byte_or_zero() % (ENTRIES_MAX + 1);
        let (bids, asks): (Vec<_>, Vec<_>) = (0..count)
            .map(|_| entry(input, is_snapshot))
            .partition(|(side, _)| *side == OrderSide::Buy);
        let bids: Vec<BookL3Entry> = bids.into_iter().map(|(_, entry)| entry).collect();
        let asks: Vec<BookL3Entry> = asks.into_iter().map(|(_, entry)| entry).collect();

        let instrument_id = self.instrument.id();

        let (outcome, deltas, levels, orders) = if self.data_type == DataType::Book {
            let level = |entry: &BookL3Entry| BookLevel {
                id: None,
                price: entry.price,
                size: entry.size,
            };

            let book = Book {
                data_type: DataType::Book,
                symbol_id: Ustr::from(SYMBOL_ID),
                sequence,
                time_exchange,
                time_coinapi: Timestamp::UNIX_EPOCH,
                is_snapshot: Some(is_snapshot),
                asks: asks.iter().map(level).collect(),
                bids: bids.iter().map(level).collect(),
            };

            let (outcome, deltas) = self.tracker.apply(
                instrument_id,
                BookFrame::Levels(&book),
                &self.instrument,
                self.now,
            );
            (outcome, deltas, Some(book), None)
        } else {
            let book = BookL3 {
                symbol_id: Ustr::from(SYMBOL_ID),
                sequence,
                time_exchange,
                time_coinapi: Timestamp::UNIX_EPOCH,
                is_snapshot,
                asks,
                bids,
            };

            let (outcome, deltas) = self.tracker.apply(
                instrument_id,
                BookFrame::Orders(&book),
                &self.instrument,
                self.now,
            );
            (outcome, deltas, None, Some(book))
        };

        let contiguous =
            is_snapshot || self.synced.and_then(|synced| synced.checked_add(1)) == Some(sequence);
        let changes = orders
            .as_ref()
            .filter(|_| contiguous)
            .map(|book| replay(&self.orders, book));

        if let (Some(changes), Some(book)) = (&changes, &orders) {
            assert_eq!(
                outcome == BookSequenceOutcome::Accept,
                changes.is_some(),
                "tracker and reference model disagree on {book:?}"
            );
        }

        if outcome != BookSequenceOutcome::Accept {
            assert!(deltas.is_none());
            self.synced = None;
            return;
        }

        if !is_snapshot {
            assert_eq!(
                self.synced.and_then(|synced| synced.checked_add(1)),
                Some(sequence),
                "update accepted without continuing the synced sequence"
            );
        }

        self.synced = Some(sequence);

        let emitted = deltas.as_ref().map(|deltas| deltas.sequence);
        if let Some(emitted) = emitted {
            assert!(
                self.emitted.is_none_or(|last| emitted > last),
                "emitted sequence {emitted} after {:?}",
                self.emitted
            );
            self.emitted = Some(emitted);
            if is_snapshot {
                self.offset = Some(emitted - sequence);
            } else {
                assert_eq!(
                    Some(emitted - sequence),
                    self.offset,
                    "offset changed in episode"
                );
            }
        }

        match (levels, orders, deltas) {
            (Some(book), None, Some(deltas)) => {
                assert_level_deltas(&book, &deltas, emitted.unwrap());
            }
            (Some(book), None, None) => {
                assert!(!is_snapshot && book.bids.is_empty() && book.asks.is_empty());
            }
            (None, Some(book), deltas) => {
                let changes = changes.flatten().expect("accepted L3 message was replayed");
                self.check_orders(&book, &changes, deltas.as_ref());
            }
            _ => unreachable!(),
        }
    }

    // Applies an accepted L3 message's changes to the reference model, matching the emitted deltas
    fn check_orders(
        &mut self,
        book: &BookL3,
        changes: &[Change],
        deltas: Option<&OrderBookDeltas>,
    ) {
        let emitted = deltas.map_or(&[][..], |deltas| &deltas.deltas[..]);
        let mut emitted = emitted.iter();

        if book.is_snapshot {
            assert_eq!(emitted.next().unwrap().action, BookAction::Clear);
            self.orders.clear();
        }

        for change in changes {
            let delta = emitted.next().expect("change without an emitted delta");

            match *change {
                Change::Add {
                    id,
                    side,
                    price,
                    size,
                } => {
                    assert_eq!(delta.action, BookAction::Add);
                    assert!(
                        self.orders
                            .values()
                            .all(|order| order.order_id != delta.order.order_id),
                        "ADD reused a resting order ID"
                    );
                    assert_order(delta, side, price, size);
                    self.orders.insert(id, delta.order);
                }
                Change::Update {
                    id,
                    side,
                    price,
                    size,
                } => {
                    assert_eq!(delta.action, BookAction::Update);
                    assert_eq!(delta.order.order_id, self.orders[id].order_id);
                    assert_order(delta, side, price, size);
                    self.orders.insert(id, delta.order);
                }
                Change::Delete { id } => {
                    assert_eq!(delta.action, BookAction::Delete);
                    // `BookOrder` equality compares only the order ID
                    let resting = self.orders[id];
                    assert_eq!(
                        (
                            delta.order.order_id,
                            delta.order.side,
                            delta.order.price,
                            delta.order.size
                        ),
                        (resting.order_id, resting.side, resting.price, resting.size),
                        "DELETE of a changed resting order"
                    );
                    self.orders.remove(id);
                }
            }
        }

        assert!(emitted.next().is_none(), "unmatched emitted delta");
        assert!(
            self.orders
                .values()
                .all(|order| order.size.as_decimal() > Decimal::ZERO),
            "reference model holds a non-positive remaining size"
        );

        if let Some(deltas) = deltas {
            let snapshot_flag = if book.is_snapshot {
                RecordFlag::F_SNAPSHOT as u8
            } else {
                0
            };

            assert_batch(deltas, deltas.sequence, snapshot_flag);
        }
    }
}

// A change to one resting order that an L3 message makes, in message order
enum Change {
    Add {
        id: &'static str,
        side: OrderSide,
        price: Decimal,
        size: Decimal,
    },
    Update {
        id: &'static str,
        side: OrderSide,
        price: Decimal,
        size: Decimal,
    },
    Delete {
        id: &'static str,
    },
}

/// Replays an L3 message on the resting `orders`, returning the changes it makes, or `None` when
/// the tracker must reject it.
fn replay(orders: &AHashMap<&'static str, BookOrder>, book: &BookL3) -> Option<Vec<Change>> {
    nanos(book.time_exchange)?;
    nanos(book.time_coinapi)?;

    let mut sizes: AHashMap<&'static str, Decimal> = if book.is_snapshot {
        AHashMap::new()
    } else {
        orders
            .iter()
            .map(|(id, order)| (*id, order.size.as_decimal()))
            .collect()
    };

    let entries = book
        .bids
        .iter()
        .map(|entry| (OrderSide::Buy, entry))
        .chain(book.asks.iter().map(|entry| (OrderSide::Sell, entry)));

    let mut changes = Vec::new();
    for (side, entry) in entries {
        replay_entry(&mut sizes, &mut changes, book.is_snapshot, side, entry)?;
    }

    Some(changes)
}

// Records the entry's change, if any, or returns `None` when the entry makes the tracker reject its
// message
fn replay_entry(
    sizes: &mut AHashMap<&'static str, Decimal>,
    changes: &mut Vec<Change>,
    is_snapshot: bool,
    side: OrderSide,
    entry: &BookL3Entry,
) -> Option<()> {
    let id = order_id(entry)?;
    let price = entry.price;
    if is_snapshot || entry.update_type == Some(BookUpdateType::Add) {
        let size = entry.size;

        if sizes.contains_key(id)
            || size <= Decimal::ZERO
            || !fits(price, PRICE_PRECISION)
            || !fits(size, SIZE_PRECISION)
        {
            return None;
        }

        sizes.insert(id, size);
        changes.push(Change::Add {
            id,
            side,
            price,
            size,
        });

        return Some(());
    }

    let Some(&resting) = sizes.get(id) else {
        return (entry.update_type == Some(BookUpdateType::Delete)).then_some(());
    };

    let size = match entry.update_type {
        Some(BookUpdateType::Update) => entry.size,
        Some(BookUpdateType::Subtract | BookUpdateType::Match)
            if !entry.size.is_sign_negative() =>
        {
            resting - entry.size
        }
        Some(BookUpdateType::Delete) => Decimal::ZERO,
        _ => return None,
    };

    if size.is_sign_negative() {
        return None;
    }

    if size.is_zero() {
        sizes.remove(id);
        changes.push(Change::Delete { id });
        return Some(());
    }

    if !fits(price, PRICE_PRECISION) || !fits(size, SIZE_PRECISION) {
        return None;
    }

    sizes.insert(id, size);
    changes.push(Change::Update {
        id,
        side,
        price,
        size,
    });

    Some(())
}

fn fits(value: Decimal, precision: u8) -> bool {
    value.normalize().scale() <= u32::from(precision)
}

fn assert_order(delta: &OrderBookDelta, side: OrderSide, price: Decimal, size: Decimal) {
    assert_eq!(delta.order.side, Some(side));
    assert_eq!(delta.order.price.as_decimal(), price);
    assert_eq!(delta.order.size.as_decimal(), size);
    assert!(size > Decimal::ZERO);
}

fn order_id(entry: &BookL3Entry) -> Option<&'static str> {
    let id = entry.id.as_deref()?;
    ORDER_IDS.into_iter().find(|known| *known == id)
}

fn entry(input: &mut Input, is_snapshot: bool) -> (OrderSide, BookL3Entry) {
    let control = input.byte_or_zero();

    let side = if control & 1 == 0 {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    let id = (control >> 1) % 8;
    let id = ORDER_IDS.get(usize::from(id)).map(|id| (*id).to_string());

    // Occasionally one digit finer than the instrument precision
    let fine = control >= 224;
    let price_ticks = i64::from(u16::from_le_bytes([
        input.byte_or_zero(),
        input.byte_or_zero(),
    ]));
    let price = Decimal::new(price_ticks, u32::from(PRICE_PRECISION) + u32::from(fine));
    let size_units = i64::from(i16::from_le_bytes([
        input.byte_or_zero(),
        input.byte_or_zero(),
    ]));
    let size = Decimal::new(size_units, u32::from(SIZE_PRECISION) + u32::from(fine));

    let update_type = if is_snapshot {
        None
    } else {
        match input.byte_or_zero() % 7 {
            0 => Some(BookUpdateType::Add),
            1 => Some(BookUpdateType::Update),
            2 => Some(BookUpdateType::Subtract),
            3 => Some(BookUpdateType::Delete),
            4 => Some(BookUpdateType::Match),
            5 => Some(BookUpdateType::Other),
            _ => None,
        }
    };

    (
        side,
        BookL3Entry {
            id,
            price,
            size,
            update_type,
        },
    )
}
