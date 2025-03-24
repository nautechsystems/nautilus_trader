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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    orders::{Order, OrderAny},
    types::{Money, Price, Quantity},
};

/// Represents an order state snapshot as a certain instant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderSnapshot {
    /// The trader ID associated with the order.
    pub trader_id: TraderId,
    /// The strategy ID associated with the order.
    pub strategy_id: StrategyId,
    /// The order instrument ID.
    pub instrument_id: InstrumentId,
    /// The client order ID.
    pub client_order_id: ClientOrderId,
    /// The venue assigned order ID.
    pub venue_order_id: Option<VenueOrderId>,
    /// The position ID associated with the order.
    pub position_id: Option<PositionId>,
    /// The account ID associated with the order.
    pub account_id: Option<AccountId>,
    /// The orders last trade match ID.
    pub last_trade_id: Option<TradeId>,
    /// The order type.
    pub order_type: OrderType,
    /// The order side.
    pub order_side: OrderSide,
    /// The order quantity.
    pub quantity: Quantity,
    /// The order price (LIMIT).
    pub price: Option<Price>,
    /// The order trigger price (STOP).
    pub trigger_price: Option<Price>,
    /// The trigger type for the order.
    pub trigger_type: Option<TriggerType>,
    /// The trailing offset for the orders limit price.
    pub limit_offset: Option<Decimal>,
    /// The trailing offset for the orders trigger price (STOP).
    pub trailing_offset: Option<Decimal>,
    /// The trailing offset type.
    pub trailing_offset_type: Option<TrailingOffsetType>,
    /// The order time in force.
    pub time_in_force: TimeInForce,
    /// The order expiration (UNIX epoch nanoseconds), zero for no expiration.
    pub expire_time: Option<UnixNanos>,
    /// The order total filled quantity.
    pub filled_qty: Quantity,
    /// The order liquidity side.
    pub liquidity_side: Option<LiquiditySide>,
    /// The order average fill price.
    pub avg_px: Option<f64>,
    /// The order total price slippage.
    pub slippage: Option<f64>,
    /// The commissions for the order.
    pub commissions: Vec<Money>,
    /// The order status.
    pub status: OrderStatus,
    /// If the order will only provide liquidity (make a market).
    pub is_post_only: bool,
    /// If the order carries the 'reduce-only' execution instruction.
    pub is_reduce_only: bool,
    /// If the order quantity is denominated in the quote currency.
    pub is_quote_quantity: bool,
    /// The quantity of the `LIMIT` order to display on the public book (iceberg).
    pub display_qty: Option<Quantity>,
    /// The order emulation trigger type.
    pub emulation_trigger: Option<TriggerType>,
    /// The order emulation trigger instrument ID (will be `instrument_id` if `None`).
    pub trigger_instrument_id: Option<InstrumentId>,
    /// The orders contingency type.
    pub contingency_type: Option<ContingencyType>,
    /// The order list ID associated with the order.
    pub order_list_id: Option<OrderListId>,
    /// The orders linked client order ID(s).
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    /// The parent client order ID.
    pub parent_order_id: Option<ClientOrderId>,
    /// The execution algorithm ID for the order.
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    /// The execution algorithm parameters for the order.
    pub exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
    /// The execution algorithm spawning client order ID.
    pub exec_spawn_id: Option<ClientOrderId>,
    /// The order custom user tags.
    pub tags: Option<Vec<Ustr>>,
    /// The event ID of the `OrderInitialized` event.
    pub init_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the last event occurred.
    pub ts_last: UnixNanos,
}

impl From<OrderAny> for OrderSnapshot {
    fn from(order: OrderAny) -> Self {
        Self {
            trader_id: order.trader_id(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            venue_order_id: order.venue_order_id(),
            position_id: order.position_id(),
            account_id: order.account_id(),
            last_trade_id: order.last_trade_id(),
            order_type: order.order_type(),
            order_side: order.order_side(),
            quantity: order.quantity(),
            price: order.price(),
            trigger_price: order.trigger_price(),
            trigger_type: order.trigger_type(),
            limit_offset: order.limit_offset(),
            trailing_offset: order.trailing_offset(),
            trailing_offset_type: order.trailing_offset_type(),
            time_in_force: order.time_in_force(),
            expire_time: order.expire_time(),
            filled_qty: order.filled_qty(),
            liquidity_side: order.liquidity_side(),
            avg_px: order.avg_px(),
            slippage: order.slippage(),
            commissions: order.commissions().values().cloned().collect(),
            status: order.status(),
            is_post_only: order.is_post_only(),
            is_reduce_only: order.is_reduce_only(),
            is_quote_quantity: order.is_quote_quantity(),
            display_qty: order.display_qty(),
            emulation_trigger: order.emulation_trigger(),
            trigger_instrument_id: order.trigger_instrument_id(),
            contingency_type: order.contingency_type(),
            order_list_id: order.order_list_id(),
            linked_order_ids: order.linked_order_ids().map(Vec::from),
            parent_order_id: order.parent_order_id(),
            exec_algorithm_id: order.exec_algorithm_id(),
            exec_algorithm_params: order.exec_algorithm_params().cloned(),
            exec_spawn_id: order.exec_spawn_id(),
            tags: order.tags().map(Vec::from),
            init_id: order.init_id(),
            ts_init: order.ts_init(),
            ts_last: order.ts_last(),
        }
    }
}
