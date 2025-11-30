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

// TODO: We'll use anyhow for now, but would be best to implement some specific Error(s)
use nautilus_model::{
    enums::{OrderSideSpecified, OrderType},
    orders::{Order, OrderAny},
    types::Price,
};

/// Calculates the protection price for stop limit and stop market orders using best bid or ask price..
///
/// # Returns
/// A calculated protection price.
///
/// # Errors
/// Returns an error if:
/// - the order type is invalid.
/// - protection points or best bid/ask are provided but not valid
///
/// # Panics
///
/// Panics if the values required for calculation cannot be converted to a float.
pub fn protection_price_calculate(
    price_increment: Price,
    order: &OrderAny,
    protection_points: Option<u32>,
    bid: Option<Price>,
    ask: Option<Price>,
) -> anyhow::Result<Price> {
    let order_type = order.order_type();
    if !matches!(order_type, OrderType::Market | OrderType::StopMarket) {
        anyhow::bail!("Invalid `OrderType` {order_type} for protection price calculation");
    }

    let protection_points =
        protection_points.ok_or_else(|| anyhow::anyhow!("Protection points required"))?;
    let offset = f64::from(protection_points) * price_increment.as_f64();

    let order_side = order.order_side_specified();
    let protection_price = match order_side {
        OrderSideSpecified::Buy => {
            let opposite = ask.ok_or_else(|| anyhow::anyhow!("Ask required"))?;
            let opposite_f64 = opposite.as_f64();
            opposite_f64 + offset
        }
        OrderSideSpecified::Sell => {
            let opposite = bid.ok_or_else(|| anyhow::anyhow!("Bid required"))?;
            let opposite_f64 = opposite.as_f64();
            opposite_f64 - offset
        }
    };

    Ok(Price::new(protection_price, price_increment.precision))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType, TriggerType},
        orders::builder::OrderTestBuilder,
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    fn build_stop_order(order_type: OrderType, side: OrderSide) -> OrderAny {
        let mut builder = OrderTestBuilder::new(order_type);
        builder
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .quantity(Quantity::from(1))
            .trigger_price(Price::new(100.0, 2))
            .trigger_type(TriggerType::LastPrice);

        if order_type == OrderType::StopLimit {
            builder.price(Price::new(99.5, 2));
        }

        builder.build()
    }

    #[rstest]
    fn test_calculate_with_invalid_order_type() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .price(Price::new(100.0, 2))
            .quantity(Quantity::from(1))
            .build();

        let result = protection_price_calculate(Price::new(0.01, 2), &order, Some(600), None, None);

        assert!(result.is_err());
    }

    #[rstest]
    fn test_calculate_requires_protection_points() {
        let order = build_stop_order(OrderType::StopMarket, OrderSide::Buy);

        let result = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            None,
            Some(Price::new(99.0, 2)),
            Some(Price::new(101.0, 2)),
        );

        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_calculate_requires_opposite_quote(#[case] side: OrderSide) {
        let order = build_stop_order(OrderType::StopMarket, side);
        let price_increment = Price::new(0.01, 2);

        let (bid, ask) = match side {
            OrderSide::Buy => (Some(Price::new(99.5, 2)), None),
            OrderSide::Sell => (None, Some(Price::new(100.5, 2))),
            OrderSide::NoOrderSide => panic!("Side is required"),
        };

        let result = protection_price_calculate(price_increment, &order, Some(25), bid, ask);

        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::Market)]
    fn test_protection_price_buy(#[case] order_type: OrderType) {
        let order = build_stop_order(order_type, OrderSide::Buy);

        let protection_price = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            Some(50),
            Some(Price::new(99.0, 2)),
            Some(Price::new(101.0, 2)),
        )
        .unwrap();

        assert_eq!(protection_price.as_f64(), 101.5);
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::Market)]
    fn test_protection_price_sell(#[case] order_type: OrderType) {
        let order = build_stop_order(order_type, OrderSide::Sell);

        let protection_price = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            Some(50),
            Some(Price::new(99.0, 2)),
            Some(Price::new(101.0, 2)),
        )
        .unwrap();

        assert_eq!(protection_price.as_f64(), 98.5);
    }
}
