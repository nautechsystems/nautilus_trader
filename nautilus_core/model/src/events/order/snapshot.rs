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

use indexmap::IndexMap;
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{money::Money, price::Price, quantity::Quantity},
};

/// Represents an order state snapshot as a certain instant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderSnapshot {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub position_id: Option<PositionId>,
    pub account_id: AccountId,
    pub last_trade_id: Option<TradeId>,
    pub order_type: OrderType,
    pub order_side: OrderSide,
    pub quantity: Quantity,
    pub price: Option<Price>,
    pub trigger_price: Option<Price>,
    pub trigger_type: TriggerType,
    pub limit_offset: Option<Decimal>,
    pub trailing_offset: Option<Decimal>,
    pub trailing_offset_type: Option<TriggerType>,
    pub time_in_force: TimeInForce,
    pub expire_time_ns: Option<u64>,
    pub filled_qty: Quantity,
    // pub last_qty: Quantity,  // TODO: Implement
    // pub last_px: Price,  // TODO: Implement
    pub liquidity_side: LiquiditySide,
    pub avg_px: Option<f64>,
    pub slippage: Option<f64>,
    pub commissions: Vec<Money>,
    pub status: OrderStatus,
    pub is_post_only: bool,
    pub is_reduce_only: bool,
    pub is_quote_quantity: bool,
    pub display_qty: Option<Quantity>,
    pub emulation_trigger: TriggerType,
    pub trigger_instrument_id: Option<InstrumentId>,
    pub contingency_type: ContingencyType,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    pub parent_order_id: Option<ClientOrderId>,
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    pub exec_algorithm_params: Option<IndexMap<String, String>>,
    pub exec_spawn_id: Option<ClientOrderId>,
    pub tags: Option<Vec<Ustr>>,
    pub init_id: UUID4,
    pub ts_init: UnixNanos,
    pub ts_last: UnixNanos,
}
