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

//! Shared order metadata for cache-free live execution dispatch.

use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce, TriggerType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId},
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};

/// Identifies an order when live execution tasks cannot access the engine cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderIdentity {
    /// The client order ID.
    pub client_order_id: ClientOrderId,
    /// The strategy ID associated with the order.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the order.
    pub instrument_id: InstrumentId,
    /// The order side.
    pub order_side: OrderSide,
    /// The order type.
    pub order_type: OrderType,
}

impl From<&OrderAny> for OrderIdentity {
    fn from(order: &OrderAny) -> Self {
        Self {
            client_order_id: order.client_order_id(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
        }
    }
}

/// Common order terms captured for live execution tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderContext {
    /// The order identity.
    pub identity: OrderIdentity,
    /// The order quantity.
    pub quantity: Quantity,
    /// The order price (LIMIT).
    pub price: Option<Price>,
    /// The order trigger price (STOP).
    pub trigger_price: Option<Price>,
    /// The trigger type for the order.
    pub trigger_type: Option<TriggerType>,
    /// The order time in force.
    pub time_in_force: TimeInForce,
    /// Whether the order must add liquidity.
    pub is_post_only: bool,
    /// Whether the order may only reduce a position.
    pub is_reduce_only: bool,
    /// Whether quantity is denominated in the quote currency.
    pub is_quote_quantity: bool,
}

impl From<&OrderAny> for OrderContext {
    fn from(order: &OrderAny) -> Self {
        Self {
            identity: OrderIdentity::from(order),
            quantity: order.quantity(),
            price: order.price(),
            trigger_price: order.trigger_price(),
            trigger_type: order.trigger_type(),
            time_in_force: order.time_in_force(),
            is_post_only: order.is_post_only(),
            is_reduce_only: order.is_reduce_only(),
            is_quote_quantity: order.is_quote_quantity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce, TriggerType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId},
        orders::{OrderAny, OrderTestBuilder},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::{OrderContext, OrderIdentity};

    #[rstest]
    fn test_order_identity_from_order() {
        let order = test_order(false, false, false);

        let identity = OrderIdentity::from(&order);

        assert_eq!(
            identity,
            OrderIdentity {
                client_order_id: ClientOrderId::from("O-CONTEXT-001"),
                strategy_id: StrategyId::from("S-CONTEXT-002"),
                instrument_id: InstrumentId::from("ETH-USDT-PERP.TEST"),
                order_side: OrderSide::Sell,
                order_type: OrderType::StopLimit,
            }
        );
    }

    #[rstest]
    #[case(true, false, false)]
    #[case(false, true, false)]
    #[case(false, false, true)]
    fn test_order_context_from_order(
        #[case] is_post_only: bool,
        #[case] is_reduce_only: bool,
        #[case] is_quote_quantity: bool,
    ) {
        let order = test_order(is_post_only, is_reduce_only, is_quote_quantity);

        let context = OrderContext::from(&order);

        assert_eq!(
            context,
            OrderContext {
                identity: OrderIdentity {
                    client_order_id: ClientOrderId::from("O-CONTEXT-001"),
                    strategy_id: StrategyId::from("S-CONTEXT-002"),
                    instrument_id: InstrumentId::from("ETH-USDT-PERP.TEST"),
                    order_side: OrderSide::Sell,
                    order_type: OrderType::StopLimit,
                },
                quantity: Quantity::from("12.345"),
                price: Some(Price::from("2345.67")),
                trigger_price: Some(Price::from("2301.23")),
                trigger_type: Some(TriggerType::MarkPrice),
                time_in_force: TimeInForce::Day,
                is_post_only,
                is_reduce_only,
                is_quote_quantity,
            }
        );
    }

    fn test_order(is_post_only: bool, is_reduce_only: bool, is_quote_quantity: bool) -> OrderAny {
        OrderTestBuilder::new(OrderType::StopLimit)
            .client_order_id(ClientOrderId::from("O-CONTEXT-001"))
            .strategy_id(StrategyId::from("S-CONTEXT-002"))
            .instrument_id(InstrumentId::from("ETH-USDT-PERP.TEST"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("12.345"))
            .price(Price::from("2345.67"))
            .trigger_price(Price::from("2301.23"))
            .trigger_type(TriggerType::MarkPrice)
            .time_in_force(TimeInForce::Day)
            .post_only(is_post_only)
            .reduce_only(is_reduce_only)
            .quote_quantity(is_quote_quantity)
            .build()
    }
}
