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

//! Resting orders of one CoinAPI `book_l3` stream.
//!
//! CoinAPI identifies orders by string and reports `SUBTRACT` and `MATCH` sizes as the amount
//! removed from an order, so [`OrderIndex`] keeps each resting order's side, price, and remaining
//! size under a numeric Nautilus order ID. An order that `SUBTRACT` or `MATCH` exhausts is deleted
//! at once, so a later `DELETE` for it is ignored. An entry without an identifier is an
//! aggregated level from a source without order-by-order data, which an L3 book cannot hold.

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use rust_decimal::Decimal;

use crate::websocket::{
    messages::{BookL3, BookL3Entry, BookUpdateType},
    parse::{price, quantity, timestamp},
};

/// Resting orders by CoinAPI identifier.
#[derive(Debug, Default)]
pub(crate) struct OrderIndex {
    orders: AHashMap<String, BookOrder>,
    order_id_last: u64,
}

impl OrderIndex {
    /// Builds the index from a snapshot, returning it with the snapshot's deltas.
    ///
    /// # Errors
    ///
    /// Returns an error if an entry has no or a repeated identifier, is finer than the instrument
    /// precision, or has a non-positive size, or a timestamp is invalid.
    pub(crate) fn snapshot(
        book: &BookL3,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<(Self, OrderBookDeltas)> {
        let mut index = Self::default();
        let mut batch = Batch::new(book, instrument, RecordFlag::F_SNAPSHOT as u8)?;
        batch.deltas.push(OrderBookDelta::clear(
            batch.instrument_id,
            book.sequence,
            batch.ts_event,
            batch.ts_init,
        ));

        for (side, entry) in entries(book) {
            let id = order_id(entry)?;
            let order = index.insert(id, side, entry, instrument)?;
            batch.push(BookAction::Add, order)?;
        }

        Ok((index, batch.finish().expect("snapshot starts with a clear")))
    }

    /// Applies an update, returning its deltas or `None` when it changes no resting order.
    ///
    /// # Errors
    ///
    /// Returns an error if an entry has no identifier, adds a resting order, changes an unknown
    /// order, subtracts a negative amount or more than an order holds, has an unsupported update
    /// type, or is finer than the instrument precision, or a timestamp is invalid. The index is
    /// then inconsistent until the next snapshot replaces it.
    pub(crate) fn update(
        &mut self,
        book: &BookL3,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Option<OrderBookDeltas>> {
        let mut batch = Batch::new(book, instrument, 0)?;

        for (side, entry) in entries(book) {
            let id = order_id(entry)?;
            let update_type = entry.update_type;

            if update_type == Some(BookUpdateType::Add) {
                anyhow::ensure!(!self.orders.contains_key(id), "ADD for resting order {id}");
                let order = self.insert(id, side, entry, instrument)?;
                batch.push(BookAction::Add, order)?;
                continue;
            }

            let Some(resting) = self.orders.get(id).copied() else {
                anyhow::ensure!(
                    update_type == Some(BookUpdateType::Delete),
                    "{update_type:?} for unknown order {id}"
                );
                continue;
            };

            let remaining = match update_type {
                Some(BookUpdateType::Update) => entry.size,
                Some(BookUpdateType::Subtract | BookUpdateType::Match) => {
                    anyhow::ensure!(
                        !entry.size.is_sign_negative(),
                        "{update_type:?} of negative {} for order {id}",
                        entry.size
                    );
                    resting.size.as_decimal() - entry.size
                }
                Some(BookUpdateType::Delete) => Decimal::ZERO,
                other => anyhow::bail!("unsupported update type {other:?} for order {id}"),
            };

            anyhow::ensure!(
                !remaining.is_sign_negative(),
                "{update_type:?} of {} exceeds order {id} size {}",
                entry.size,
                resting.size
            );

            if remaining.is_zero() {
                self.orders.remove(id);
                batch.push(BookAction::Delete, resting)?;
                continue;
            }

            let order = BookOrder::new(
                side,
                price(entry.price, instrument, "price")?,
                quantity(remaining, instrument, "size")?,
                resting.order_id,
            );
            self.orders.insert(id.to_owned(), order);
            batch.push(BookAction::Update, order)?;
        }

        Ok(batch.finish())
    }

    fn insert(
        &mut self,
        id: &str,
        side: OrderSide,
        entry: &BookL3Entry,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<BookOrder> {
        self.order_id_last += 1;

        let order = BookOrder::new(
            side,
            price(entry.price, instrument, "price")?,
            quantity(entry.size, instrument, "size")?,
            self.order_id_last,
        );

        anyhow::ensure!(
            self.orders.insert(id.to_owned(), order).is_none(),
            "repeated order {id}"
        );
        Ok(order)
    }
}

// Deltas of one message, taking `ts_event` from `time_exchange` and `ts_init` from `time_coinapi`
struct Batch {
    instrument_id: InstrumentId,
    sequence: u64,
    flags: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    deltas: Vec<OrderBookDelta>,
}

impl Batch {
    fn new(book: &BookL3, instrument: &InstrumentAny, flags: u8) -> anyhow::Result<Self> {
        Ok(Self {
            instrument_id: instrument.id(),
            sequence: book.sequence,
            flags,
            ts_event: timestamp(book.time_exchange, "time_exchange")?,
            ts_init: timestamp(book.time_coinapi, "time_coinapi")?,
            deltas: Vec::with_capacity(book.bids.len() + book.asks.len() + 1),
        })
    }

    fn push(&mut self, action: BookAction, order: BookOrder) -> anyhow::Result<()> {
        self.deltas.push(OrderBookDelta::new_checked(
            self.instrument_id,
            action,
            order,
            self.flags,
            self.sequence,
            self.ts_event,
            self.ts_init,
        )?);
        Ok(())
    }

    fn finish(mut self) -> Option<OrderBookDeltas> {
        let last = self.deltas.last_mut()?;
        last.flags |= self.flags | RecordFlag::F_LAST as u8;
        Some(OrderBookDeltas::new(self.instrument_id, self.deltas))
    }
}

fn entries(book: &BookL3) -> impl Iterator<Item = (OrderSide, &BookL3Entry)> {
    let bids = book.bids.iter().map(|entry| (OrderSide::Buy, entry));
    let asks = book.asks.iter().map(|entry| (OrderSide::Sell, entry));
    bids.chain(asks)
}

fn order_id(entry: &BookL3Entry) -> anyhow::Result<&str> {
    entry.id.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "entry at {} has no order ID; the source provides only aggregated levels",
            entry.price
        )
    })
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::InstrumentId,
        instruments::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    const SNAPSHOT: u8 = RecordFlag::F_SNAPSHOT as u8;
    const LAST: u8 = RecordFlag::F_LAST as u8;

