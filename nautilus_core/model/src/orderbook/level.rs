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

use crate::{
    data::order::BookOrder,
    orderbook::{book::BookIntegrityError, ladder::BookPrice},
    types::fixed::FIXED_SCALAR,
};

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
    pub fn volume_raw(&self) -> u64 {
        let mut sum = 0u64;
        for o in self.orders.iter() {
            sum += o.size.raw
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

    #[must_use]
    pub fn exposure_raw(&self) -> u64 {
        let mut sum = 0u64;
        for o in self.orders.iter() {
            sum += ((o.price.as_f64() * o.size.as_f64()) * FIXED_SCALAR) as u64
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
        Some(self.cmp(other))
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
    use crate::{
        data::order::BookOrder,
        enums::OrderSide,
        orderbook::{ladder::BookPrice, level::Level},
        types::{price::Price, quantity::Quantity},
    };

    #[test]
    fn test_comparisons_bid_side() {
        let level0 = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let level1 = Level::new(BookPrice::new(Price::from("1.01"), OrderSide::Buy));
        assert_eq!(level0, level0);
        assert!(level0 > level1);
    }

    #[test]
    fn test_comparisons_ask_side() {
        let level0 = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Sell));
        let level1 = Level::new(BookPrice::new(Price::from("1.01"), OrderSide::Sell));
        assert_eq!(level0, level0);
        assert!(level0 < level1);
    }

    #[test]
    fn test_add_one_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);

        level.add(order);
        assert!(!level.is_empty());
        assert_eq!(level.len(), 1);
        assert_eq!(level.volume(), 10.0);
    }

    #[test]
    fn test_add_multiple_orders() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.len(), 2);
        assert_eq!(level.volume(), 30.0);
        assert_eq!(level.exposure(), 60.0);
    }

    #[test]
    fn test_update_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 0);

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 1);
        assert_eq!(level.volume(), 20.0);
        assert_eq!(level.exposure(), 20.0);
    }

    #[test]
    fn test_update_order_with_zero_size() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.00"),
            Quantity::zero(0).unwrap(),
            0,
        );

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 0);
        assert_eq!(level.volume(), 0.0);
        assert_eq!(level.exposure(), 0.0);
    }

    #[test]
    fn test_delete_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1_id = 0;
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.00"),
            Quantity::from(10),
            order1_id,
        );
        let order2_id = 0;
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.00"),
            Quantity::from(20),
            order2_id,
        );

        level.add(order1);
        level.add(order2);
        level.delete(&order1);
        assert_eq!(level.len(), 1);
        assert_eq!(level.orders.first().unwrap().order_id, order2_id);
        assert_eq!(level.volume(), 20.0);
        assert_eq!(level.exposure(), 20.0);
    }

    #[test]
    fn test_remove_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1_id = 0;
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.00"),
            Quantity::from(10),
            order1_id,
        );
        let order2_id = 1;
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.00"),
            Quantity::from(20),
            order2_id,
        );

        level.add(order1);
        level.add(order2);
        level.remove(order2_id);
        assert_eq!(level.len(), 1);
        assert_eq!(level.orders.first().unwrap().order_id, order1_id);
        assert_eq!(level.volume(), 10.0);
        assert_eq!(level.exposure(), 10.0);
    }

    #[test]
    fn test_add_bulk_orders() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1_id = 0;
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("2.00"),
            Quantity::from(10),
            order1_id,
        );
        let order2_id = 1;
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("2.00"),
            Quantity::from(20),
            order2_id,
        );

        let orders = vec![order1, order2];
        level.add_bulk(orders);
        assert_eq!(level.len(), 2);
        assert_eq!(level.volume(), 30.0);
        assert_eq!(level.exposure(), 60.0);
    }

    #[test]
    #[should_panic(expected = "Invalid book operation: order ID 1 not found")]
    fn test_remove_nonexistent_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        level.remove(1);
    }

    #[test]
    fn test_volume() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(15), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.volume(), 25.0);
    }

    #[test]
    fn test_volume_raw() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.volume_raw(), 30_000_000_000);
    }

    #[test]
    fn test_exposure() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.exposure(), 60.0);
    }

    #[test]
    fn test_exposure_raw() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.exposure_raw(), 60_000_000_000);
    }
}
