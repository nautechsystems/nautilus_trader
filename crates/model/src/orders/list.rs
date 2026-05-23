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
    collections::HashSet,
    fmt::Display,
    hash::{Hash, Hasher},
};

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{ClientOrderId, InstrumentId, OrderListId, StrategyId},
    orders::{Order, OrderAny},
};

/// Lightweight identifier container for a group of related orders.
///
/// Stores only the order IDs - full order data lives in the cache.
/// For serialization payload, see `SubmitOrderList.order_inits`.
#[derive(Clone, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct OrderList {
    pub id: OrderListId,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub client_order_ids: Vec<ClientOrderId>,
    pub ts_init: UnixNanos,
}

impl OrderList {
    /// Creates a new [`OrderList`] instance.
    ///
    /// Construction is infallible. [`OrderList::validate`] checks the
    /// syntactic invariants (non-empty, unique `client_order_ids`); the
    /// strategy submission path (`Strategy::submit_order_list`) runs it
    /// before the list reaches the cache.
    #[must_use]
    pub fn new(
        order_list_id: OrderListId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        client_order_ids: Vec<ClientOrderId>,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            id: order_list_id,
            instrument_id,
            strategy_id,
            client_order_ids,
            ts_init,
        }
    }

    /// Creates a new [`OrderList`] from a slice of orders.
    ///
    /// Derives `order_list_id`, `instrument_id`, and `strategy_id` from
    /// the first order without checking that subsequent orders share
    /// them; callers in the production path (`OrderFactory` plus a single
    /// strategy instance) produce orders with consistent identifiers.
    /// [`OrderList::validate`] checks the syntactic invariants (non-empty,
    /// unique `client_order_ids`); it does not check cross-field
    /// consistency.
    ///
    /// # Panics
    ///
    /// Panics if `orders` is empty or if the first order has no
    /// `order_list_id`. Callers are expected to guard non-empty input;
    /// `Strategy::submit_order_list` filters out the empty case before
    /// reaching this constructor.
    #[must_use]
    pub fn from_orders(orders: &[OrderAny], ts_init: UnixNanos) -> Self {
        let first = orders
            .first()
            .expect("OrderList::from_orders requires non-empty orders");
        let order_list_id = first
            .order_list_id()
            .expect("OrderList::from_orders requires first order to have order_list_id");
        let instrument_id = first.instrument_id();
        let strategy_id = first.strategy_id();
        let client_order_ids = orders.iter().map(|o| o.client_order_id()).collect();

        Self {
            id: order_list_id,
            instrument_id,
            strategy_id,
            client_order_ids,
            ts_init,
        }
    }

    /// Validates this [`OrderList`]'s own invariants.
    ///
    /// # Errors
    ///
    /// Returns an error if `client_order_ids` is empty or contains duplicates.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.client_order_ids.is_empty() {
            anyhow::bail!("OrderList {} has no orders", self.id);
        }

        let unique: HashSet<&ClientOrderId> = self.client_order_ids.iter().collect();
        if unique.len() != self.client_order_ids.len() {
            anyhow::bail!("OrderList {} contains duplicate client_order_ids", self.id);
        }

        Ok(())
    }

    #[must_use]
    pub fn first(&self) -> Option<&ClientOrderId> {
        self.client_order_ids.first()
    }

    /// Returns the number of orders in the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.client_order_ids.len()
    }

    /// Returns true if the list contains no orders.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.client_order_ids.is_empty()
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
            client_order_ids={:?}, \
            ts_init={}\
            )",
            self.id, self.instrument_id, self.strategy_id, self.client_order_ids, self.ts_init,
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
            "OrderList(id=OL-001, instrument_id=AUD/USD.SIM, strategy_id=S-001, client_order_ids="
        ));
    }

    #[rstest]
    fn test_from_orders() {
        let order_list_id = OrderListId::from("OL-002");
        let orders = create_orders(3, order_list_id);

        let order_list = OrderList::from_orders(&orders, UnixNanos::default());

        assert_eq!(order_list.id, order_list_id);
        assert_eq!(order_list.len(), 3);
        assert_eq!(order_list.instrument_id, InstrumentId::from("AUD/USD.SIM"));
        assert_eq!(order_list.client_order_ids[0], ClientOrderId::from("O-001"));
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

    #[rstest]
    fn test_validate_accepts_well_formed_list() {
        let orders = create_client_order_ids(3);
        let order_list = OrderList::new(
            OrderListId::from("OL-VALID-001"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            orders,
            UnixNanos::default(),
        );
        order_list
            .validate()
            .expect("well-formed list should validate");
    }

    #[rstest]
    fn test_validate_rejects_empty_list() {
        let order_list = OrderList::new(
            OrderListId::from("OL-EMPTY-001"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            Vec::new(),
            UnixNanos::default(),
        );
        let err = order_list.validate().expect_err("empty list should fail");
        assert!(
            err.to_string().contains("OL-EMPTY-001") && err.to_string().contains("no orders"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_validate_rejects_duplicate_client_order_ids() {
        let id = ClientOrderId::from("O-001");
        let order_list = OrderList::new(
            OrderListId::from("OL-DUP-001"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            vec![id, id],
            UnixNanos::default(),
        );
        let err = order_list
            .validate()
            .expect_err("duplicate client_order_ids should fail");
        assert!(
            err.to_string().contains("duplicate client_order_ids"),
            "unexpected error: {err}",
        );
    }
}
