// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::hash_map::DefaultHasher;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};

use nautilus_core::cvec::CVec;
use nautilus_core::time::UnixNanos;

use crate::enums::BookAction;
use crate::enums::OrderSide;
use crate::identifiers::instrument_id::InstrumentId;
use crate::orderbook::ladder::BookPrice;
use crate::types::price::Price;
use crate::types::quantity::Quantity;

/// Represents an order in a book.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BookOrder {
    pub side: OrderSide,
    pub price: Price,
    pub size: Quantity,
    pub order_id: u64,
}

impl BookOrder {
    #[must_use]
    pub fn new(side: OrderSide, price: Price, size: Quantity, order_id: u64) -> Self {
        Self {
            side,
            price,
            size,
            order_id,
        }
    }

    #[must_use]
    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price.clone(), self.side)
    }

    #[must_use]
    pub fn exposure(&self) -> f64 {
        self.price.as_f64() * self.size.as_f64()
    }

    #[must_use]
    pub fn signed_size(&self) -> f64 {
        match self.side {
            OrderSide::Buy => self.size.as_f64(),
            OrderSide::Sell => -(self.size.as_f64()),
            _ => panic!("Invalid `OrderSize` for signed size, was {}", self.side),
        }
    }
}

impl Display for BookOrder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.price, self.size, self.side, self.order_id,
        )
    }
}

/// Represents a single change/delta in an order book.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrderBookDelta {
    pub instrument_id: InstrumentId,
    pub action: BookAction,
    pub order: BookOrder,
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderBookDelta {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }
}

impl Display for OrderBookDelta {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.instrument_id,
            self.action,
            self.order,
            self.flags,
            self.sequence,
            self.ts_event,
            self.ts_init
        )
    }
}

// Represents a snapshot of an order book.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct OrderBookSnapshot {
    pub instrument_id: InstrumentId,
    pub bids: CVec,
    pub asks: CVec,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderBookSnapshot {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bids: CVec,
        asks: CVec,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            bids,
            asks,
            sequence,
            ts_event,
            ts_init,
        }
    }
}

impl Display for OrderBookSnapshot {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: Add display for bids and asks
        write!(
            f,
            "{},{},{},{}",
            self.instrument_id, self.sequence, self.ts_event, self.ts_init
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn book_order_drop(order: BookOrder) {
    drop(order); // Memory freed here
}

#[no_mangle]
pub extern "C" fn book_order_clone(order: &BookOrder) -> BookOrder {
    order.clone()
}

#[no_mangle]
pub extern "C" fn book_order_hash(order: &BookOrder) -> u64 {
    let mut hasher = DefaultHasher::new();
    order.hash(&mut hasher);
    hasher.finish()
}

#[no_mangle]
pub extern "C" fn book_order_new(
    order_side: OrderSide,
    price: Price,
    quantity: Quantity,
    order_id: u64,
) -> BookOrder {
    BookOrder::new(order_side, price, quantity, order_id)
}

#[no_mangle]
pub extern "C" fn orderbook_delta_drop(delta: OrderBookDelta) {
    drop(delta); // Memory freed here
}

#[no_mangle]
pub extern "C" fn orderbook_delta_clone(delta: &OrderBookDelta) -> OrderBookDelta {
    delta.clone()
}

#[no_mangle]
pub extern "C" fn orderbook_delta_new(
    instrument_id: InstrumentId,
    action: BookAction,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

#[no_mangle]
pub extern "C" fn orderbook_snapshot_drop(snapshot: OrderBookSnapshot) {
    drop(snapshot); // Memory freed here
}

#[no_mangle]
/// Creates a new `OrderBookSnapshot` from the provided data.
///
/// # Safety
///
/// This function is marked as `unsafe` because it relies on the assumption that the `CVec`
/// objects were correctly initialized and point to valid memory regions with a valid layout.
/// Improper use of this function with incorrect or uninitialized `CVec` objects can lead
/// to undefined behavior, including memory unsafety and crashes.
///
/// It is the responsibility of the caller to ensure that the `CVec` objects are valid and
/// have the correct layout matching the expected `Vec` types (`BookOrder` in this case).
/// Failure to do so can result in memory corruption or access violations.
///
/// Additionally, the ownership of the provided memory is transferred to the returned
/// `OrderBookSnapshot` object. It is crucial to ensure proper memory management and
/// deallocation of the `OrderBookSnapshot` object to prevent memory leaks by calling
/// `orderbook_snapshot_drop(...).
pub unsafe extern "C" fn orderbook_snapshot_new(
    instrument_id: InstrumentId,
    bids: CVec,
    asks: CVec,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookSnapshot {
    OrderBookSnapshot::new(instrument_id, bids, asks, sequence, ts_event, ts_init)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_book_order_new() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);

        assert_eq!(order.price, price);
        assert_eq!(order.size, size);
        assert_eq!(order.side, side);
        assert_eq!(order.order_id, order_id);
    }

    #[test]
    fn test_book_order_to_book_price() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let book_price = order.to_book_price();

        assert_eq!(book_price.value, price);
        assert_eq!(book_price.side, side);
    }

    #[test]
    fn test_book_order_display() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let display = format!("{}", order);

        let expected = format!("{},{},{},{}", price, size, side, order_id);
        assert_eq!(display, expected);
    }

    #[test]
    fn test_order_book_delta_new() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let delta = OrderBookDelta::new(
            instrument_id.clone(),
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, action);
        assert_eq!(delta.order.price, price);
        assert_eq!(delta.order.size, size);
        assert_eq!(delta.order.side, side);
        assert_eq!(delta.order.order_id, order_id);
        assert_eq!(delta.flags, flags);
        assert_eq!(delta.sequence, sequence);
        assert_eq!(delta.ts_event, ts_event);
        assert_eq!(delta.ts_init, ts_init);
    }

    #[test]
    fn test_order_book_delta_display() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);

        let delta = OrderBookDelta::new(
            instrument_id.clone(),
            action,
            order.clone(),
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(
            format!("{}", delta),
            "AAPL.NASDAQ,ADD,100.00,10,BUY,123456,0,1,1,2".to_string()
        );
    }

    #[test]
    fn test_order_book_snapshot_display() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let bids = vec![
            BookOrder::new(
                OrderSide::Buy,
                Price::from("100.00"),
                Quantity::from("10"),
                123,
            ),
            BookOrder::new(
                OrderSide::Buy,
                Price::from("99.00"),
                Quantity::from("5"),
                124,
            ),
        ];
        let asks = vec![
            BookOrder::new(
                OrderSide::Sell,
                Price::from("101.00"),
                Quantity::from("15"),
                234,
            ),
            BookOrder::new(
                OrderSide::Sell,
                Price::from("102.00"),
                Quantity::from("20"),
                235,
            ),
        ];
        let sequence = 123456;
        let ts_event = 1;
        let ts_init = 2;

        let snapshot = OrderBookSnapshot::new(
            instrument_id.clone(),
            bids.clone().into(),
            asks.clone().into(),
            sequence,
            ts_event,
            ts_init,
        );

        // TODO(cs): WIP
        assert_eq!(
            format!("{}", snapshot),
            "AAPL.NASDAQ,123456,1,2".to_string()
        );
    }
}
