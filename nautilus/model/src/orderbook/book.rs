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
    bids: Box<Vec<Level>>,
    asks: Box<Vec<Level>>,
}

impl OrderBook {
    pub fn new(instrument_id: InstrumentId, book_level: BookLevel) -> Self {
        OrderBook {
            instrument_id,
            book_level,
            bids: Box::new(vec![]),
            asks: Box::new(vec![]),
        }
    }

    #[no_mangle]
    pub extern "C" fn new_order_book(
        instrument_id: InstrumentId,
        book_level: BookLevel,
    ) -> OrderBook {
        OrderBook::new(instrument_id, book_level)
    }
}
