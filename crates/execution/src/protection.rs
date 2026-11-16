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

use nautilus_core::correctness::CorrectnessError;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    orders::{Order, OrderAny},
    types::{Price, price::PriceRaw},
};
use thiserror::Error;

/// Calculates the protection price for market and stop market orders using best bid or ask price.
///
/// Uses checked fixed-point arithmetic to preserve all stored price units.
///
/// # Returns
/// A calculated protection price at the price increment's precision.
///
/// # Errors
/// Returns an error if:
/// - the order type is invalid.
/// - best bid/ask is not provided when required for the order side.
/// - the calculated price is outside the representable range.
/// - the calculated price has fractional digits beyond the price increment's precision.
pub fn protection_price_calculate(
    price_increment: Price,
    order: &OrderAny,
    protection_points: u32,
    bid: Option<Price>,
    ask: Option<Price>,
) -> Result<Price, ProtectionPriceError> {
    let order_type = order.order_type();
    if !matches!(order_type, OrderType::Market | OrderType::StopMarket) {
        return Err(ProtectionPriceError::InvalidOrderType(order_type));
    }

    let offset = price_increment
        .raw()
        .checked_mul(PriceRaw::from(protection_points))
        .ok_or(ProtectionPriceError::OffsetOverflow)?;

    let raw = match order.order_side() {
        OrderSide::Buy => ask
            .ok_or(ProtectionPriceError::MissingAsk)?
            .raw()
            .checked_add(offset),
        OrderSide::Sell => bid
            .ok_or(ProtectionPriceError::MissingBid)?
            .raw()
            .checked_sub(offset),
    }
    .ok_or(ProtectionPriceError::PriceOverflow)?;

    Ok(Price::from_raw_checked(raw, price_increment.precision)?)
}

/// An error calculating a market or stop market order's protection price.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum ProtectionPriceError {
    /// The order is neither a market order nor a stop market order.
    #[error("Invalid `OrderType` {0} for protection price calculation")]
    InvalidOrderType(OrderType),
    /// A sell order has no bid price.
    #[error("Bid required")]
    MissingBid,
    /// A buy order has no ask price.
    #[error("Ask required")]
    MissingAsk,
    /// Multiplying the price increment by the protection points overflows.
    #[error("Protection offset exceeds raw price bounds")]
    OffsetOverflow,
    /// Applying the protection offset to the quote overflows.
    #[error("Protection price exceeds raw price bounds")]
    PriceOverflow,
    /// The raw price or precision fails validation, with a boxed [`CorrectnessError`] as its source.
    #[error("{0}")]
    InvalidPrice(#[source] Box<CorrectnessError>),
}

