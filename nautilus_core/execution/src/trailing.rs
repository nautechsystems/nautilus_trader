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

use nautilus_model::{
    enums::{OrderSide, OrderType, TrailingOffsetType, TriggerType},
    orders::{base::OrderError, OrderAny},
    types::Price,
};

pub fn trailing_stop_calculate(
    price_increment: Price,
    order: &OrderAny,
    bid: Option<Price>,
    ask: Option<Price>,
    last: Option<Price>,
) -> Result<(Option<Price>, Option<Price>), OrderError> {
    if !matches!(
        order.order_type(),
        OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
    ) {
        return Err(OrderError::InvalidOrderEvent);
    }

    let mut trigger_price = order.trigger_price();
    let mut price = None;
    let mut new_trigger_price = None;
    let mut new_price = None;

    if order.order_type() == OrderType::TrailingStopLimit {
        price = order.price();
    }

    match order.trigger_type() {
        Some(TriggerType::Default)
        | Some(TriggerType::LastPrice)
        | Some(TriggerType::MarkPrice) => {
            let last = last.ok_or(OrderError::InvalidStateTransition)?;

            let temp_trigger_price = trailing_stop_calculate_with_last(
                price_increment,
                order
                    .trailing_offset_type()
                    .unwrap_or(TrailingOffsetType::Price),
                order.order_side(),
                order.trailing_offset().unwrap_or_default(),
                last,
            )?;

            match order.order_side() {
                OrderSide::Buy => {
                    if let Some(trigger) = trigger_price {
                        if trigger > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_last(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                OrderSide::Sell => {
                    if let Some(trigger) = trigger_price {
                        if trigger < temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_last(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                OrderSide::NoOrderSide => {
                    return Err(OrderError::NoOrderSide);
                }
            }
        }
        Some(TriggerType::BidAsk) => {
            let bid = bid.ok_or(OrderError::InvalidStateTransition)?;
            let ask = ask.ok_or(OrderError::InvalidStateTransition)?;

            let temp_trigger_price = trailing_stop_calculate_with_bid_ask(
                price_increment,
                order
                    .trailing_offset_type()
                    .unwrap_or(TrailingOffsetType::Price),
                order.order_side(),
                order.trailing_offset().unwrap_or_default(),
                bid,
                ask,
            )?;

            match order.order_side() {
                OrderSide::Buy => {
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
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                OrderSide::Sell => {
                    if let Some(trigger) = trigger_price {
                        if trigger < temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                    } else {
                        new_trigger_price = Some(temp_trigger_price);
                    }

                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                OrderSide::NoOrderSide => {
                    return Err(OrderError::NoOrderSide);
                }
            }
        }
        Some(TriggerType::LastOrBidAsk) => {
            let last = last.ok_or(OrderError::InvalidStateTransition)?;
            let bid = bid.ok_or(OrderError::InvalidStateTransition)?;
            let ask = ask.ok_or(OrderError::InvalidStateTransition)?;

            let mut temp_trigger_price = trailing_stop_calculate_with_last(
                price_increment,
                order
                    .trailing_offset_type()
                    .unwrap_or(TrailingOffsetType::Price),
                order.order_side(),
                order.trailing_offset().unwrap_or_default(),
                last,
            )?;

            match order.order_side() {
                OrderSide::Buy => {
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
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                        order
                            .trailing_offset_type()
                            .unwrap_or(TrailingOffsetType::Price),
                        order.order_side(),
                        order.trailing_offset().unwrap_or_default(),
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
                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                OrderSide::Sell => {
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
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                        order
                            .trailing_offset_type()
                            .unwrap_or(TrailingOffsetType::Price),
                        order.order_side(),
                        order.trailing_offset().unwrap_or_default(),
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
                    if order.order_type() == OrderType::TrailingStopLimit {
                        let temp_price = trailing_stop_calculate_with_bid_ask(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order.limit_offset().unwrap_or_default(),
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
                OrderSide::NoOrderSide => {
                    return Err(OrderError::NoOrderSide);
                }
            }
        }
        _ => {
            return Err(OrderError::InvalidStateTransition);
        }
    }

    Ok((new_trigger_price, new_price))
}

fn trailing_stop_calculate_with_last(
    price_increment: Price,
    trailing_offset_type: TrailingOffsetType,
    side: OrderSide,
    offset: Price,
    last: Price,
) -> Result<Price, OrderError> {
    let mut offset_value = offset.as_f64();
    let last_f64 = last.as_f64();

    match trailing_offset_type {
        TrailingOffsetType::Price => {} // Offset already calculated
        TrailingOffsetType::BasisPoints => {
            offset_value = last_f64 * (offset_value / 100.0) / 100.0;
        }
        TrailingOffsetType::Ticks => {
            offset_value *= price_increment.as_f64();
        }
        TrailingOffsetType::NoTrailingOffset | TrailingOffsetType::PriceTier => {
            return Err(OrderError::InvalidStateTransition);
        }
    }

    match side {
        OrderSide::Buy => Ok(Price::new(
            last_f64 + offset_value,
            price_increment.precision,
        )),
        OrderSide::Sell => Ok(Price::new(
            last_f64 - offset_value,
            price_increment.precision,
        )),
        OrderSide::NoOrderSide => Err(OrderError::NoOrderSide),
    }
}

fn trailing_stop_calculate_with_bid_ask(
    price_increment: Price,
    trailing_offset_type: TrailingOffsetType,
    side: OrderSide,
    offset: Price,
    bid: Price,
    ask: Price,
) -> Result<Price, OrderError> {
    let mut offset_value = offset.as_f64();
    let bid_f64 = bid.as_f64();
    let ask_f64 = ask.as_f64();

    match trailing_offset_type {
        TrailingOffsetType::Price => {} // Offset already calculated
        TrailingOffsetType::BasisPoints => match side {
            OrderSide::Buy => offset_value = ask_f64 * (offset_value / 100.0) / 100.0,
            OrderSide::Sell => offset_value = bid_f64 * (offset_value / 100.0) / 100.0,
            OrderSide::NoOrderSide => return Err(OrderError::NoOrderSide),
        },
        TrailingOffsetType::Ticks => {
            offset_value *= price_increment.as_f64();
        }
        TrailingOffsetType::NoTrailingOffset | TrailingOffsetType::PriceTier => {
            return Err(OrderError::InvalidStateTransition);
        }
    }

    match side {
        OrderSide::Buy => Ok(Price::new(
            ask_f64 + offset_value,
            price_increment.precision,
        )),
        OrderSide::Sell => Ok(Price::new(
            bid_f64 - offset_value,
            price_increment.precision,
        )),
        OrderSide::NoOrderSide => Err(OrderError::NoOrderSide),
    }
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

    use super::*;

    #[rstest]
    fn test_calculate_with_invalid_order_type() {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order.into(), None, None, None);

        assert!(matches!(result, Err(OrderError::InvalidOrderEvent)));
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
            .trailing_offset(Price::new(1.0, 2))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order.into(), None, None, None);

        assert!(matches!(result, Err(OrderError::InvalidStateTransition)));
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
            .trailing_offset(Price::new(1.0, 2))
            .trigger_type(TriggerType::BidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order.into(), None, None, None);

        assert!(matches!(result, Err(OrderError::InvalidStateTransition)));
    }

    #[rstest]
    fn test_calculate_with_unsupported_trigger_type() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Price::new(1.0, 2))
            .trigger_type(TriggerType::IndexPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), &order.into(), None, None, None);

        assert!(matches!(result, Err(OrderError::InvalidStateTransition)));
    }

    #[rstest]
    fn test_calculate_with_no_order_side() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::NoOrderSide)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Price::new(1.0, 2))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            &order.into(),
            None,
            None,
            Some(Price::new(100.0, 2)),
        );

        assert!(matches!(result, Err(OrderError::NoOrderSide)));
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
            .trailing_offset(Price::new(offset, 2))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            &order.into(),
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!(
                "Expected trigger {:?} but got {:?}",
                expected_trigger, actual_trigger
            ),
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
            .trailing_offset(Price::new(basis_points, 2))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            &order.into(),
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!(
                "Expected trigger {:?} but got {:?}",
                expected_trigger, actual_trigger
            ),
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
            .trailing_offset(Price::new(offset, 2))
            .trigger_type(TriggerType::BidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            &order.into(),
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            None, // last price not needed for BidAsk trigger type
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!(
                "Expected trigger {:?} but got {:?}",
                expected_trigger, actual_trigger
            ),
        }
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 5.0, 98.0, Some(98.05))] // 5 ticks * 0.01 = 0.05 offset
    #[case(OrderSide::Buy, 100.0, 10.0, 97.0, Some(97.10))] // 10 ticks * 0.01 = 0.10 offset
    #[case(OrderSide::Sell, 100.0, 5.0, 102.0, Some(101.95))] // 5 ticks * 0.01 = 0.05 offset
    #[case(OrderSide::Sell, 100.0, 10.0, 103.0, Some(102.90))] // 10 ticks * 0.01 = 0.10 offset
    fn test_trailing_stop_market_ticks(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] ticks: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<f64>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Ticks)
            .trailing_offset(Price::new(ticks, 2))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            &order.into(),
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!(
                "Expected trigger {:?} but got {:?}",
                expected_trigger, actual_trigger
            ),
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
            .trailing_offset(Price::new(offset, 2))
            .trigger_type(TriggerType::LastOrBidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            &order.into(),
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            Some(Price::new(last_price, 2)),
        );

        let actual_trigger = result.unwrap().0;
        match (actual_trigger, expected_trigger) {
            (Some(actual), Some(expected)) => assert_eq!(actual.as_f64(), expected),
            (None, None) => (),
            _ => panic!(
                "Expected trigger {:?} but got {:?}",
                expected_trigger, actual_trigger
            ),
        }
    }
}
