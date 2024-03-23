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

use std::collections::BTreeMap;

use super::{book::OrderBook, ladder::BookPrice, level::Level};
use crate::{
    enums::{BookType, OrderSide},
    orderbook::error::BookIntegrityError,
    types::{price::Price, quantity::Quantity},
};

/// Calculates the estimated fill quantity for a specified price from a set of
/// order book levels and order side.
#[must_use]
pub fn get_quantity_for_price(
    price: Price,
    order_side: OrderSide,
    levels: &BTreeMap<BookPrice, Level>,
) -> f64 {
    let mut matched_size: f64 = 0.0;

    for (book_price, level) in levels {
        match order_side {
            OrderSide::Buy => {
                if book_price.value > price {
                    break;
                }
            }
            OrderSide::Sell => {
                if book_price.value < price {
                    break;
                }
            }
            _ => panic!("Invalid `OrderSide` {order_side}"),
        }
        matched_size += level.size();
    }

    matched_size
}

/// Calculates the estimated average price for a specified quantity from a set of
/// order book levels.
#[must_use]
pub fn get_avg_px_for_quantity(qty: Quantity, levels: &BTreeMap<BookPrice, Level>) -> f64 {
    let mut cumulative_size_raw = 0u64;
    let mut cumulative_value = 0.0;

    for (book_price, level) in levels {
        let size_this_level = level.size_raw().min(qty.raw - cumulative_size_raw);
        cumulative_size_raw += size_this_level;
        cumulative_value += book_price.value.as_f64() * size_this_level as f64;

        if cumulative_size_raw >= qty.raw {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        0.0
    } else {
        cumulative_value / cumulative_size_raw as f64
    }
}

pub fn book_check_integrity(book: &OrderBook) -> Result<(), BookIntegrityError> {
    match book.book_type {
        BookType::L1_MBP => {
            if book.bids.len() > 1 {
                return Err(BookIntegrityError::TooManyLevels(
                    OrderSide::Buy,
                    book.bids.len(),
                ));
            }
            if book.asks.len() > 1 {
                return Err(BookIntegrityError::TooManyLevels(
                    OrderSide::Sell,
                    book.asks.len(),
                ));
            }
        }
        BookType::L2_MBP => {
            for bid_level in book.bids.levels.values() {
                let num_orders = bid_level.orders.len();
                if num_orders > 1 {
                    return Err(BookIntegrityError::TooManyOrders(
                        OrderSide::Buy,
                        num_orders,
                    ));
                }
            }

            for ask_level in book.asks.levels.values() {
                let num_orders = ask_level.orders.len();
                if num_orders > 1 {
                    return Err(BookIntegrityError::TooManyOrders(
                        OrderSide::Sell,
                        num_orders,
                    ));
                }
            }
        }
        BookType::L3_MBO => {}
    };

    if let (Some(top_bid_level), Some(top_ask_level)) = (book.bids.top(), book.asks.top()) {
        let best_bid = top_bid_level.price;
        let best_ask = top_ask_level.price;

        if best_bid.value >= best_ask.value {
            return Err(BookIntegrityError::OrdersCrossed(best_bid, best_ask));
        }
    }

    Ok(())
}
