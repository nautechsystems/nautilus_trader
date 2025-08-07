// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Functions related to order book display.

use ahash::AHashSet;
use rust_decimal::Decimal;
use tabled::{Table, Tabled, settings::Style};

use super::{BookPrice, level::BookLevel, own::OwnBookLevel};
use crate::orderbook::{OrderBook, own::OwnOrderBook};

#[derive(Tabled)]
struct BookLevelDisplay {
    bids: String,
    price: String,
    asks: String,
}

/// Return a [`String`] representation of the order book in a human-readable table format.
#[must_use]
pub(crate) fn pprint_book(
    order_book: &OrderBook,
    num_levels: usize,
    group_size: Option<Decimal>,
) -> String {
    let data: Vec<BookLevelDisplay> = if let Some(group_size) = group_size {
        let bid_quantities = order_book.group_bids(group_size, Some(num_levels));
        let ask_quantities = order_book.group_asks(group_size, Some(num_levels));

        let all_prices: AHashSet<Decimal> = bid_quantities
            .keys()
            .chain(ask_quantities.keys())
            .cloned()
            .collect();
        let mut sorted_prices: Vec<Decimal> = all_prices.into_iter().collect();
        sorted_prices.sort_by(|a, b| b.cmp(a)); // Descending order for display

        // Determine consistent precision from the first price
        let precision = sorted_prices.first().map_or(1, |p| p.scale());

        sorted_prices
            .iter()
            .map(|price| {
                let bid_quantity = bid_quantities.get(price);
                let ask_quantity = ask_quantities.get(price);

                BookLevelDisplay {
                    bids: if let Some(qty) = bid_quantity {
                        if *qty > Decimal::ZERO {
                            qty.to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    },
                    price: format!("{price:.precision$}", precision = precision as usize),
                    asks: if let Some(qty) = ask_quantity {
                        if *qty > Decimal::ZERO {
                            qty.to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    },
                }
            })
            .collect()
    } else {
        let ask_levels: Vec<(&BookPrice, &BookLevel)> = order_book
            .asks
            .levels
            .iter()
            .take(num_levels)
            .rev()
            .collect();
        let bid_levels: Vec<(&BookPrice, &BookLevel)> =
            order_book.bids.levels.iter().take(num_levels).collect();
        let levels: Vec<(&BookPrice, &BookLevel)> =
            ask_levels.into_iter().chain(bid_levels).collect();

        levels
            .iter()
            .map(|(book_price, level)| {
                let is_bid_level = order_book.bids.levels.contains_key(book_price);
                let is_ask_level = order_book.asks.levels.contains_key(book_price);

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

                BookLevelDisplay {
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
            .collect()
    };

    let table = Table::new(data).with(Style::rounded()).to_string();

    let header = format!(
        "bid_levels: {}\nask_levels: {}",
        order_book.bids.levels.len(),
        order_book.asks.levels.len()
    );

    format!("{header}\n{table}")
}

/// Return a [`String`] representation of the own order book in a human-readable table format.
#[must_use]
pub(crate) fn pprint_own_book(
    own_order_book: &OwnOrderBook,
    num_levels: usize,
    group_size: Option<Decimal>,
) -> String {
    let data: Vec<BookLevelDisplay> = if let Some(group_size) = group_size {
        let bid_quantities =
            own_order_book.bid_quantity(None, Some(num_levels), Some(group_size), None, None);
        let ask_quantities =
            own_order_book.ask_quantity(None, Some(num_levels), Some(group_size), None, None);

        let all_prices: AHashSet<Decimal> = bid_quantities
            .keys()
            .chain(ask_quantities.keys())
            .cloned()
            .collect();
        let mut sorted_prices: Vec<Decimal> = all_prices.into_iter().collect();
        sorted_prices.sort_by(|a, b| b.cmp(a)); // Descending order for display

        // Determine consistent precision from the first price
        let precision = sorted_prices.first().map_or(1, |p| p.scale());

        sorted_prices
            .iter()
            .map(|price| {
                let bid_quantity = bid_quantities.get(price);
                let ask_quantity = ask_quantities.get(price);

                BookLevelDisplay {
                    bids: if let Some(qty) = bid_quantity {
                        if *qty > Decimal::ZERO {
                            qty.to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    },
                    price: format!("{price:.precision$}", precision = precision as usize),
                    asks: if let Some(qty) = ask_quantity {
                        if *qty > Decimal::ZERO {
                            qty.to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    },
                }
            })
            .collect()
    } else {
        let ask_levels: Vec<(&BookPrice, &OwnBookLevel)> = own_order_book
            .asks
            .levels
            .iter()
            .take(num_levels)
            .rev()
            .collect();
        let bid_levels: Vec<(&BookPrice, &OwnBookLevel)> =
            own_order_book.bids.levels.iter().take(num_levels).collect();
        let levels: Vec<(&BookPrice, &OwnBookLevel)> =
            ask_levels.into_iter().chain(bid_levels).collect();

        levels
            .iter()
            .map(|(book_price, level)| {
                let is_bid_level = own_order_book.bids.levels.contains_key(book_price);
                let is_ask_level = own_order_book.asks.levels.contains_key(book_price);

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

                BookLevelDisplay {
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
            .collect()
    };

    let table = Table::new(data).with(Style::rounded()).to_string();

    let header = format!(
        "bid_levels: {}\nask_levels: {}",
        own_order_book.bids.levels.len(),
        own_order_book.asks.levels.len()
    );

    format!("{header}\n{table}")
}
