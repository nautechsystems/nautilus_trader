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

use thiserror::Error;

use super::{ladder::BookPrice, level::Level};
use crate::{
    enums::{BookType, OrderSide},
    types::{price::Price, quantity::Quantity},
};

#[derive(thiserror::Error, Debug)]
pub enum InvalidBookOperation {
    #[error("Invalid book operation: cannot pre-process order for {0} book")]
    PreProcessOrder(BookType),
    #[error("Invalid book operation: cannot add order for {0} book")]
    Add(BookType),
}

#[derive(Error, Debug)]
pub enum BookIntegrityError {
    #[error("Integrity error: order not found: order_id={0}, ts_event={1}, sequence={2}")]
    OrderNotFound(u64, u64, u64),
    #[error("Integrity error: invalid `NoOrderSide` in book")]
    NoOrderSide,
    #[error("Integrity error: orders in cross [{0} {1}]")]
    OrdersCrossed(BookPrice, BookPrice),
    #[error("Integrity error: number of {0} orders at level > 1 for L2_MBP book, was {1}")]
    TooManyOrders(OrderSide, usize),
    #[error("Integrity error: number of {0} levels > 1 for L1_MBP book, was {1}")]
    TooManyLevels(OrderSide, usize),
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        data::{
            depth::{stubs::stub_depth10, OrderBookDepth10},
            order::BookOrder,
        },
        enums::OrderSide,
        identifiers::instrument_id::InstrumentId,
        orderbook::{book_mbo::OrderBookMbo, book_mbp::OrderBookMbp},
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_best_bid_and_ask_when_nothing_in_book() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBookMbp::new(instrument_id, false);

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), None);
        assert_eq!(book.best_bid_size(), None);
        assert_eq!(book.best_ask_size(), None);
        assert!(!book.has_bid());
        assert!(!book.has_ask());
    }

    #[rstest]
    fn test_bid_side_with_one_order() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbo::new(instrument_id);
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        book.add(order1, 100, 1);

        assert_eq!(book.best_bid_price(), Some(Price::from("1.000")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("1.0")));
        assert!(book.has_bid());
    }

    #[rstest]
    fn test_ask_side_with_one_order() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbo::new(instrument_id);
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(order, 200, 2);

        assert_eq!(book.best_ask_price(), Some(Price::from("2.000")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("2.0")));
        assert!(book.has_ask());
    }

    #[rstest]
    fn test_spread_with_no_bids_or_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBookMbo::new(instrument_id);
        assert_eq!(book.spread(), None);
    }

    #[rstest]
    fn test_spread_with_bids_and_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbo::new(instrument_id);
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(bid1, 100, 1);
        book.add(ask1, 200, 2);

        assert_eq!(book.spread(), Some(1.0));
    }

    #[rstest]
    fn test_midpoint_with_no_bids_or_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBookMbp::new(instrument_id, false);
        assert_eq!(book.midpoint(), None);
    }

    #[rstest]
    fn test_midpoint_with_bids_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbp::new(instrument_id, false);

        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(bid1, 100, 1);
        book.add(ask1, 200, 2);

        assert_eq!(book.midpoint(), Some(1.5));
    }

    #[rstest]
    fn test_get_price_for_quantity_no_market() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBookMbp::new(instrument_id, false);

        let qty = Quantity::from(1);

        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Buy), 0.0);
        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_quantity_for_price_no_market() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBookMbp::new(instrument_id, false);

        let price = Price::from("1.0");

        assert_eq!(book.get_quantity_for_price(price, OrderSide::Buy), 0.0);
        assert_eq!(book.get_quantity_for_price(price, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_price_for_quantity() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbp::new(instrument_id, false);

        let ask2 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.010"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("0.990"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        book.add(bid1, 0, 1);
        book.add(bid2, 0, 1);
        book.add(ask1, 0, 1);
        book.add(ask2, 0, 1);

        let qty = Quantity::from("1.5");

        assert_eq!(
            book.get_avg_px_for_quantity(qty, OrderSide::Buy),
            2.003_333_333_333_333_4
        );
        assert_eq!(
            book.get_avg_px_for_quantity(qty, OrderSide::Sell),
            0.996_666_666_666_666_7
        );
    }

    #[rstest]
    fn test_get_quantity_for_price() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbp::new(instrument_id, false);

        let ask3 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.011"),
            Quantity::from("3.0"),
            0, // order_id not applicable
        );
        let ask2 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.010"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("0.990"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        let bid3 = BookOrder::new(
            OrderSide::Buy,
            Price::from("0.989"),
            Quantity::from("3.0"),
            0, // order_id not applicable
        );
        book.add(bid1, 0, 1);
        book.add(bid2, 0, 1);
        book.add(bid3, 0, 1);
        book.add(ask1, 0, 1);
        book.add(ask2, 0, 1);
        book.add(ask3, 0, 1);

        assert_eq!(
            book.get_quantity_for_price(Price::from("2.010"), OrderSide::Buy),
            3.0
        );
        assert_eq!(
            book.get_quantity_for_price(Price::from("0.990"), OrderSide::Sell),
            3.0
        );
    }

    #[rstest]
    fn test_apply_depth(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBookMbp::new(instrument_id, false);

        book.apply_depth(depth);

        assert_eq!(book.best_bid_price().unwrap().as_f64(), 99.00);
        assert_eq!(book.best_ask_price().unwrap().as_f64(), 100.00);
        assert_eq!(book.best_bid_size().unwrap().as_f64(), 100.0);
        assert_eq!(book.best_ask_size().unwrap().as_f64(), 100.0);
    }

    #[rstest]
    fn test_pprint() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbo::new(instrument_id);

        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.500"),
            Quantity::from("2.0"),
            2,
        );
        let order3 = BookOrder::new(
            OrderSide::Buy,
            Price::from("2.000"),
            Quantity::from("3.0"),
            3,
        );
        let order4 = BookOrder::new(
            OrderSide::Sell,
            Price::from("3.000"),
            Quantity::from("3.0"),
            4,
        );
        let order5 = BookOrder::new(
            OrderSide::Sell,
            Price::from("4.000"),
            Quantity::from("4.0"),
            5,
        );
        let order6 = BookOrder::new(
            OrderSide::Sell,
            Price::from("5.000"),
            Quantity::from("8.0"),
            6,
        );

        book.add(order1, 100, 1);
        book.add(order2, 200, 2);
        book.add(order3, 300, 3);
        book.add(order4, 400, 4);
        book.add(order5, 500, 5);
        book.add(order6, 600, 6);

        let pprint_output = book.pprint(3);

        let expected_output = "╭───────┬───────┬───────╮\n\
                               │ bids  │ price │ asks  │\n\
                               ├───────┼───────┼───────┤\n\
                               │       │ 5.000 │ [8.0] │\n\
                               │       │ 4.000 │ [4.0] │\n\
                               │       │ 3.000 │ [3.0] │\n\
                               │ [3.0] │ 2.000 │       │\n\
                               │ [2.0] │ 1.500 │       │\n\
                               │ [1.0] │ 1.000 │       │\n\
                               ╰───────┴───────┴───────╯";

        println!("{pprint_output}");
        assert_eq!(pprint_output, expected_output);
    }
}
