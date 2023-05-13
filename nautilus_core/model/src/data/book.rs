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

use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};

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
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub order_id: u64,
}

impl BookOrder {
    #[must_use]
    pub fn new(price: Price, size: Quantity, side: OrderSide, order_id: u64) -> Self {
        Self {
            price,
            size,
            side,
            order_id,
        }
    }

    #[must_use]
    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price.clone(), self.side)
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrderBookSnapshot {
    pub instrument_id: InstrumentId,
    pub bids: Vec<BookOrder>,
    pub asks: Vec<BookOrder>,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderBookSnapshot {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            bids: Vec::new(),
            asks: Vec::new(),
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

#[repr(C)]
pub struct OrderBookSnapshotAPI(Box<OrderBookSnapshot>);

impl Deref for OrderBookSnapshotAPI {
    type Target = OrderBookSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OrderBookSnapshotAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// #[no_mangle]
// pub extern "C" fn orderbook_snapshot_new() -> OrderBookSnapshotAPI {
//     OrderBookSnapshotAPI(Box::new(OrderBookSnapshot::new()))
// }

#[no_mangle]
pub extern "C" fn orderbook_snapshot_drop(snapshot: OrderBookSnapshotAPI) {
    drop(snapshot); // Memory freed here
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
