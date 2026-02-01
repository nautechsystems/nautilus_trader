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

//! Binary market book views.

use ahash::AHashSet;
use indexmap::IndexMap;
use rust_decimal::Decimal;

use super::{BinaryMarketBookViewError, book::OrderBook, own::OwnOrderBook};
use crate::{
    data::BookOrder,
    enums::{OrderSide, OrderStatus},
    types::{Price, Quantity},
};
use nautilus_core::correctness::FAILED;

/// A filtered book view for binary markets, including synthetic orders.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BinaryMarketBookView {
    pub book: OrderBook,
}

impl BinaryMarketBookView {
    /// Creates a new [`BinaryMarketBookView`] by filtering bids/asks and rebuilding a book.
    /// # Panics
    ///
    /// Panics if Price::from_decimal or Quantity::from_decimal fails when reconstructing orders.
    ///
    /// # Panics
    ///
    /// Panics if `book` and `own_book` have different instrument IDs.
    /// Panics if `book` and `own_synthetic_book` have the same instrument ID.
    /// [`Self::new_checked`] for fallible construction.
    #[must_use]
    pub fn new(
        book: OrderBook,
        own_book: OwnOrderBook,
        own_synthetic_book: OwnOrderBook,
        depth: Option<usize>,
        status: Option<AHashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> Self {
        Self::new_checked(
            book,
            own_book,
            own_synthetic_book,
            depth,
            status,
            accepted_buffer_ns,
            now,
        )
        .expect(FAILED)
    }

    /// Fallible constructor for [`BinaryMarketBookView`].
    ///
    /// # Errors
    ///
    /// Returns [`BinaryMarketBookViewError::BookAndOwnBookMustBeSameInstrumentId`] if `book` and `own_book`
    /// have different instrument IDs.
    /// Returns [`BinaryMarketBookViewError::BookAndOwnSyntheticBookMustBeDifferentInstrumentId`] if `book` and
    /// `own_synthetic_book` have the same instrument ID.
    ///
    /// # Panics
    ///
    /// Panics if Price::from_decimal or Quantity::from_decimal fails when reconstructing orders.
    pub fn new_checked(
        book: OrderBook,
        own_book: OwnOrderBook,
        own_synthetic_book: OwnOrderBook,
        depth: Option<usize>,
        status: Option<AHashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> Result<Self, BinaryMarketBookViewError> {
        if book.instrument_id != own_book.instrument_id {
            return Err(
                BinaryMarketBookViewError::BookAndOwnBookMustBeSameInstrumentId(
                    book.instrument_id,
                    own_book.instrument_id,
                ),
            );
        }

        if book.instrument_id == own_synthetic_book.instrument_id {
            return Err(
                BinaryMarketBookViewError::BookAndOwnSyntheticBookMustBeDifferentInstrumentId(
                    book.instrument_id,
                    own_synthetic_book.instrument_id,
                ),
            );
        }

        let mut bids_map = book
            .bids(depth)
            .map(|level| (level.price.value.as_decimal(), level.size_decimal()))
            .collect::<IndexMap<Decimal, Decimal>>();

        filter_quantities(
            &mut bids_map,
            own_book.bid_quantity(status.clone(), None, None, accepted_buffer_ns, now),
        );

        let synthetic_as_bids = own_synthetic_book
            .ask_quantity(status.clone(), None, None, accepted_buffer_ns, now)
            .into_iter()
            .map(|(price, quantity)| (Decimal::ONE - price, quantity))
            .collect::<IndexMap<Decimal, Decimal>>();

        filter_quantities(&mut bids_map, synthetic_as_bids);

        let mut asks_map = book
            .asks(depth)
            .map(|level| (level.price.value.as_decimal(), level.size_decimal()))
            .collect::<IndexMap<Decimal, Decimal>>();

        filter_quantities(
            &mut asks_map,
            own_book.ask_quantity(status.clone(), None, None, accepted_buffer_ns, now),
        );

        let synthetic_as_asks = own_synthetic_book
            .bid_quantity(status, None, None, accepted_buffer_ns, now)
            .into_iter()
            .map(|(price, quantity)| (Decimal::ONE - price, quantity))
            .collect::<IndexMap<Decimal, Decimal>>();

        filter_quantities(&mut asks_map, synthetic_as_asks);

        let mut filtered_book = OrderBook::new(book.instrument_id, book.book_type);
        let sequence = book.sequence;
        let ts_event = book.ts_last;

        let mut order_id = 1_u64;
        for (price, quantity) in bids_map {
            if quantity <= Decimal::ZERO {
                continue;
            }

            let order = BookOrder::new(
                OrderSide::Buy,
                Price::from_decimal(price).expect("Invalid bid price for BinaryMarketBookView"),
                Quantity::from_decimal(quantity)
                    .expect("Invalid bid quantity for BinaryMarketBookView"),
                order_id,
            );
            order_id += 1;
            filtered_book.add(order, 0, sequence, ts_event);
        }

        for (price, quantity) in asks_map {
            if quantity <= Decimal::ZERO {
                continue;
            }

            let order = BookOrder::new(
                OrderSide::Sell,
                Price::from_decimal(price).expect("Invalid ask price for BinaryMarketBookView"),
                Quantity::from_decimal(quantity)
                    .expect("Invalid ask quantity for BinaryMarketBookView"),
                order_id,
            );
            order_id += 1;
            filtered_book.add(order, 0, sequence, ts_event);
        }

        Ok(Self {
            book: filtered_book,
        })
    }
}

fn filter_quantities(
    public_map: &mut IndexMap<Decimal, Decimal>,
    own_map: IndexMap<Decimal, Decimal>,
) {
    for (price, own_size) in own_map {
        if let Some(public_size) = public_map.get_mut(&price) {
            *public_size = (*public_size - own_size).max(Decimal::ZERO);

            if *public_size == Decimal::ZERO {
                public_map.shift_remove(&price);
            }
        }
    }
}
