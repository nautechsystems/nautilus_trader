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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    events::OrderInitialized,
    identifiers::{
        ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId, TraderId,
    },
    orders::OrderList,
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
    pub params: Option<IndexMap<String, String>>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl SubmitOrder {
    /// Creates a new [`SubmitOrder`] instance.
    #[allow(clippy::too_many_arguments)]
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
        params: Option<IndexMap<String, String>>,
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
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub position_id: Option<PositionId>,
    pub params: Option<IndexMap<String, String>>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl SubmitOrderList {
    /// Creates a new [`SubmitOrderList`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_list: OrderList,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<IndexMap<String, String>>,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            order_list,
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
            "SubmitOrderList(instrument_id={}, order_list=TBD, position_id={})",
            self.instrument_id,
            self.position_id
                .map_or("None".to_string(), |position_id| format!("{position_id}")),
        )
    }
}
