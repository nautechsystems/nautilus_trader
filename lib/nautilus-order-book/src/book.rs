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
pub struct OrderBook
{
    /// The order book symbol.
    pub symbol: String,
    /// The price currency.
    pub currency: String,
    /// The last update timestamp.
    pub timestamp: u64,
    /// The last update identifier.
    pub last_update_id: u64,

    bid_book: Vec<OrderBookEntry>,
    ask_book: Vec<OrderBookEntry>,
}


impl OrderBook
{
    /// Initialize a new instance of the `OrderBook` struct.
    pub fn new(
        symbol: String,
        currency: String,
        timestamp: u64,
    ) -> OrderBook {
        return OrderBook {
            symbol,
            currency,
            timestamp,
            last_update_id: 0,
            bid_book: vec![],
            ask_book: vec![],
        };
    }

    pub fn apply_float_diffs(
        &mut self,
        bids: Vec<[f64; 2]>,
        asks: Vec<[f64; 2]>,
        timestamp: u64,
        update_id: u64,
    ) {
        // TODO: WIP
        for entry in &bids {
            self.bid_book.push(OrderBookEntry{
                price: entry[0],
                amount: entry[1],
                update_id,
            });
        }

        // TODO: WIP
        for entry in &asks {
            self.ask_book.push(OrderBookEntry{
                price: entry[0],
                amount: entry[1],
                update_id,
            });
        }

        self.timestamp = timestamp;
        self.last_update_id = update_id;
    }

    /// Update the order book by applying the given differences.
    pub fn apply_diffs(
        &mut self,
        bids: Vec<OrderBookEntry>,
        asks: Vec<OrderBookEntry>,
        timestamp: u64,
        update_id: u64,
    ) {
        // TODO: WIP
        for entry in bids {
            println!("{}", entry.price);
        }

        // TODO: WIP
        for entry in asks {
            println!("{}", entry.price);
        }

        self.timestamp = timestamp;
        self.last_update_id = update_id;
    }

    /// Returns the current spread from the top of the order book.
    pub fn spread(&self) -> f64 {
        if self.bid_book.is_empty() || self.ask_book.is_empty() {
            return 0.0
        }

        let bid = self.bid_book[0].price;
        let ask = self.ask_book[0].price;
        ask - bid
    }

    pub fn best_bid_price(&self) -> f64 {
        if self.bid_book.is_empty() {
            return 0.0
        }
        return self.bid_book[0].price;
    }
    pub fn best_ask_price(&self) -> f64  {
        if self.ask_book.is_empty() {
            return 0.0
        }
        return self.ask_book[0].price;
    }

    pub fn best_bid_amount(&self) -> f64 {
        if self.bid_book.is_empty() {
            return 0.0
        }
        return self.bid_book[0].amount;
    }

    pub fn best_ask_amount(&self) -> f64  {
        if self.ask_book.is_empty() {
            return 0.0
        }
        return self.ask_book[0].amount;
    }
}
