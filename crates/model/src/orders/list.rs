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

use std::{
    fmt::Display,
    hash::{Hash, Hasher},
};

use nautilus_core::{UnixNanos, correctness::check_slice_not_empty};
use serde::{Deserialize, Serialize};

use super::{Order, OrderAny};
use crate::identifiers::{InstrumentId, OrderListId, StrategyId};

#[derive(Clone, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderList {
    pub id: OrderListId,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub orders: Vec<OrderAny>,
    pub ts_init: UnixNanos,
}

impl OrderList {
    /// Creates a new [`OrderList`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `orders` is empty or if any order's instrument or strategy ID does not match.
    pub fn new(
        order_list_id: OrderListId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        orders: Vec<OrderAny>,
        ts_init: UnixNanos,
    ) -> Self {
        check_slice_not_empty(orders.as_slice(), stringify!(orders)).unwrap();
        for order in &orders {
            assert_eq!(instrument_id, order.instrument_id());
            assert_eq!(strategy_id, order.strategy_id());
        }
        Self {
            id: order_list_id,
            instrument_id,
            strategy_id,
            orders,
            ts_init,
        }
    }

    /// Returns a reference to the first order in the list.
    #[must_use]
    pub fn first(&self) -> Option<&OrderAny> {
        self.orders.first()
    }

    /// Returns the number of orders in the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Returns true if the list contains no orders.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

impl PartialEq for OrderList {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Hash for OrderList {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Display for OrderList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OrderList(\
            id={}, \
            instrument_id={}, \
            strategy_id={}, \
            orders={:?}, \
            ts_init={}\
            )",
            self.id, self.instrument_id, self.strategy_id, self.orders, self.ts_init,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;

    use rstest::rstest;

    use super::*;
    use crate::{
        enums::{OrderSide, OrderType},
        identifiers::{OrderListId, StrategyId},
        instruments::{CurrencyPair, stubs::*},
        orders::OrderTestBuilder,
        stubs::TestDefault,
        types::{Price, Quantity},
    };

    #[rstest]
    fn test_new_and_display(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        let order3 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();

        let orders = vec![order1, order2, order3];

        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );

        assert!(order_list.to_string().starts_with(
            "OrderList(id=OL-001, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders="
        ));
    }

    #[rstest]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn test_order_list_creation_with_mismatched_instrument_id(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("EUR/USD.SIM"))
            .side(OrderSide::Sell)
            .price(Price::from("1.01000"))
            .quantity(Quantity::from(50_000))
            .build();

        let orders = vec![order1, order2];

        // This should panic because the instrument IDs do not match
        OrderList::new(
            OrderListId::from("OL-003"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );
    }

    #[rstest]
    #[should_panic(expected = "called `Result::unwrap()` on an `Err` value: the 'orders' slice")]
    fn test_order_list_creation_with_empty_orders(audusd_sim: CurrencyPair) {
        let orders: Vec<OrderAny> = vec![];

        // This should panic because the orders list is empty
        OrderList::new(
            OrderListId::from("OL-004"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );
    }

    #[rstest]
    fn test_order_list_equality(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();

        let orders = vec![order1];

        let order_list1 = OrderList::new(
            OrderListId::from("OL-006"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders.clone(),
            UnixNanos::default(),
        );

        let order_list2 = OrderList::new(
            OrderListId::from("OL-006"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );

        assert_eq!(order_list1, order_list2);
    }

    #[rstest]
    fn test_order_list_inequality(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();

        let orders = vec![order1];

        let order_list1 = OrderList::new(
            OrderListId::from("OL-007"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders.clone(),
            UnixNanos::default(),
        );

        let order_list2 = OrderList::new(
            OrderListId::from("OL-008"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );

        assert_ne!(order_list1, order_list2);
    }

    #[rstest]
    fn test_order_list_first(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Sell)
            .price(Price::from("1.01000"))
            .quantity(Quantity::from(50_000))
            .build();

        let first_order_id = order1.client_order_id();
        let orders = vec![order1, order2];

        let order_list = OrderList::new(
            OrderListId::from("OL-009"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );

        let first = order_list.first();
        assert!(first.is_some());
        assert_eq!(first.unwrap().client_order_id(), first_order_id);
    }

    #[rstest]
    fn test_order_list_len(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Sell)
            .price(Price::from("1.01000"))
            .quantity(Quantity::from(50_000))
            .build();
        let order3 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("0.99000"))
            .quantity(Quantity::from(75_000))
            .build();

        let orders = vec![order1, order2, order3];

        let order_list = OrderList::new(
            OrderListId::from("OL-010"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );

        assert_eq!(order_list.len(), 3);
        assert!(!order_list.is_empty());
    }

    #[rstest]
    fn test_order_list_hash(audusd_sim: CurrencyPair) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build();

        let orders = vec![order1];

        let order_list1 = OrderList::new(
            OrderListId::from("OL-011"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders.clone(),
            UnixNanos::default(),
        );

        let order_list2 = OrderList::new(
            OrderListId::from("OL-011"),
            audusd_sim.id,
            StrategyId::test_default(),
            orders,
            UnixNanos::default(),
        );

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        order_list1.hash(&mut hasher1);
        order_list2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }
}
