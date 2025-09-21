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
    enums::{OrderSideSpecified, OrderType, TrailingOffsetType, TriggerType},
    orders::{Order, OrderAny, OrderError},
    types::Price,
};
use rust_decimal::{Decimal, prelude::*};

/// Calculates the new trigger and limit prices for a trailing stop order.
///
/// `trigger_px` and `activation_px` are optional **overrides** for the prices already
/// carried inside `order`.  If `Some(_)`, they take priority over the values on the
/// order itself, otherwise the function falls back to the values stored on the order.
///
/// # Returns
/// A tuple with the *newly-set* trigger-price and limit-price (if any).
/// `None` in either position means the respective price did **not** improve.
///
/// # Errors
/// Returns an error if:
/// - the order type or trigger type is invalid.
/// - the order does not carry a valid `TriggerType` or `TrailingOffsetType`.
///
/// # Panics
/// - If the `trailing_offset_type` is `NoTrailingOffset` or the `trigger_type` is `NoTrigger`.
/// - If the `trailing_offset` cannot be converted to a float.
/// - If the `trigger_type` is not supported by this function.
/// - If the `order_type` is not a trailing stop type.
pub fn trailing_stop_calculate(
    price_increment: Price,
    trigger_px: Option<Price>,
    activation_px: Option<Price>,
    order: &OrderAny,
    bid: Option<Price>,
    ask: Option<Price>,
    last: Option<Price>,
) -> anyhow::Result<(Option<Price>, Option<Price>)> {
    let order_side = order.order_side_specified();
    let order_type = order.order_type();

    if !matches!(
        order_type,
        OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
    ) {
        anyhow::bail!("Invalid `OrderType` {order_type} for trailing stop calculation");
    }

    let mut trigger_price = trigger_px
        .or(order.trigger_price())
        .or(activation_px)
        .or(order.activation_price());

    let mut limit_price = if order_type == OrderType::TrailingStopLimit {
        order.price()
    } else {
        None
    };

    let trigger_type = order.trigger_type().unwrap();
    let trailing_offset = order.trailing_offset().unwrap();
    let trailing_offset_type = order.trailing_offset_type().unwrap();
    assert!(trigger_type != TriggerType::NoTrigger);
    assert!(trailing_offset_type != TrailingOffsetType::NoTrailingOffset,);

    let mut new_trigger_price: Option<Price>;
    let mut new_limit_price: Option<Price> = None;

    let maybe_move = |current: &mut Option<Price>,
                      candidate: Price,
                      better: fn(Price, Price) -> bool|
     -> Option<Price> {
        match current {
            Some(p) if better(candidate, *p) => {
                *current = Some(candidate);
                Some(candidate)
            }
            None => {
                *current = Some(candidate);
                Some(candidate)
            }
            _ => None,
        }
    };

    let better_trigger: fn(Price, Price) -> bool = match order_side {
        OrderSideSpecified::Buy => |c, p| c < p,
        OrderSideSpecified::Sell => |c, p| c > p,
    };
    let better_limit = better_trigger;

    let compute = |off: Decimal, basis: f64| -> Price {
        Price::new(
            match trailing_offset_type {
                TrailingOffsetType::Price => off.to_f64().unwrap().mul_add(
                    match order_side {
                        OrderSideSpecified::Buy => 1.0,
                        OrderSideSpecified::Sell => -1.0,
                    },
                    basis,
                ),
                TrailingOffsetType::BasisPoints => {
                    let delta = basis * (off.to_f64().unwrap() / 10_000.0);
                    delta.mul_add(
                        match order_side {
                            OrderSideSpecified::Buy => 1.0,
                            OrderSideSpecified::Sell => -1.0,
                        },
                        basis,
                    )
                }
                TrailingOffsetType::Ticks => {
                    let delta = off.to_f64().unwrap() * price_increment.as_f64();
                    delta.mul_add(
                        match order_side {
                            OrderSideSpecified::Buy => 1.0,
                            OrderSideSpecified::Sell => -1.0,
                        },
                        basis,
                    )
                }
                _ => unreachable!("checked above"),
            },
            price_increment.precision,
        )
    };

    match trigger_type {
        TriggerType::Default | TriggerType::LastPrice | TriggerType::MarkPrice => {
            let last = last.ok_or(OrderError::InvalidStateTransition)?;
            let cand_trigger = compute(trailing_offset, last.as_f64());
            new_trigger_price = maybe_move(&mut trigger_price, cand_trigger, better_trigger);

            if order_type == OrderType::TrailingStopLimit {
                let cand_limit = compute(order.limit_offset().unwrap(), last.as_f64());
                new_limit_price = maybe_move(&mut limit_price, cand_limit, better_limit);
            }
        }
        TriggerType::BidAsk | TriggerType::LastOrBidAsk => {
            let (bid, ask) = (
                bid.ok_or_else(|| anyhow::anyhow!("Bid required"))?,
                ask.ok_or_else(|| anyhow::anyhow!("Ask required"))?,
            );
            let basis = match order_side {
                OrderSideSpecified::Buy => ask.as_f64(),
                OrderSideSpecified::Sell => bid.as_f64(),
            };
            let cand_trigger = compute(trailing_offset, basis);
            new_trigger_price = maybe_move(&mut trigger_price, cand_trigger, better_trigger);

            if order_type == OrderType::TrailingStopLimit {
                let cand_limit = compute(order.limit_offset().unwrap(), basis);
                new_limit_price = maybe_move(&mut limit_price, cand_limit, better_limit);
            }

            if trigger_type == TriggerType::LastOrBidAsk {
                let last = last.ok_or_else(|| anyhow::anyhow!("Last required"))?;
                let cand_trigger = compute(trailing_offset, last.as_f64());
                let updated = maybe_move(&mut trigger_price, cand_trigger, better_trigger);
                if updated.is_some() {
                    new_trigger_price = updated;
                }

                if order_type == OrderType::TrailingStopLimit {
                    let cand_limit = compute(order.limit_offset().unwrap(), last.as_f64());
                    let updated = maybe_move(&mut limit_price, cand_limit, better_limit);
                    if updated.is_some() {
                        new_limit_price = updated;
                    }
                }
            }
        }
        _ => anyhow::bail!("`TriggerType` {trigger_type} not currently supported"),
    }

    Ok((new_trigger_price, new_limit_price))
}

