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

use std::fmt::Display;

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
}

impl PartialEq for OrderList {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::{OrderSide, OrderType},
        identifiers::{OrderListId, StrategyId},
        instruments::{CurrencyPair, stubs::*},
        orders::OrderTestBuilder,
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
            StrategyId::default(),
            orders,
            UnixNanos::default(),
        );

        assert!(order_list.to_string().starts_with(
            "OrderList(id=OL-001, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders="
        ));
    }
}
