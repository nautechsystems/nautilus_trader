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

use std::fmt::Display;

use nautilus_core::{Params, UUID4, UnixNanos, correctness::check_equal};
use nautilus_model::{
    events::OrderInitialized,
    identifiers::{
        ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId, TraderId,
    },
    orders::{Order, OrderAny, OrderList},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct SubmitOrder {
    pub trader_id: TraderId,
    pub client_id: Option<ClientId>,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub order_init: OrderInitialized,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub position_id: Option<PositionId>,
    pub params: Option<Params>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl SubmitOrder {
    /// Creates a new [`SubmitOrder`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_init: OrderInitialized,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<Params>,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_init,
            exec_algorithm_id,
            position_id,
            params,
            command_id,
            ts_init,
        }
    }

    /// Creates a new [`SubmitOrder`] from an existing order.
    #[must_use]
    pub fn from_order(
        order: &OrderAny,
        trader_id: TraderId,
        client_id: Option<ClientId>,
        position_id: Option<PositionId>,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            order_init: OrderInitialized::from(order),
            exec_algorithm_id: order.exec_algorithm_id(),
            position_id,
            params: None,
            command_id,
            ts_init,
        }
    }
}

impl Display for SubmitOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SubmitOrder(instrument_id={}, client_order_id={}, position_id={})",
            self.instrument_id,
            self.client_order_id,
            self.position_id
                .map_or("None".to_string(), |position_id| format!("{position_id}")),
        )
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct SubmitOrderList {
    pub trader_id: TraderId,
    pub client_id: Option<ClientId>,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub order_list: OrderList,
    pub order_inits: Vec<OrderInitialized>,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub position_id: Option<PositionId>,
    pub params: Option<Params>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl SubmitOrderList {
    /// Creates a new [`SubmitOrderList`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `order_inits` length doesn't match `order_list.client_order_ids`, or if
    /// the client order IDs don't match in order.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        order_list: OrderList,
        order_inits: Vec<OrderInitialized>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<Params>,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        check_equal(
            &order_inits.len(),
            &order_list.client_order_ids.len(),
            "order_inits.len()",
            "order_list.client_order_ids.len()",
        )
        .unwrap();

        for (init, id) in order_inits.iter().zip(order_list.client_order_ids.iter()) {
            check_equal(
                &init.client_order_id,
                id,
                "order_init.client_order_id",
                "order_list.client_order_ids id",
            )
            .unwrap();
        }

        Self {
            trader_id,
            client_id,
            strategy_id,
            instrument_id: order_list.instrument_id,
            order_list,
            order_inits,
            exec_algorithm_id,
            position_id,
            params,
            command_id,
            ts_init,
        }
    }
}

impl Display for SubmitOrderList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SubmitOrderList(instrument_id={}, order_list={}, position_id={})",
            self.instrument_id,
            self.order_list.id,
            self.position_id
                .map_or("None".to_string(), |position_id| format!("{position_id}")),
        )
    }
}
