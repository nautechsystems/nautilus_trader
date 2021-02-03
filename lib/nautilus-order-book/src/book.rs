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

    bid_book: [[f64; 2]; 25],
    ask_book: [[f64; 2]; 25],
}


impl OrderBook
{
    /// Initialize a new instance of the `OrderBook` struct.
    #[no_mangle]
    pub extern "C" fn new(timestamp: u64) -> OrderBook {
        return OrderBook {
            timestamp,
            last_update_id: 0,
            best_bid_price: f64::MIN,
            best_ask_price: f64::MAX,
            best_bid_qty: 0.0,
            best_ask_qty: 0.0,
            bid_book: [[0.0, 0.0]; 25],
            ask_book: [[0.0, 0.0]; 25],
        };
    }

    /// Clear stateful values from the order book.
    #[no_mangle]
    pub extern "C" fn reset(&mut self) {
        self.bid_book = [[0.0, 0.0]; 25];
        self.ask_book = [[0.0, 0.0]; 25];
    }

    /// Apply the snapshot of price and quantity float arrays.
    /// Assumption that bids and asks are correctly ordered.
    #[no_mangle]
    pub extern "C" fn apply_snapshot(
        &mut self,
        bids: &[[f64; 2]; 25],
        asks: &[[f64; 2]; 25],
        timestamp: u64,
        update_id: u64,
    ) {
        self.reset();

        for i in 0..bids.len() {
            self.bid_book[i] = bids[i];
        }

        for i in 0..asks.len() {
            self.ask_book[i] = asks[i];
        }

        self.best_bid_price = self.bid_book[0][0];
        self.best_ask_price = self.ask_book[0][0];
        self.best_bid_qty = self.bid_book[0][1];
        self.best_ask_qty = self.ask_book[0][1];
        self.timestamp = timestamp;
        self.last_update_id = update_id;
    }

    // /// Update the order book by applying the given differences.
    // #[no_mangle]
    // pub extern "C" fn apply_diffs(
    //     &mut self,
    //     bids: [[f64; 2]; 25],
    //     asks: [[f64; 2]; 25],
    //     timestamp: u64,
    //     update_id: u64,
    // ) {
    //     // Add bids by price
    //     let mut idx = 0;
    //     while idx < self.bid_book.len()
    //     {
    //         for entry in &bids {
    //             let price = entry[0];
    //             let qty = entry[1];
    //             let bid_book_price = self.bid_book[idx].price;
    //             if price > bid_book_price {
    //                 self.bid_book.insert(idx, OrderBookEntry{ price, qty, update_id });
    //             } else if price == bid_book_price {
    //                     self.bid_book[idx] = OrderBookEntry{ price, qty, update_id };
    //                 }
    //             idx += 1
    //         }
    //     }
    //
    //     // Add remaining bids
    //     if idx < bids.len() - 1 {
    //         for i in idx..bids.len() {
    //             let row = bids[i];
    //             self.bid_book.push(OrderBookEntry{ price: row[0], qty: row[1], update_id });
    //         }
    //     }
    //
    //     // Add asks by price
    //     idx = 0;
    //     while idx < self.ask_book.len()
    //     {
    //         for entry in &asks {
    //             let price = entry[0];
    //             let qty = entry[1];
    //             let ask_book_price = self.ask_book[idx].price;
    //             if price < ask_book_price {
    //                 self.ask_book.insert(idx, OrderBookEntry{ price, qty, update_id });
    //             } else if price == ask_book_price {
    //                 self.ask_book[idx] = OrderBookEntry{ price, qty, update_id };
    //             }
    //             idx += 1
    //         }
    //     }
    //
    //     // Add remaining asks
    //     if idx < asks.len() - 1 {
    //         for i in idx..asks.len() {
    //             let row = asks[i];
    //             self.bid_book.push(OrderBookEntry{ price: row[0], qty: row[1], update_id });
    //         }
    //     }
    //
    //     self.timestamp = timestamp;
    //     self.last_update_id = update_id;
    // }

    /// Returns the current spread from the top of the order book.
    #[no_mangle]
    pub extern "C" fn spread(&self) -> f64 {
        self.best_ask_price - self.best_bid_price
    }
}
