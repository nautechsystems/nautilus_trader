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

use enum_dispatch::enum_dispatch;
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use super::{
    base::Order, limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder, market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder, market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder, stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder, trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{
    enums::{ContingencyType, OrderSideSpecified},
    events::OrderEventAny,
    identifiers::ClientOrderId,
    types::Price,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[enum_dispatch]
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
}

impl PartialEq for OrderAny {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id() == other.client_order_id()
    }
}

// TODO: fix equality
impl Eq for OrderAny {}

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
            OrderAny::MarketToLimit(_) => PassiveOrderAny::Limit(order.into()),
            _ => panic!("WIP: Implement trait bound to require `HasPrice`"),
        }
    }
}

impl From<PassiveOrderAny> for OrderAny {
    fn from(order: PassiveOrderAny) -> OrderAny {
        match order {
            PassiveOrderAny::Limit(order) => order.into(),
            PassiveOrderAny::Stop(order) => order.into(),
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

impl From<StopOrderAny> for OrderAny {
    fn from(order: StopOrderAny) -> OrderAny {
        match order {
            StopOrderAny::LimitIfTouched(order) => OrderAny::LimitIfTouched(order),
            StopOrderAny::MarketIfTouched(order) => OrderAny::MarketIfTouched(order),
            StopOrderAny::StopLimit(order) => OrderAny::StopLimit(order),
            StopOrderAny::StopMarket(order) => OrderAny::StopMarket(order),
            StopOrderAny::TrailingStopLimit(order) => OrderAny::TrailingStopLimit(order),
            StopOrderAny::TrailingStopMarket(order) => OrderAny::TrailingStopMarket(order),
        }
    }
}

impl From<OrderAny> for LimitOrderAny {
    fn from(order: OrderAny) -> LimitOrderAny {
        match order {
            OrderAny::Limit(order) => LimitOrderAny::Limit(order),
            OrderAny::MarketToLimit(order) => LimitOrderAny::MarketToLimit(order),
            OrderAny::StopLimit(order) => LimitOrderAny::StopLimit(order),
            OrderAny::TrailingStopLimit(order) => LimitOrderAny::TrailingStopLimit(order),
            _ => panic!("WIP: Implement trait bound to require `HasLimitPrice`"),
        }
    }
}

impl From<LimitOrderAny> for OrderAny {
    fn from(order: LimitOrderAny) -> OrderAny {
        match order {
            LimitOrderAny::Limit(order) => OrderAny::Limit(order),
            LimitOrderAny::MarketToLimit(order) => OrderAny::MarketToLimit(order),
            LimitOrderAny::StopLimit(order) => OrderAny::StopLimit(order),
            LimitOrderAny::TrailingStopLimit(order) => OrderAny::TrailingStopLimit(order),
        }
    }
}

impl AsRef<StopMarketOrder> for OrderAny {
    fn as_ref(&self) -> &StopMarketOrder {
        match self {
            OrderAny::StopMarket(order) => order,
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
    pub fn is_open(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_open(),
            Self::Stop(order) => order.is_open(),
        }
    }

    #[must_use]
    pub fn is_inflight(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_inflight(),
            Self::Stop(order) => order.is_inflight(),
        }
    }

    #[must_use]
    pub fn expire_time(&self) -> Option<UnixNanos> {
        match self {
            Self::Limit(order) => order.expire_time(),
            Self::Stop(order) => order.expire_time(),
        }
    }

    #[must_use]
    pub fn contingency_type(&self) -> Option<ContingencyType> {
        match self {
            Self::Limit(order) => order.contingency_type(),
            Self::Stop(order) => order.contingency_type(),
        }
    }

    #[must_use]
    pub fn to_any(&self) -> OrderAny {
        match self {
            Self::Limit(order) => order.clone().into(),
            Self::Stop(order) => order.clone().into(),
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
    pub fn is_open(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_open(),
            Self::MarketToLimit(order) => order.is_open(),
            Self::StopLimit(order) => order.is_open(),
            Self::TrailingStopLimit(order) => order.is_open(),
        }
    }

    #[must_use]
    pub fn is_inflight(&self) -> bool {
        match self {
            Self::Limit(order) => order.is_inflight(),
            Self::MarketToLimit(order) => order.is_inflight(),
            Self::StopLimit(order) => order.is_inflight(),
            Self::TrailingStopLimit(order) => order.is_inflight(),
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

    #[must_use]
    pub fn contingency_type(&self) -> Option<ContingencyType> {
        match self {
            Self::Limit(order) => order.contingency_type,
            Self::MarketToLimit(order) => order.contingency_type,
            Self::StopLimit(order) => order.contingency_type,
            Self::TrailingStopLimit(order) => order.contingency_type,
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
    pub fn is_open(&self) -> bool {
        match self {
            Self::LimitIfTouched(order) => order.is_open(),
            Self::MarketIfTouched(order) => order.is_open(),
            Self::StopLimit(order) => order.is_open(),
            Self::StopMarket(order) => order.is_open(),
            Self::TrailingStopLimit(order) => order.is_open(),
            Self::TrailingStopMarket(order) => order.is_open(),
        }
    }

    #[must_use]
    pub fn is_inflight(&self) -> bool {
        match self {
            Self::LimitIfTouched(order) => order.is_inflight(),
            Self::MarketIfTouched(order) => order.is_inflight(),
            Self::StopLimit(order) => order.is_inflight(),
            Self::StopMarket(order) => order.is_inflight(),
            Self::TrailingStopLimit(order) => order.is_inflight(),
            Self::TrailingStopMarket(order) => order.is_inflight(),
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

    #[must_use]
    pub fn contingency_type(&self) -> Option<ContingencyType> {
        match self {
            Self::LimitIfTouched(order) => order.contingency_type,
            Self::MarketIfTouched(order) => order.contingency_type,
            Self::StopLimit(order) => order.contingency_type,
            Self::StopMarket(order) => order.contingency_type,
            Self::TrailingStopLimit(order) => order.contingency_type,
            Self::TrailingStopMarket(order) => order.contingency_type,
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
