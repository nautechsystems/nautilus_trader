// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use rust_decimal::Decimal;

use super::{BookLevel, BookPrice, OrderBook};
use crate::{
    enums::{BookType, OrderSide},
    orderbook::BookIntegrityError,
    types::{Price, Quantity, fixed::FIXED_SCALAR, quantity::QuantityRaw},
};

/// Calculates the estimated fill quantity for a specified price from a set of
/// order book levels and order side.
#[must_use]
pub fn get_quantity_for_price(
    price: Price,
    order_side: OrderSide,
    levels: &BTreeMap<BookPrice, BookLevel>,
) -> f64 {
    let mut matched_size: f64 = 0.0;

    for (book_price, level) in levels {
        if !is_level_within_price(order_side, book_price.value, price) {
            break;
        }

        matched_size += level.size();
    }

    matched_size
}

/// Returns all price levels that would be crossed by an order at the given price.
///
/// Unlike `get_quantity_for_price` which returns just the total, this returns
/// each individual level as (price, size). Used when liquidity consumption
/// tracking needs visibility into all available levels.
#[must_use]
pub fn get_levels_for_price(
    price: Price,
    order_side: OrderSide,
    levels: &BTreeMap<BookPrice, BookLevel>,
    size_precision: u8,
) -> Vec<(Price, Quantity)> {
    let mut result = Vec::new();

    for (book_price, level) in levels {
        if !is_level_within_price(order_side, book_price.value, price) {
            break;
        }

        let level_size = Quantity::from_raw(level.size_raw(), size_precision);
        result.push((level.price.value, level_size));
    }

    result
}

fn is_level_within_price(order_side: OrderSide, level_price: Price, limit_price: Price) -> bool {
    match order_side {
        OrderSide::Buy => level_price <= limit_price,
        OrderSide::Sell => level_price >= limit_price,
    }
}

/// Calculates the estimated average price for a specified quantity from a set of
/// order book levels.
///
/// # Panics
///
/// Panics if the calculated average price cannot be parsed as an `f64`.
#[must_use]
pub fn get_avg_px_for_quantity(qty: Quantity, levels: &BTreeMap<BookPrice, BookLevel>) -> f64 {
    let mut cumulative_size_raw: QuantityRaw = 0;
    let mut cumulative_size = Decimal::ZERO;
    let mut cumulative_value = Decimal::ZERO;

    for (book_price, level) in levels {
        let size_this_level = level.size_raw().min(qty.raw - cumulative_size_raw);
        let size_this_level_decimal = Quantity::raw_as_decimal(size_this_level);
        cumulative_size_raw += size_this_level;
        cumulative_size += size_this_level_decimal;
        cumulative_value += book_price.value.as_decimal() * size_this_level_decimal;

        if cumulative_size_raw >= qty.raw {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        0.0
    } else {
        (cumulative_value / cumulative_size)
            .to_string()
            .parse::<f64>()
            .expect("Decimal average price must parse as f64")
    }
}

/// Calculates the worst (last-touched) price while filling a specified quantity
/// from order book levels.
///
/// For buy-side traversal this is the highest ask touched; for sell-side traversal
/// this is the lowest bid touched. Returns `None` when no quantity can be matched.
#[must_use]
pub fn get_worst_px_for_quantity(
    qty: Quantity,
    levels: &BTreeMap<BookPrice, BookLevel>,
) -> Option<Price> {
    let mut cumulative_size_raw: QuantityRaw = 0;
    let mut worst_price: Option<Price> = None;

    for (book_price, level) in levels {
        let size_this_level = level.size_raw().min(qty.raw - cumulative_size_raw);

        if size_this_level == 0 {
            continue;
        }

        cumulative_size_raw += size_this_level;
        worst_price = Some(book_price.value);

        if cumulative_size_raw >= qty.raw {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        None
    } else {
        worst_price
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

    let target_exposure_raw = target_exposure.raw as f64;

    for (book_price, level) in levels {
        let price = book_price.value.as_f64();

        if price == 0.0 {
            continue;
        }

        let level_exposure = price * level.size_raw() as f64;
        let exposure_this_level = level_exposure.min(target_exposure_raw - cumulative_exposure);
        let size_this_level = (exposure_this_level / price).floor() as QuantityRaw;

        if size_this_level == 0 {
            continue;
        }

        final_price = price;
        cumulative_exposure += price * size_this_level as f64;
        cumulative_size_raw += size_this_level;

        if cumulative_exposure >= target_exposure_raw {
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
            for (side, ladder) in [(OrderSide::Buy, &book.bids), (OrderSide::Sell, &book.asks)] {
                let level_count = ladder.len();

                if level_count > 1 {
                    return Err(BookIntegrityError::TooManyLevels(side, level_count));
                }
            }
        }
        BookType::L2_MBP => {
            for (side, ladder) in [(OrderSide::Buy, &book.bids), (OrderSide::Sell, &book.asks)] {
                for level in ladder.levels.values() {
                    let order_count = level.orders.len();

                    if order_count > 1 {
                        return Err(BookIntegrityError::TooManyOrders(side, order_count));
                    }
                }
            }
        }
        BookType::L3_MBO => {}
    }

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
