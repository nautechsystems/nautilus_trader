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

use nautilus_core::nanos::UnixNanos;
use serde::{Deserialize, Serialize};

use super::{
    base::{Order, OrderError},
    limit::LimitOrder,
    limit_if_touched::LimitIfTouchedOrder,
    market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder,
    market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder,
    stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder,
    trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderSideSpecified, OrderStatus, OrderType,
        PositionSide, TriggerType,
    },
    events::order::OrderEventAny,
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId, TraderId,
        VenueOrderId,
    },
    types::{price::Price, quantity::Quantity},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OrderAny {
    Limit(LimitOrder),
    LimitIfTouched(LimitIfTouchedOrder),
    Market(MarketOrder),
    MarketIfTouched(MarketIfTouchedOrder),
    MarketToLimit(MarketToLimitOrder),
    StopLimit(StopLimitOrder),
    StopMarket(StopMarketOrder),
    TrailingStopLimit(TrailingStopLimitOrder),
    TrailingStopMarket(TrailingStopMarketOrder),
}

impl OrderAny {
    /// Applies the given `event` to the order.
    pub fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        match self {
            OrderAny::Limit(order) => order.apply(event),
            OrderAny::LimitIfTouched(order) => order.apply(event),
            OrderAny::Market(order) => order.apply(event),
            OrderAny::MarketIfTouched(order) => order.apply(event),
            OrderAny::MarketToLimit(order) => order.apply(event),
            OrderAny::StopLimit(order) => order.apply(event),
            OrderAny::StopMarket(order) => order.apply(event),
            OrderAny::TrailingStopLimit(order) => order.apply(event),
            OrderAny::TrailingStopMarket(order) => order.apply(event),
        }
    }

    #[must_use]
    pub fn from_limit(order: LimitOrder) -> Self {
        Self::Limit(order)
    }

    #[must_use]
    pub fn from_limit_if_touched(order: LimitIfTouchedOrder) -> Self {
        Self::LimitIfTouched(order)
    }

    #[must_use]
    pub fn from_market(order: MarketOrder) -> Self {
        Self::Market(order)
    }

    #[must_use]
    pub fn from_market_if_touched(order: MarketIfTouchedOrder) -> Self {
        Self::MarketIfTouched(order)
    }

    #[must_use]
    pub fn from_market_to_limit(order: MarketToLimitOrder) -> Self {
        Self::MarketToLimit(order)
    }

    #[must_use]
    pub fn from_stop_limit(order: StopLimitOrder) -> Self {
        Self::StopLimit(order)
    }

    #[must_use]
    pub fn from_stop_market(order: StopMarketOrder) -> Self {
        Self::StopMarket(order)
    }

    #[must_use]
    pub fn from_trailing_stop_limit(order: StopLimitOrder) -> Self {
        Self::StopLimit(order)
    }

    #[must_use]
    pub fn from_trailing_stop_market(order: StopMarketOrder) -> Self {
        Self::StopMarket(order)
    }

    pub fn from_events(events: Vec<OrderEventAny>) -> anyhow::Result<Self> {
        if events.is_empty() {
            anyhow::bail!("No order events provided to create OrderAny");
        }

        // Pop the first event
        let init_event = events.first().unwrap();
        match init_event {
            OrderEventAny::Initialized(init) => {
                let mut order = Self::from(init.clone());
                // Apply the rest of the events
                for event in events.into_iter().skip(1) {
                    // Apply event to order
                    println!("Applying event: {:?}", event);
                    order.apply(event).unwrap();
                }
                Ok(order)
            }
            _ => {
                anyhow::bail!("First event must be `OrderInitialized`");
            }
        }
    }

    #[must_use]
    pub fn last_event(&self) -> &OrderEventAny {
        match self {
            Self::Limit(order) => order.last_event(),
            Self::LimitIfTouched(order) => order.last_event(),
            Self::Market(order) => order.last_event(),
            Self::MarketIfTouched(order) => order.last_event(),
            Self::MarketToLimit(order) => order.last_event(),
            Self::StopLimit(order) => order.last_event(),
            Self::StopMarket(order) => order.last_event(),
            Self::TrailingStopLimit(order) => order.last_event(),
            Self::TrailingStopMarket(order) => order.last_event(),
        }
    }

    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        match self {
            Self::Limit(order) => order.trader_id,
            Self::LimitIfTouched(order) => order.trader_id,
            Self::Market(order) => order.trader_id,
            Self::MarketIfTouched(order) => order.trader_id,
            Self::MarketToLimit(order) => order.trader_id,
            Self::StopLimit(order) => order.trader_id,
            Self::StopMarket(order) => order.trader_id,
            Self::TrailingStopLimit(order) => order.trader_id,
            Self::TrailingStopMarket(order) => order.trader_id,
        }
    }

    #[must_use]
    pub fn strategy_id(&self) -> StrategyId {
        match self {
            Self::Limit(order) => order.strategy_id,
            Self::LimitIfTouched(order) => order.strategy_id,
            Self::Market(order) => order.strategy_id,
            Self::MarketIfTouched(order) => order.strategy_id,
            Self::MarketToLimit(order) => order.strategy_id,
            Self::StopLimit(order) => order.strategy_id,
            Self::StopMarket(order) => order.strategy_id,
            Self::TrailingStopLimit(order) => order.strategy_id,
            Self::TrailingStopMarket(order) => order.strategy_id,
        }
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Limit(order) => order.instrument_id,
            Self::LimitIfTouched(order) => order.instrument_id,
            Self::Market(order) => order.instrument_id,
            Self::MarketIfTouched(order) => order.instrument_id,
            Self::MarketToLimit(order) => order.instrument_id,
            Self::StopLimit(order) => order.instrument_id,
            Self::StopMarket(order) => order.instrument_id,
            Self::TrailingStopLimit(order) => order.instrument_id,
            Self::TrailingStopMarket(order) => order.instrument_id,
        }
    }

    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Limit(order) => order.client_order_id,
            Self::LimitIfTouched(order) => order.client_order_id,
            Self::Market(order) => order.client_order_id,
            Self::MarketIfTouched(order) => order.client_order_id,
            Self::MarketToLimit(order) => order.client_order_id,
            Self::StopLimit(order) => order.client_order_id,
            Self::StopMarket(order) => order.client_order_id,
            Self::TrailingStopLimit(order) => order.client_order_id,
            Self::TrailingStopMarket(order) => order.client_order_id,
        }
    }

    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        match self {
            Self::Limit(order) => order.account_id,
            Self::LimitIfTouched(order) => order.account_id,
            Self::Market(order) => order.account_id,
            Self::MarketIfTouched(order) => order.account_id,
            Self::MarketToLimit(order) => order.account_id,
            Self::StopLimit(order) => order.account_id,
            Self::StopMarket(order) => order.account_id,
            Self::TrailingStopLimit(order) => order.account_id,
            Self::TrailingStopMarket(order) => order.account_id,
        }
    }

    #[must_use]
    pub fn venue_order_id(&self) -> Option<VenueOrderId> {
        match self {
            Self::Limit(order) => order.venue_order_id,
            Self::LimitIfTouched(order) => order.venue_order_id,
            Self::Market(order) => order.venue_order_id,
            Self::MarketIfTouched(order) => order.venue_order_id,
            Self::MarketToLimit(order) => order.venue_order_id,
            Self::StopLimit(order) => order.venue_order_id,
            Self::StopMarket(order) => order.venue_order_id,
            Self::TrailingStopLimit(order) => order.venue_order_id,
            Self::TrailingStopMarket(order) => order.venue_order_id,
        }
    }

    #[must_use]
    pub fn position_id(&self) -> Option<PositionId> {
        match self {
            Self::Limit(order) => order.position_id,
            Self::LimitIfTouched(order) => order.position_id,
            Self::Market(order) => order.position_id,
            Self::MarketIfTouched(order) => order.position_id,
            Self::MarketToLimit(order) => order.position_id,
            Self::StopLimit(order) => order.position_id,
            Self::StopMarket(order) => order.position_id,
            Self::TrailingStopLimit(order) => order.position_id,
            Self::TrailingStopMarket(order) => order.position_id,
        }
    }

    #[must_use]
    pub fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        match self {
            Self::Limit(order) => order.exec_algorithm_id,
            Self::LimitIfTouched(order) => order.exec_algorithm_id,
            Self::Market(order) => order.exec_algorithm_id,
            Self::MarketIfTouched(order) => order.exec_algorithm_id,
            Self::MarketToLimit(order) => order.exec_algorithm_id,
            Self::StopLimit(order) => order.exec_algorithm_id,
            Self::StopMarket(order) => order.exec_algorithm_id,
            Self::TrailingStopLimit(order) => order.exec_algorithm_id,
            Self::TrailingStopMarket(order) => order.exec_algorithm_id,
        }
    }

    #[must_use]
    pub fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        match self {
            Self::Limit(order) => order.exec_spawn_id,
            Self::LimitIfTouched(order) => order.exec_spawn_id,
            Self::Market(order) => order.exec_spawn_id,
            Self::MarketIfTouched(order) => order.exec_spawn_id,
            Self::MarketToLimit(order) => order.exec_spawn_id,
            Self::StopLimit(order) => order.exec_spawn_id,
            Self::StopMarket(order) => order.exec_spawn_id,
            Self::TrailingStopLimit(order) => order.exec_spawn_id,
            Self::TrailingStopMarket(order) => order.exec_spawn_id,
        }
    }

    #[must_use]
    pub fn order_side(&self) -> OrderSide {
        match self {
            Self::Limit(order) => order.side,
            Self::LimitIfTouched(order) => order.side,
            Self::Market(order) => order.side,
            Self::MarketIfTouched(order) => order.side,
            Self::MarketToLimit(order) => order.side,
            Self::StopLimit(order) => order.side,
            Self::StopMarket(order) => order.side,
            Self::TrailingStopLimit(order) => order.side,
            Self::TrailingStopMarket(order) => order.side,
        }
    }

    #[must_use]
    pub fn order_type(&self) -> OrderType {
        match self {
            Self::Limit(order) => order.order_type,
            Self::LimitIfTouched(order) => order.order_type,
            Self::Market(order) => order.order_type,
            Self::MarketIfTouched(order) => order.order_type,
            Self::MarketToLimit(order) => order.order_type,
            Self::StopLimit(order) => order.order_type,
            Self::StopMarket(order) => order.order_type,
            Self::TrailingStopLimit(order) => order.order_type,
            Self::TrailingStopMarket(order) => order.order_type,
        }
    }

    #[must_use]
    pub fn quantity(&self) -> Quantity {
        match self {
            Self::Limit(order) => order.quantity,
            Self::LimitIfTouched(order) => order.quantity,
            Self::Market(order) => order.quantity,
            Self::MarketIfTouched(order) => order.quantity,
            Self::MarketToLimit(order) => order.quantity,
            Self::StopLimit(order) => order.quantity,
            Self::StopMarket(order) => order.quantity,
            Self::TrailingStopLimit(order) => order.quantity,
            Self::TrailingStopMarket(order) => order.quantity,
        }
    }

    #[must_use]
    pub fn status(&self) -> OrderStatus {
        match self {
            Self::Limit(order) => order.status,
            Self::LimitIfTouched(order) => order.status,
            Self::Market(order) => order.status,
            Self::MarketIfTouched(order) => order.status,
            Self::MarketToLimit(order) => order.status,
            Self::StopLimit(order) => order.status,
            Self::StopMarket(order) => order.status,
            Self::TrailingStopLimit(order) => order.status,
            Self::TrailingStopMarket(order) => order.status,
        }
    }

    #[must_use]
    pub fn filled_qty(&self) -> Quantity {
        match self {
            Self::Limit(order) => order.filled_qty(),
            Self::LimitIfTouched(order) => order.filled_qty(),
            Self::Market(order) => order.filled_qty(),
            Self::MarketIfTouched(order) => order.filled_qty(),
            Self::MarketToLimit(order) => order.filled_qty(),
            Self::StopLimit(order) => order.filled_qty(),
            Self::StopMarket(order) => order.filled_qty(),
            Self::TrailingStopLimit(order) => order.filled_qty(),
            Self::TrailingStopMarket(order) => order.filled_qty(),
        }
    }

    #[must_use]
    pub fn leaves_qty(&self) -> Quantity {
        match self {
            Self::Limit(order) => order.leaves_qty(),
            Self::LimitIfTouched(order) => order.leaves_qty(),
            Self::Market(order) => order.leaves_qty(),
            Self::MarketIfTouched(order) => order.leaves_qty(),
            Self::MarketToLimit(order) => order.leaves_qty(),
            Self::StopLimit(order) => order.leaves_qty(),
            Self::StopMarket(order) => order.leaves_qty(),
            Self::TrailingStopLimit(order) => order.leaves_qty(),
            Self::TrailingStopMarket(order) => order.leaves_qty(),
        }
    }

    #[must_use]
    pub fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::Limit(order) => order.side.as_specified(),
            Self::LimitIfTouched(order) => order.side.as_specified(),
            Self::Market(order) => order.side.as_specified(),
            Self::MarketIfTouched(order) => order.side.as_specified(),
            Self::MarketToLimit(order) => order.side.as_specified(),
            Self::StopLimit(order) => order.side.as_specified(),
            Self::StopMarket(order) => order.side.as_specified(),
            Self::TrailingStopLimit(order) => order.side.as_specified(),
            Self::TrailingStopMarket(order) => order.side.as_specified(),
        }
    }

    #[must_use]
    pub fn liquidity_side(&self) -> Option<LiquiditySide> {
        match self {
            Self::Limit(order) => order.liquidity_side,
            Self::LimitIfTouched(order) => order.liquidity_side,
            Self::Market(order) => order.liquidity_side,
            Self::MarketIfTouched(order) => order.liquidity_side,
            Self::MarketToLimit(order) => order.liquidity_side,
            Self::StopLimit(order) => order.liquidity_side,
            Self::StopMarket(order) => order.liquidity_side,
            Self::TrailingStopLimit(order) => order.liquidity_side,
            Self::TrailingStopMarket(order) => order.liquidity_side,
        }
    }

    #[must_use]
    pub fn emulation_trigger(&self) -> Option<TriggerType> {
        match self {
            Self::Limit(order) => order.emulation_trigger,
            Self::LimitIfTouched(order) => order.emulation_trigger,
            Self::Market(order) => order.emulation_trigger,
            Self::MarketIfTouched(order) => order.emulation_trigger,
            Self::MarketToLimit(order) => order.emulation_trigger,
            Self::StopLimit(order) => order.emulation_trigger,
            Self::StopMarket(order) => order.emulation_trigger,
            Self::TrailingStopLimit(order) => order.emulation_trigger,
            Self::TrailingStopMarket(order) => order.emulation_trigger,
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_open(),
            Self::LimitIfTouched(order) => order.is_open(),
            Self::Market(order) => order.is_open(),
            Self::MarketIfTouched(order) => order.is_open(),
            Self::MarketToLimit(order) => order.is_open(),
            Self::StopLimit(order) => order.is_open(),
            Self::StopMarket(order) => order.is_open(),
            Self::TrailingStopLimit(order) => order.is_open(),
            Self::TrailingStopMarket(order) => order.is_open(),
        }
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_closed(),
            Self::LimitIfTouched(order) => order.is_closed(),
            Self::Market(order) => order.is_closed(),
            Self::MarketIfTouched(order) => order.is_closed(),
            Self::MarketToLimit(order) => order.is_closed(),
            Self::StopLimit(order) => order.is_closed(),
            Self::StopMarket(order) => order.is_closed(),
            Self::TrailingStopLimit(order) => order.is_closed(),
            Self::TrailingStopMarket(order) => order.is_closed(),
        }
    }

    #[must_use]
    pub fn is_inflight(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_inflight(),
            Self::LimitIfTouched(order) => order.is_inflight(),
            Self::Market(order) => order.is_inflight(),
            Self::MarketIfTouched(order) => order.is_inflight(),
            Self::MarketToLimit(order) => order.is_inflight(),
            Self::StopLimit(order) => order.is_inflight(),
            Self::StopMarket(order) => order.is_inflight(),
            Self::TrailingStopLimit(order) => order.is_inflight(),
            Self::TrailingStopMarket(order) => order.is_inflight(),
        }
    }

    #[must_use]
    pub fn price(&self) -> Option<Price> {
        match self {
            Self::Limit(order) => Some(order.price),
            Self::LimitIfTouched(order) => Some(order.price),
            Self::Market(_) => None,
            Self::MarketIfTouched(_) => None,
            Self::MarketToLimit(order) => order.price,
            Self::StopLimit(order) => Some(order.price),
            Self::StopMarket(_) => None,
            Self::TrailingStopLimit(order) => Some(order.price),
            Self::TrailingStopMarket(_) => None,
        }
    }

    #[must_use]
    pub fn trigger_price(&self) -> Option<Price> {
        match self {
            Self::Limit(_) => None,
            Self::LimitIfTouched(order) => Some(order.trigger_price),
            Self::Market(_) => None,
            Self::MarketIfTouched(order) => Some(order.trigger_price),
            Self::MarketToLimit(_) => None,
            Self::StopLimit(order) => Some(order.trigger_price),
            Self::StopMarket(order) => Some(order.trigger_price),
            Self::TrailingStopLimit(order) => Some(order.trigger_price),
            Self::TrailingStopMarket(order) => Some(order.trigger_price),
        }
    }

    #[must_use]
    pub fn would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool {
        match self {
            Self::Limit(order) => order.would_reduce_only(side, position_qty),
            Self::Market(order) => order.would_reduce_only(side, position_qty),
            Self::MarketToLimit(order) => order.would_reduce_only(side, position_qty),
            Self::LimitIfTouched(order) => order.would_reduce_only(side, position_qty),
            Self::MarketIfTouched(order) => order.would_reduce_only(side, position_qty),
            Self::StopLimit(order) => order.would_reduce_only(side, position_qty),
            Self::StopMarket(order) => order.would_reduce_only(side, position_qty),
            Self::TrailingStopLimit(order) => order.would_reduce_only(side, position_qty),
            Self::TrailingStopMarket(order) => order.would_reduce_only(side, position_qty),
        }
    }

    #[must_use]
    pub fn is_reduce_only(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_reduce_only(),
            Self::Market(order) => order.is_reduce_only(),
            Self::MarketToLimit(order) => order.is_reduce_only(),
            Self::LimitIfTouched(order) => order.is_reduce_only(),
            Self::MarketIfTouched(order) => order.is_reduce_only(),
            Self::StopLimit(order) => order.is_reduce_only(),
            Self::StopMarket(order) => order.is_reduce_only(),
            Self::TrailingStopLimit(order) => order.is_reduce_only(),
            Self::TrailingStopMarket(order) => order.is_reduce_only(),
        }
    }

    #[must_use]
    pub fn is_buy(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_buy(),
            Self::LimitIfTouched(order) => order.is_buy(),
            Self::Market(order) => order.is_buy(),
            Self::MarketIfTouched(order) => order.is_buy(),
            Self::MarketToLimit(order) => order.is_buy(),
            Self::StopLimit(order) => order.is_buy(),
            Self::StopMarket(order) => order.is_buy(),
            Self::TrailingStopLimit(order) => order.is_buy(),
            Self::TrailingStopMarket(order) => order.is_buy(),
        }
    }

    #[must_use]
    pub fn is_sell(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_sell(),
            Self::LimitIfTouched(order) => order.is_sell(),
            Self::Market(order) => order.is_sell(),
            Self::MarketIfTouched(order) => order.is_sell(),
            Self::MarketToLimit(order) => order.is_sell(),
            Self::StopLimit(order) => order.is_sell(),
            Self::StopMarket(order) => order.is_sell(),
            Self::TrailingStopLimit(order) => order.is_sell(),
            Self::TrailingStopMarket(order) => order.is_sell(),
        }
    }

    #[must_use]
    pub fn parent_order_id(&self) -> Option<ClientOrderId> {
        match self {
            Self::Limit(order) => order.parent_order_id,
            Self::LimitIfTouched(order) => order.parent_order_id,
            Self::Market(order) => order.parent_order_id,
            Self::MarketIfTouched(order) => order.parent_order_id,
            Self::MarketToLimit(order) => order.parent_order_id,
            Self::StopLimit(order) => order.parent_order_id,
            Self::StopMarket(order) => order.parent_order_id,
            Self::TrailingStopLimit(order) => order.parent_order_id,
            Self::TrailingStopMarket(order) => order.parent_order_id,
        }
    }

    #[must_use]
    pub fn contingency_type(&self) -> Option<ContingencyType> {
        match self {
            Self::Limit(order) => order.contingency_type,
            Self::LimitIfTouched(order) => order.contingency_type,
            Self::Market(order) => order.contingency_type,
            Self::MarketIfTouched(order) => order.contingency_type,
            Self::MarketToLimit(order) => order.contingency_type,
            Self::StopLimit(order) => order.contingency_type,
            Self::StopMarket(order) => order.contingency_type,
            Self::TrailingStopLimit(order) => order.contingency_type,
            Self::TrailingStopMarket(order) => order.contingency_type,
        }
    }

    #[must_use]
    pub fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        match self {
            Self::Limit(order) => order.linked_order_ids.clone(),
            Self::LimitIfTouched(order) => order.linked_order_ids.clone(),
            Self::Market(order) => order.linked_order_ids.clone(),
            Self::MarketIfTouched(order) => order.linked_order_ids.clone(),
            Self::MarketToLimit(order) => order.linked_order_ids.clone(),
            Self::StopLimit(order) => order.linked_order_ids.clone(),
            Self::StopMarket(order) => order.linked_order_ids.clone(),
            Self::TrailingStopLimit(order) => order.linked_order_ids.clone(),
            Self::TrailingStopMarket(order) => order.linked_order_ids.clone(),
        }
    }
}

