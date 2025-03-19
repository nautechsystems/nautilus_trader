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
use serde::{Deserialize, Serialize};

use super::{
    Order, limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder, market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder, market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder, stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder, trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{events::OrderEventAny, types::Price};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[enum_dispatch(Order)]
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
                    println!("Applying event: {event:?}"); // TODO: Development
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
                Self::LimitIfTouched(order) => format!("{order:?}"), // TODO: Implement
                Self::Market(order) => order.to_string(),
                Self::MarketIfTouched(order) => format!("{order:?}"), // TODO: Implement
                Self::MarketToLimit(order) => format!("{order:?}"),   // TODO: Implement
                Self::StopLimit(order) => order.to_string(),
                Self::StopMarket(order) => format!("{order:?}"), // TODO: Implement
                Self::TrailingStopLimit(order) => format!("{order:?}"), // TODO: Implement
                Self::TrailingStopMarket(order) => format!("{order:?}"), // TODO: Implement
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

#[derive(Clone, Debug)]
#[enum_dispatch(Order)]
pub enum PassiveOrderAny {
    Limit(LimitOrderAny),
    Stop(StopOrderAny),
}

impl PassiveOrderAny {
    #[must_use]
    pub fn to_any(&self) -> OrderAny {
        match self {
            Self::Limit(order) => order.clone().into(),
            Self::Stop(order) => order.clone().into(),
        }
    }
}

// TODO: Derive equality
impl PartialEq for PassiveOrderAny {
    fn eq(&self, rhs: &Self) -> bool {
        match self {
            Self::Limit(order) => order.client_order_id() == rhs.client_order_id(),
            Self::Stop(order) => order.client_order_id() == rhs.client_order_id(),
        }
    }
}

#[derive(Clone, Debug)]
#[enum_dispatch(Order)]
pub enum LimitOrderAny {
    Limit(LimitOrder),
    MarketToLimit(MarketToLimitOrder),
    StopLimit(StopLimitOrder),
    TrailingStopLimit(TrailingStopLimitOrder),
}

impl LimitOrderAny {
    #[must_use]
    pub fn limit_px(&self) -> Price {
        match self {
            Self::Limit(order) => order.price,
            Self::MarketToLimit(order) => order.price.expect("No price for order"), // TBD
            Self::StopLimit(order) => order.price,
            Self::TrailingStopLimit(order) => order.price,
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
#[enum_dispatch(Order)]
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
}

// TODO: Derive equality
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
