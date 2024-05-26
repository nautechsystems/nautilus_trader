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
    enums::{OrderSide, OrderSideSpecified, OrderStatus, TriggerType},
    events::order::event::OrderEventAny,
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, exec_algorithm_id::ExecAlgorithmId,
        instrument_id::InstrumentId, position_id::PositionId, strategy_id::StrategyId,
        trader_id::TraderId, venue_order_id::VenueOrderId,
    },
    polymorphism::{
        ApplyOrderEventAny, GetAccountId, GetClientOrderId, GetEmulationTrigger,
        GetExecAlgorithmId, GetExecSpawnId, GetInstrumentId, GetLimitPrice, GetOrderFilledQty,
        GetOrderLeavesQty, GetOrderQuantity, GetOrderSide, GetOrderSideSpecified, GetOrderStatus,
        GetPositionId, GetStopPrice, GetStrategyId, GetTraderId, GetVenueOrderId, IsClosed,
        IsInflight, IsOpen,
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
        println!("from events");
        println!("events: {:?}", events);
        if events.is_empty() {
            anyhow::bail!("No events provided");
        }
        // pop the first event
        let init_event = events.first().unwrap();
        match init_event {
            OrderEventAny::Initialized(init) => {
                let mut order = Self::from(init.clone());
                // apply the rest of the events
                for event in events.into_iter().skip(1) {
                    // apply event to order
                    println!("applying event: {:?}", event);
                    order.apply(event).unwrap();
                }
                Ok(order)
            }
            _ => {
                anyhow::bail!("First event must be OrderInitialized");
            }
        }
    }
}

impl PartialEq for OrderAny {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id() == other.client_order_id()
    }
}

impl GetTraderId for OrderAny {
    fn trader_id(&self) -> TraderId {
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
}

impl GetStrategyId for OrderAny {
    fn strategy_id(&self) -> StrategyId {
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
}

impl GetInstrumentId for OrderAny {
    fn instrument_id(&self) -> InstrumentId {
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
}

impl GetAccountId for OrderAny {
    fn account_id(&self) -> Option<AccountId> {
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
}

impl GetClientOrderId for OrderAny {
    fn client_order_id(&self) -> ClientOrderId {
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
}

impl GetVenueOrderId for OrderAny {
    fn venue_order_id(&self) -> Option<VenueOrderId> {
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
}

impl GetPositionId for OrderAny {
    fn position_id(&self) -> Option<PositionId> {
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
}

impl GetExecAlgorithmId for OrderAny {
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
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
}

impl GetExecSpawnId for OrderAny {
    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
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
}

impl GetOrderSide for OrderAny {
    fn order_side(&self) -> OrderSide {
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
}

impl GetOrderQuantity for OrderAny {
    fn quantity(&self) -> Quantity {
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
}

impl GetOrderStatus for OrderAny {
    fn status(&self) -> OrderStatus {
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
}

impl GetOrderFilledQty for OrderAny {
    fn filled_qty(&self) -> Quantity {
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
}

impl GetOrderLeavesQty for OrderAny {
    fn leaves_qty(&self) -> Quantity {
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
}

impl GetOrderSideSpecified for OrderAny {
    fn order_side_specified(&self) -> OrderSideSpecified {
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
}

impl GetEmulationTrigger for OrderAny {
    fn emulation_trigger(&self) -> Option<TriggerType> {
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
}

impl IsOpen for OrderAny {
    fn is_open(&self) -> bool {
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
}

impl IsClosed for OrderAny {
    fn is_closed(&self) -> bool {
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
}

impl IsInflight for OrderAny {
    fn is_inflight(&self) -> bool {
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
}

impl ApplyOrderEventAny for OrderAny {
    fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        match self {
            Self::Limit(order) => order.apply(event),
            Self::LimitIfTouched(order) => order.apply(event),
            Self::Market(order) => order.apply(event),
            Self::MarketIfTouched(order) => order.apply(event),
            Self::MarketToLimit(order) => order.apply(event),
            Self::StopLimit(order) => order.apply(event),
            Self::StopMarket(order) => order.apply(event),
            Self::TrailingStopLimit(order) => order.apply(event),
            Self::TrailingStopMarket(order) => order.apply(event),
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

impl GetClientOrderId for PassiveOrderAny {
    fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Limit(order) => order.client_order_id(),
            Self::Stop(order) => order.client_order_id(),
        }
    }
}

impl GetOrderSideSpecified for PassiveOrderAny {
    fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::Limit(order) => order.order_side_specified(),
            Self::Stop(order) => order.order_side_specified(),
        }
    }
}

impl GetClientOrderId for LimitOrderAny {
    fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Limit(order) => order.client_order_id,
            Self::MarketToLimit(order) => order.client_order_id,
            Self::StopLimit(order) => order.client_order_id,
            Self::TrailingStopLimit(order) => order.client_order_id,
        }
    }
}

impl GetOrderSideSpecified for LimitOrderAny {
    fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::Limit(order) => order.side.as_specified(),
            Self::MarketToLimit(order) => order.side.as_specified(),
            Self::StopLimit(order) => order.side.as_specified(),
            Self::TrailingStopLimit(order) => order.side.as_specified(),
        }
    }
}

impl GetLimitPrice for LimitOrderAny {
    fn limit_px(&self) -> Price {
        match self {
            Self::Limit(order) => order.price,
            Self::MarketToLimit(order) => order.price.expect("No price for order"), // TBD
            Self::StopLimit(order) => order.price,
            Self::TrailingStopLimit(order) => order.price,
        }
    }
}

impl GetClientOrderId for StopOrderAny {
    fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::LimitIfTouched(order) => order.client_order_id,
            Self::MarketIfTouched(order) => order.client_order_id,
            Self::StopLimit(order) => order.client_order_id,
            Self::StopMarket(order) => order.client_order_id,
            Self::TrailingStopLimit(order) => order.client_order_id,
            Self::TrailingStopMarket(order) => order.client_order_id,
        }
    }
}

impl GetOrderSideSpecified for StopOrderAny {
    fn order_side_specified(&self) -> OrderSideSpecified {
        match self {
            Self::LimitIfTouched(order) => order.side.as_specified(),
            Self::MarketIfTouched(order) => order.side.as_specified(),
            Self::StopLimit(order) => order.side.as_specified(),
            Self::StopMarket(order) => order.side.as_specified(),
            Self::TrailingStopLimit(order) => order.side.as_specified(),
            Self::TrailingStopMarket(order) => order.side.as_specified(),
        }
    }
}

impl GetStopPrice for StopOrderAny {
    fn stop_px(&self) -> Price {
        match self {
            Self::LimitIfTouched(order) => order.trigger_price,
            Self::MarketIfTouched(order) => order.trigger_price,
            Self::StopLimit(order) => order.trigger_price,
            Self::StopMarket(order) => order.trigger_price,
            Self::TrailingStopLimit(order) => order.trigger_price,
            Self::TrailingStopMarket(order) => order.trigger_price,
        }
    }
}
