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

use std::{cmp::Ordering, collections::BTreeMap};

use crate::{
    data::order::{BookOrder, OrderId},
    orderbook::{book::BookIntegrityError, ladder::BookPrice},
    types::fixed::FIXED_SCALAR,
};

/// Represents a discrete price level in an order book.
///
/// The level maintains a collection of orders as well as tracking insertion order
/// to preserve FIFO queue dynamics.
#[derive(Clone, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Level {
    pub price: BookPrice,
    pub orders: BTreeMap<OrderId, BookOrder>,
    insertion_order: Vec<OrderId>,
}

impl Level {
    #[must_use]
    pub fn new(price: BookPrice) -> Self {
        Self {
            price,
            orders: BTreeMap::new(),
            insertion_order: Vec::new(),
        }
    }

    #[must_use]
    pub fn from_order(order: BookOrder) -> Self {
        let mut level = Self {
            price: order.to_book_price(),
            orders: BTreeMap::new(),
            insertion_order: Vec::new(),
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

    #[must_use]
    pub fn first(&self) -> Option<&BookOrder> {
        self.insertion_order
            .first()
            .and_then(|&id| self.orders.get(&id))
    }

    /// Returns the orders in the insertion order.
    #[must_use]
    pub fn get_orders(&self) -> Vec<BookOrder> {
        self.insertion_order
            .iter()
            .filter_map(|id| self.orders.get(id))
            .copied()
            .collect()
    }

    #[must_use]
    pub fn size(&self) -> f64 {
        self.orders.values().map(|o| o.size.as_f64()).sum()
    }

    #[must_use]
    pub fn size_raw(&self) -> u64 {
        self.orders.values().map(|o| o.size.raw).sum()
    }

    #[must_use]
    pub fn exposure(&self) -> f64 {
        self.orders
            .values()
            .map(|o| o.price.as_f64() * o.size.as_f64())
            .sum()
    }

    #[must_use]
    pub fn exposure_raw(&self) -> u64 {
        self.orders
            .values()
            .map(|o| ((o.price.as_f64() * o.size.as_f64()) * FIXED_SCALAR) as u64)
            .sum()
    }

    pub fn add_bulk(&mut self, orders: Vec<BookOrder>) {
        self.insertion_order
            .extend(orders.iter().map(|o| o.order_id));

        for order in orders {
            self.check_order_for_this_level(&order);
            self.orders.insert(order.order_id, order);
        }
    }

    pub fn add(&mut self, order: BookOrder) {
        self.check_order_for_this_level(&order);

        self.orders.insert(order.order_id, order);
        self.insertion_order.push(order.order_id);
    }

    pub fn update(&mut self, order: BookOrder) {
        self.check_order_for_this_level(&order);

        if order.size.raw == 0 {
            self.orders.remove(&order.order_id);
            self.update_insertion_order();
        } else {
            self.orders.insert(order.order_id, order);
        }
    }

    pub fn delete(&mut self, order: &BookOrder) {
        self.orders.remove(&order.order_id);
        self.update_insertion_order();
    }

    pub fn remove_by_id(&mut self, order_id: OrderId, ts_event: u64, sequence: u64) {
        assert!(
            self.orders.remove(&order_id).is_some(),
            "{}",
            &BookIntegrityError::OrderNotFound(order_id, ts_event, sequence)
        );
        self.update_insertion_order();
    }

    fn check_order_for_this_level(&self, order: &BookOrder) {
        assert_eq!(order.price, self.price.value);
    }

    fn update_insertion_order(&mut self) {
        self.insertion_order
            .retain(|&id| self.orders.contains_key(&id));
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
    use rstest::rstest;

    use crate::{
        data::order::BookOrder,
        enums::OrderSide,
        orderbook::{ladder::BookPrice, level::Level},
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_empty_level() {
        let level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        assert!(level.first().is_none());
    }

    #[rstest]
    fn test_comparisons_bid_side() {
        let level0 = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let level1 = Level::new(BookPrice::new(Price::from("1.01"), OrderSide::Buy));
        assert_eq!(level0, level0);
        assert!(level0 > level1);
    }

    #[rstest]
    fn test_comparisons_ask_side() {
        let level0 = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Sell));
        let level1 = Level::new(BookPrice::new(Price::from("1.01"), OrderSide::Sell));
        assert_eq!(level0, level0);
        assert!(level0 < level1);
    }

    #[rstest]
    fn test_add_one_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);

        level.add(order);
        assert!(!level.is_empty());
        assert_eq!(level.len(), 1);
        assert_eq!(level.size(), 10.0);
        assert_eq!(level.first().unwrap(), &order);
    }

    #[rstest]
    fn test_add_multiple_orders() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.len(), 2);
        assert_eq!(level.size(), 30.0);
        assert_eq!(level.exposure(), 60.0);
        assert_eq!(level.first().unwrap(), &order1);
    }

    #[rstest]
    fn test_update_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 0);

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 1);
        assert_eq!(level.size(), 20.0);
        assert_eq!(level.exposure(), 20.0);
    }

    #[rstest]
    fn test_update_order_with_zero_size() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::zero(0), 0);

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 0);
        assert_eq!(level.size(), 0.0);
        assert_eq!(level.exposure(), 0.0);
    }

    #[rstest]
    fn test_delete_order() {
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
        level.delete(&order1);
        assert_eq!(level.len(), 1);
        assert_eq!(level.size(), 20.0);
        assert!(level.orders.contains_key(&order2_id));
        assert_eq!(level.exposure(), 20.0);
    }

    #[rstest]
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
        level.remove_by_id(order2_id, 0, 0);
        assert_eq!(level.len(), 1);
        assert!(level.orders.contains_key(&order1_id));
        assert_eq!(level.size(), 10.0);
        assert_eq!(level.exposure(), 10.0);
    }

    #[rstest]
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
        assert_eq!(level.size(), 30.0);
        assert_eq!(level.exposure(), 60.0);
    }

    #[rstest]
    #[should_panic(
        expected = "Integrity error: order not found: order_id=1, ts_event=2, sequence=3"
    )]
    fn test_remove_nonexistent_order() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        level.remove_by_id(1, 2, 3);
    }

    #[rstest]
    fn test_size() {
        let mut level = Level::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(15), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.size(), 25.0);
    }

    #[rstest]
    fn test_size_raw() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.size_raw(), 30_000_000_000);
    }

    #[rstest]
    fn test_exposure() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.exposure(), 60.0);
    }

    #[rstest]
    fn test_exposure_raw() {
        let mut level = Level::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.exposure_raw(), 60_000_000_000);
    }
}
