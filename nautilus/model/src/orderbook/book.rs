// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enums::{BookLevel, OrderSide};
use crate::identifiers::instrument_id::InstrumentId;
use crate::orderbook::ladder::Ladder;
use crate::orderbook::order::Order;

#[repr(C)]
#[derive(Debug)]
pub struct OrderBook {
    pub instrument_id: InstrumentId,
    pub book_level: BookLevel,
    bids: Ladder,
    asks: Ladder,
}

impl OrderBook {
    pub fn new(instrument_id: InstrumentId, book_level: BookLevel) -> Self {
        OrderBook {
            instrument_id,
            book_level,
            bids: Ladder::new(OrderSide::Buy),
            asks: Ladder::new(OrderSide::Sell),
        }
    }

    #[no_mangle]
    pub extern "C" fn new_order_book(
        instrument_id: InstrumentId,
        book_level: BookLevel,
    ) -> OrderBook {
        OrderBook::new(instrument_id, book_level)
    }

    pub fn _add(&mut self, order: Order) {
        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
        }
    }
}
