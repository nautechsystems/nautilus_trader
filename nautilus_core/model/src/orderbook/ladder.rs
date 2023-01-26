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

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use crate::enums::OrderSide;
use crate::orderbook::level::Level;
use crate::orderbook::order::BookOrder;
use crate::types::price::Price;

#[repr(C)]
#[derive(Clone, Debug, Eq)]
pub struct BookPrice {
    pub value: Price,
    pub side: OrderSide,
}

impl BookPrice {
    pub fn new(value: Price, side: OrderSide) -> Self {
        BookPrice { value, side }
    }
}

impl PartialOrd for BookPrice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.side {
            OrderSide::Buy => Some(other.value.cmp(&self.value)),
            OrderSide::Sell => Some(self.value.cmp(&other.value)),
            _ => panic!("`OrderSide` was None"),
        }
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
            _ => panic!("`OrderSide` was None"),
        }
    }
}

#[repr(C)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct Ladder {
    pub side: OrderSide,
    pub levels: Box<BTreeMap<BookPrice, Level>>,
    pub cache: Box<HashMap<u64, BookPrice>>,
}

impl Ladder {
    pub fn new(side: OrderSide) -> Self {
        Ladder {
            side,
            levels: Box::<BTreeMap<BookPrice, Level>>::default(),
            cache: Box::<HashMap<u64, BookPrice>>::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.levels.len() == 0
    }

    pub fn add_bulk(&mut self, orders: Vec<BookOrder>) {
        for order in orders {
            self.add(order)
        }
    }

    pub fn add(&mut self, order: BookOrder) {
        let book_price = order.to_book_price();
        match self.levels.get_mut(&book_price) {
            None => {
                let order_id = order.order_id;
                let level = Level::from_order(order);
                self.cache.insert(order_id, book_price.clone());
                self.levels.insert(book_price, level);
            }
            Some(level) => {
                level.add(order);
            }
        }
    }

    pub fn update(&mut self, order: BookOrder) {
        match self.cache.get(&order.order_id) {
            None => panic!("No order with ID {}", &order.order_id),
            Some(price) => {
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
            }
        }
    }

    pub fn delete(&mut self, order: BookOrder) {
        match self.cache.remove(&order.order_id) {
            None => panic!("No order with ID {}", &order.order_id),
            Some(price) => {
                let level = self.levels.get_mut(&price).unwrap();
                level.delete(&order);
                if level.is_empty() {
                    self.levels.remove(&price);
                }
            }
        }
    }

    pub fn volumes(&self) -> f64 {
        return self.levels.iter().map(|(_, l)| l.volume()).sum();
    }

    pub fn exposures(&self) -> f64 {
        return self.levels.iter().map(|(_, l)| l.exposure()).sum();
    }

    pub fn top(&self) -> Option<&Level> {
        match self.levels.iter().next() {
            None => Option::None,
            Some((_, l)) => Option::Some(l),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::enums::OrderSide;
    use crate::orderbook::ladder::{BookPrice, Ladder};
    use crate::orderbook::order::BookOrder;
    use crate::types::price::Price;
    use crate::types::quantity::Quantity;

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
    fn test_ladder_add_single_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            Price::new(10.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            0,
        );

        ladder.add(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 20.0);
        assert_eq!(ladder.exposures(), 200.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0)
    }

    #[test]
    fn test_ladder_add_multiple_buy_orders() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order1 = BookOrder::new(
            Price::new(10.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            0,
        );
        let order2 = BookOrder::new(
            Price::new(9.00, 2),
            Quantity::new(30.0, 0),
            OrderSide::Buy,
            0,
        );
        let order3 = BookOrder::new(
            Price::new(9.00, 2),
            Quantity::new(50.0, 0),
            OrderSide::Buy,
            0,
        );
        let order4 = BookOrder::new(
            Price::new(8.00, 2),
            Quantity::new(200.0, 0),
            OrderSide::Buy,
            0,
        );

        ladder.add_bulk(vec![order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.volumes(), 300.0);
        assert_eq!(ladder.exposures(), 2520.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0)
    }

    #[test]
    fn test_ladder_add_multiple_sell_orders() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order1 = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Sell,
            0,
        );
        let order2 = BookOrder::new(
            Price::new(12.00, 2),
            Quantity::new(30.0, 0),
            OrderSide::Sell,
            0,
        );
        let order3 = BookOrder::new(
            Price::new(12.00, 2),
            Quantity::new(50.0, 0),
            OrderSide::Sell,
            0,
        );
        let order4 = BookOrder::new(
            Price::new(13.00, 2),
            Quantity::new(200.0, 0),
            OrderSide::Sell,
            0,
        );

        ladder.add_bulk(vec![order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.volumes(), 300.0);
        assert_eq!(ladder.exposures(), 3780.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0)
    }

    #[test]
    fn test_ladder_update_buy_order_price() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            Price::new(11.10, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 20.0);
        assert_eq!(ladder.exposures(), 222.00000000000003);
        assert_eq!(
            ladder.top().unwrap().price.value.as_f64(),
            11.100000000000001
        )
    }

    #[test]
    fn test_ladder_update_sell_order_price() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Sell,
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            Price::new(11.10, 2),
            Quantity::new(20.0, 0),
            OrderSide::Sell,
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 20.0);
        assert_eq!(ladder.exposures(), 222.00000000000003);
        assert_eq!(
            ladder.top().unwrap().price.value.as_f64(),
            11.100000000000001
        )
    }

    #[test]
    fn test_ladder_update_buy_order_size() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0)
    }

    #[test]
    fn test_ladder_update_sell_order_size() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Sell,
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Sell,
            1,
        );

        ladder.update(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.volumes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 11.0)
    }

    #[test]
    fn test_ladder_delete_buy_order() {
        let mut ladder = Ladder::new(OrderSide::Buy);
        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            Price::new(11.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            1,
        );

        ladder.delete(order);
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.volumes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None)
    }

    #[test]
    fn test_ladder_delete_sell_order() {
        let mut ladder = Ladder::new(OrderSide::Sell);
        let order = BookOrder::new(
            Price::new(10.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Sell,
            1,
        );

        ladder.add(order);

        let order = BookOrder::new(
            Price::new(10.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Sell,
            1,
        );

        ladder.delete(order);
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.volumes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None)
    }
}