impl PartialEq for OrderAny {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id() == other.client_order_id()
    }
}

impl Display for OrderAny {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Limit(order) => order.to_string(),
                Self::LimitIfTouched(order) => format!("{:?}", order), // TODO: Implement
                Self::Market(order) => order.to_string(),
                Self::MarketIfTouched(order) => format!("{:?}", order), // TODO: Implement
                Self::MarketToLimit(order) => format!("{:?}", order),   // TODO: Implement
                Self::StopLimit(order) => order.to_string(),
                Self::StopMarket(order) => format!("{:?}", order), // TODO: Implement
                Self::TrailingStopLimit(order) => format!("{:?}", order), // TODO: Implement
                Self::TrailingStopMarket(order) => format!("{:?}", order), // TODO: Implement
            }
        )
    }
}

impl From<OrderAny> for PassiveOrderAny {
    fn from(order: OrderAny) -> PassiveOrderAny {
        match order {
            OrderAny::Limit(_) => PassiveOrderAny::Limit(order.into()),
            OrderAny::LimitIfTouched(_) => PassiveOrderAny::Stop(order.into()),
            OrderAny::MarketIfTouched(_) => PassiveOrderAny::Stop(order.into()),
            OrderAny::StopLimit(_) => PassiveOrderAny::Stop(order.into()),
            OrderAny::StopMarket(_) => PassiveOrderAny::Stop(order.into()),
            OrderAny::TrailingStopLimit(_) => PassiveOrderAny::Stop(order.into()),
            OrderAny::TrailingStopMarket(_) => PassiveOrderAny::Stop(order.into()),
            _ => panic!("WIP: Implement trait bound to require `HasPrice`"),
        }
    }
}

