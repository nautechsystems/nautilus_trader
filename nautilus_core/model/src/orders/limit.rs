// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::rc::Rc;

use nautilus_core::{time::UnixNanos, uuid::UUID4};

use crate::{
    enums::{ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    events::order::OrderIdentifiers,
    identifiers::{
        client_order_id::ClientOrderId, instrument_id::InstrumentId, order_list_id::OrderListId,
        strategy_id::StrategyId, trader_id::TraderId,
    },
    types::{price::Price, quantity::Quantity},
};

use super::Order;

pub trait LimitOrder {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        tags: Option<String>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self;

    fn price(&self) -> &Price;
}

impl LimitOrder for Order {
    fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        tags: Option<String>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        let metadata = OrderIdentifiers {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
        };
        Self {
            events: Vec::new(),
            venue_order_ids: Vec::new(),
            trade_ids: Vec::new(),
            previous_status: None,
            triggered_price: None,
            status: OrderStatus::Initialized,
            ids: Rc::new(metadata),
            venue_order_id: None,
            position_id: None,
            account_id: None,
            last_trade_id: None,
            side: order_side,
            order_type: OrderType::Limit,
            quantity: quantity.clone(),
            price: Some(price),
            trigger_price: None,
            trigger_type: None,
            time_in_force,
            expire_time,
            liquidity_side: None,
            is_post_only: post_only,
            is_reduce_only: reduce_only,
            display_qty,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
            emulation_trigger,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            tags,
            filled_qty: Quantity::new(0.0, 0),
            leaves_qty: quantity,
            avg_px: None,
            slippage: None,
            init_id,
            ts_triggered: None,
            ts_init,
            ts_last: ts_init,
        }
    }

    fn price(&self) -> &Price {
        match &self.price {
            Some(price) => price,
            _ => panic!("Invalid `LimitOrder`: did not have a price"),
        }
    }
}