impl From<CorrectnessError> for ProtectionPriceError {
    fn from(source: CorrectnessError) -> Self {
        Self::InvalidPrice(Box::new(source))
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType, TriggerType},
        orders::builder::OrderTestBuilder,
        types::{Quantity, fixed::FIXED_PRECISION},
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
    #[case(OrderSide::Buy, "123.456821")]
    #[case(OrderSide::Sell, "123.456761")]
    fn test_protection_rejects_sub_increment_units(
        #[case] side: OrderSide,
        #[case] expected: &str,
        #[values(5, 6)] quote_precision: u8,
    ) {
        let order = build_stop_order(OrderType::Market, side);
        let mut quote = Price::from("123.456791");
        quote.precision = quote_precision;
        let increment = Price::from("0.00001");
        let precision = increment.precision;
        let raw = Price::from(expected).raw();
        let scale = PriceRaw::pow(10, u32::from(FIXED_PRECISION - precision));
        let remainder = raw % scale;

        let error =
            protection_price_calculate(increment, &order, 3, Some(quote), Some(quote)).unwrap_err();
        let source = Price::from_raw_checked(raw, precision).unwrap_err();

        assert_eq!(
            error,
            ProtectionPriceError::InvalidPrice(Box::new(source.clone()))
        );
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<Box<CorrectnessError>>()
                .map(Box::as_ref),
            Some(&source)
        );
        assert_eq!(
            error.to_string(),
            format!(
                "Invalid fixed-point raw value {raw} for precision {precision}: \
                 remainder {remainder} when divided by scale {scale}. \
                 Raw value should be a multiple of {scale}. \
                 This indicates data corruption or incorrect precision/scaling upstream"
            )
        );
    }

    #[rstest]
    #[case(OrderSide::Buy, "123.456821")]
    #[case(OrderSide::Sell, "123.456761")]
    fn test_protection_preserves_sub_increment_units(
        #[case] side: OrderSide,
        #[case] expected: &str,
    ) {
        let order = build_stop_order(OrderType::Market, side);
        let quote = Price::from("123.456791");

        let price = protection_price_calculate(
            Price::from("0.000010"),
            &order,
            3,
            Some(quote),
            Some(quote),
        )
        .unwrap();

        assert_eq!(price, Price::from(expected));
        assert_eq!(price.precision, 6);
    }

    #[rstest]
    fn test_calculate_with_invalid_order_type() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .price(Price::new(100.0, 2))
            .quantity(Quantity::from(1))
            .build();

        let result = protection_price_calculate(Price::new(0.01, 2), &order, 600, None, None);

        let error = result.unwrap_err();

        assert_eq!(
            error,
            ProtectionPriceError::InvalidOrderType(OrderType::Limit)
        );
        assert_eq!(
            error.to_string(),
            "Invalid `OrderType` LIMIT for protection price calculation"
        );
    }

    #[rstest]
    #[case(OrderSide::Buy, ProtectionPriceError::MissingAsk, "Ask required")]
    #[case(OrderSide::Sell, ProtectionPriceError::MissingBid, "Bid required")]
    fn test_calculate_requires_opposite_quote(
        #[case] side: OrderSide,
        #[case] expected_error: ProtectionPriceError,
        #[case] expected: &str,
    ) {
        let order = build_stop_order(OrderType::StopMarket, side);
        let price_increment = Price::new(0.01, 2);

        let (bid, ask) = match side {
            OrderSide::Buy => (Some(Price::new(99.5, 2)), None),
            OrderSide::Sell => (None, Some(Price::new(100.5, 2))),
        };

        let error = protection_price_calculate(price_increment, &order, 25, bid, ask).unwrap_err();

        assert_eq!(error, expected_error);
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_protection_offset_overflow_precedes_missing_quote() {
        let order = build_stop_order(OrderType::Market, OrderSide::Buy);

        let error =
            protection_price_calculate(Price::max(0), &order, u32::MAX, None, None).unwrap_err();

        assert_eq!(error, ProtectionPriceError::OffsetOverflow);
        assert_eq!(
            error.to_string(),
            "Protection offset exceeds raw price bounds"
        );
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_protection_price_overflow(#[case] side: OrderSide) {
        let order = build_stop_order(OrderType::Market, side);
        let increment = Price::max(0);
        let points = u32::try_from(PriceRaw::MAX / increment.raw()).unwrap();

        let error = protection_price_calculate(
            increment,
            &order,
            points,
            Some(Price::min(0)),
            Some(Price::max(0)),
        )
        .unwrap_err();

        assert_eq!(error, ProtectionPriceError::PriceOverflow);
        assert_eq!(
            error.to_string(),
            "Protection price exceeds raw price bounds"
        );
    }

    #[rstest]
    fn test_protection_price_outside_domain_bounds() {
        let order = build_stop_order(OrderType::Market, OrderSide::Buy);
        let increment = Price::from_raw(1, FIXED_PRECISION);
        let quote = Price::max(FIXED_PRECISION);
        let source = Price::from_raw_checked(quote.raw() + 1, FIXED_PRECISION).unwrap_err();

        let error =
            protection_price_calculate(increment, &order, 1, None, Some(quote)).unwrap_err();

        assert_eq!(error, ProtectionPriceError::InvalidPrice(Box::new(source)));
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::Market)]
    fn test_protection_price_buy(#[case] order_type: OrderType) {
        let order = build_stop_order(order_type, OrderSide::Buy);

        let protection_price = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            50,
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
            50,
            Some(Price::new(99.0, 2)),
            Some(Price::new(101.0, 2)),
        )
        .unwrap();

        assert_eq!(protection_price.as_f64(), 98.5);
    }

    #[rstest]
    fn test_protection_price_zero_points() {
        let order = build_stop_order(OrderType::Market, OrderSide::Buy);

        let protection_price = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            0,
            Some(Price::new(99.0, 2)),
            Some(Price::new(101.0, 2)),
        )
        .unwrap();

        // With 0 points, protection_price = ask + 0 = 101.0
        assert_eq!(protection_price.as_f64(), 101.0);
    }

    #[rstest]
    fn test_protection_price_sell_negative_result() {
        let order = build_stop_order(OrderType::Market, OrderSide::Sell);

        let protection_price = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            1000,
            Some(Price::new(5.0, 2)),
            Some(Price::new(6.0, 2)),
        )
        .unwrap();

        // protection_price = 5.0 - (1000 * 0.01) = 5.0 - 10.0 = -5.0
        assert_eq!(protection_price.as_f64(), -5.0);
    }

    #[rstest]
    fn test_protection_price_large_points() {
        let order = build_stop_order(OrderType::Market, OrderSide::Buy);

        let protection_price = protection_price_calculate(
            Price::new(0.01, 2),
            &order,
            100_000,
            Some(Price::new(50_000.0, 2)),
            Some(Price::new(50_001.0, 2)),
        )
        .unwrap();

        // protection_price = 50001.0 + (100_000 * 0.01) = 50001.0 + 1000.0 = 51001.0
        assert_eq!(protection_price.as_f64(), 51001.0);
    }

    #[rstest]
    #[case(OrderSide::Buy, "123.45682")]
    #[case(OrderSide::Sell, "123.45667")]
    fn test_protection_price_preserves_increment_precision(
        #[case] side: OrderSide,
        #[case] expected: &str,
    ) {
        let order = build_stop_order(OrderType::Market, side);
        let (bid, ask) = match side {
            OrderSide::Buy => (None, Some(Price::from("123.456790"))),
            OrderSide::Sell => (Some(Price::from("123.456700")), None),
        };

        let price =
            protection_price_calculate(Price::from("0.00001"), &order, 3, bid, ask).unwrap();

        assert_eq!(price, Price::from(expected));
        assert_eq!(price.precision, 5);
    }
}
