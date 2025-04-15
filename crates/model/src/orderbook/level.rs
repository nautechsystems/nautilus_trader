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

//! Represents a discrete price level in an order book.

use std::cmp::Ordering;

use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use rust_decimal::Decimal;

use crate::{
    data::order::{BookOrder, OrderId},
    enums::OrderSideSpecified,
    orderbook::{BookIntegrityError, BookPrice},
    types::{fixed::FIXED_SCALAR, quantity::QuantityRaw},
};

/// Represents a discrete price level in an order book.
///
/// Orders are stored in an [`IndexMap`] which preserves FIFO (insertion) order.
#[derive(Clone, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BookLevel {
    pub price: BookPrice,
    pub(crate) orders: IndexMap<OrderId, BookOrder>,
}

impl BookLevel {
    /// Creates a new [`BookLevel`] instance.
    #[must_use]
    pub fn new(price: BookPrice) -> Self {
        Self {
            price,
            orders: IndexMap::new(),
        }
    }

    /// Creates a new [`BookLevel`] from an order, using the order's price and side.
    #[must_use]
    pub fn from_order(order: BookOrder) -> Self {
        let mut level = Self {
            price: order.to_book_price(),
            orders: IndexMap::new(),
        };
        level.add(order);
        level
    }

    pub fn side(&self) -> OrderSideSpecified {
        self.price.side
    }

    /// Returns the number of orders at this price level.
    #[must_use]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Returns true if this price level has no orders.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Returns a reference to the first order at this price level in FIFO order.
    #[inline]
    #[must_use]
    pub fn first(&self) -> Option<&BookOrder> {
        self.orders.get_index(0).map(|(_key, order)| order)
    }

    /// Returns an iterator over the orders at this price level in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = &BookOrder> {
        self.orders.values()
    }

    /// Returns all orders at this price level in FIFO insertion order.
    #[must_use]
    pub fn get_orders(&self) -> Vec<BookOrder> {
        self.orders.values().copied().collect()
    }

    /// Returns the total size of all orders at this price level as a float.
    #[must_use]
    pub fn size(&self) -> f64 {
        self.orders.values().map(|o| o.size.as_f64()).sum()
    }

    /// Returns the total size of all orders at this price level as raw integer units.
    #[must_use]
    pub fn size_raw(&self) -> QuantityRaw {
        self.orders.values().map(|o| o.size.raw).sum()
    }

    /// Returns the total size of all orders at this price level as a decimal.
    #[must_use]
    pub fn size_decimal(&self) -> Decimal {
        self.orders.values().map(|o| o.size.as_decimal()).sum()
    }

    /// Returns the total exposure (price * size) of all orders at this price level as a float.
    #[must_use]
    pub fn exposure(&self) -> f64 {
        self.orders
            .values()
            .map(|o| o.price.as_f64() * o.size.as_f64())
            .sum()
    }

    /// Returns the total exposure (price * size) of all orders at this price level as raw integer units.
    #[must_use]
    pub fn exposure_raw(&self) -> QuantityRaw {
        self.orders
            .values()
            .map(|o| ((o.price.as_f64() * o.size.as_f64()) * FIXED_SCALAR) as QuantityRaw)
            .sum()
    }

    /// Adds multiple orders to this price level in FIFO order. Orders must match the level's price.
    pub fn add_bulk(&mut self, orders: Vec<BookOrder>) {
        for order in orders {
            self.add(order);
        }
    }

    /// Adds an order to this price level. Order must match the level's price.
    pub fn add(&mut self, order: BookOrder) {
        debug_assert_eq!(order.price, self.price.value);

        self.orders.insert(order.order_id, order);
    }

    /// Updates an existing order at this price level. Updated order must match the level's price.
    /// Removes the order if size becomes zero.
    pub fn update(&mut self, order: BookOrder) {
        debug_assert_eq!(order.price, self.price.value);

        if order.size.raw == 0 {
            self.orders.shift_remove(&order.order_id);
        } else {
            self.orders.insert(order.order_id, order);
        }
    }

    /// Deletes an order from this price level.
    pub fn delete(&mut self, order: &BookOrder) {
        self.orders.shift_remove(&order.order_id);
    }

    /// Removes an order by its ID. Panics if the order doesn't exist.
    pub fn remove_by_id(&mut self, order_id: OrderId, sequence: u64, ts_event: UnixNanos) {
        assert!(
            self.orders.shift_remove(&order_id).is_some(),
            "{}",
            &BookIntegrityError::OrderNotFound(order_id, sequence, ts_event)
        );
    }
}

