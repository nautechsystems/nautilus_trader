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

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    fmt::{Display, Formatter},
};

use super::book::BookIntegrityError;
use crate::{
    data::order::{BookOrder, OrderId},
    enums::OrderSide,
    orderbook::level::Level,
    types::{price::Price, quantity::Quantity},
};

/// Represents a price level with a specified side in an order books ladder.
#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BookPrice {
    pub value: Price,
    pub side: OrderSide,
}

impl BookPrice {
    #[must_use]
    pub fn new(value: Price, side: OrderSide) -> Self {
        Self { value, side }
    }
}

impl PartialOrd for BookPrice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for BookPrice {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Ord for BookPrice {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.side {
            OrderSide::Buy => other.value.cmp(&self.value),
            OrderSide::Sell => self.value.cmp(&other.value),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }
}

impl Display for BookPrice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

/// Represents one side of an order book as a ladder of price levels.
#[derive(Clone, Debug)]
pub struct Ladder {
    pub side: OrderSide,
    pub levels: BTreeMap<BookPrice, Level>,
    pub cache: HashMap<u64, BookPrice>,
}

impl Ladder {
    #[must_use]
    pub fn new(side: OrderSide) -> Self {
        Self {
            side,
            levels: BTreeMap::new(),
            cache: HashMap::new(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    pub fn add_bulk(&mut self, orders: Vec<BookOrder>) {
        for order in orders {
            self.add(order);
        }
    }

    pub fn clear(&mut self) {
        self.levels.clear();
        self.cache.clear();
    }

    pub fn add(&mut self, order: BookOrder) {
        let order_id = order.order_id;
        let book_price = order.to_book_price();

        self.cache.insert(order_id, book_price);

        match self.levels.get_mut(&book_price) {
            Some(level) => {
                level.add(order);
            }
            None => {
                let level = Level::from_order(order);
                self.levels.insert(book_price, level);
            }
        }
    }

    pub fn update(&mut self, order: BookOrder) {
        let price_opt = self.cache.get(&order.order_id).copied();

        if let Some(price) = price_opt {
            if let Some(level) = self.levels.get_mut(&price) {
                if order.price == level.price.value {
                    // Update at current price level
                    level.update(order);
                    return;
                }

                // Price update: delete and insert at new level
                self.cache.remove(&order.order_id);
                level.delete(&order);
                if level.is_empty() {
                    self.levels.remove(&price);
                }
            }
        }

        self.add(order);
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        self.remove(order.order_id, ts_event, sequence);
    }

    pub fn remove(&mut self, order_id: OrderId, ts_event: u64, sequence: u64) {
        if let Some(price) = self.cache.remove(&order_id) {
            if let Some(level) = self.levels.get_mut(&price) {
                level.remove_by_id(order_id, ts_event, sequence);
                if level.is_empty() {
                    self.levels.remove(&price);
                }
            }
        }
    }

    #[must_use]
    pub fn sizes(&self) -> f64 {
        self.levels.values().map(super::level::Level::size).sum()
    }

    #[must_use]
    pub fn exposures(&self) -> f64 {
        self.levels
            .values()
            .map(super::level::Level::exposure)
            .sum()
    }

    #[must_use]
    pub fn top(&self) -> Option<&Level> {
        match self.levels.iter().next() {
            Some((_, l)) => Option::Some(l),
            None => Option::None,
        }
    }

    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        let is_reversed = self.side == OrderSide::Buy;

        let mut fills = Vec::new();
        let mut cumulative_denominator = Quantity::zero(order.size.precision);
        let target = order.size;

        for level in self.levels.values() {
            if (is_reversed && level.price.value < order.price)
                || (!is_reversed && level.price.value > order.price)
            {
                break;
            }

            for book_order in level.orders.values() {
                let current = book_order.size;
                if cumulative_denominator + current >= target {
                    // This order has filled us, add fill and return
                    let remainder = target - cumulative_denominator;
                    if remainder.is_positive() {
                        fills.push((book_order.price, remainder));
                    }
                    return fills;
                }

                // Add this fill and continue
                fills.push((book_order.price, current));
                cumulative_denominator += current;
            }
        }

        fills
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        data::order::BookOrder,
        enums::OrderSide,
        orderbook::ladder::{BookPrice, Ladder},
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_book_price_bid_sorting() {
        let mut bid_prices = [
            BookPrice::new(Price::from("2.0"), OrderSide::Buy),
            BookPrice::new(Price::from("4.0"), OrderSide::Buy),
            BookPrice::new(Price::from("1.0"), OrderSide::Buy),
            BookPrice::new(Price::from("3.0"), OrderSide::Buy),
        ];
        bid_prices.sort();
        assert_eq!(bid_prices[0].value.as_f64(), 4.0);
    }

    #[rstest]
    fn test_book_price_ask_sorting() {
        let mut ask_prices = [
            BookPrice::new(Price::from("2.0"), OrderSide::Sell),
            BookPrice::new(Price::from("4.0"), OrderSide::Sell),
            BookPrice::new(Price::from("1.0"), OrderSide::Sell),
            BookPrice::new(Price::from("3.0"), OrderSide::Sell),
        ];

        ask_prices.sort();
        assert_eq!(ask_prices[0].value.as_f64(), 1.0);
    }

    #[rstest]
    fn test_add_single_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 0);

        ladder.add(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 200.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0);
    }

    #[rstest]
    fn test_add_multiple_buy_orders() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(30), 1);
        let order3 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(50), 2);
        let order4 = BookOrder::new(OrderSide::Buy, Price::from("8.00"), Quantity::from(200), 3);

