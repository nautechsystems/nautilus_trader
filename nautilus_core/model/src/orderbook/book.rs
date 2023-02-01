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

use crate::enums::{BookType, OrderSide};
use crate::identifiers::instrument_id::InstrumentId;
use crate::orderbook::ladder::Ladder;
use crate::orderbook::order::BookOrder;

#[repr(C)]
pub struct OrderBook {
    bids: Ladder,
    asks: Ladder,
    pub instrument_id: InstrumentId,
    pub book_level: BookType,
    pub last_side: OrderSide,
    pub ts_last: u64,
}

impl OrderBook {
    pub fn new(instrument_id: InstrumentId, book_level: BookType) -> Self {
        OrderBook {
            bids: Ladder::new(OrderSide::Buy),
            asks: Ladder::new(OrderSide::Sell),
            instrument_id,
            book_level,
            last_side: OrderSide::Buy,
            ts_last: 0,
        }
    }

    pub fn add(&mut self, order: BookOrder, ts_event: u64) {
        self.last_side = order.side;
        self.ts_last = ts_event;
        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
            _ => panic!("`OrderSide` was None"),
        }
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64) {
        self.last_side = order.side;
        self.ts_last = ts_event;
        if order.size.raw == 0 {
            self.delete(order, ts_event);
        } else {
            match order.side {
                OrderSide::Buy => self.bids.update(order),
                OrderSide::Sell => self.asks.update(order),
                _ => panic!("`OrderSide` was None"),
            }
        }
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64) {
        self.last_side = order.side;
        self.ts_last = ts_event;
        match order.side {
            OrderSide::Buy => self.bids.delete(order),
            OrderSide::Sell => self.asks.delete(order),
            _ => panic!("`OrderSide` was None"),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn order_book_new(instrument_id: InstrumentId, book_level: BookType) -> OrderBook {
    OrderBook::new(instrument_id, book_level)
}
