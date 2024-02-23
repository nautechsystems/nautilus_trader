// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use tabled::{settings::Style, Table, Tabled};

use super::{ladder::BookPrice, level::Level};
use crate::orderbook::ladder::Ladder;

#[derive(Tabled)]
struct OrderLevelDisplay {
    bids: String,
    price: String,
    asks: String,
}

/// Return a [`String`] representation of the order book in a human-readable table format.
#[must_use]
pub fn pprint_book(bids: &Ladder, asks: &Ladder, num_levels: usize) -> String {
    let ask_levels: Vec<(&BookPrice, &Level)> = asks.levels.iter().take(num_levels).rev().collect();
    let bid_levels: Vec<(&BookPrice, &Level)> = bids.levels.iter().take(num_levels).collect();
    let levels: Vec<(&BookPrice, &Level)> = ask_levels.into_iter().chain(bid_levels).collect();

    let data: Vec<OrderLevelDisplay> = levels
        .iter()
        .map(|(book_price, level)| {
            let is_bid_level = bids.levels.contains_key(book_price);
            let is_ask_level = asks.levels.contains_key(book_price);

            let bid_sizes: Vec<String> = level
                .orders
                .iter()
                .filter(|_| is_bid_level)
                .map(|order| format!("{}", order.1.size))
                .collect();

            let ask_sizes: Vec<String> = level
                .orders
                .iter()
                .filter(|_| is_ask_level)
                .map(|order| format!("{}", order.1.size))
                .collect();

            OrderLevelDisplay {
                bids: if bid_sizes.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", bid_sizes.join(", "))
                },
                price: format!("{}", level.price),
                asks: if ask_sizes.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", ask_sizes.join(", "))
                },
            }
        })
        .collect();

    Table::new(data).with(Style::rounded()).to_string()
}
