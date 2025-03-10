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

//! Represents a ladder of price levels for one side of an order book.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    fmt::{Debug, Display, Formatter},
};

use nautilus_core::UnixNanos;

use crate::{
    data::order::{BookOrder, OrderId},
    enums::OrderSideSpecified,
    orderbook::BookLevel,
    types::{Price, Quantity},
};

/// Represents a price level with a specified side in an order books ladder.
#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BookPrice {
    pub value: Price,
    pub side: OrderSideSpecified,
}

impl BookPrice {
    /// Creates a new [`BookPrice`] instance.
    #[must_use]
    pub fn new(value: Price, side: OrderSideSpecified) -> Self {
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
            OrderSideSpecified::Buy => other.value.cmp(&self.value),
            OrderSideSpecified::Sell => self.value.cmp(&other.value),
        }
    }
}

impl Display for BookPrice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

/// Represents a ladder of price levels for one side of an order book.
#[derive(Clone, Debug)]
pub(crate) struct BookLadder {
    pub side: OrderSideSpecified,
    pub levels: BTreeMap<BookPrice, BookLevel>,
    pub cache: HashMap<u64, BookPrice>,
}

impl BookLadder {
    /// Creates a new [`Ladder`] instance.
    #[must_use]
    pub fn new(side: OrderSideSpecified) -> Self {
        Self {
            side,
            levels: BTreeMap::new(),
            cache: HashMap::new(),
        }
    }

    /// Returns the number of price levels in the ladder.
    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Returns true if the ladder has no price levels.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    #[allow(dead_code)] // Used in tests
    /// Adds multiple orders to the ladder.
    pub fn add_bulk(&mut self, orders: Vec<BookOrder>) {
        for order in orders {
            self.add(order);
        }
    }

    /// Removes all orders and price levels from the ladder.
    pub fn clear(&mut self) {
        self.levels.clear();
        self.cache.clear();
    }

    /// Adds an order to the ladder at its price level.
    pub fn add(&mut self, order: BookOrder) {
        let book_price = order.to_book_price();
        self.cache.insert(order.order_id, book_price);

        match self.levels.get_mut(&book_price) {
            Some(level) => {
                level.add(order);
            }
            None => {
                let level = BookLevel::from_order(order);
                self.levels.insert(book_price, level);
            }
        }
    }

    /// Updates an existing order in the ladder, moving it to a new price level if needed.
    pub fn update(&mut self, order: BookOrder) {
        let price = self.cache.get(&order.order_id).copied();
        if let Some(price) = price {
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

    /// Deletes an order from the ladder.
    pub fn delete(&mut self, order: BookOrder, sequence: u64, ts_event: UnixNanos) {
        self.remove(order.order_id, sequence, ts_event);
    }

    /// Removes an order by its ID from the ladder.
    pub fn remove(&mut self, order_id: OrderId, sequence: u64, ts_event: UnixNanos) {
        if let Some(price) = self.cache.remove(&order_id) {
            if let Some(level) = self.levels.get_mut(&price) {
                level.remove_by_id(order_id, sequence, ts_event);
                if level.is_empty() {
                    self.levels.remove(&price);
                }
            }
        }
    }

    /// Returns the total size of all orders in the ladder.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn sizes(&self) -> f64 {
        self.levels.values().map(BookLevel::size).sum()
    }

    /// Returns the total value exposure (price * size) of all orders in the ladder.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn exposures(&self) -> f64 {
        self.levels.values().map(BookLevel::exposure).sum()
    }

    /// Returns the best price level in the ladder.
    #[must_use]
    pub fn top(&self) -> Option<&BookLevel> {
        match self.levels.iter().next() {
            Some((_, l)) => Option::Some(l),
            None => Option::None,
        }
    }

