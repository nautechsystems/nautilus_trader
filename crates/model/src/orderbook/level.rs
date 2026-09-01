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

//! Represents a discrete price level in an order book.

use std::cmp::Ordering;

#[cfg(feature = "defi")]
use alloy_primitives::U256;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use rust_decimal::Decimal;

#[cfg(feature = "defi")]
use crate::types::fixed::FIXED_PRECISION;
use crate::{
    data::order::{BookOrder, OrderId},
    enums::OrderSide,
    orderbook::{BookIntegrityError, BookPrice},
    types::{fixed::checked_mul_div_fixed, price::PriceRaw, quantity::QuantityRaw},
};

/// Represents a discrete price level in an order book.
///
/// Orders are stored in an [`IndexMap`] which preserves FIFO (insertion) order.
#[derive(Clone, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
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

    #[must_use]
    pub fn side(&self) -> OrderSide {
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
    ///
    /// # Panics
    ///
    /// Panics if the total raw size exceeds [`QuantityRaw::MAX`].
    #[must_use]
    pub fn size_raw(&self) -> QuantityRaw {
        self.orders
            .values()
            .try_fold(0, |total: QuantityRaw, order| {
                total.checked_add(order.size.raw)
            })
            .expect("Overflow occurred when summing `BookLevel` raw size")
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
    ///
    /// Fixed-scale orders contribute `price.raw * size.raw / FIXED_SCALAR`.
    /// Native DeFi scales are normalized to the same fixed-scale result.
    /// Division truncates toward zero.
    /// Non-positive prices contribute zero.
    /// Saturates at `QuantityRaw::MAX` if the total exposure would overflow.
    #[must_use]
    pub fn exposure_raw(&self) -> QuantityRaw {
        self.orders
            .values()
            .map(|order| {
                calculate_exposure_raw(
                    order.price.raw,
                    order.size.raw,
                    order.price.precision,
                    order.size.precision,
                )
            })
            .fold(0, QuantityRaw::saturating_add)
    }

    /// Adds multiple orders to this price level in FIFO order. Orders must match the level's price.
    pub fn add_bulk(&mut self, orders: &[BookOrder]) {
        for order in orders {
            self.add(*order);
        }
    }

    /// Adds an order to this price level. Order must match the level's price.
    pub fn add(&mut self, order: BookOrder) {
        debug_assert_eq!(order.price, self.price.value);

        if !order.size.is_positive() {
            log::warn!(
                "Attempted to add order with non-positive size: order_id={order_id}, size={size}, ignoring",
                order_id = order.order_id,
                size = order.size
            );
            return;
        }

        self.orders.insert(order.order_id, order);
    }

    /// Updates an order at this price level, inserting it if missing. Updated order
    /// must match the level's price. Removes the order if the size becomes zero.
    pub fn update(&mut self, order: BookOrder) {
        debug_assert_eq!(order.price, self.price.value);

        if order.size.raw == 0 {
            // Updating non-existent order to zero size is a no-op, which is valid
            self.orders.shift_remove(&order.order_id);
        } else {
            debug_assert!(
                order.size.is_positive(),
                "Order size must be positive: {}",
                order.size
            );
            self.orders.insert(order.order_id, order);
        }
    }

    /// Deletes an order from this price level.
    pub fn delete(&mut self, order: &BookOrder) {
        self.orders.shift_remove(&order.order_id);
    }

    /// Removes an order by its ID.
    ///
    /// # Panics
    ///
    /// Panics if no order with the given `order_id` exists at this level.
    pub fn remove_by_id(&mut self, order_id: OrderId, sequence: u64, ts_event: UnixNanos) {
        assert!(
            self.orders.shift_remove(&order_id).is_some(),
            "{}",
            &BookIntegrityError::OrderNotFound(order_id, sequence, ts_event)
        );
    }
}

fn calculate_exposure_raw(
    price_raw: PriceRaw,
    size_raw: QuantityRaw,
    price_precision: u8,
    size_precision: u8,
) -> QuantityRaw {
    let Ok(price_raw) = QuantityRaw::try_from(price_raw) else {
        return 0;
    };

    #[cfg(feature = "defi")]
    if price_precision > FIXED_PRECISION || size_precision > FIXED_PRECISION {
        return calculate_exposure_raw_native(price_raw, size_raw, price_precision, size_precision);
    }

    #[cfg(not(feature = "defi"))]
    let _ = (price_precision, size_precision);

    checked_mul_div_fixed(price_raw, size_raw).unwrap_or(QuantityRaw::MAX)
}

#[cfg(feature = "defi")]
fn calculate_exposure_raw_native(
    price_raw: QuantityRaw,
    size_raw: QuantityRaw,
    price_precision: u8,
    size_precision: u8,
) -> QuantityRaw {
    let scale_precision = price_precision.max(FIXED_PRECISION)
        + size_precision.max(FIXED_PRECISION)
        - FIXED_PRECISION;
    let scalar = 10_u128.pow(u32::from(scale_precision));
    let exposure = U256::from(price_raw)
        .checked_mul(U256::from(size_raw))
        .expect("a positive i128 times a u128 fits U256")
        / U256::from(scalar);

    QuantityRaw::try_from(exposure).unwrap_or(QuantityRaw::MAX)
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    #[cfg(feature = "high-precision")]
    use super::calculate_exposure_raw;
    use crate::{
        data::order::BookOrder,
        enums::OrderSide,
        orderbook::{BookLevel, BookPrice},
        types::{
            Price, Quantity,
            fixed::{FIXED_PRECISION, FIXED_SCALAR},
            price::PriceRaw,
            quantity::QuantityRaw,
        },
    };

    #[rstest]
    fn test_empty_level() {
        let level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        assert!(level.first().is_none());
        assert_eq!(level.side(), OrderSide::Buy);
    }

    #[rstest]
    fn test_level_from_order() {
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let level = BookLevel::from_order(order);

        assert_eq!(level.price.value, Price::from("1.00"));
        assert_eq!(level.price.side, OrderSide::Buy);
        assert_eq!(level.len(), 1);
        assert_eq!(level.first().unwrap(), &order);
        assert_eq!(level.size(), 10.0);
    }

    #[rstest]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn test_add_order_incorrect_price_level() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let incorrect_price_order =
            BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 1);
        level.add(incorrect_price_order);
    }

    #[rstest]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn test_add_bulk_orders_incorrect_price() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let orders = [
            BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1),
            BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 2), // Incorrect price
        ];
        level.add_bulk(&orders);
    }

    #[rstest]
    fn test_add_bulk_empty() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        level.add_bulk(&[]);
        assert!(level.is_empty());
    }

    #[rstest]
    #[case::bid(OrderSide::Buy, true)]
    #[case::ask(OrderSide::Sell, false)]
    fn test_comparisons(#[case] side: OrderSide, #[case] first_is_greater: bool) {
        let level0 = BookLevel::new(BookPrice::new(Price::from("1.00"), side));
        let same = BookLevel::new(BookPrice::new(Price::from("1.00"), side));
        let level1 = BookLevel::new(BookPrice::new(Price::from("1.01"), side));

        assert_eq!(level0, same);
        assert_eq!(level0 > level1, first_is_greater);
        assert_eq!(level0 < level1, !first_is_greater);
    }

    #[rstest]
    fn test_book_level_sorting() {
        let mut levels = [
            BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Sell)),
            BookLevel::new(BookPrice::new(Price::from("1.02"), OrderSide::Sell)),
            BookLevel::new(BookPrice::new(Price::from("1.01"), OrderSide::Sell)),
        ];
        levels.sort();
        assert_eq!(levels[0].price.value, Price::from("1.00"));
        assert_eq!(levels[1].price.value, Price::from("1.01"));
        assert_eq!(levels[2].price.value, Price::from("1.02"));
    }

    #[rstest]
    fn test_add_single_order() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 0);

        level.add(order);
        assert!(!level.is_empty());
        assert_eq!(level.len(), 1);
        assert_eq!(level.size(), 10.0);
        assert_eq!(level.first().unwrap(), &order);
    }

    #[rstest]
    fn test_add_multiple_orders() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(20), 2);
        level.add(order1);
        level.add(order2);

        let orders: Vec<_> = level.iter().copied().collect();
        assert_eq!(orders, vec![order1, order2]);
    }

    #[rstest]
    fn test_update_order() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        level.update(order);
        assert_eq!(level.len(), 1);
        assert_eq!(level.first().unwrap(), &order);
    }

    #[rstest]
    fn test_update_zero_size_nonexistent() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::zero(0), 1);
        level.update(order);
        assert_eq!(level.len(), 0);
    }

    #[rstest]
    fn test_fifo_order_after_updates() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));

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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));

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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        let order = BookOrder::new(OrderSide::Buy, Price::from("1.00"), Quantity::from(10), 1);
        level.delete(&order);
        assert_eq!(level.len(), 0);
    }

    #[rstest]
    fn test_delete_order() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
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

        let orders = [order1, order2];
        level.add_bulk(&orders);
        assert_eq!(level.len(), 2);
        assert_eq!(level.size(), 30.0);
        assert_eq!(level.exposure(), 60.0);
    }

    #[rstest]
    fn test_maximum_order_id() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));

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
        let mut level = BookLevel::new(BookPrice::new(Price::from("1.00"), OrderSide::Buy));
        level.remove_by_id(1, 2, 3.into());
    }

    #[rstest]
    fn test_size_raw() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
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
        let mut level = BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.size_decimal(), dec!(30.0));
    }

    #[rstest]
    #[case::negative("-2", "10", 0)]
    #[case::zero("0", "1", 0)]
    #[case::small("2", "10", 20)]
    fn test_exposure_raw_exact_whole(
        #[case] price: &str,
        #[case] size: &str,
        #[case] expected_units: QuantityRaw,
    ) {
        let price = Price::from(price);
        let mut level = BookLevel::new(BookPrice::new(price, OrderSide::Buy));
        level.add(BookOrder::new(
            OrderSide::Buy,
            price,
            Quantity::from(size),
            0,
        ));

        assert_eq!(
            level.exposure_raw(),
            expected_units * FIXED_SCALAR as QuantityRaw
        );
    }

    #[rstest]
    fn test_exposure_raw_truncates_sub_raw_unit() {
        let scalar = FIXED_SCALAR as QuantityRaw;
        let price = Price::from_raw((scalar + 1) as PriceRaw, FIXED_PRECISION);
        let size = Quantity::from_raw(scalar + 1, FIXED_PRECISION);
        let mut level = BookLevel::new(BookPrice::new(price, OrderSide::Buy));
        level.add(BookOrder::new(OrderSide::Buy, price, size, 0));

        assert_eq!(level.exposure_raw(), scalar + 2);
    }

    #[rstest]
    fn test_exposure_raw_accumulates_exactly() {
        let mut level = BookLevel::new(BookPrice::new(Price::from("2.00"), OrderSide::Buy));
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(10), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("2.00"), Quantity::from(20), 1);

        level.add(order1);
        level.add(order2);
        assert_eq!(level.exposure_raw(), 60 * FIXED_SCALAR as QuantityRaw);
    }

    #[cfg(not(feature = "high-precision"))]
    #[rstest]
    fn test_exposure_raw_preserves_non_saturating_raw_units() {
        let price = Price::from("9007199253.999999999");
        let size = Quantity::from("2.000000001");
        let mut level = BookLevel::new(BookPrice::new(price, OrderSide::Buy));
        level.add(BookOrder::new(OrderSide::Buy, price, size, 0));

        assert_eq!(level.exposure_raw(), 18_014_398_517_007_199_251);
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn test_exposure_raw_avoids_phantom_overflow() {
        let scalar = FIXED_SCALAR as QuantityRaw;
        let price_raw = 100_000 * scalar;
        let size_raw = 100 * scalar;

        assert_eq!(price_raw.checked_mul(size_raw), None);
        assert_eq!(
            calculate_exposure_raw(
                price_raw as PriceRaw,
                size_raw,
                FIXED_PRECISION,
                FIXED_PRECISION,
            ),
            10_000_000 * scalar
        );
    }

    #[rstest]
    fn test_exposure_raw_saturates_single_order() {
        #[cfg(feature = "high-precision")]
        let (price_str, qty_str) = ("1000000000000.00", "1000000000000.00");
        #[cfg(not(feature = "high-precision"))]
        let (price_str, qty_str) = ("100000000.00", "1000000000.00");

        let mut level = BookLevel::new(BookPrice::new(Price::from(price_str), OrderSide::Buy));
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from(price_str),
            Quantity::from(qty_str),
            0,
        );

        level.add(order);

        assert_eq!(level.exposure_raw(), QuantityRaw::MAX);
    }

    #[rstest]
    fn test_exposure_raw_accumulation_saturates() {
        #[cfg(feature = "high-precision")]
        let (price_str, qty_str, expected_single) = (
            "100000000000.0",
            "200000000000.0",
            200_000_000_000_000_000_000_000_000_000_000_000_000,
        );
        #[cfg(not(feature = "high-precision"))]
        let (price_str, qty_str, expected_single) =
            ("2.0", "5000000000.0", 10_000_000_000_000_000_000);

        let mut level = BookLevel::new(BookPrice::new(Price::from(price_str), OrderSide::Buy));
        level.add(BookOrder::new(
            OrderSide::Buy,
            Price::from(price_str),
            Quantity::from(qty_str),
            0,
        ));
        assert_eq!(level.exposure_raw(), expected_single);

        level.add(BookOrder::new(
            OrderSide::Buy,
            Price::from(price_str),
            Quantity::from(qty_str),
            1,
        ));
        assert_eq!(level.exposure_raw(), QuantityRaw::MAX);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_exposure_raw_preserves_native_defi_scales() {
        let price_precision = FIXED_PRECISION + 1;
        let size_precision = FIXED_PRECISION + 2;
        let price = Price::from_raw(
            125 * 10_i128.pow(u32::from(price_precision - 2)),
            price_precision,
        );
        let size = Quantity::from_raw(
            24 * 10_u128.pow(u32::from(size_precision - 1)),
            size_precision,
        );
        let mut level = BookLevel::new(BookPrice::new(price, OrderSide::Buy));
        level.add(BookOrder::new(OrderSide::Buy, price, size, 0));

        assert_eq!(level.exposure_raw(), 3 * FIXED_SCALAR as QuantityRaw);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case::native_price(1_250_000_000_000_000_000, 24_000_000_000_000_000, 18, 8)]
    #[case::native_size(12_500_000_000_000_000, 2_400_000_000_000_000_000, 8, 18)]
    fn test_exposure_raw_preserves_mixed_defi_scales(
        #[case] price_raw: PriceRaw,
        #[case] size_raw: QuantityRaw,
        #[case] price_precision: u8,
        #[case] size_precision: u8,
    ) {
        assert_eq!(
            calculate_exposure_raw(price_raw, size_raw, price_precision, size_precision),
            3 * FIXED_SCALAR as QuantityRaw
        );
    }
}