/// Calculates the trailing stop price using the last traded price.
///
/// # Errors
///
/// Returns an error if the offset calculation fails or the offset type is unsupported.
///
/// # Panics
///
/// Panics if the offset cannot be converted to a float.
pub fn trailing_stop_calculate_with_last(
    price_increment: Price,
    trailing_offset_type: TrailingOffsetType,
    side: OrderSideSpecified,
    offset: Decimal,
    last: Price,
) -> anyhow::Result<Price> {
    let mut offset_value = offset.to_f64().expect("Invalid `offset` value");
    let last_f64 = last.as_f64();

    match trailing_offset_type {
        TrailingOffsetType::Price => {} // Offset already calculated
        TrailingOffsetType::BasisPoints => {
            offset_value = last_f64 * (offset_value / 100.0) / 100.0;
        }
        TrailingOffsetType::Ticks => {
            offset_value *= price_increment.as_f64();
        }
        _ => anyhow::bail!("`TrailingOffsetType` {trailing_offset_type} not currently supported"),
    }

    let price_value = match side {
        OrderSideSpecified::Buy => last_f64 + offset_value,
        OrderSideSpecified::Sell => last_f64 - offset_value,
    };

    Ok(Price::new(price_value, price_increment.precision))
}

/// Calculates the trailing stop price using bid and ask prices.
///
/// # Errors
///
/// Returns an error if the offset calculation fails or the offset type is unsupported.
///
/// # Panics
///
/// Panics if the offset cannot be converted to a float.
pub fn trailing_stop_calculate_with_bid_ask(
    price_increment: Price,
    trailing_offset_type: TrailingOffsetType,
    side: OrderSideSpecified,
    offset: Decimal,
    bid: Price,
    ask: Price,
) -> anyhow::Result<Price> {
    let mut offset_value = offset.to_f64().expect("Invalid `offset` value");
    let bid_f64 = bid.as_f64();
    let ask_f64 = ask.as_f64();

    match trailing_offset_type {
        TrailingOffsetType::Price => {} // Offset already calculated
        TrailingOffsetType::BasisPoints => match side {
            OrderSideSpecified::Buy => offset_value = ask_f64 * (offset_value / 100.0) / 100.0,
            OrderSideSpecified::Sell => offset_value = bid_f64 * (offset_value / 100.0) / 100.0,
        },
        TrailingOffsetType::Ticks => {
            offset_value *= price_increment.as_f64();
        }
        _ => anyhow::bail!("`TrailingOffsetType` {trailing_offset_type} not currently supported"),
    }

    let price_value = match side {
        OrderSideSpecified::Buy => ask_f64 + offset_value,
        OrderSideSpecified::Sell => bid_f64 - offset_value,
    };

    Ok(Price::new(price_value, price_increment.precision))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType, TrailingOffsetType, TriggerType},
        orders::builder::OrderTestBuilder,
        types::Quantity,
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_calculate_with_invalid_order_type() {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1))
            .build();

        let result =
            trailing_stop_calculate(Price::new(0.01, 2), None, None, &order, None, None, None);

        // TODO: Basic error assert for now
        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_calculate_with_last_price_no_last(#[case] side: OrderSide) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result =
            trailing_stop_calculate(Price::new(0.01, 2), None, None, &order, None, None, None);

        // TODO: Basic error assert for now
        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_calculate_with_bid_ask_no_bid_ask(#[case] side: OrderSide) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .trigger_type(TriggerType::BidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result =
            trailing_stop_calculate(Price::new(0.01, 2), None, None, &order, None, None, None);

        // TODO: Basic error assert for now
        assert!(result.is_err());
    }

    #[rstest]
    fn test_calculate_with_unsupported_trigger_type() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .trigger_type(TriggerType::IndexPrice) // not supported by algo
            .quantity(Quantity::from(1))
            .build();

        let result =
            trailing_stop_calculate(Price::new(0.01, 2), None, None, &order, None, None, None);

        // TODO: Basic error assert for now
        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 99.0, None)] // Last price 99 > trigger 98, no update needed
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, Some(99.0))] // Last price 98 < trigger 100, update to 98 + 1
    #[case(OrderSide::Sell, 100.0, 1.0, 101.0, None)] // Last price 101 < trigger 102, no update needed
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, Some(101.0))] // Last price 102 > trigger 100, update to 102 - 1
    fn test_trailing_stop_market_last_price(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Decimal::from_f64(offset).unwrap())
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!("Expected trigger {expected_trigger:?} but got {actual_trigger:?}"),
        }
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 50.0, 98.0, Some(98.49))] // 50bp = 0.5% of 98 = 0.49
    #[case(OrderSide::Buy, 100.0, 100.0, 97.0, Some(97.97))] // 100bp = 1% of 97 = 0.97
    #[case(OrderSide::Sell, 100.0, 50.0, 102.0, Some(101.49))] // 50bp = 0.5% of 102 = 0.51
    #[case(OrderSide::Sell, 100.0, 100.0, 103.0, Some(101.97))] // 100bp = 1% of 103 = 1.03
    fn test_trailing_stop_market_basis_points(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] basis_points: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::BasisPoints)
            .trailing_offset(Decimal::from_f64(basis_points).unwrap())
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!("Expected trigger {expected_trigger:?} but got {actual_trigger:?}"),
        }
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, 99.0, None)] // Ask 99 > trigger 100, no update
    #[case(OrderSide::Buy, 100.0, 1.0, 97.0, 98.0, Some(99.0))] // Ask 98 < trigger 100, update to 98 + 1
    #[case(OrderSide::Sell, 100.0, 1.0, 101.0, 102.0, None)] // Bid 101 < trigger 100, no update
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, 103.0, Some(101.0))] // Bid 102 > trigger 100, update to 102 - 1
    fn test_trailing_stop_market_bid_ask(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] bid: f64,
        #[case] ask: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Decimal::from_f64(offset).unwrap())
            .trigger_type(TriggerType::BidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            None, // last price not needed for BidAsk trigger type
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!("Expected trigger {expected_trigger:?} but got {actual_trigger:?}"),
        }
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 5, 98.0, Some(98.05))] // 5 ticks * 0.01 = 0.05 offset
    #[case(OrderSide::Buy, 100.0, 10, 97.0, Some(97.10))] // 10 ticks * 0.01 = 0.10 offset
    #[case(OrderSide::Sell, 100.0, 5, 102.0, Some(101.95))] // 5 ticks * 0.01 = 0.05 offset
    #[case(OrderSide::Sell, 100.0, 10, 103.0, Some(102.90))] // 10 ticks * 0.01 = 0.10 offset
    fn test_trailing_stop_market_ticks(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] ticks: u32,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Ticks)
            .trailing_offset(Decimal::from_u32(ticks).unwrap())
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!("Expected trigger {expected_trigger:?} but got {actual_trigger:?}"),
        }
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, 97.0, 98.0, Some(99.0))] // Last price gives higher trigger
    #[case(OrderSide::Buy, 100.0, 1.0, 97.0, 96.0, 99.0, Some(98.0))] // Bid/Ask gives higher trigger
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, 102.0, 103.0, Some(101.0))] // Last price gives lower trigger
    #[case(OrderSide::Sell, 100.0, 1.0, 103.0, 101.0, 102.0, Some(102.0))] // Bid/Ask gives lower trigger
    fn test_trailing_stop_last_or_bid_ask(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] last_price: f64,
        #[case] bid: f64,
        #[case] ask: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Decimal::from_f64(offset).unwrap())
            .trigger_type(TriggerType::LastOrBidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!("Expected trigger {expected_trigger:?} but got {actual_trigger:?}"),
        }
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, Some(99.0))]
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, Some(101.0))]
    fn test_trailing_stop_market_last_price_move_in_favour(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Decimal::from_f64(offset).unwrap())
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let (maybe_trigger, _) = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        )
        .unwrap();

        match (maybe_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!("expected {expected_trigger:?}, was {maybe_trigger:?}"),
        }
    }

    #[rstest]
    fn test_trailing_stop_limit_last_price_buy_improve_trigger_and_limit() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trigger_price(Price::new(105.0, 2))
            .price(Price::new(104.5, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .limit_offset(dec!(0.5))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let (new_trigger, new_limit) = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(100.0, 2)),
        )
        .unwrap();

        assert_eq!(new_trigger.unwrap().as_f64(), 101.0);
        assert_eq!(new_limit.unwrap().as_f64(), 100.5);
    }

    #[rstest]
    fn test_trailing_stop_limit_last_price_sell_improve() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Sell)
            .trigger_price(Price::new(95.0, 2))
            .price(Price::new(95.5, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .limit_offset(dec!(0.5))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let (new_trigger, new_limit) = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(100.0, 2)),
        )
        .unwrap();

        assert_eq!(new_trigger.unwrap().as_f64(), 99.0);
        assert_eq!(new_limit.unwrap().as_f64(), 99.5);
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 99.0)]
    #[case(OrderSide::Sell, 100.0, 1.0, 101.0)]
    fn test_no_update_when_candidate_worse(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] basis: f64,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Decimal::from_f64(offset).unwrap())
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let (maybe_trigger, _) = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(basis, 2)),
        )
        .unwrap();

        assert!(maybe_trigger.is_none());
    }

    #[rstest]
    fn test_trailing_stop_limit_basis_points_buy_improve() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trigger_price(Price::new(110.0, 2))
            .price(Price::new(109.5, 2))
            .trailing_offset_type(TrailingOffsetType::BasisPoints)
            .trailing_offset(dec!(50))
            .limit_offset(dec!(25))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let (new_trigger, new_limit) = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            None,
            &order,
            None,
            None,
            Some(Price::new(98.0, 2)),
        )
        .unwrap();

        assert_eq!(new_trigger.unwrap().as_f64(), 98.49);
        assert_eq!(new_limit.unwrap().as_f64(), 98.25);
    }
}
