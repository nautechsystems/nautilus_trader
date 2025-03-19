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

pub fn trailing_stop_calculate(
    price_increment: Price,
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

    // SAFETY: TrailingStop order guaranteed to have trigger_type and offset properties
    let trigger_type = order.trigger_type().unwrap();
    let trailing_offset = order.trailing_offset().unwrap();
    let trailing_offset_type = order.trailing_offset_type().unwrap();
    assert!(trigger_type != TriggerType::NoTrigger);
    assert!(trailing_offset_type != TrailingOffsetType::NoTrailingOffset,);

    let mut trigger_price = order.trigger_price();
    let mut price = None;
    let mut new_trigger_price = None;
    let mut new_price = None;

    if order_type == OrderType::TrailingStopLimit {
        price = order.price();
    }

    match trigger_type {
        TriggerType::Default | TriggerType::LastPrice | TriggerType::MarkPrice => {
            let last = last.ok_or(OrderError::InvalidStateTransition)?;

            let temp_trigger_price = trailing_stop_calculate_with_last(
                price_increment,
                trailing_offset_type,
                order_side,
                trailing_offset,
                last,
            )?;

            match order_side {
                OrderSideSpecified::Buy => {
                    if let Some(trigger) = trigger_price {
                        if trigger > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order_type == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_last(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            last,
                        )?;
                        if let Some(p) = price {
                            if p > temp_price {
                                new_price = Some(temp_price);
                            }
                        } else {
                            new_price = Some(temp_price);
                        }
                    }
                }
                OrderSideSpecified::Sell => {
                    if let Some(trigger) = trigger_price {
                        if trigger < temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order_type == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_last(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            last,
                        )?;
                        if let Some(p) = price {
                            if p < temp_price {
                                new_price = Some(temp_price);
                            }
                        } else {
                            new_price = Some(temp_price);
                        }
                    }
                }
            }
        }
        TriggerType::BidAsk => {
            let bid =
                bid.ok_or_else(|| anyhow::anyhow!("`BidAsk` calculation requires `bid` price"))?;
            let ask =
                ask.ok_or_else(|| anyhow::anyhow!("`BidAsk` calculation requires `ask` price"))?;

            let temp_trigger_price = trailing_stop_calculate_with_bid_ask(
                price_increment,
                trailing_offset_type,
                order_side,
                trailing_offset,
                bid,
                ask,
            )?;

            match order_side {
                OrderSideSpecified::Buy => {
                    if let Some(trigger) = trigger_price {
                        if trigger > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            bid,
                            ask,
                        )?;
                        if let Some(p) = price {
                            if p > temp_price {
                                new_price = Some(temp_price);
                            }
                        } else {
                            new_price = Some(temp_price);
                        }
                    }
                }
                OrderSideSpecified::Sell => {
                    if let Some(trigger) = trigger_price {
                        if trigger < temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order_type == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            bid,
                            ask,
                        )?;
                        if let Some(p) = price {
                            if p < temp_price {
                                new_price = Some(temp_price);
                            }
                        } else {
                            new_price = Some(temp_price);
                        }
                    }
                }
            }
        }
        TriggerType::LastOrBidAsk => {
            let bid = bid.ok_or_else(|| {
                anyhow::anyhow!("`LastOrBidAsk` calculation requires `bid` price")
            })?;
            let ask = ask.ok_or_else(|| {
                anyhow::anyhow!("`LastOrBidAsk` calculation requires `ask` price")
            })?;
            let last = last.ok_or_else(|| {
                anyhow::anyhow!("`LastOrBidAsk` calculation requires `last` price")
            })?;

            let mut temp_trigger_price = trailing_stop_calculate_with_last(
                price_increment,
                trailing_offset_type,
                order_side,
                trailing_offset,
                last,
            )?;

            match order_side {
                OrderSideSpecified::Buy => {
                    if let Some(trigger) = trigger_price {
                        if trigger > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                            trigger_price = new_trigger_price;
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                        trigger_price = new_trigger_price;
                    }
                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_last(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            last,
                        )?;
                        if let Some(p) = price {
                            if p > temp_price {
                                new_price = Some(temp_price);
                                price = new_price;
                            }
                        } else {
                            new_price = Some(temp_price);
                            price = new_price;
                        }
                    }
                    temp_trigger_price = trailing_stop_calculate_with_bid_ask(
                        price_increment,
                        trailing_offset_type,
                        order_side,
                        trailing_offset,
                        bid,
                        ask,
                    )?;
                    if let Some(trigger) = trigger_price {
                        if trigger > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }
                    if order_type == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            bid,
                            ask,
                        )?;
                        if let Some(p) = price {
                            if p > temp_price {
                                new_price = Some(temp_price);
                            }
                        } else {
                            new_price = Some(temp_price);
                        }
                    }
                }
                OrderSideSpecified::Sell => {
                    if let Some(trigger) = trigger_price {
                        if trigger < temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                            trigger_price = new_trigger_price;
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                        trigger_price = new_trigger_price;
                    }

                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_last(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            last,
                        )?;
                        if let Some(p) = price {
                            if p < temp_price {
                                new_price = Some(temp_price);
                                price = new_price;
                            }
                        } else {
                            new_price = Some(temp_price);
                            price = new_price;
                        }
                    }
                    temp_trigger_price = trailing_stop_calculate_with_bid_ask(
                        price_increment,
                        trailing_offset_type,
                        order_side,
                        trailing_offset,
                        bid,
                        ask,
                    )?;
                    if let Some(trigger) = trigger_price {
                        if trigger < temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }
                    if order_type == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            trailing_offset_type,
                            order_side,
                            order.limit_offset().expect("Invalid order"),
                            bid,
                            ask,
                        )?;
                        if let Some(p) = price {
                            if p < temp_price {
                                new_price = Some(temp_price);
                            }
                        } else {
                            new_price = Some(temp_price);
                        }
                    }
                }
            }
        }
        _ => anyhow::bail!("`TriggerType` {trigger_type} not currently supported"),
    }

    Ok((new_trigger_price, new_price))
}

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
    use rust_decimal::prelude::*;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_calculate_with_invalid_order_type() {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order, None, None, None);

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

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order, None, None, None);

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

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order, None, None, None);

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
            .trigger_type(TriggerType::IndexPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order, None, None, None);

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
}