impl From<StopOrderAny> for PassiveOrderAny {
    fn from(order: StopOrderAny) -> PassiveOrderAny {
        match order {
            StopOrderAny::LimitIfTouched(_) => PassiveOrderAny::Stop(order),
            StopOrderAny::MarketIfTouched(_) => PassiveOrderAny::Stop(order),
            StopOrderAny::StopLimit(_) => PassiveOrderAny::Stop(order),
            StopOrderAny::StopMarket(_) => PassiveOrderAny::Stop(order),
            StopOrderAny::TrailingStopLimit(_) => PassiveOrderAny::Stop(order),
            StopOrderAny::TrailingStopMarket(_) => PassiveOrderAny::Stop(order),
        }
    }
}

impl From<LimitOrderAny> for PassiveOrderAny {
    fn from(order: LimitOrderAny) -> PassiveOrderAny {
        match order {
            LimitOrderAny::Limit(_) => PassiveOrderAny::Limit(order),
            LimitOrderAny::MarketToLimit(_) => PassiveOrderAny::Limit(order),
            LimitOrderAny::StopLimit(_) => PassiveOrderAny::Limit(order),
            LimitOrderAny::TrailingStopLimit(_) => PassiveOrderAny::Limit(order),
        }
    }
}

impl From<OrderAny> for StopOrderAny {
    fn from(order: OrderAny) -> StopOrderAny {
        match order {
            OrderAny::LimitIfTouched(order) => StopOrderAny::LimitIfTouched(order),
            OrderAny::MarketIfTouched(order) => StopOrderAny::MarketIfTouched(order),
            OrderAny::StopLimit(order) => StopOrderAny::StopLimit(order),
            OrderAny::StopMarket(order) => StopOrderAny::StopMarket(order),
            OrderAny::TrailingStopLimit(order) => StopOrderAny::TrailingStopLimit(order),
            OrderAny::TrailingStopMarket(order) => StopOrderAny::TrailingStopMarket(order),
            _ => panic!("WIP: Implement trait bound to require `HasStopPrice`"),
        }
    }
}

