// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    data::order::BookOrder,
    enums::OrderSide,
    orderbook::level::Level,
    types::{price::Price, quantity::Quantity},
};

#[derive(Copy, Clone, Debug, Eq)]
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
            self.add(order)
        }
    }

    pub fn clear(&mut self) {
        self.levels.clear();
        self.cache.clear();
    }

    pub fn add(&mut self, order: BookOrder) {
        let book_price = order.to_book_price();
        match self.levels.get_mut(&book_price) {
            Some(level) => {
                level.add(order);
            }
            None => {
                let order_id = order.order_id;
                let level = Level::from_order(order);
                self.cache.insert(order_id, book_price);
                self.levels.insert(book_price, level);
            }
        }
    }

    pub fn update(&mut self, order: BookOrder) {
        if let Some(price) = self.cache.get(&order.order_id) {
            let level = self.levels.get_mut(price).unwrap();
            if order.price == level.price.value {
                // Size update for this level
                level.update(order);
            } else {
                // Price update, delete and insert at new level
                level.delete(&order);
                if level.is_empty() {
                    self.levels.remove(price);
                }
                self.add(order);
            }
        } else {
            // TODO(cs): Reinstate this with strict mode
            // None => panic!("No order with ID {}", &order.order_id),
            self.add(order);
        }
    }

    pub fn delete(&mut self, order: BookOrder) {
        self.remove(order.order_id);
    }

    pub fn remove(&mut self, order_id: u64) {
        if let Some(price) = self.cache.remove(&order_id) {
            let level = self.levels.get_mut(&price).unwrap();
            level.remove(order_id);
            if level.is_empty() {
                self.levels.remove(&price);
            }
        }
    }

    #[must_use]
    pub fn volumes(&self) -> f64 {
        return self.levels.values().map(|l| l.volume()).sum();
    }

    #[must_use]
    pub fn exposures(&self) -> f64 {
        return self.levels.values().map(|l| l.exposure()).sum();
    }

    #[must_use]
    pub fn top(&self) -> Option<&Level> {
        match self.levels.iter().next() {
            Some((_, l)) => Option::Some(l),
            None => Option::None,
        }
    }

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

            for book_order in &level.orders {
                let current = book_order.size;
                if cumulative_denominator + current >= target {
                    // This order has filled us, add fill and return
                    let remainder = target - cumulative_denominator;
                    if remainder.is_positive() {
                        fills.push((book_order.price, remainder));
                    }
                    return fills;
                } else {
                    // Add this fill and continue
                    fills.push((book_order.price, current));
                    cumulative_denominator += current;
                }
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
        types::{
            price::{Price, PRICE_MAX, PRICE_MIN},
            quantity::Quantity,
        },
    };

    #[test]
    fn test_book_price_bid_sorting() {
        let mut bid_prices = vec![
            BookPrice::new(Price::new(2.0, 0), OrderSide::Buy),
            BookPrice::new(Price::new(4.0, 0), OrderSide::Buy),
            BookPrice::new(Price::new(1.0, 0), OrderSide::Buy),
            BookPrice::new(Price::new(3.0, 0), OrderSide::Buy),
        ];
        bid_prices.sort();
        assert_eq!(bid_prices[0].value.as_f64(), 4.0);
    }

    #[test]
    fn test_book_price_ask_sorting() {
        let mut ask_prices = vec![
            BookPrice::new(Price::new(2.0, 0), OrderSide::Sell),
            BookPrice::new(Price::new(4.0, 0), OrderSide::Sell),
            BookPrice::new(Price::new(1.0, 0), OrderSide::Sell),
            BookPrice::new(Price::new(3.0, 0), OrderSide::Sell),
        ];

        ask_prices.sort();
        assert_eq!(ask_prices[0].value.as_f64(), 1.0);
    }

    #[test]
    fn test_add_single_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(10.00, 2),
            Quantity::new(20.0, 0),
            0,
        );

        ladder.add(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 20.0);
        assert_eq!(ladder.exposures(), 200.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0)
    }

    #[test]
    fn test_add_multiple_buy_orders() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::new(10.00, 2),
            Quantity::new(20.0, 0),
            0,
        );
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::new(9.00, 2),
            Quantity::new(30.0, 0),
            0,
        );
        let order3 = BookOrder::new(
            OrderSide::Buy,
            Price::new(9.00, 2),
            Quantity::new(50.0, 0),
            0,
        );
        let order4 = BookOrder::new(
            OrderSide::Buy,
            Price::new(8.00, 2),
            Quantity::new(200.0, 0),
            0,
        );

        ladder.add_bulk(vec![order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.volumes(), 300.0);
        assert_eq!(ladder.exposures(), 2520.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0)
    }

    #[test]
    fn test_add_multiple_sell_orders() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order1 = BookOrder::new(
            OrderSide::Sell,
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            0,
        );
        let order2 = BookOrder::new(
            OrderSide::Sell,
            Price::new(12.00, 2),
            Quantity::new(30.0, 0),
            0,
        );
        let order3 = BookOrder::new(
            OrderSide::Sell,
            Price::new(12.00, 2),
            Quantity::new(50.0, 0),
            0,
        );
        let order4 = BookOrder::new(
            OrderSide::Sell,
            Price::new(13.00, 2),
            Quantity::new(200.0, 0),
            0,
        );

        ladder.add_bulk(vec![order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.volumes(), 300.0);
        assert_eq!(ladder.exposures(), 3780.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0)
    }

    #[test]
    fn test_update_buy_order_price() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(11.10, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 20.0);
        assert_eq!(ladder.exposures(), 222.000_000_000_000_03);
        assert_eq!(
            ladder.top().unwrap().price.value.as_f64(),
            11.100_000_000_000_001
        )
    }

    #[test]
    fn test_update_sell_order_price() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(11.10, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 20.0);
        assert_eq!(ladder.exposures(), 222.000_000_000_000_03);
        assert_eq!(
            ladder.top().unwrap().price.value.as_f64(),
            11.100_000_000_000_001
        )
    }

    #[test]
    fn test_update_buy_order_size() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(11.00, 2),
            Quantity::new(10.0, 0),
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0)
    }

    #[test]
    fn test_update_sell_order_size() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(11.00, 2),
            Quantity::new(10.0, 0),
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0)
    }

    #[test]
    fn test_delete_buy_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(11.00, 2),
            Quantity::new(10.0, 0),
            1,
        );

        ladder.delete(order);
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.volumes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None)
    }

    #[test]
    fn test_delete_sell_order() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(10.00, 2),
            Quantity::new(10.0, 0),
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(10.00, 2),
            Quantity::new(10.0, 0),
            1,
        );

        ladder.delete(order);
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.volumes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None)
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
            size: Quantity::new(500.0, 0),
            side,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    #[case(OrderSide::Buy, OrderSide::Sell, Price::new(60.0, 2))]
    #[case(OrderSide::Sell, OrderSide::Buy, Price::new(40.0, 2))]
    fn test_simulate_order_fills_buy_when_far_from_market(
        #[case] order_side: OrderSide,
        #[case] ladder_side: OrderSide,
        #[case] ladder_price: Price,
    ) {
        let mut ladder = Ladder::new(ladder_side);

        ladder.add(BookOrder {
            price: ladder_price,
            size: Quantity::new(100.0, 0),
            side: ladder_side,
            order_id: 1,
        });

        let order = BookOrder {
            price: Price::new(50.00, 2),
            size: Quantity::new(500.0, 0),
            side: order_side,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[test]
    fn test_simulate_order_fills_sell_when_far_from_market() {
        let mut ladder = Ladder::new(OrderSide::Buy);

        ladder.add(BookOrder {
            price: Price::new(100.00, 2),
            size: Quantity::new(100.0, 0),
            side: OrderSide::Buy,
            order_id: 1,
        });

        let order = BookOrder {
            price: Price::new(150.00, 2), // <-- Simulate a MARKET order
            size: Quantity::new(500.0, 0),
            side: OrderSide::Buy,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[test]
    fn test_simulate_order_fills_buy_with_volume_depth_type() {
        let mut ladder = Ladder::new(OrderSide::Sell);

        ladder.add_bulk(vec![
            BookOrder {
                price: Price::new(100.00, 2),
                size: Quantity::new(100.0, 0),
                side: OrderSide::Sell,
                order_id: 1,
            },
            BookOrder {
                price: Price::new(101.00, 2),
                size: Quantity::new(200.0, 0),
                side: OrderSide::Sell,
                order_id: 2,
            },
            BookOrder {
                price: Price::new(102.00, 2),
                size: Quantity::new(400.0, 0),
                side: OrderSide::Sell,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::new(PRICE_MAX, 2), // <-- Simulate a MARKET order
            size: Quantity::new(500.0, 0),
            side: OrderSide::Buy,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = &fills[0];
        assert_eq!(price1, &Price::new(100.00, 2));
        assert_eq!(size1, &Quantity::new(100.0, 0));

        let (price2, size2) = &fills[1];
        assert_eq!(price2, &Price::new(101.00, 2));
        assert_eq!(size2, &Quantity::new(200.0, 0));

        let (price3, size3) = &fills[2];
        assert_eq!(price3, &Price::new(102.00, 2));
        assert_eq!(size3, &Quantity::new(200.0, 0));
    }

    #[test]
    fn test_simulate_order_fills_sell_with_volume_depth_type() {
        let mut ladder = Ladder::new(OrderSide::Buy);

        ladder.add_bulk(vec![
            BookOrder {
                price: Price::new(102.00, 2),
                size: Quantity::new(100.0, 0),
                side: OrderSide::Buy,
                order_id: 1,
            },
            BookOrder {
                price: Price::new(101.00, 2),
                size: Quantity::new(200.0, 0),
                side: OrderSide::Buy,
                order_id: 2,
            },
            BookOrder {
                price: Price::new(100.00, 2),
                size: Quantity::new(400.0, 0),
                side: OrderSide::Buy,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::new(PRICE_MIN, 2), // <-- Simulate a MARKET order
            size: Quantity::new(500.0, 0),
            side: OrderSide::Sell,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = &fills[0];
        assert_eq!(price1, &Price::new(102.00, 2));
        assert_eq!(size1, &Quantity::new(100.0, 0));

        let (price2, size2) = &fills[1];
        assert_eq!(price2, &Price::new(101.00, 2));
        assert_eq!(size2, &Quantity::new(200.0, 0));

        let (price3, size3) = &fills[2];
        assert_eq!(price3, &Price::new(100.00, 2));
        assert_eq!(size3, &Quantity::new(200.0, 0));
    }

    #[test]
    fn test_simulate_order_fills_sell_with_volume_at_limit_of_precision() {
        let mut ladder = Ladder::new(OrderSide::Buy);

        ladder.add_bulk(vec![
            BookOrder {
                price: Price::new(102.00, 2),
                size: Quantity::new(100.0, 9),
                side: OrderSide::Buy,
                order_id: 1,
            },
            BookOrder {
                price: Price::new(101.00, 2),
                size: Quantity::new(200.0, 9),
                side: OrderSide::Buy,
                order_id: 2,
            },
            BookOrder {
                price: Price::new(100.00, 2),
                size: Quantity::new(400.0, 9),
                side: OrderSide::Buy,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::new(PRICE_MIN, 2),       // <-- Simulate a MARKET order
            size: Quantity::new(699.999999999, 9), // <-- Size slightly less than total size in ladder
            side: OrderSide::Sell,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = &fills[0];
        assert_eq!(price1, &Price::new(102.00, 2));
        assert_eq!(size1, &Quantity::new(100.0, 9));

        let (price2, size2) = &fills[1];
        assert_eq!(price2, &Price::new(101.00, 2));
        assert_eq!(size2, &Quantity::new(200.0, 9));

        let (price3, size3) = &fills[2];
        assert_eq!(price3, &Price::new(100.00, 2));
        assert_eq!(size3, &Quantity::new(399.999999999, 9));
    }
}
