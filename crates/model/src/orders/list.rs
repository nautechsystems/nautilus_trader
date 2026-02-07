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

use nautilus_core::{
    UnixNanos,
    correctness::{check_equal, check_slice_not_empty},
};
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{ClientOrderId, InstrumentId, OrderListId, StrategyId},
    orders::{Order, OrderAny},
};

/// Lightweight runtime state for a group of related orders.
///
/// Stores only the order IDs - full order data lives in the cache.
/// For serialization payload, see `SubmitOrderList.order_inits`.
#[derive(Clone, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderList {
    pub id: OrderListId,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub orders: Vec<ClientOrderId>,
    pub ts_init: UnixNanos,
}

impl OrderList {
    /// Creates a new [`OrderList`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `orders` is empty.
    #[must_use]
    pub fn new(
        order_list_id: OrderListId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        orders: Vec<ClientOrderId>,
        ts_init: UnixNanos,
    ) -> Self {
        check_slice_not_empty(orders.as_slice(), stringify!(orders)).unwrap();
        Self {
            id: order_list_id,
            instrument_id,
            strategy_id,
            orders,
            ts_init,
        }
    }

    /// Creates a new [`OrderList`] from a slice of orders.
    ///
    /// Derives `order_list_id`, `instrument_id`, and `strategy_id` from the first order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `orders` is empty
    /// - Any order has a different `order_list_id` than the first
    /// - Any order has a different `instrument_id` than the first
    /// - Any order has a different `strategy_id` than the first
    /// - Any order has `None` for `order_list_id`
    #[must_use]
    pub fn from_orders(orders: &[OrderAny], ts_init: UnixNanos) -> Self {
        check_slice_not_empty(orders, stringify!(orders)).unwrap();

        let first = &orders[0];
        let order_list_id = first
            .order_list_id()
            .expect("First order must have order_list_id");
        let instrument_id = first.instrument_id();
        let strategy_id = first.strategy_id();

        for order in orders.iter().skip(1) {
            let other_list_id = order
                .order_list_id()
                .expect("All orders must have order_list_id");
            check_equal(
                &other_list_id,
                &order_list_id,
                "order_list_id",
                "first order order_list_id",
            )
            .unwrap();
            check_equal(
                &order.instrument_id(),
                &instrument_id,
                "instrument_id",
                "first order instrument_id",
            )
            .unwrap();
            check_equal(
                &order.strategy_id(),
                &strategy_id,
                "strategy_id",
                "first order strategy_id",
            )
            .unwrap();
        }

        let client_order_ids = orders.iter().map(|o| o.client_order_id()).collect();

        Self {
            id: order_list_id,
            instrument_id,
            strategy_id,
            orders: client_order_ids,
            ts_init,
        }
    }

    #[must_use]
    pub fn first(&self) -> Option<&ClientOrderId> {
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
        enums::OrderType,
        identifiers::{InstrumentId, OrderListId},
        orders::builder::OrderTestBuilder,
        types::Quantity,
    };

    fn create_client_order_ids(count: usize) -> Vec<ClientOrderId> {
        (0..count)
            .map(|i| ClientOrderId::from(format!("O-00{}", i + 1).as_str()))
            .collect()
    }

    fn create_orders(count: usize, order_list_id: OrderListId) -> Vec<OrderAny> {
        (0..count)
            .map(|i| {
                OrderTestBuilder::new(OrderType::Market)
                    .instrument_id(InstrumentId::from("AUD/USD.SIM"))
                    .client_order_id(ClientOrderId::from(format!("O-00{}", i + 1).as_str()))
                    .order_list_id(order_list_id)
                    .quantity(Quantity::from(1))
                    .build()
            })
            .collect()
    }

    #[rstest]
    fn test_new_and_display() {
        let orders = create_client_order_ids(3);

        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );

        assert!(order_list.to_string().starts_with(
            "OrderList(id=OL-001, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders="
        ));
    }

    #[rstest]
    #[should_panic(expected = "called `Result::unwrap()` on an `Err` value: the 'orders'")]
    fn test_order_list_creation_with_empty_orders() {
        let orders: Vec<ClientOrderId> = vec![];

        let _ = OrderList::new(
            OrderListId::from("OL-004"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );
    }

    #[rstest]
    fn test_from_orders() {
        let order_list_id = OrderListId::from("OL-002");
        let orders = create_orders(3, order_list_id);

        let order_list = OrderList::from_orders(&orders, UnixNanos::default());

        assert_eq!(order_list.id, order_list_id);
        assert_eq!(order_list.len(), 3);
        assert_eq!(order_list.instrument_id, InstrumentId::from("AUD/USD.SIM"));
        assert_eq!(order_list.orders[0], ClientOrderId::from("O-001"));
    }

    #[rstest]
    fn test_order_list_equality() {
        let orders = create_client_order_ids(1);

        let order_list1 = OrderList::new(
            OrderListId::from("OL-006"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders.clone(),
            UnixNanos::default(),
        );

        let order_list2 = OrderList::new(
            OrderListId::from("OL-006"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );

        assert_eq!(order_list1, order_list2);
    }

    #[rstest]
    fn test_order_list_inequality() {
        let orders = create_client_order_ids(1);

        let order_list1 = OrderList::new(
            OrderListId::from("OL-007"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders.clone(),
            UnixNanos::default(),
        );

        let order_list2 = OrderList::new(
            OrderListId::from("OL-008"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );

        assert_ne!(order_list1, order_list2);
    }

    #[rstest]
    fn test_order_list_first() {
        let orders = create_client_order_ids(2);
        let first_id = orders[0];

        let order_list = OrderList::new(
            OrderListId::from("OL-009"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );

        let first = order_list.first();
        assert!(first.is_some());
        assert_eq!(*first.unwrap(), first_id);
    }

    #[rstest]
    fn test_order_list_len() {
        let orders = create_client_order_ids(3);

        let order_list = OrderList::new(
            OrderListId::from("OL-010"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );

        assert_eq!(order_list.len(), 3);
        assert!(!order_list.is_empty());
    }

    #[rstest]
    fn test_order_list_hash() {
        let orders = create_client_order_ids(1);

        let order_list1 = OrderList::new(
            OrderListId::from("OL-011"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders.clone(),
            UnixNanos::default(),
        );

        let order_list2 = OrderList::new(
            OrderListId::from("OL-011"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
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
