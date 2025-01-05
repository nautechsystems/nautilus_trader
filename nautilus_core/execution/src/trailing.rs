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

/// Provides trailing stop calculation functionality
pub struct TrailingStopCalculator;

impl TrailingStopCalculator {
    pub fn calculate(
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
        let mut price = order.price();

        let mut new_trigger_price = None;
        let mut new_price = None;

        match order.trigger_type() {
            Some(TriggerType::Default)
            | Some(TriggerType::LastPrice)
            | Some(TriggerType::MarkPrice) => {
                let last = last.ok_or(OrderError::InvalidStateTransition)?;

                let temp_trigger_price = Self::calculate_with_last(
                    price_increment,
                    order
                        .trailing_offset_type()
                        .unwrap_or(TrailingOffsetType::Price),
                    order.order_side(),
                    order
                        .trailing_offset()
                        .map(|p| p.as_f64())
                        .unwrap_or_default(),
                    last,
                )?;

                match order.order_side() {
                    OrderSide::Buy => {
                        if order.trigger_price().is_none()
                            || order.trigger_price().unwrap() > temp_trigger_price
                        {
                            new_trigger_price = Some(temp_trigger_price);
                        }

                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_last(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                last,
                            )?;
                            if order.price().is_none() || order.price().unwrap() > temp_price {
                                new_price = Some(temp_price);
                            }
                        }
                    }
                    OrderSide::Sell => {
                        if order.trigger_price().is_none()
                            || order.trigger_price().unwrap() < temp_trigger_price
                        {
                            new_trigger_price = Some(temp_trigger_price);
                        }

                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_last(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                last,
                            )?;
                            if order.price().is_none() || order.price().unwrap() < temp_price {
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

                let temp_trigger_price = Self::calculate_with_bid_ask(
                    price_increment,
                    order
                        .trailing_offset_type()
                        .unwrap_or(TrailingOffsetType::Price),
                    order.order_side(),
                    order
                        .trailing_offset()
                        .map(|p| p.as_f64())
                        .unwrap_or_default(),
                    bid,
                    ask,
                )?;

                match order.order_side() {
                    OrderSide::Buy => {
                        if order.trigger_price().is_none()
                            || order.trigger_price().unwrap() > temp_trigger_price
                        {
                            new_trigger_price = Some(temp_trigger_price);
                        }

                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_bid_ask(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                bid,
                                ask,
                            )?;
                            if order.price().is_none() || order.price().unwrap() > temp_price {
                                new_price = Some(temp_price);
                            }
                        }
                    }
                    OrderSide::Sell => {
                        if order.trigger_price().is_none()
                            || order.trigger_price().unwrap() < temp_trigger_price
                        {
                            new_trigger_price = Some(temp_trigger_price);
                        }

                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_bid_ask(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                bid,
                                ask,
                            )?;
                            if order.price().is_none() || order.price().unwrap() < temp_price {
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

                let mut temp_trigger_price = Self::calculate_with_last(
                    price_increment,
                    order
                        .trailing_offset_type()
                        .unwrap_or(TrailingOffsetType::Price),
                    order.order_side(),
                    order
                        .trailing_offset()
                        .map(|p| p.as_f64())
                        .unwrap_or_default(),
                    last,
                )?;

                match order.order_side() {
                    OrderSide::Buy => {
                        if trigger_price.is_none() || trigger_price.unwrap() > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                            trigger_price = new_trigger_price;
                        }
                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_last(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                last,
                            )?;
                            if price.is_none() || price.unwrap() > temp_price {
                                new_price = Some(temp_price);
                                price = new_price;
                            }
                        }
                        temp_trigger_price = Self::calculate_with_bid_ask(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order
                                .trailing_offset()
                                .map(|p| p.as_f64())
                                .unwrap_or_default(),
                            bid,
                            ask,
                        )?;
                        if trigger_price.is_none() || trigger_price.unwrap() > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_bid_ask(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                bid,
                                ask,
                            )?;
                            if price.is_none() || price.unwrap() > temp_price {
                                new_price = Some(temp_price);
                            }
                        }
                    }
                    OrderSide::Sell => {
                        if order.trigger_price().is_none()
                            || order.trigger_price().unwrap() < temp_trigger_price
                        {
                            new_trigger_price = Some(temp_trigger_price);
                            trigger_price = new_trigger_price;
                        }

                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_last(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                last,
                            )?;
                            if price.is_none() || price.unwrap() < temp_price {
                                new_price = Some(temp_price);
                                price = new_price;
                            }
                        }
                        temp_trigger_price = Self::calculate_with_bid_ask(
                            price_increment,
                            order
                                .trailing_offset_type()
                                .unwrap_or(TrailingOffsetType::Price),
                            order.order_side(),
                            order
                                .trailing_offset()
                                .map(|p| p.as_f64())
                                .unwrap_or_default(),
                            bid,
                            ask,
                        )?;
                        if trigger_price.is_none() || trigger_price.unwrap() > temp_trigger_price {
                            new_trigger_price = Some(temp_trigger_price);
                        }
                        if order.order_type() == OrderType::TrailingStopLimit {
                            let temp_price = Self::calculate_with_bid_ask(
                                price_increment,
                                order
                                    .trailing_offset_type()
                                    .unwrap_or(TrailingOffsetType::Price),
                                order.order_side(),
                                order.limit_offset().map(|p| p.as_f64()).unwrap_or_default(),
                                bid,
                                ask,
                            )?;
                            if price.is_none() || price.unwrap() > temp_price {
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

    fn calculate_with_last(
        price_increment: Price,
        trailing_offset_type: TrailingOffsetType,
        side: OrderSide,
        offset: f64,
        last: Price,
    ) -> Result<Price, OrderError> {
        let mut offset = offset;
        let last_f64 = last.as_f64();

        match trailing_offset_type {
            TrailingOffsetType::Price => {} // Offset already calculated
            TrailingOffsetType::BasisPoints => {
                offset = last_f64 * (offset / 100.0) / 100.0;
            }
            TrailingOffsetType::Ticks => {
                offset *= price_increment.as_f64();
            }
            TrailingOffsetType::NoTrailingOffset | TrailingOffsetType::PriceTier => {
                return Err(OrderError::InvalidStateTransition);
            }
        }

        match side {
            OrderSide::Buy => Ok(Price::new(last_f64 + offset, price_increment.precision)),
            OrderSide::Sell => Ok(Price::new(last_f64 - offset, price_increment.precision)),
            OrderSide::NoOrderSide => Err(OrderError::NoOrderSide),
        }
    }

    fn calculate_with_bid_ask(
        price_increment: Price,
        trailing_offset_type: TrailingOffsetType,
        side: OrderSide,
        offset: f64,
        bid: Price,
        ask: Price,
    ) -> Result<Price, OrderError> {
        let mut offset = offset;
        let bid_f64 = bid.as_f64();
        let ask_f64 = ask.as_f64();

        match trailing_offset_type {
            TrailingOffsetType::Price => {} // Offset already calculated
            TrailingOffsetType::BasisPoints => match side {
                OrderSide::Buy => offset = ask_f64 * (offset / 100.0) / 100.0,
                OrderSide::Sell => offset = bid_f64 * (offset / 100.0) / 100.0,
                OrderSide::NoOrderSide => return Err(OrderError::NoOrderSide),
            },
            TrailingOffsetType::Ticks => {
                offset *= price_increment.as_f64();
            }
            TrailingOffsetType::NoTrailingOffset | TrailingOffsetType::PriceTier => {
                return Err(OrderError::InvalidStateTransition);
            }
        }

        match side {
            OrderSide::Buy => Ok(Price::new(ask_f64 + offset, price_increment.precision)),
            OrderSide::Sell => Ok(Price::new(bid_f64 - offset, price_increment.precision)),
            OrderSide::NoOrderSide => Err(OrderError::NoOrderSide),
        }
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

        let result =
            TrailingStopCalculator::calculate(Price::new(0.01, 2), &order.into(), None, None, None);

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

        let result =
            TrailingStopCalculator::calculate(Price::new(0.01, 2), &order.into(), None, None, None);

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

        let result =
            TrailingStopCalculator::calculate(Price::new(0.01, 2), &order.into(), None, None, None);

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

        let result =
            TrailingStopCalculator::calculate(Price::new(0.01, 2), &order.into(), None, None, None);

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

        let result = TrailingStopCalculator::calculate(
            Price::new(0.01, 2),
            &order.into(),
            None,
            None,
            Some(Price::new(100.0, 2)),
        );

        assert!(matches!(result, Err(OrderError::NoOrderSide)));
    }
}
