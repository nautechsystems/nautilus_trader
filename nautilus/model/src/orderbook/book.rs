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

use crate::enums::BookLevel;
use crate::identifiers::instrument_id::InstrumentId;
use crate::orderbook::level::Level;

#[repr(C)]
#[derive(Debug, Hash)]
pub struct OrderBook {
    pub instrument_id: InstrumentId,
    pub book_level: BookLevel,
    pub bids: *mut Level,
    pub asks: *mut Level,
    pub bids_len: usize,
    pub asks_len: usize,
    bids_cap: usize,
    asks_cap: usize,
}

impl OrderBook {
    pub fn new(
        instrument_id: InstrumentId,
        book_level: BookLevel,
        bids: Vec<Level>,
        asks: Vec<Level>,
    ) -> Self {
        let (bids_ptr, bids_len, bids_cap) = bids.into_raw_parts();
        let (asks_ptr, asks_len, asks_cap) = asks.into_raw_parts();
        OrderBook {
            instrument_id,
            book_level,
            bids: bids_ptr,
            bids_len,
            bids_cap,
            asks: asks_ptr,
            asks_len,
            asks_cap,
        }
    }

    #[no_mangle]
    pub extern "C" fn new_order_book(
        instrument_id: InstrumentId,
        book_level: BookLevel,
    ) -> OrderBook {
        OrderBook::new(instrument_id, book_level, vec![], vec![])
    }

    unsafe fn _update_bids(&mut self, bids: Vec<Level>) {
        let (ptr, len, cap) = bids.into_raw_parts();
        self.bids = ptr;
        self.bids_len = len;
        self.bids_cap = cap;
    }

    unsafe fn _update_asks(&mut self, asks: Vec<Level>) {
        let (ptr, len, cap) = asks.into_raw_parts();
        self.asks = ptr;
        self.asks_len = len;
        self.asks_cap = cap;
    }
}
