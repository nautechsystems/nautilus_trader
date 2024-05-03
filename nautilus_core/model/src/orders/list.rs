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

use std::fmt::Display;

use nautilus_core::{correctness::check_slice_not_empty, nanos::UnixNanos};
use serde::{Deserialize, Serialize};

use super::any::OrderAny;
use crate::{
    identifiers::{
        instrument_id::InstrumentId, order_list_id::OrderListId, strategy_id::StrategyId,
    },
    polymorphism::{GetInstrumentId, GetStrategyId},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    pub fn new(
        order_list_id: OrderListId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        orders: Vec<OrderAny>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_slice_not_empty(orders.as_slice(), stringify!(orders))?;
        for order in &orders {
            assert_eq!(instrument_id, order.instrument_id());
            assert_eq!(strategy_id, order.strategy_id());
        }

        Ok(Self {
            id: order_list_id,
            instrument_id,
            strategy_id,
            orders,
            ts_init,
        })
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
        enums::OrderSide,
        identifiers::{order_list_id::OrderListId, strategy_id::StrategyId},
        instruments::{currency_pair::CurrencyPair, stubs::*},
        orders::{any::OrderAny, stubs::TestOrderStubs},
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_new_and_display(audusd_sim: CurrencyPair) {
        let order1 = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );
        let order2 = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );
        let order3 = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );

        let orders = vec![
            OrderAny::Limit(order1),
            OrderAny::Limit(order2),
            OrderAny::Limit(order3),
        ];

        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            audusd_sim.id,
            StrategyId::from("EMACross-001"),
            orders,
            UnixNanos::default(),
        )
        .unwrap();

        assert!(order_list.to_string().starts_with(
            "OrderList(id=OL-001, instrument_id=AUD/USD.SIM, strategy_id=EMACross-001, orders="
        ));
    }
}