    fn instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::from("BCH-USD.COINBASE"))
                .raw_symbol("BCH-USD".into())
                .base_currency(Currency::get_or_create_crypto("BCH"))
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

    fn entry(
        id: Option<&str>,
        price: &str,
        size: &str,
        update_type: Option<BookUpdateType>,
    ) -> BookL3Entry {
        BookL3Entry {
            id: id.map(Into::into),
            price: Decimal::from_str_exact(price).unwrap(),
            size: Decimal::from_str_exact(size).unwrap(),
            update_type,
        }
    }

    fn message(
        sequence: u64,
        is_snapshot: bool,
        bids: Vec<BookL3Entry>,
        asks: Vec<BookL3Entry>,
    ) -> BookL3 {
        let mut book: BookL3 =
            serde_json::from_str(include_str!("../../test_data/ws_book_l3.json")).unwrap();
        book.sequence = sequence;
        book.is_snapshot = is_snapshot;
        book.bids = bids;
        book.asks = asks;
        book
    }

    fn delta(
        action: BookAction,
        side: OrderSide,
        price: &str,
        size: &str,
        order_id: u64,
        flags: u8,
        sequence: u64,
    ) -> OrderBookDelta {
        OrderBookDelta::new(
            InstrumentId::from("BCH-USD.COINBASE"),
            action,
            BookOrder::new(side, Price::from(price), Quantity::from(size), order_id),
            flags,
            sequence,
            UnixNanos::from(1_598_445_364_346_470_600),
            UnixNanos::from(1_598_445_364_280_775_000),
        )
    }

    fn snapshot() -> OrderIndex {
        let book = message(
            1,
            true,
            vec![entry(Some("b1"), "272.94", "2.00000000", None)],
            vec![entry(Some("a1"), "273.01", "1.50000000", None)],
        );
        OrderIndex::snapshot(&book, &instrument()).unwrap().0
    }

    #[rstest]
    fn snapshot_clears_then_adds_each_order_with_numeric_id() {
        let book = message(
            1,
            true,
            vec![
                entry(Some("b1"), "272.94", "2", None),
                entry(Some("b2"), "272.94", "0.5", None),
            ],
            vec![entry(Some("a1"), "273.01", "1.5", None)],
        );

        let (_, deltas) = OrderIndex::snapshot(&book, &instrument()).unwrap();

        let mut clear = OrderBookDelta::clear(
            InstrumentId::from("BCH-USD.COINBASE"),
            1,
            UnixNanos::from(1_598_445_364_346_470_600),
            UnixNanos::from(1_598_445_364_280_775_000),
        );
        clear.flags = SNAPSHOT;

        assert_eq!(
            deltas.deltas,
            vec![
                clear,
                delta(
                    BookAction::Add,
                    OrderSide::Buy,
                    "272.94",
                    "2.00000000",
                    1,
                    SNAPSHOT,
                    1
                ),
                delta(
                    BookAction::Add,
                    OrderSide::Buy,
                    "272.94",
                    "0.50000000",
                    2,
                    SNAPSHOT,
                    1
                ),
                delta(
                    BookAction::Add,
                    OrderSide::Sell,
                    "273.01",
                    "1.50000000",
                    3,
                    SNAPSHOT | LAST,
                    1
                ),
            ]
        );
    }

    #[rstest]
    fn update_applies_each_update_type_to_resting_orders() {
        let mut index = snapshot();
        let book = message(
            2,
            false,
            vec![
                entry(Some("b1"), "272.94", "0.25", Some(BookUpdateType::Match)),
                entry(Some("b1"), "272.94", "1", Some(BookUpdateType::Subtract)),
                entry(Some("b2"), "272.90", "3", Some(BookUpdateType::Add)),
                entry(Some("b2"), "272.90", "4", Some(BookUpdateType::Update)),
            ],
            vec![entry(
                Some("a1"),
                "273.01",
                "1.5",
                Some(BookUpdateType::Delete),
            )],
        );

        let deltas = index.update(&book, &instrument()).unwrap().unwrap();

        assert_eq!(
            deltas.deltas,
            vec![
                delta(
                    BookAction::Update,
                    OrderSide::Buy,
                    "272.94",
                    "1.75000000",
                    1,
                    0,
                    2
                ),
                delta(
                    BookAction::Update,
                    OrderSide::Buy,
                    "272.94",
                    "0.75000000",
                    1,
                    0,
                    2
                ),
                delta(
                    BookAction::Add,
                    OrderSide::Buy,
                    "272.90",
                    "3.00000000",
                    3,
                    0,
                    2
                ),
                delta(
                    BookAction::Update,
                    OrderSide::Buy,
                    "272.90",
                    "4.00000000",
                    3,
                    0,
                    2
                ),
                delta(
                    BookAction::Delete,
                    OrderSide::Sell,
                    "273.01",
                    "1.50000000",
                    2,
                    LAST,
                    2
                ),
            ]
        );
    }

    // A fill that exhausts an order removes it, so the venue's later delete finds nothing
    #[rstest]
    fn exhausting_match_deletes_order_and_later_delete_is_ignored() {
        let mut index = snapshot();
        let fill = message(
            2,
            false,
            vec![entry(
                Some("b1"),
                "272.94",
                "2",
                Some(BookUpdateType::Match),
            )],
            vec![],
        );
        let delete = message(
            3,
            false,
            vec![entry(
                Some("b1"),
                "272.94",
                "0",
                Some(BookUpdateType::Delete),
            )],
            vec![],
        );

        let filled = index.update(&fill, &instrument()).unwrap().unwrap();
        let deleted = index.update(&delete, &instrument()).unwrap();

        assert_eq!(
            filled.deltas,
            vec![delta(
                BookAction::Delete,
                OrderSide::Buy,
                "272.94",
                "2.00000000",
                1,
                LAST,
                2
            )]
        );
        assert_eq!(deleted, None);
    }

    #[rstest]
    #[case::aggregated_entry(
        entry(None, "272.93", "4.5", Some(BookUpdateType::Match)),
        "entry at 272.93 has no order ID; the source provides only aggregated levels"
    )]
    #[case::add_resting_order(
        entry(Some("b1"), "272.94", "1", Some(BookUpdateType::Add)),
        "ADD for resting order b1"
    )]
    #[case::unknown_order(
        entry(Some("x"), "272.94", "1", Some(BookUpdateType::Update)),
        "Some(Update) for unknown order x"
    )]
    #[case::negative_subtract(
        entry(Some("b1"), "272.94", "-1", Some(BookUpdateType::Subtract)),
        "Some(Subtract) of negative -1 for order b1"
    )]
    #[case::negative_match(
        entry(Some("b1"), "272.94", "-1", Some(BookUpdateType::Match)),
        "Some(Match) of negative -1 for order b1"
    )]
    #[case::over_subtract(
        entry(Some("b1"), "272.94", "3", Some(BookUpdateType::Subtract)),
        "Some(Subtract) of 3 exceeds order b1 size 2.00000000"
    )]
    #[case::unsupported_type(
        entry(Some("b1"), "272.94", "1", Some(BookUpdateType::Other)),
        "unsupported update type Some(Other) for order b1"
    )]
    #[case::missing_type(
        entry(Some("b1"), "272.94", "1", None),
        "unsupported update type None for order b1"
    )]
    fn inconsistent_update_is_rejected(#[case] bid: BookL3Entry, #[case] expected: &str) {
        let mut index = snapshot();

        let error = index
            .update(&message(2, false, vec![bid], vec![]), &instrument())
            .unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::aggregated_entry(
        vec![entry(None, "272.93", "4.5", None)],
        "entry at 272.93 has no order ID; the source provides only aggregated levels"
    )]
    #[case::repeated_order(
        vec![entry(Some("b1"), "272.94", "1", None), entry(Some("b1"), "272.93", "1", None)],
        "repeated order b1"
    )]
    fn invalid_snapshot_is_rejected(#[case] bids: Vec<BookL3Entry>, #[case] expected: &str) {
        let error =
            OrderIndex::snapshot(&message(1, true, bids, vec![]), &instrument()).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }
}