    /// Simulates fills for an order against this ladder's liquidity.
    /// Returns a list of (price, size) tuples representing the simulated fills.
    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        let is_reversed = self.side == OrderSideSpecified::Buy;
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

impl Display for BookLadder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}(side={})", stringify!(BookLadder), self.side)?;
        for (price, level) in &self.levels {
            writeln!(f, "  {} -> {} orders", price, level.len())?;
        }
        Ok(())
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
        enums::{OrderSide, OrderSideSpecified},
        orderbook::ladder::{BookLadder, BookPrice},
        types::{Price, Quantity},
    };

    #[rstest]
    fn test_is_empty() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy);
        assert!(ladder.is_empty(), "A new ladder should be empty");
    }

    #[rstest]
    fn test_is_empty_after_add() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        assert!(ladder.is_empty(), "Ladder should start empty");
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(100), 1);
        ladder.add(order);
        assert!(
            !ladder.is_empty(),
            "Ladder should not be empty after adding an order"
        );
    }

    #[rstest]
    fn test_add_bulk_empty() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        ladder.add_bulk(vec![]);
        assert!(
            ladder.is_empty(),
            "Adding an empty vector should leave the ladder empty"
        );
    }

    #[rstest]
    fn test_add_bulk_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let orders = vec![
            BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1),
            BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(30), 2),
            BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(50), 3),
        ];
        ladder.add_bulk(orders);
        // All orders share the same price, so there should be one price level.
        assert_eq!(ladder.len(), 1, "Ladder should have one price level");
        let orders_in_level = ladder.top().unwrap().get_orders();
        assert_eq!(
            orders_in_level.len(),
            3,
            "Price level should contain all bulk orders"
        );
    }

    #[rstest]
    fn test_book_price_bid_sorting() {
        let mut bid_prices = [
            BookPrice::new(Price::from("2.0"), OrderSideSpecified::Buy),
            BookPrice::new(Price::from("4.0"), OrderSideSpecified::Buy),
            BookPrice::new(Price::from("1.0"), OrderSideSpecified::Buy),
            BookPrice::new(Price::from("3.0"), OrderSideSpecified::Buy),
        ];
        bid_prices.sort();
        assert_eq!(bid_prices[0].value.as_f64(), 4.0);
    }

    #[rstest]
    fn test_book_price_ask_sorting() {
        let mut ask_prices = [
            BookPrice::new(Price::from("2.0"), OrderSideSpecified::Sell),
            BookPrice::new(Price::from("4.0"), OrderSideSpecified::Sell),
            BookPrice::new(Price::from("1.0"), OrderSideSpecified::Sell),
            BookPrice::new(Price::from("3.0"), OrderSideSpecified::Sell),
        ];

        ask_prices.sort();
        assert_eq!(ask_prices[0].value.as_f64(), 1.0);
    }

    #[rstest]
    fn test_add_single_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 0);

        ladder.add(order);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 200.0);
        assert_eq!(ladder.top().unwrap().price.value.as_f64(), 10.0);
    }

    #[rstest]
    fn test_add_multiple_buy_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("8.00"), Quantity::from(30), 2);

        ladder.add(order1);
        ladder.add(order2);

        assert_eq!(ladder.top().unwrap().price.value, Price::from("9.00"));
    }

    #[rstest]
    fn test_add_ascending_sell_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell);
        let order1 = BookOrder::new(OrderSide::Sell, Price::from("8.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Sell, Price::from("9.00"), Quantity::from(30), 2);

        ladder.add(order1);
        ladder.add(order2);

        assert_eq!(ladder.top().unwrap().price.value, Price::from("8.00"));
    }

    #[rstest]
    fn test_update_buy_order_price() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell);
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);

        ladder.delete(order, 0, 0.into());

        assert_eq!(ladder.len(), 0);
    }

    #[rstest]
    fn test_delete_buy_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(10), 1);

        ladder.delete(order, 0, 0.into());
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.sizes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None);
    }

    #[rstest]
    fn test_delete_sell_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell);
        let order = BookOrder::new(OrderSide::Sell, Price::from("10.00"), Quantity::from(10), 1);

        ladder.add(order);

        let order = BookOrder::new(OrderSide::Sell, Price::from("10.00"), Quantity::from(10), 1);

        ladder.delete(order, 0, 0.into());
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.sizes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None);
    }

    #[rstest]
    fn test_ladder_sizes_empty() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy);
        assert_eq!(
            ladder.sizes(),
            0.0,
            "An empty ladder should have total size 0.0"
        );
    }

    #[rstest]
    fn test_ladder_exposures_empty() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy);
        assert_eq!(
            ladder.exposures(),
            0.0,
            "An empty ladder should have total exposure 0.0"
        );
    }

    #[rstest]
    fn test_ladder_sizes() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.50"), Quantity::from(30), 2);
        ladder.add(order1);
        ladder.add(order2);

        let expected_size = 20.0 + 30.0;
        assert_eq!(
            ladder.sizes(),
            expected_size,
            "Ladder total size should match the sum of order sizes"
        );
    }

    #[rstest]
    fn test_ladder_exposures() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.50"), Quantity::from(30), 2);
        ladder.add(order1);
        ladder.add(order2);

        let expected_exposure = 10.00 * 20.0 + 9.50 * 30.0;
        assert_eq!(
            ladder.exposures(),
            expected_exposure,
            "Ladder total exposure should match the sum of individual exposures"
        );
    }

    #[rstest]
    fn test_iter_returns_fifo() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(30), 2);
        ladder.add(order1);
        ladder.add(order2);
        let orders: Vec<BookOrder> = ladder.top().unwrap().iter().copied().collect();
        assert_eq!(
            orders,
            vec![order1, order2],
            "Iterator should return orders in FIFO order"
        );
    }

    #[rstest]
    fn test_update_missing_order_inserts() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        // Call update on an order that hasn't been added yet (upsert behavior)
        ladder.update(order);
        assert_eq!(
            ladder.len(),
            1,
            "Ladder should have one level after upsert update"
        );
        let orders = ladder.top().unwrap().get_orders();
        assert_eq!(
            orders.len(),
            1,
            "Price level should contain the inserted order"
        );
        assert_eq!(orders[0], order, "The inserted order should match");
    }

    #[rstest]
    fn test_cache_consistency_after_operations() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(30), 2);
        ladder.add(order1);
        ladder.add(order2);

        // Ensure that each order in the cache is present in the corresponding price level.
        for (order_id, price) in &ladder.cache {
            let level = ladder
                .levels
                .get(price)
                .expect("Every price in the cache should have a corresponding level");
            assert!(
                level.orders.contains_key(order_id),
                "Order id {order_id} should be present in the level for price {price}",
            );
        }
    }

    #[rstest]
    fn test_simulate_fills_with_empty_book() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy);
        let order = BookOrder::new(OrderSide::Buy, Price::max(2), Quantity::from(500), 1);

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    #[case(OrderSide::Buy, Price::max(2), OrderSideSpecified::Sell)]
    #[case(OrderSide::Sell, Price::min(2), OrderSideSpecified::Buy)]
    fn test_simulate_order_fills_with_no_size(
        #[case] side: OrderSide,
        #[case] price: Price,
        #[case] ladder_side: OrderSideSpecified,
    ) {
        let ladder = BookLadder::new(ladder_side);
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
    #[case(OrderSide::Buy, OrderSideSpecified::Sell, Price::from("60.0"))]
    #[case(OrderSide::Sell, OrderSideSpecified::Buy, Price::from("40.0"))]
    fn test_simulate_order_fills_buy_when_far_from_market(
        #[case] order_side: OrderSide,
        #[case] ladder_side: OrderSideSpecified,
        #[case] ladder_price: Price,
    ) {
        let mut ladder = BookLadder::new(ladder_side);

        ladder.add(BookOrder {
            price: ladder_price,
            size: Quantity::from(100),
            side: ladder_side.as_order_side(),
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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);

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
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell);

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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);

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
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy);

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

        let mut ladder_buy = BookLadder::new(OrderSideSpecified::Buy);
        let mut ladder_sell = BookLadder::new(OrderSideSpecified::Sell);

        let order_buy = BookOrder::new(OrderSide::Buy, min_price, Quantity::from(1), 1);
        let order_sell = BookOrder::new(OrderSide::Sell, max_price, Quantity::from(1), 1);

        ladder_buy.add(order_buy);
        ladder_sell.add(order_sell);

        assert_eq!(ladder_buy.top().unwrap().price.value, min_price);
        assert_eq!(ladder_sell.top().unwrap().price.value, max_price);
    }
}