impl From<OrderAny> for LimitOrderAny {
    fn from(order: OrderAny) -> LimitOrderAny {
        match order {
            OrderAny::Limit(order) => LimitOrderAny::Limit(order),
            OrderAny::MarketToLimit(order) => LimitOrderAny::MarketToLimit(order),
            _ => panic!("WIP: Implement trait bound to require `HasLimitPrice`"),
        }
    }
}

impl AsRef<StopMarketOrder> for OrderAny {
    fn as_ref(&self) -> &StopMarketOrder {
        match self {
            OrderAny::StopMarket(ref order) => order,
            _ => panic!(
                "Invalid `OrderAny` not `{}`, was {self:?}",
                stringify!(StopMarketOrder),
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PassiveOrderAny {
    Limit(LimitOrderAny),
    Stop(StopOrderAny),
}

impl PassiveOrderAny {
    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Limit(order) => order.client_order_id(),
            Self::Stop(order) => order.client_order_id(),
        }
    }

    #[must_use]
    pub fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::Limit(order) => order.order_side_specified(),
            Self::Stop(order) => order.order_side_specified(),
        }
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_closed(),
            Self::Stop(order) => order.is_closed(),
        }
    }

    #[must_use]
    pub fn expire_time(&self) -> Option<UnixNanos> {
        match self {
            Self::Limit(order) => order.expire_time(),
            Self::Stop(order) => order.expire_time(),
        }
    }
}

