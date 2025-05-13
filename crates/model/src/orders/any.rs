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
    /// Creates a new [`OrderAny`] instance from the given `events`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `events` is empty.
    /// - The first event is not `OrderInitialized`.
    ///
    /// # Panics
    ///
    /// Panics if `events` is empty or when applying events via `order.apply`, which may panic on invalid transitions.
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
                Self::LimitIfTouched(order) => order.to_string(),
                Self::Market(order) => order.to_string(),
                Self::MarketIfTouched(order) => order.to_string(),
                Self::MarketToLimit(order) => order.to_string(),
                Self::StopLimit(order) => order.to_string(),
                Self::StopMarket(order) => order.to_string(),
                Self::TrailingStopLimit(order) => order.to_string(),
                Self::TrailingStopMarket(order) => order.to_string(),
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
    /// # Panics
    ///
    /// Panics if called on a `MarketToLimit` variant when the inner `price` is `None`.
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        enums::{OrderType, TrailingOffsetType},
        events::{OrderEventAny, OrderUpdated, order::initialized::OrderInitializedBuilder},
        identifiers::{ClientOrderId, InstrumentId, StrategyId},
        orders::builder::OrderTestBuilder,
        types::{Price, Quantity},
    };

    #[test]
    fn test_order_any_equality() {
        // Create two orders with different types but same client_order_id
        let client_order_id = ClientOrderId::from("ORDER-001");

        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .client_order_id(client_order_id.clone())
            .build();

        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .client_order_id(client_order_id)
            .build();

        // They should be equal because they have the same client_order_id
        assert_eq!(market_order, limit_order);
    }

    #[test]
    fn test_order_any_conversion_from_events() {
        // Create an OrderInitialized event
        let init_event = OrderInitializedBuilder::default()
            .order_type(OrderType::Market)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .build()
            .unwrap();

        // Create a vector of events
        let events = vec![OrderEventAny::Initialized(init_event.clone())];

        // Create OrderAny from events
        let order = OrderAny::from_events(events).unwrap();

        // Verify the order was created properly
        assert_eq!(order.order_type(), OrderType::Market);
        assert_eq!(order.instrument_id(), init_event.instrument_id);
        assert_eq!(order.quantity(), init_event.quantity);
    }

    #[test]
    fn test_order_any_from_events_empty_error() {
        let events: Vec<OrderEventAny> = vec![];
        let result = OrderAny::from_events(events);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "No order events provided to create OrderAny"
        );
    }

    #[test]
    fn test_order_any_from_events_wrong_first_event() {
        // Create an event that is not OrderInitialized
        let client_order_id = ClientOrderId::from("ORDER-001");
        let strategy_id = StrategyId::from("STRATEGY-001");

        let update_event = OrderUpdated {
            client_order_id,
            strategy_id,
            quantity: Quantity::from(20),
            ..Default::default()
        };

        // Create a vector with a non-initialization event first
        let events = vec![OrderEventAny::Updated(update_event)];

        // Attempt to create order should fail
        let result = OrderAny::from_events(events);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "First event must be `OrderInitialized`"
        );
    }

    #[test]
    fn test_passive_order_any_conversion() {
        // Create a limit order
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .build();

        // Convert to PassiveOrderAny and back
        let passive_order: PassiveOrderAny = limit_order.clone().into();
        let order_any: OrderAny = passive_order.into();

        // Verify it maintained its properties
        assert_eq!(order_any.order_type(), OrderType::Limit);
        assert_eq!(order_any.quantity(), Quantity::from(10));
    }

    #[test]
    fn test_stop_order_any_conversion() {
        // Create a stop market order
        let stop_order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .trigger_price(Price::new(100.0, 2))
            .build();

        // Convert to StopOrderAny and back
        let stop_order_any: StopOrderAny = stop_order.into();
        let order_any: OrderAny = stop_order_any.into();

        // Verify it maintained its properties
        assert_eq!(order_any.order_type(), OrderType::StopMarket);
        assert_eq!(order_any.quantity(), Quantity::from(10));
        assert_eq!(order_any.trigger_price(), Some(Price::new(100.0, 2)));
    }

    #[test]
    fn test_limit_order_any_conversion() {
        // Create a limit order
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .build();

        // Convert to LimitOrderAny and back
        let limit_order_any: LimitOrderAny = limit_order.into();
        let order_any: OrderAny = limit_order_any.into();

        // Verify it maintained its properties
        assert_eq!(order_any.order_type(), OrderType::Limit);
        assert_eq!(order_any.quantity(), Quantity::from(10));
    }

    #[test]
    fn test_limit_order_any_limit_price() {
        // Create a limit order
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .build();

        // Convert to LimitOrderAny
        let limit_order_any: LimitOrderAny = limit_order.into();

        // Check limit price accessor
        let limit_px = limit_order_any.limit_px();
        assert_eq!(limit_px, Price::new(100.0, 2));
    }

    #[test]
    fn test_stop_order_any_stop_price() {
        // Create a stop market order
        let stop_order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .trigger_price(Price::new(100.0, 2))
            .build();

        // Convert to StopOrderAny
        let stop_order_any: StopOrderAny = stop_order.into();

        // Check stop price accessor
        let stop_px = stop_order_any.stop_px();
        assert_eq!(stop_px, Price::new(100.0, 2));
    }

    #[test]
    fn test_trailing_stop_market_order_conversion() {
        // Create a trailing stop market order
        let trailing_stop_order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset(Decimal::new(5, 1)) // 0.5
            .trailing_offset_type(TrailingOffsetType::NoTrailingOffset)
            .build();

        // Convert to StopOrderAny
        let stop_order_any: StopOrderAny = trailing_stop_order.clone().into();

        // And back to OrderAny
        let order_any: OrderAny = stop_order_any.into();

        // Verify properties are preserved
        assert_eq!(order_any.order_type(), OrderType::TrailingStopMarket);
        assert_eq!(order_any.quantity(), Quantity::from(10));
        assert_eq!(order_any.trigger_price(), Some(Price::new(100.0, 2)));
        assert_eq!(order_any.trailing_offset(), Some(Decimal::new(5, 1)));
        assert_eq!(
            order_any.trailing_offset_type(),
            Some(TrailingOffsetType::NoTrailingOffset)
        );
    }

    #[test]
    fn test_trailing_stop_limit_order_conversion() {
        // Create a trailing stop limit order
        let trailing_stop_limit = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(99.0, 2))
            .trigger_price(Price::new(100.0, 2))
            .limit_offset(Decimal::new(10, 1)) // 1.0
            .trailing_offset(Decimal::new(5, 1)) // 0.5
            .trailing_offset_type(TrailingOffsetType::NoTrailingOffset)
            .build();

        // Convert to LimitOrderAny
        let limit_order_any: LimitOrderAny = trailing_stop_limit.clone().into();

        // Check limit price
        assert_eq!(limit_order_any.limit_px(), Price::new(99.0, 2));

        // Convert back to OrderAny
        let order_any: OrderAny = limit_order_any.into();

        // Verify properties are preserved
        assert_eq!(order_any.order_type(), OrderType::TrailingStopLimit);
        assert_eq!(order_any.quantity(), Quantity::from(10));
        assert_eq!(order_any.price(), Some(Price::new(99.0, 2)));
        assert_eq!(order_any.trigger_price(), Some(Price::new(100.0, 2)));
        assert_eq!(order_any.trailing_offset(), Some(Decimal::new(5, 1)));
    }

    #[test]
    fn test_passive_order_any_to_any() {
        // Create a limit order
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .build();

        // Convert to PassiveOrderAny
        let passive_order: PassiveOrderAny = limit_order.into();

        // Use to_any method
        let order_any = passive_order.to_any();

        // Verify it maintained its properties
        assert_eq!(order_any.order_type(), OrderType::Limit);
        assert_eq!(order_any.quantity(), Quantity::from(10));
        assert_eq!(order_any.price(), Some(Price::new(100.0, 2)));
    }
}
