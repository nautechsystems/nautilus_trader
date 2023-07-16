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

use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use nautilus_core::{time::UnixNanos, uuid::UUID4};

use super::base::{Order, OrderCore};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::order::{OrderEvent, OrderInitialized},
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, order_list_id::OrderListId, position_id::PositionId,
        strategy_id::StrategyId, trade_id::TradeId, trader_id::TraderId,
        venue_order_id::VenueOrderId,
    },
    types::{price::Price, quantity::Quantity},
};

pub struct TrailingStopMarketOrder {
    core: OrderCore,
    pub trigger_price: Price,
    pub trigger_type: TriggerType,
    pub trailing_offset: Price,
    pub trailing_offset_type: TrailingOffsetType,
    pub expire_time: Option<UnixNanos>,
    pub display_qty: Option<Quantity>,
    pub is_triggered: bool,
    pub ts_triggered: Option<UnixNanos>,
}

impl TrailingStopMarketOrder {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType,
        trailing_offset: Price,
        trailing_offset_type: TrailingOffsetType,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        reduce_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<HashMap<String, String>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<String>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            core: OrderCore::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                order_side,
                OrderType::TrailingStopMarket,
                quantity,
                time_in_force,
                false,
                reduce_only,
                quote_quantity,
                emulation_trigger,
                contingency_type,
                order_list_id,
                linked_order_ids,
                parent_order_id,
                exec_algorithm_id,
                exec_algorithm_params,
                exec_spawn_id,
                tags,
                init_id,
                ts_init,
            ),
            trigger_price,
            trigger_type,
            trailing_offset,
            trailing_offset_type,
            expire_time,
            display_qty,
            is_triggered: false,
            ts_triggered: None,
        }
    }
}

/// Provides a default [`TrailingStopMarketOrder`] used for testing.
impl Default for TrailingStopMarketOrder {
    fn default() -> Self {
        TrailingStopMarketOrder::new(
            TraderId::default(),
            StrategyId::default(),
            InstrumentId::default(),
            ClientOrderId::default(),
            OrderSide::Buy,
            Quantity::new(100_000.0, 0),
            Price::new(1.0, 5),
            TriggerType::BidAsk,
            Price::new(0.001, 5),
            TrailingOffsetType::Price,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            0,
        )
    }
}

impl Deref for TrailingStopMarketOrder {
    type Target = OrderCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for TrailingStopMarketOrder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Order for TrailingStopMarketOrder {
    fn status(&self) -> OrderStatus {
        self.status
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id.clone()
    }

    fn strategy_id(&self) -> StrategyId {
        self.strategy_id.clone()
    }

    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id.clone()
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id.clone()
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id.clone()
    }

    fn position_id(&self) -> Option<PositionId> {
        self.position_id.clone()
    }

    fn account_id(&self) -> Option<AccountId> {
        self.account_id.clone()
    }

    fn last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id.clone()
    }

    fn side(&self) -> OrderSide {
        self.side
    }

    fn order_type(&self) -> OrderType {
        self.order_type
    }

    fn quantity(&self) -> Quantity {
        self.quantity
    }

    fn time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    fn price(&self) -> Option<Price> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        Some(self.trigger_price)
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        Some(self.trigger_type)
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        self.liquidity_side
    }

    fn is_post_only(&self) -> bool {
        self.is_post_only
    }

    fn is_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    fn is_quote_quantity(&self) -> bool {
        self.is_quote_quantity
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id.clone()
    }

    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        self.linked_order_ids.clone()
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id.clone()
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id.clone()
    }

    fn exec_algorithm_params(&self) -> Option<HashMap<String, String>> {
        self.exec_algorithm_params.clone()
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id.clone()
    }

    fn tags(&self) -> Option<String> {
        self.tags.clone()
    }

    fn filled_qty(&self) -> Quantity {
        self.filled_qty
    }

    fn leaves_qty(&self) -> Quantity {
        self.leaves_qty
    }

    fn avg_px(&self) -> Option<f64> {
        self.avg_px
    }

    fn slippage(&self) -> Option<f64> {
        self.slippage
    }

    fn init_id(&self) -> UUID4 {
        self.init_id
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn ts_last(&self) -> UnixNanos {
        self.ts_last
    }

    fn events(&self) -> Vec<&OrderEvent> {
        self.events.iter().collect()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId> {
        self.venue_order_ids.iter().collect()
    }

    fn trade_ids(&self) -> Vec<&TradeId> {
        self.trade_ids.iter().collect()
    }
}

impl From<OrderInitialized> for TrailingStopMarketOrder {
    fn from(event: OrderInitialized) -> Self {
        TrailingStopMarketOrder::new(
            event.trader_id,
            event.strategy_id,
            event.instrument_id,
            event.client_order_id,
            event.order_side,
            event.quantity,
            event
                .trigger_price // TODO: Improve this error, model order domain errors
                .expect(
                    "Error initializing order: `trigger_price` was `None` for `TrailingStopMarketOrder`",
                ),
            event
                .trigger_type
                .expect("Error initializing order: `trigger_type` was `None` for `TrailingStopMarketOrder`"),
            event.trailing_offset.unwrap(),  // TODO
            event.trailing_offset_type.unwrap(),  // TODO
            event.time_in_force,
            event.expire_time,
            event.reduce_only,
            event.quote_quantity,
            event.display_qty,
            event.emulation_trigger,
            event.contingency_type,
            event.order_list_id,
            event.linked_order_ids,
            event.parent_order_id,
            event.exec_algorithm_id,
            event.exec_algorithm_params,
            event.exec_spawn_id,
            event.tags,
            event.event_id,
            event.ts_event,
        )
    }
}

impl From<&TrailingStopMarketOrder> for OrderInitialized {
    fn from(order: &TrailingStopMarketOrder) -> Self {
        Self {
            trader_id: order.trader_id.clone(),
            strategy_id: order.strategy_id.clone(),
            instrument_id: order.instrument_id.clone(),
            client_order_id: order.client_order_id.clone(),
            order_side: order.side,
            order_type: order.order_type,
            quantity: order.quantity,
            price: None,
            trigger_price: Some(order.trigger_price),
            trigger_type: Some(order.trigger_type),
            time_in_force: order.time_in_force,
            expire_time: order.expire_time,
            post_only: order.is_post_only,
            reduce_only: order.is_reduce_only,
            quote_quantity: order.is_quote_quantity,
            display_qty: order.display_qty,
            limit_offset: None,
            trailing_offset: Some(order.trailing_offset),
            trailing_offset_type: Some(order.trailing_offset_type),
            emulation_trigger: order.emulation_trigger,
            contingency_type: order.contingency_type,
            order_list_id: order.order_list_id.clone(),
            linked_order_ids: order.linked_order_ids.clone(),
            parent_order_id: order.parent_order_id.clone(),
            exec_algorithm_id: order.exec_algorithm_id.clone(),
            exec_algorithm_params: order.exec_algorithm_params.clone(),
            exec_spawn_id: order.exec_spawn_id.clone(),
            tags: order.tags.clone(),
            event_id: order.init_id,
            ts_event: order.ts_init,
            ts_init: order.ts_init,
            reconciliation: false,
        }
    }
}
