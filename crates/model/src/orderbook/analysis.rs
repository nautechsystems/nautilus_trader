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

//! Functions related to order book analysis.

use std::collections::BTreeMap;

use super::{BookLevel, BookPrice, OrderBook};
use crate::{
    enums::{BookType, OrderSide},
    orderbook::BookIntegrityError,
    types::{Price, Quantity, fixed::FIXED_SCALAR, quantity::QuantityRaw},
};

/// Calculates the estimated fill quantity for a specified price from a set of
/// order book levels and order side.
///
/// # Panics
///
/// Panics if `order_side` is neither [`OrderSide::Buy`] nor [`OrderSide::Sell`].
#[must_use]
pub fn get_quantity_for_price(
    price: Price,
    order_side: OrderSide,
    levels: &BTreeMap<BookPrice, BookLevel>,
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
pub fn get_avg_px_for_quantity(qty: Quantity, levels: &BTreeMap<BookPrice, BookLevel>) -> f64 {
    let mut cumulative_size_raw: QuantityRaw = 0;
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

/// Calculates the estimated average price for a specified exposure from a set of
/// order book levels.
#[must_use]
pub fn get_avg_px_qty_for_exposure(
    target_exposure: Quantity,
    levels: &BTreeMap<BookPrice, BookLevel>,
) -> (f64, f64, f64) {
    let mut cumulative_exposure = 0.0;
    let mut cumulative_size_raw: QuantityRaw = 0;
    let mut final_price = levels
        .first_key_value()
        .map_or(0.0, |(price, _)| price.value.as_f64());

    for (book_price, level) in levels {
        let price = book_price.value.as_f64();
        final_price = price;

        let level_exposure = price * level.size_raw() as f64;
        let exposure_this_level =
            level_exposure.min(target_exposure.raw as f64 - cumulative_exposure);
        let size_this_level = (exposure_this_level / price).floor() as QuantityRaw;

        cumulative_exposure += price * size_this_level as f64;
        cumulative_size_raw += size_this_level;

        if cumulative_exposure >= target_exposure.as_f64() {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        (0.0, 0.0, final_price)
    } else {
        let avg_price = cumulative_exposure / cumulative_size_raw as f64;
        (
            avg_price,
            cumulative_size_raw as f64 / FIXED_SCALAR,
            final_price,
        )
    }
}

/// Checks the integrity of the given order `book`.
///
/// # Errors
///
/// Returns an error if a book integrity check fails.
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

        // Only strictly crossed books (bid > ask) are invalid; locked markets (bid == ask) are valid
        if best_bid.value > best_ask.value {
            return Err(BookIntegrityError::OrdersCrossed(best_bid, best_ask));
        }
    }

    Ok(())
}