        ladder.add_bulk(vec![order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.sizes(), 300.0);
        assert_eq!(ladder.exposures(), 2520.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0);
    }

    #[rstest]
    fn test_add_multiple_sell_orders() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order1 = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(20), 0);
        let order2 = BookOrder::new(OrderSide::Sell, Price::from("12.00"), Quantity::from(30), 1);
        let order3 = BookOrder::new(OrderSide::Sell, Price::from("12.00"), Quantity::from(50), 2);
        let order4 = BookOrder::new(
            OrderSide::Sell,
            Price::from("13.00"),
            Quantity::from(200),
            0,
        );

        ladder.add_bulk(vec![order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.sizes(), 300.0);
        assert_eq!(ladder.exposures(), 3780.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0);
    }

    #[rstest]
    fn test_add_to_same_price_level() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(30), 2);

        ladder.add(order1);
        ladder.add(order2);

        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 50.0);
        assert_eq!(ladder.exposures(), 500.0);
    }

    #[rstest]
    fn test_add_descending_buy_orders() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("8.00"), Quantity::from(30), 2);

        ladder.add(order1);
        ladder.add(order2);

        assert_eq!(ladder.top().unwrap().price.value, Price::from("9.00"));
    }

    #[rstest]
    fn test_add_ascending_sell_orders() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order1 = BookOrder::new(OrderSide::Sell, Price::from("8.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Sell, Price::from("9.00"), Quantity::from(30), 2);

        ladder.add(order1);
        ladder.add(order2);

        assert_eq!(ladder.top().unwrap().price.value, Price::from("8.00"));
    }

    #[rstest]
    fn test_update_buy_order_price() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.10"), Quantity::from(20), 1);

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 222.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.1);
    }

    #[rstest]
    fn test_update_sell_order_price() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Sell, Price::from("11.10"), Quantity::from(20), 1);

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 222.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.1);
    }

    #[rstest]
    fn test_update_buy_order_size() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(10), 1);

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0);
    }

    #[rstest]
    fn test_update_sell_order_size() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(10), 1);

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0);
    }

    #[rstest]
    fn test_delete_non_existing_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);

        ladder.delete(order, 0, 0);

        assert_eq!(ladder.len(), 0);
    }

    #[rstest]
    fn test_delete_buy_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(10), 1);

        ladder.delete(order, 0, 0);
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.sizes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None);
    }

    #[rstest]
    fn test_delete_sell_order() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(OrderSide::Sell, Price::from("10.00"), Quantity::from(10), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Sell, Price::from("10.00"), Quantity::from(10), 1);

        ladder.delete(order, 0, 0);
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.sizes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None);
    }

    #[rstest]
    fn test_simulate_fills_with_empty_book() {
        let ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::max(2), Quantity::from(500), 1);

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    #[case(OrderSide::Buy, Price::max(2), OrderSide::Sell)]
    #[case(OrderSide::Sell, Price::min(2), OrderSide::Buy)]
    fn test_simulate_order_fills_with_no_size(
        #[case] side: OrderSide,
        #[case] price: Price,
        #[case] ladder_side: OrderSide,
    ) {
        let ladder = Ladder::new(ladder_side);
        let order = BookOrder {
            price, // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    #[case(OrderSide::Buy, OrderSide::Sell, Price::from("60.0"))]
    #[case(OrderSide::Sell, OrderSide::Buy, Price::from("40.0"))]
    fn test_simulate_order_fills_buy_when_far_from_market(
        #[case] order_side: OrderSide,
        #[case] ladder_side: OrderSide,
        #[case] ladder_price: Price,
    ) {
        let mut ladder = Ladder::new(ladder_side);

        ladder.add(BookOrder {
            price: ladder_price,
            size: Quantity::from(100),
            side: ladder_side,
            order_id: 1,
        });

        let order = BookOrder {
            price: Price::from("50.00"),
            size: Quantity::from(500),
            side: order_side,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    fn test_simulate_order_fills_sell_when_far_from_market() {
        let mut ladder = Ladder::new(OrderSide::Buy);

        ladder.add(BookOrder {
            price: Price::from("100.00"),
            size: Quantity::from(100),
            side: OrderSide::Buy,
            order_id: 1,
        });

        let order = BookOrder {
            price: Price::from("150.00"), // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side: OrderSide::Buy,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    fn test_simulate_order_fills_buy() {
        let mut ladder = Ladder::new(OrderSide::Sell);

        ladder.add_bulk(vec![
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from(100),
                side: OrderSide::Sell,
                order_id: 1,
            },
            BookOrder {
                price: Price::from("101.00"),
                size: Quantity::from(200),
                side: OrderSide::Sell,
                order_id: 2,
            },
            BookOrder {
                price: Price::from("102.00"),
                size: Quantity::from(400),
                side: OrderSide::Sell,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::max(2), // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side: OrderSide::Buy,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = fills[0];
        assert_eq!(price1, Price::from("100.00"));
        assert_eq!(size1, Quantity::from(100));

        let (price2, size2) = fills[1];
        assert_eq!(price2, Price::from("101.00"));
        assert_eq!(size2, Quantity::from(200));

        let (price3, size3) = fills[2];
        assert_eq!(price3, Price::from("102.00"));
        assert_eq!(size3, Quantity::from(200));
    }

    #[rstest]
    fn test_simulate_order_fills_sell() {
        let mut ladder = Ladder::new(OrderSide::Buy);

        ladder.add_bulk(vec![
            BookOrder {
                price: Price::from("102.00"),
                size: Quantity::from(100),
                side: OrderSide::Buy,
                order_id: 1,
            },
            BookOrder {
                price: Price::from("101.00"),
                size: Quantity::from(200),
                side: OrderSide::Buy,
                order_id: 2,
            },
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from(400),
                side: OrderSide::Buy,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::min(2), // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side: OrderSide::Sell,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = fills[0];
        assert_eq!(price1, Price::from("102.00"));
        assert_eq!(size1, Quantity::from(100));

        let (price2, size2) = fills[1];
        assert_eq!(price2, Price::from("101.00"));
        assert_eq!(size2, Quantity::from(200));

        let (price3, size3) = fills[2];
        assert_eq!(price3, Price::from("100.00"));
        assert_eq!(size3, Quantity::from(200));
    }

    #[rstest]
    fn test_simulate_order_fills_sell_with_size_at_limit_of_precision() {
        let mut ladder = Ladder::new(OrderSide::Buy);

        ladder.add_bulk(vec![
            BookOrder {
                price: Price::from("102.00"),
                size: Quantity::from("100.000000000"),
                side: OrderSide::Buy,
                order_id: 1,
            },
            BookOrder {
                price: Price::from("101.00"),
                size: Quantity::from("200.000000000"),
                side: OrderSide::Buy,
                order_id: 2,
            },
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from("400.000000000"),
                side: OrderSide::Buy,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::min(2),                  // <-- Simulate a MARKET order
            size: Quantity::from("699.999999999"), // <-- Size slightly less than total size in ladder
            side: OrderSide::Sell,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = fills[0];
        assert_eq!(price1, Price::from("102.00"));
        assert_eq!(size1, Quantity::from("100.000000000"));

        let (price2, size2) = fills[1];
        assert_eq!(price2, Price::from("101.00"));
        assert_eq!(size2, Quantity::from("200.000000000"));

        let (price3, size3) = fills[2];
        assert_eq!(price3, Price::from("100.00"));
        assert_eq!(size3, Quantity::from("399.999999999"));
    }

    #[rstest]
    fn test_boundary_prices() {
        let max_price = Price::max(1);
        let min_price = Price::min(1);

        let mut ladder_buy = Ladder::new(OrderSide::Buy);
        let mut ladder_sell = Ladder::new(OrderSide::Sell);

        let order_buy = BookOrder::new(OrderSide::Buy, min_price, Quantity::from(1), 1);
        let order_sell = BookOrder::new(OrderSide::Sell, max_price, Quantity::from(1), 1);

        ladder_buy.add(order_buy);
        ladder_sell.add(order_sell);

        assert_eq!(ladder_buy.top().unwrap().price.value, min_price);
        assert_eq!(ladder_sell.top().unwrap().price.value, max_price);
    }
}