impl PartialEq for PassiveOrderAny {
    fn eq(&self, rhs: &Self) -> bool {
        match self {
            Self::Limit(order) => order.client_order_id() == rhs.client_order_id(),
            Self::Stop(order) => order.client_order_id() == rhs.client_order_id(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum LimitOrderAny {
    Limit(LimitOrder),
    MarketToLimit(MarketToLimitOrder),
    StopLimit(StopLimitOrder),
    TrailingStopLimit(TrailingStopLimitOrder),
}

impl LimitOrderAny {
    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Limit(order) => order.client_order_id,
            Self::MarketToLimit(order) => order.client_order_id,
            Self::StopLimit(order) => order.client_order_id,
            Self::TrailingStopLimit(order) => order.client_order_id,
        }
    }

    #[must_use]
    pub fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::Limit(order) => order.side.as_specified(),
            Self::MarketToLimit(order) => order.side.as_specified(),
            Self::StopLimit(order) => order.side.as_specified(),
            Self::TrailingStopLimit(order) => order.side.as_specified(),
        }
    }

    #[must_use]
    pub fn limit_px(&self) -> Price {
        match self {
            Self::Limit(order) => order.price,
            Self::MarketToLimit(order) => order.price.expect("No price for order"), // TBD
            Self::StopLimit(order) => order.price,
            Self::TrailingStopLimit(order) => order.price,
        }
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_closed(),
            Self::MarketToLimit(order) => order.is_closed(),
            Self::StopLimit(order) => order.is_closed(),
            Self::TrailingStopLimit(order) => order.is_closed(),
        }
    }

    #[must_use]
    pub fn expire_time(&self) -> Option<UnixNanos> {
        match self {
            Self::Limit(order) => order.expire_time,
            Self::MarketToLimit(order) => order.expire_time,
            Self::StopLimit(order) => order.expire_time,
            Self::TrailingStopLimit(order) => order.expire_time,
        }
    }
}

