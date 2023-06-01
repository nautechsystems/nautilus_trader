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

use crate::data::book::BookOrder;
use crate::orderbook::book::BookIntegrityError;
use crate::orderbook::ladder::BookPrice;

#[derive(Clone, Debug, Eq)]
pub struct Level {
    pub price: BookPrice,
    pub orders: Vec<BookOrder>,
}

impl Level {
    #[must_use]
    pub fn new(price: BookPrice) -> Self {
        Self {
            price,
            orders: Vec::new(),
        }
    }

    #[must_use]
    pub fn from_order(order: BookOrder) -> Self {
        let mut level = Self {
            price: order.to_book_price(),
            orders: Vec::new(),
        };
        level.add(order);
        level
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn add_bulk(&mut self, orders: Vec<BookOrder>) {
        for order in orders {
            self.add(order)
        }
    }

    pub fn add(&mut self, order: BookOrder) {
        assert_eq!(order.price, self.price.value); // Confirm order for this level

        self.orders.push(order);
    }

    pub fn update(&mut self, order: BookOrder) {
        assert_eq!(order.price, self.price.value); // Confirm order for this level

        if order.size.raw == 0 {
            self.delete(&order)
        } else {
            let idx = self
                .orders
                .iter()
                .position(|o| o.order_id == order.order_id)
                .unwrap_or_else(|| {
                    panic!("{}", &BookIntegrityError::OrderNotFound(order.order_id))
                });
            self.orders[idx] = order;
        }
    }

    pub fn delete(&mut self, order: &BookOrder) {
        self.remove(order.order_id);
    }

    pub fn remove(&mut self, order_id: u64) {
        let index = self
            .orders
            .iter()
            .position(|o| o.order_id == order_id)
            .unwrap_or_else(|| panic!("{}", &BookIntegrityError::OrderNotFound(order_id)));
        self.orders.remove(index);
    }

    #[must_use]
    pub fn volume(&self) -> f64 {
        let mut sum: f64 = 0.0;
        for o in self.orders.iter() {
            sum += o.size.as_f64()
        }
        sum
    }

    #[must_use]
    pub fn exposure(&self) -> f64 {
        let mut sum: f64 = 0.0;
        for o in self.orders.iter() {
            sum += o.price.as_f64() * o.size.as_f64()
        }
        sum
    }
}

impl PartialEq for Level {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }
}

impl PartialOrd for Level {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.price.partial_cmp(&other.price)
    }

    fn lt(&self, other: &Self) -> bool {
        self.price.lt(&other.price)
    }

    fn le(&self, other: &Self) -> bool {
        self.price.le(&other.price)
    }

    fn gt(&self, other: &Self) -> bool {
        self.price.gt(&other.price)
    }

    fn ge(&self, other: &Self) -> bool {
        self.price.ge(&other.price)
    }
}

impl Ord for Level {
    fn cmp(&self, other: &Self) -> Ordering {
        self.price.cmp(&other.price)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::data::book::BookOrder;
    use crate::enums::OrderSide;
    use crate::orderbook::ladder::BookPrice;
    use crate::orderbook::level::Level;
    use crate::types::price::Price;
    use crate::types::quantity::Quantity;

    #[test]
    fn test_level_comparisons_bid_side() {
        let level0 = Level::new(BookPrice::new(Price::new(1.00, 2), OrderSide::Buy));
        let level1 = Level::new(BookPrice::new(Price::new(1.01, 2), OrderSide::Buy));
        assert_eq!(level0, level0);
        assert!(level0 > level1);
    }

    #[test]
    fn test_level_comparisons_ask_side() {
        let level0 = Level::new(BookPrice::new(Price::new(1.00, 2), OrderSide::Sell));
        let level1 = Level::new(BookPrice::new(Price::new(1.01, 2), OrderSide::Sell));
        assert_eq!(level0, level0);
        assert!(level0 < level1);
    }

    #[test]
    fn test_level_add_one_order() {
        let mut level = Level::new(BookPrice::new(Price::new(1.00, 2), OrderSide::Buy));
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            0,
        );

        level.add(order);
        assert!(!level.is_empty());
        assert_eq!(level.len(), 1);
        assert_eq!(level.volume(), 10.0);
    }

    #[test]
    fn test_level_add_multiple_orders() {
        let mut level = Level::new(BookPrice::new(Price::new(2.00, 2), OrderSide::Buy));
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::new(2.00, 2),
            Quantity::new(10.0, 0),
            0,
        );
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::new(2.00, 2),
            Quantity::new(20.0, 0),
            1,
        );

        level.add(order1);
        level.add(order2);
        assert_eq!(level.len(), 2);
        assert_eq!(level.volume(), 30.0);
        assert_eq!(level.exposure(), 60.0);
    }

    #[test]
    fn test_level_update_order() {
        let mut level = Level::new(BookPrice::new(Price::new(1.00, 2), OrderSide::Buy));
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            0,
        );
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::new(1.00, 2),
            Quantity::new(20.0, 0),
            0,
        );

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 1);
        assert_eq!(level.volume(), 20.0);
        assert_eq!(level.exposure(), 20.0);
    }

    #[test]
    fn test_level_update_order_with_zero_size_deletes() {
        let mut level = Level::new(BookPrice::new(Price::new(1.00, 2), OrderSide::Buy));
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            0,
        );
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::new(1.00, 2),
            Quantity::new(0.0, 0),
            0,
        );

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 0);
        assert_eq!(level.volume(), 0.0);
        assert_eq!(level.exposure(), 0.0);
    }
}