impl PartialEq for BookLevel {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }
}

impl PartialOrd for BookLevel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BookLevel {
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
    use rust_decimal_macros::dec;

    use crate::{
        data::order::BookOrder,
        enums::{OrderSide, OrderSideSpecified},
        orderbook::{BookLevel, BookPrice},
        types::{Price, Quantity, fixed::FIXED_SCALAR, quantity::QuantityRaw},
    };

    #[rstest]
    fn test_empty_level() {
        let level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        assert!(level.first().is_none());
        assert_eq!(level.side(), OrderSideSpecified::Buy);
    }

    #[rstest]
    fn test_level_from_order() {
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let level = BookLevel::from_order(order);

        assert_eq!(level.price.value, Price::from("1.00"));
        assert_eq!(level.price.side, OrderSideSpecified::Buy);
        assert_eq!(level.len(), 1);
        assert_eq!(level.first().unwrap(), &order);
        assert_eq!(level.size(), 10.0);
    }

    #[rstest]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn test_add_order_incorrect_price_level() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let incorrect_price_order =
            BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 1);
        level.add(incorrect_price_order);
    }

    #[rstest]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn test_add_bulk_orders_incorrect_price() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let orders = vec![
            BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1),
            BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 2), // Incorrect price
        ];
        level.add_bulk(orders);
    }

    #[rstest]
    fn test_add_bulk_empty() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        level.add_bulk(vec![]);
        assert!(level.is_empty());
    }

    #[rstest]
    fn test_comparisons_bid_side() {
        let level0 = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let level1 = BookLevel::new(BookPrice::new(Price::from("1.01"), OrderSideSpecified::Buy));
        assert_eq!(level0, level0);
        assert!(level0 > level1);
    }

    #[rstest]
    fn test_comparisons_ask_side() {
        let level0 = BookLevel::new(BookPrice::new(
            Price::from("1.00"),
            OrderSideSpecified::Sell,
        ));
        let level1 = BookLevel::new(BookPrice::new(
            Price::from("1.01"),
            OrderSideSpecified::Sell,
        ));
        assert_eq!(level0, level0);
        assert!(level0 < level1);
    }

    #[rstest]
    fn test_book_level_sorting() {
        let mut levels = vec![
            BookLevel::new(BookPrice::new(
                Price::from("1.00"),
                OrderSideSpecified::Sell,
            )),
            BookLevel::new(BookPrice::new(
                Price::from("1.02"),
                OrderSideSpecified::Sell,
            )),
            BookLevel::new(BookPrice::new(
                Price::from("1.01"),
                OrderSideSpecified::Sell,
            )),
        ];
        levels.sort();
        assert_eq!(levels[0].price.value, Price::from("1.00"));
        assert_eq!(levels[1].price.value, Price::from("1.01"));
        assert_eq!(levels[2].price.value, Price::from("1.02"));
    }

    #[rstest]
    fn test_add_single_order() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);

        level.add(order);
        assert!(!level.is_empty());
        assert_eq!(level.len(), 1);
        assert_eq!(level.size(), 10.0);
        assert_eq!(level.first().unwrap(), &order);
    }

    #[rstest]
    fn test_add_multiple_orders() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSideSpecified::Buy));
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
    fn test_get_orders() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 2);

        level.add(order1);
        level.add(order2);

        let orders = level.get_orders();
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0], order1); // Checks FIFO order maintained
        assert_eq!(orders[1], order2);
    }

    #[rstest]
    fn test_iter_returns_fifo() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 2);
        level.add(order1);
        level.add(order2);

        let orders: Vec<_> = level.iter().copied().collect();
        assert_eq!(orders, vec![order1, order2]);
    }

    #[rstest]
    fn test_update_order() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 0);

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 1);
        assert_eq!(level.size(), 20.0);
        assert_eq!(level.exposure(), 20.0);
    }

    #[rstest]
    fn test_update_inserts_if_missing() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        level.update(order);
        assert_eq!(level.len(), 1);
        assert_eq!(level.first().unwrap(), &order);
    }

    #[rstest]
    fn test_update_zero_size_nonexistent() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::zero(0), 1);
        level.update(order);
        assert_eq!(level.len(), 0);
    }

    #[rstest]
    fn test_fifo_order_after_updates() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));

        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 2);

        level.add(order1);
        level.add(order2);

        // Update order1 size
        let updated_order1 =
            BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(15), 1);
        level.update(updated_order1);

        let orders = level.get_orders();
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0], updated_order1); // First order still first
        assert_eq!(orders[1], order2); // Second order still second
    }

    #[rstest]
    fn test_insertion_order_after_mixed_operations() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 2);
        let order3 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(30), 3);

        level.add(order1);
        level.add(order2);
        level.add(order3);

        // Update order2 (should keep its position)
        let updated_order2 =
            BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(25), 2);
        level.update(updated_order2);

        // Remove order1; order2 (updated) should now be first
        level.delete(&order1);

        let orders = level.get_orders();
        assert_eq!(orders, vec![updated_order2, order3]);
    }

    #[rstest]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn test_update_order_incorrect_price() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));

        // Add initial order at correct price level
        let initial_order =
            BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        level.add(initial_order);

        // Attempt to update with order at incorrect price level
        let updated_order =
            BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);
        level.update(updated_order);
    }

    #[rstest]
    fn test_update_order_with_zero_size() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::zero(0), 0);

        level.add(order1);
        level.update(order2);
        assert_eq!(level.len(), 0);
        assert_eq!(level.size(), 0.0);
        assert_eq!(level.exposure(), 0.0);
    }

    #[rstest]
    fn test_delete_nonexistent_order() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        level.delete(&order);
        assert_eq!(level.len(), 0);
    }

    #[rstest]
    fn test_delete_order() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
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
    fn test_remove_order_by_id() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
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
        level.remove_by_id(order2_id, 0, 0.into());
        assert_eq!(level.len(), 1);
        assert!(level.orders.contains_key(&order1_id));
        assert_eq!(level.size(), 10.0);
        assert_eq!(level.exposure(), 10.0);
    }

    #[rstest]
    fn test_add_bulk_orders() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSideSpecified::Buy));
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
    fn test_maximum_order_id() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.00"),
            Quantity::from(10),
            u64::MAX,
        );
        level.add(order);

        assert_eq!(level.len(), 1);
        assert_eq!(level.first().unwrap(), &order);
    }

    #[rstest]
    #[should_panic(
        expected = "Integrity error: order not found: order_id=1, sequence=2, ts_event=3"
    )]
    fn test_remove_nonexistent_order() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        level.remove_by_id(1, 2, 3.into());
    }

    #[rstest]
    fn test_size() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(15), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.size(), 25.0);
    }

    #[rstest]
    fn test_size_raw() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(
            level.size_raw(),
            (30.0 * FIXED_SCALAR).round() as QuantityRaw
        );
    }

    #[rstest]
    fn test_size_decimal() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.size_decimal(), dec!(30.0));
    }

    #[rstest]
    fn test_exposure() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.exposure(), 60.0);
    }

    #[rstest]
    fn test_exposure_raw() {
        let mut level =
            BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSideSpecified::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(
            level.exposure_raw(),
            (60.0 * FIXED_SCALAR).round() as QuantityRaw
        );
    }
}