impl PartialEq for LimitOrderAny {
    fn eq(&self, rhs: &Self) -> bool {
        match self {
            Self::Limit(order) => order.client_order_id == rhs.client_order_id(),
            Self::MarketToLimit(order) => order.client_order_id == rhs.client_order_id(),
            Self::StopLimit(order) => order.client_order_id == rhs.client_order_id(),
            Self::TrailingStopLimit(order) => order.client_order_id == rhs.client_order_id(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum StopOrderAny {
    LimitIfTouched(LimitIfTouchedOrder),
    MarketIfTouched(MarketIfTouchedOrder),
    StopLimit(StopLimitOrder),
    StopMarket(StopMarketOrder),
    TrailingStopLimit(TrailingStopLimitOrder),
    TrailingStopMarket(TrailingStopMarketOrder),
}

impl StopOrderAny {
    #[must_use]
    pub fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::LimitIfTouched(order) => order.client_order_id,
            Self::MarketIfTouched(order) => order.client_order_id,
            Self::StopLimit(order) => order.client_order_id,
            Self::StopMarket(order) => order.client_order_id,
            Self::TrailingStopLimit(order) => order.client_order_id,
            Self::TrailingStopMarket(order) => order.client_order_id,
        }
    }

    #[must_use]
    pub fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::LimitIfTouched(order) => order.side.as_specified(),
            Self::MarketIfTouched(order) => order.side.as_specified(),
            Self::StopLimit(order) => order.side.as_specified(),
            Self::StopMarket(order) => order.side.as_specified(),
            Self::TrailingStopLimit(order) => order.side.as_specified(),
            Self::TrailingStopMarket(order) => order.side.as_specified(),
        }
    }

