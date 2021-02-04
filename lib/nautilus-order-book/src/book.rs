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

use crate::entry::OrderBookEntry;


/// Represents a limit order book
#[repr(C)]
pub struct OrderBook
{
    pub timestamp: u64,
    pub last_update_id: u64,
    pub best_bid_price: f64,
    pub best_ask_price: f64,
    pub best_bid_qty: f64,
    pub best_ask_qty: f64,

    _bid_book: [OrderBookEntry; 25],
    _ask_book: [OrderBookEntry; 25],
}


impl OrderBook
{
    /// Initialize a new instance of the `OrderBook` structure.
    #[no_mangle]
    pub extern "C" fn new(timestamp: u64) -> OrderBook {
        return OrderBook {
            timestamp,
            last_update_id: 0,
            best_bid_price: f64::MIN,
            best_ask_price: f64::MAX,
            best_bid_qty: 0.0,
            best_ask_qty: 0.0,
            _bid_book: [OrderBookEntry { price: f64::MIN, qty: 0.0, update_id: 0 }; 25],
            _ask_book: [OrderBookEntry { price: f64::MAX, qty: 0.0, update_id: 0 }; 25],
        };
    }

    /// Clear stateful values from the order book.
    #[no_mangle]
    pub extern "C" fn reset(&mut self) {
        self._bid_book = [OrderBookEntry { price: f64::MIN, qty: 0.0, update_id: 0 }; 25];
        self._ask_book = [OrderBookEntry { price: f64::MAX, qty: 0.0, update_id: 0 }; 25];
    }

    /// Apply the snapshot of 10 bids and 10 asks.
    #[no_mangle]
    pub extern "C" fn apply_snapshot10(
        &mut self,
        bids: &[OrderBookEntry; 10],
        asks: &[OrderBookEntry; 10],
        update_id: u64,
        timestamp: u64,
    ) {
        let mut snapshot_idx = 0;
        let mut bid_book_idx = 0;
        while snapshot_idx < bids.len() && bid_book_idx < self._bid_book.len() {
            let to_enter = bids[snapshot_idx];
            for i in bid_book_idx..self._bid_book.len() {
                let next = self._bid_book[i];
                if to_enter.price > next.price {
                    self._bid_book[i] = to_enter;
                    snapshot_idx += 1;
                    bid_book_idx += 1;
                    break;
                }
                else {
                    bid_book_idx += 1;
                }
            }
        }

        snapshot_idx = 0;
        let mut ask_book_idx = 0;
        while snapshot_idx < asks.len() && ask_book_idx < self._ask_book.len() {
            let to_enter = asks[snapshot_idx];
            for i in ask_book_idx..self._ask_book.len() {
                let next = self._bid_book[i];
                if to_enter.price > next.price {
                    self._ask_book[i] = to_enter;
                    snapshot_idx += 1;
                    ask_book_idx += 1;
                    break;
                }
                else {
                    ask_book_idx += 1;
                }
            }
        }

        let best_bid = self._bid_book[0];
        self.best_bid_price = best_bid.price;
        self.best_bid_qty = best_bid.qty;

        let best_ask = self._ask_book[0];
        self.best_ask_price = best_ask.price;
        self.best_ask_qty = best_ask.qty;

        self.timestamp = timestamp;
        self.last_update_id = update_id;
    }

    /// Apply the order book entry to the bid side.
    #[no_mangle]
    pub extern "C" fn apply_bid_diff(&mut self, entry: OrderBookEntry, timestamp: u64) {
        let mut to_enter = entry;
        for i in 0..self._bid_book.len() {
            let mut next = self._bid_book[i];
            if to_enter.price > next.price {
                self._bid_book[i] = to_enter;
                to_enter = next;
                if to_enter.price == f64::MIN {
                    break;  // No need to re-enter empty entry
                }
            } else if to_enter.price == next.price {
                next.update(entry.qty, entry.update_id);
                break;
            }
        }

        let best_bid = self._bid_book[0];
        self.best_bid_price = best_bid.price;
        self.best_bid_qty = best_bid.qty;
        self.timestamp = timestamp;
        self.last_update_id = entry.update_id;
    }

    /// Apply the order book entry to the ask side.
    #[no_mangle]
    pub extern "C" fn apply_ask_diff(&mut self, entry: OrderBookEntry, timestamp: u64) {
        let mut to_enter = entry;
        for i in 0..self._ask_book.len() {
            let mut next = self._ask_book[i];
            if to_enter.price < next.price {
                self._ask_book[i] = to_enter;
                to_enter = next;
                if to_enter.price == f64::MAX {
                    break;  // No need to re-enter empty entry
                }
            } else if to_enter.price == next.price {
                next.update(entry.qty, entry.update_id);
                break;
            }
        }

        let best_ask = self._ask_book[0];
        self.best_ask_price = best_ask.price;
        self.best_ask_qty = best_ask.qty;
        self.timestamp = timestamp;
        self.last_update_id = entry.update_id;
    }

    /// Returns the current spread from the top of the order book.
    #[no_mangle]
    pub extern "C" fn spread(&self) -> f64 {
        self.best_ask_price - self.best_bid_price
    }

    /// Returns the predicted buy price for the given quantity.
    #[no_mangle]
    pub extern "C" fn buy_price_for_qty(&mut self, qty: f64) -> f64 {
        let mut cum_qty = 0.0;
        let mut out_price = f64::NAN;
        for entry in &self._ask_book {
            cum_qty += entry.qty;
            if cum_qty >= qty {
                out_price = entry.price;
                break;
            }
        }
        return out_price
    }

    /// Returns the predicted sell price for the given quantity.
    #[no_mangle]
    pub extern "C" fn sell_price_for_qty(&mut self, qty: f64) -> f64 {
        let mut cum_qty = 0.0;
        let mut out_price = f64::NAN;
        for entry in &self._bid_book {
            cum_qty += entry.qty;
            if cum_qty >= qty {
                out_price = entry.price;
                break;
            }
        }
        return out_price
    }
}