    #[must_use]
    pub fn stop_px(&self) -> Price {
        match self {
            Self::LimitIfTouched(order) => order.trigger_price,
            Self::MarketIfTouched(order) => order.trigger_price,
            Self::StopLimit(order) => order.trigger_price,
            Self::StopMarket(order) => order.trigger_price,
            Self::TrailingStopLimit(order) => order.trigger_price,
            Self::TrailingStopMarket(order) => order.trigger_price,
        }
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self {
            Self::LimitIfTouched(order) => order.is_closed(),
            Self::MarketIfTouched(order) => order.is_closed(),
            Self::StopLimit(order) => order.is_closed(),
            Self::StopMarket(order) => order.is_closed(),
            Self::TrailingStopLimit(order) => order.is_closed(),
            Self::TrailingStopMarket(order) => order.is_closed(),
        }
    }

    #[must_use]
    pub fn expire_time(&self) -> Option<UnixNanos> {
        match self {
            Self::LimitIfTouched(order) => order.expire_time,
            Self::MarketIfTouched(order) => order.expire_time,
            Self::StopLimit(order) => order.expire_time,
            Self::StopMarket(order) => order.expire_time,
            Self::TrailingStopLimit(order) => order.expire_time,
            Self::TrailingStopMarket(order) => order.expire_time,
        }
    }
}

impl PartialEq for StopOrderAny {
    fn eq(&self, rhs: &Self) -> bool {
        match self {
            Self::LimitIfTouched(order) => order.client_order_id == rhs.client_order_id(),
            Self::StopLimit(order) => order.client_order_id == rhs.client_order_id(),
            Self::StopMarket(order) => order.client_order_id == rhs.client_order_id(),
            Self::MarketIfTouched(order) => order.client_order_id == rhs.client_order_id(),
            Self::TrailingStopLimit(order) => order.client_order_id == rhs.client_order_id(),
            Self::TrailingStopMarket(order) => order.client_order_id == rhs.client_order_id(),
        }
    }
}
