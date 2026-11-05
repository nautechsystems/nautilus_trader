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

use nautilus_common::cache::Cache;
use nautilus_core::correctness::CorrectnessError;
use nautilus_model::{
    enums::{OrderSide, OrderType, TrailingOffsetType, TriggerType},
    orders::{Order, OrderAny},
    types::Price,
};
use rust_decimal::{Decimal, RoundingStrategy};
use thiserror::Error;

/// Calculates the new trigger and limit prices for a trailing stop order.
///
/// `trigger_px` is an optional **override** for the trigger price already carried inside
/// `order`.  If `Some(_)`, it takes priority over the value on the order itself, otherwise
/// the function falls back to the value stored on the order.
///
/// `Default`, `BidAsk`, and `LastOrBidAsk` require both `bid` and `ask`.
/// `LastOrBidAsk` evaluates the quote first, then requires `last`.
/// `LastPrice` and `MarkPrice` use `last` as their reference. Other trigger types are unsupported.
///
/// # Returns
/// A tuple with the *newly-set* trigger-price and limit-price (if any).
/// `None` in either position means the respective price did **not** improve.
///
/// # Errors
/// Returns an error if:
/// - the order type, trigger type, or trailing offset type is invalid.
/// - a required bid, ask, or last price is missing.
/// - the calculated price cannot be represented as a [`Price`].
pub fn trailing_stop_calculate(
    price_increment: Price,
    trigger_px: Option<Price>,
    order: &OrderAny,
    bid: Option<Price>,
    ask: Option<Price>,
    last: Option<Price>,
) -> Result<(Option<Price>, Option<Price>), TrailingStopError> {
    let order_side = order.order_side();

    let (trigger_type, trailing_offset, trailing_offset_type, mut limit_price, limit_offset) =
        match order {
            OrderAny::TrailingStopMarket(order) => (
                order.trigger_type,
                order.trailing_offset,
                order.trailing_offset_type,
                None,
                None,
            ),
            OrderAny::TrailingStopLimit(order) => (
                order.trigger_type,
                order.trailing_offset,
                order.trailing_offset_type,
                order.price,
                Some(order.limit_offset),
            ),
            _ => return Err(TrailingStopError::InvalidOrderType(order.order_type())),
        };

    // Seed from the current trigger only (never the activation price): when the trigger has
    // not yet materialized it stays `None` here so the offset candidate below becomes the
    // initial trigger on the first update (matches v1 `TrailingStopCalculator`).
    let mut trigger_price = trigger_px.or(order.trigger_price());

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

    let better_price: fn(Price, Price) -> bool = match order_side {
        OrderSide::Buy => |c, p| c < p,
        OrderSide::Sell => |c, p| c > p,
    };

    let compute = |offset: Decimal, basis: Price| {
        trailing_stop_calculate_with_last(
            price_increment,
            trailing_offset_type,
            order_side,
            offset,
            basis,
        )
    };

    let basis = match trigger_type {
        TriggerType::LastPrice | TriggerType::MarkPrice => {
            last.ok_or(TrailingStopError::MissingLast)?
        }
        TriggerType::Default | TriggerType::BidAsk | TriggerType::LastOrBidAsk => {
            let (bid, ask) = (
                bid.ok_or(TrailingStopError::MissingBid)?,
                ask.ok_or(TrailingStopError::MissingAsk)?,
            );

            match order_side {
                OrderSide::Buy => ask,
                OrderSide::Sell => bid,
            }
        }
        _ => return Err(TrailingStopError::UnsupportedTriggerType(trigger_type)),
    };

    let cand_trigger = compute(trailing_offset, basis)?;
    let mut new_trigger_price = maybe_move(&mut trigger_price, cand_trigger, better_price);
    let mut new_limit_price = None;

    if let Some(limit_offset) = limit_offset {
        let cand_limit = compute(limit_offset, basis)?;
        new_limit_price = maybe_move(&mut limit_price, cand_limit, better_price);
    }

    if trigger_type != TriggerType::LastOrBidAsk {
        return Ok((new_trigger_price, new_limit_price));
    }

    let last = last.ok_or(TrailingStopError::MissingLast)?;
    let cand_trigger = compute(trailing_offset, last)?;
    let updated = maybe_move(&mut trigger_price, cand_trigger, better_price);
    if updated.is_some() {
        new_trigger_price = updated;
    }

    if let Some(limit_offset) = limit_offset {
        let cand_limit = compute(limit_offset, last)?;
        let updated = maybe_move(&mut limit_price, cand_limit, better_price);
        if updated.is_some() {
            new_limit_price = updated;
        }
    }

    Ok((new_trigger_price, new_limit_price))
}

/// Calculates the trailing stop price using the last traded price.
///
/// # Errors
///
/// Returns an error if the offset type is unsupported or the calculated price cannot be
/// represented as a [`Price`].
pub fn trailing_stop_calculate_with_last(
    price_increment: Price,
    trailing_offset_type: TrailingOffsetType,
    side: OrderSide,
    offset: Decimal,
    last: Price,
) -> Result<Price, TrailingStopError> {
    let last = last.as_decimal();
    let offset = match trailing_offset_type {
        TrailingOffsetType::Price => offset,
        TrailingOffsetType::BasisPoints => last * offset / Decimal::from(10_000),
        TrailingOffsetType::Ticks => offset * price_increment.as_decimal(),
        _ => {
            return Err(TrailingStopError::UnsupportedOffsetType(
                trailing_offset_type,
            ));
        }
    };

    let price = match side {
        OrderSide::Buy => last + offset,
        OrderSide::Sell => last - offset,
    };

    let rounded = Price::from_decimal_dp(price, price_increment.precision)?;
    let increment_raw = price_increment.raw().abs();

    if increment_raw == 0 || rounded.raw() % increment_raw == 0 {
        return Ok(rounded);
    }

    // The increment is coarser than its precision, such as 0.25: use its nearest multiple
    let increment = price_increment.as_decimal();
    let nearest = price
        .checked_div(increment)
        .and_then(|increments| {
            increments
                .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
                .checked_mul(increment)
        })
        .unwrap_or(price);

    Price::from_decimal_dp(nearest, price_increment.precision).map_err(Into::into)
}

/// Calculates the trailing stop price using bid and ask prices.
///
/// # Errors
///
/// Returns an error if the offset type is unsupported or the calculated price cannot be
/// represented as a [`Price`].
pub fn trailing_stop_calculate_with_bid_ask(
    price_increment: Price,
    trailing_offset_type: TrailingOffsetType,
    side: OrderSide,
    offset: Decimal,
    bid: Price,
    ask: Price,
) -> Result<Price, TrailingStopError> {
    let basis = match side {
        OrderSide::Buy => ask,
        OrderSide::Sell => bid,
    };

    trailing_stop_calculate_with_last(price_increment, trailing_offset_type, side, offset, basis)
}

/// An error calculating a trailing stop order's trigger or limit price.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum TrailingStopError {
    /// The order is neither a trailing stop market order nor a trailing stop limit order.
    #[error("Invalid `OrderType` {0} for trailing stop calculation")]
    InvalidOrderType(OrderType),
    /// The trigger type has no supported market-price basis.
    #[error("`TriggerType` {0} not currently supported")]
    UnsupportedTriggerType(TriggerType),
    /// The offset type has no supported price calculation.
    #[error("`TrailingOffsetType` {0} not currently supported")]
    UnsupportedOffsetType(TrailingOffsetType),
    /// A `Default`, `BidAsk`, or `LastOrBidAsk` calculation has no `bid` price.
    #[error("Bid required")]
    MissingBid,
    /// A `Default`, `BidAsk`, or `LastOrBidAsk` calculation has no `ask` price.
    #[error("Ask required")]
    MissingAsk,
    /// A `LastPrice`, `MarkPrice`, or `LastOrBidAsk` calculation has no `last` price.
    #[error("Last required")]
    MissingLast,
    /// A calculated price fails validation, with a boxed [`CorrectnessError`] as its source.
    #[error("{0}")]
    InvalidPrice(#[source] Box<CorrectnessError>),
}

impl From<CorrectnessError> for TrailingStopError {
    fn from(source: CorrectnessError) -> Self {
        Self::InvalidPrice(Box::new(source))
    }
}

// Activation is not an order event, so copy only the activation fields into the cached order:
// `replace_order` would append its last event to the cache database again, and copying the whole
// order would overwrite a lifecycle state whose event is still queued for the execution engine.
pub(crate) fn store_trailing_stop_activation(cache: &mut Cache, order: &OrderAny) {
    let Some(mut cached) = cache.order_mut(&order.client_order_id()) else {
        log::error!(
            "Cannot store trailing stop activation: order {} not found",
            order.client_order_id()
        );
        return;
    };

    match (&mut *cached, order) {
        (OrderAny::TrailingStopMarket(cached), OrderAny::TrailingStopMarket(order)) => {
            cached.activation_price = order.activation_price;
            cached.is_activated = order.is_activated;
        }
        (OrderAny::TrailingStopLimit(cached), OrderAny::TrailingStopLimit(order)) => {
            cached.activation_price = order.activation_price;
            cached.is_activated = order.is_activated;
        }
        _ => {}
    }
}

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

    fn assert_optional_price(actual: Option<Price>, expected: Option<&str>) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => assert_eq!(actual, Price::from(expected)),
            (None, None) => {}
            (actual, expected) => panic!("expected {expected:?}, was {actual:?}"),
        }
    }

    #[rstest]
    fn test_calculate_with_invalid_order_type() {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), None, &order, None, None, None);

        let error = result.unwrap_err();

        assert_eq!(
            error,
            TrailingStopError::InvalidOrderType(OrderType::Market)
        );
        assert_eq!(
            error.to_string(),
            "Invalid `OrderType` MARKET for trailing stop calculation"
        );
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_calculate_with_last_price_no_last(
        #[case] side: OrderSide,
        #[values(TriggerType::LastPrice, TriggerType::MarkPrice)] trigger_type: TriggerType,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .trigger_type(trigger_type)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(Price::new(0.01, 2), None, &order, None, None, None);

        let error = result.unwrap_err();

        assert_eq!(error, TrailingStopError::MissingLast);
        assert_eq!(error.to_string(), "Last required");
    }

    #[rstest]
    #[case(None, None, TrailingStopError::MissingBid, "Bid required")]
    #[case(None, Some("101.00"), TrailingStopError::MissingBid, "Bid required")]
    #[case(Some("100.00"), None, TrailingStopError::MissingAsk, "Ask required")]
    fn test_calculate_with_bid_ask_no_bid_ask(
        #[case] bid: Option<&str>,
        #[case] ask: Option<&str>,
        #[case] expected: TrailingStopError,
        #[case] expected_message: &str,
        #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
        #[values(TriggerType::Default, TriggerType::BidAsk, TriggerType::LastOrBidAsk)]
        trigger_type: TriggerType,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1.0))
            .trigger_type(trigger_type)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::from("0.01"),
            None,
            &order,
            bid.map(Price::from),
            ask.map(Price::from),
            None,
        );

        let error = result.unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(error.to_string(), expected_message);
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

        let result = trailing_stop_calculate(Price::new(0.01, 2), None, &order, None, None, None);

        let error = result.unwrap_err();

        assert_eq!(
            error,
            TrailingStopError::UnsupportedTriggerType(TriggerType::IndexPrice)
        );
        assert_eq!(
            error.to_string(),
            "`TriggerType` INDEX_PRICE not currently supported"
        );
    }

    #[rstest]
    #[should_panic(expected = "Trailing offset type not set")]
    fn test_build_without_trailing_offset_type_panics() {
        let _ = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trigger_price(Price::new(100.0, 2))
            .trailing_offset(dec!(1.0))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 99.0, None)] // Last price 99 > trigger 98, no update needed
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, Some("99.0"))] // Last price 98 < trigger 100, update to 98 + 1
    #[case(OrderSide::Sell, 100.0, 1.0, 101.0, None)] // Last price 101 < trigger 102, no update needed
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, Some("101.0"))] // Last price 102 > trigger 100, update to 102 - 1
    fn test_trailing_stop_market_last_price(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<&str>,
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
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        assert_optional_price(result.unwrap().0, expected_trigger);
    }

    #[rstest]
    #[case(OrderSide::Buy, 1505.0, 1.0, 1480.0, 1479.0, Some("1481.0"))] // BUY uses ask as basis
    #[case(OrderSide::Sell, 1495.0, 1.0, 1521.0, 1520.0, Some("1519.0"))] // SELL uses bid as basis
    fn test_trailing_stop_market_default_uses_bid_ask(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] ask: f64,
        #[case] bid: f64,
        #[case] expected_trigger: Option<&str>,
    ) {
        // NOTE: TriggerType::Default is documented to behave like BID_ASK (quote-based), so it
        // should not require a last-trade price and should trail using bid/ask.
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::new(initial_trigger, 2))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(Decimal::from_f64(offset).unwrap())
            .trigger_type(TriggerType::Default)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::new(0.01, 2),
            None,
            &order,
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            None, // no last-trade price available
        );

        assert_optional_price(result.unwrap().0, expected_trigger);
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 50.0, 98.0, Some("98.49"))] // 50bp = 0.5% of 98 = 0.49
    #[case(OrderSide::Buy, 100.0, 100.0, 97.0, Some("97.97"))] // 100bp = 1% of 97 = 0.97
    #[case(OrderSide::Sell, 100.0, 50.0, 102.0, Some("101.49"))] // 50bp = 0.5% of 102 = 0.51
    #[case(OrderSide::Sell, 100.0, 100.0, 103.0, Some("101.97"))] // 100bp = 1% of 103 = 1.03
    fn test_trailing_stop_market_basis_points(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] basis_points: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<&str>,
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
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        assert_optional_price(result.unwrap().0, expected_trigger);
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, 99.0, None)] // Ask 99 > trigger 100, no update
    #[case(OrderSide::Buy, 100.0, 1.0, 97.0, 98.0, Some("99.0"))] // Ask 98 < trigger 100, update to 98 + 1
    #[case(OrderSide::Sell, 100.0, 1.0, 101.0, 102.0, None)] // Bid 101 < trigger 100, no update
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, 103.0, Some("101.0"))] // Bid 102 > trigger 100, update to 102 - 1
    fn test_trailing_stop_market_bid_ask(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] bid: f64,
        #[case] ask: f64,
        #[case] expected_trigger: Option<&str>,
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
            &order,
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            None, // last price not needed for BidAsk trigger type
        );

        assert_optional_price(result.unwrap().0, expected_trigger);
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 5, 98.0, Some("98.05"))] // 5 ticks * 0.01 = 0.05 offset
    #[case(OrderSide::Buy, 100.0, 10, 97.0, Some("97.10"))] // 10 ticks * 0.01 = 0.10 offset
    #[case(OrderSide::Sell, 100.0, 5, 102.0, Some("101.95"))] // 5 ticks * 0.01 = 0.05 offset
    #[case(OrderSide::Sell, 100.0, 10, 103.0, Some("102.90"))] // 10 ticks * 0.01 = 0.10 offset
    fn test_trailing_stop_market_ticks(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] ticks: u32,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<&str>,
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
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        );

        assert_optional_price(result.unwrap().0, expected_trigger);
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, 97.0, 98.0, Some("99.0"))] // Last price gives higher trigger
    #[case(OrderSide::Buy, 100.0, 1.0, 97.0, 96.0, 99.0, Some("98.0"))] // Bid/Ask gives higher trigger
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, 102.0, 103.0, Some("101.0"))] // Last price gives lower trigger
    #[case(OrderSide::Sell, 100.0, 1.0, 103.0, 101.0, 102.0, Some("102.0"))] // Bid/Ask gives lower trigger
    fn test_trailing_stop_last_or_bid_ask(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] last_price: f64,
        #[case] bid: f64,
        #[case] ask: f64,
        #[case] expected_trigger: Option<&str>,
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
            &order,
            Some(Price::new(bid, 2)),
            Some(Price::new(ask, 2)),
            Some(Price::new(last_price, 2)),
        );

        assert_optional_price(result.unwrap().0, expected_trigger);
    }

    #[rstest]
    #[case(OrderSide::Buy, 100.0, 1.0, 98.0, Some("99.0"))]
    #[case(OrderSide::Sell, 100.0, 1.0, 102.0, Some("101.0"))]
    fn test_trailing_stop_market_last_price_move_in_favor(
        #[case] side: OrderSide,
        #[case] initial_trigger: f64,
        #[case] offset: f64,
        #[case] last_price: f64,
        #[case] expected_trigger: Option<&str>,
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
            &order,
            None,
            None,
            Some(Price::new(last_price, 2)),
        )
        .unwrap();

        assert_optional_price(maybe_trigger, expected_trigger);
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
            &order,
            None,
            None,
            Some(Price::new(100.0, 2)),
        )
        .unwrap();

        assert_eq!(new_trigger.unwrap(), Price::from("101.0"));
        assert_eq!(new_limit.unwrap(), Price::from("100.5"));
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
            &order,
            None,
            None,
            Some(Price::new(100.0, 2)),
        )
        .unwrap();

        assert_eq!(new_trigger.unwrap(), Price::from("99.0"));
        assert_eq!(new_limit.unwrap(), Price::from("99.5"));
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
            &order,
            None,
            None,
            Some(Price::new(basis, 2)),
        )
        .unwrap();

        assert!(maybe_trigger.is_none());
    }

    #[rstest]
    #[case(
        TrailingOffsetType::Price,
        OrderSide::Buy,
        dec!(1.25),
        Price::from("98.00"),
        Price::from("99.25")
    )]
    #[case(
        TrailingOffsetType::BasisPoints,
        OrderSide::Buy,
        dec!(50),
        Price::from("98.00"),
        Price::from("98.49")
    )]
    #[case(
        TrailingOffsetType::Ticks,
        OrderSide::Sell,
        dec!(5),
        Price::from("102.00"),
        Price::from("101.95")
    )]
    fn test_calculate_with_last_uses_decimal_math(
        #[case] trailing_offset_type: TrailingOffsetType,
        #[case] side: OrderSide,
        #[case] offset: Decimal,
        #[case] last: Price,
        #[case] expected: Price,
    ) {
        let price = trailing_stop_calculate_with_last(
            Price::from("0.01"),
            trailing_offset_type,
            side,
            offset,
            last,
        )
        .unwrap();

        assert_eq!(price, expected);
    }

    #[rstest]
    #[case::basis_points_sell(
        "0.25",
        TrailingOffsetType::BasisPoints,
        OrderSide::Sell,
        dec!(33),
        "1500.00",
        "1495.00"
    )]
    #[case::basis_points_buy(
        "0.25",
        TrailingOffsetType::BasisPoints,
        OrderSide::Buy,
        dec!(33),
        "1500.25",
        "1505.25"
    )]
    #[case::price_offset(
        "0.25",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.30),
        "1500.00",
        "1499.75"
    )]
    #[case::midpoint_to_even_increment(
        "0.50",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.25),
        "100.50",
        "100.00"
    )]
    #[case::offset_under_half_increment(
        "0.25",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.10),
        "1500.00",
        "1500.00"
    )]
    #[case::negative_price(
        "0.25",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.30),
        "-10.00",
        "-10.25"
    )]
    #[case::decimal_increment(
        "0.01",
        TrailingOffsetType::BasisPoints,
        OrderSide::Sell,
        dec!(1.25),
        "100.00",
        "99.99"
    )]
    #[case::decimal_increment_midpoint(
        "0.01",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.005),
        "100.00",
        "100.00"
    )]
    #[case::decimal_increment_offset_under_half(
        "0.01",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.004),
        "100.00",
        "100.00"
    )]
    #[case::zero_increment(
        "0.00",
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(0.30),
        "1500.00",
        "1499.70"
    )]
    fn test_calculate_with_last_rounds_to_price_increment(
        #[case] price_increment: &str,
        #[case] trailing_offset_type: TrailingOffsetType,
        #[case] side: OrderSide,
        #[case] offset: Decimal,
        #[case] last: &str,
        #[case] expected: &str,
    ) {
        let price = trailing_stop_calculate_with_last(
            Price::from(price_increment),
            trailing_offset_type,
            side,
            offset,
            Price::from(last),
        )
        .unwrap();

        assert_eq!(price, Price::from(expected));
    }

    #[rstest]
    #[case(
        TrailingOffsetType::Price,
        OrderSide::Sell,
        dec!(1.25),
        Price::from("102.00"),
        Price::from("103.00"),
        Price::from("100.75")
    )]
    #[case(
        TrailingOffsetType::BasisPoints,
        OrderSide::Sell,
        dec!(50),
        Price::from("102.00"),
        Price::from("103.00"),
        Price::from("101.49")
    )]
    #[case(
        TrailingOffsetType::Ticks,
        OrderSide::Buy,
        dec!(5),
        Price::from("102.00"),
        Price::from("103.00"),
        Price::from("103.05")
    )]
    fn test_calculate_with_bid_ask_uses_decimal_math(
        #[case] trailing_offset_type: TrailingOffsetType,
        #[case] side: OrderSide,
        #[case] offset: Decimal,
        #[case] bid: Price,
        #[case] ask: Price,
        #[case] expected: Price,
    ) {
        let price = trailing_stop_calculate_with_bid_ask(
            Price::from("0.01"),
            trailing_offset_type,
            side,
            offset,
            bid,
            ask,
        )
        .unwrap();

        assert_eq!(price, expected);
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
            &order,
            None,
            None,
            Some(Price::new(98.0, 2)),
        )
        .unwrap();

        assert_eq!(new_trigger.unwrap(), Price::from("98.49"));
        assert_eq!(new_limit.unwrap(), Price::from("98.24"));
    }

    #[rstest]
    #[case(OrderSide::Buy, "105.00", "100.00", "101.00")]
    #[case(OrderSide::Sell, "95.00", "100.00", "99.00")]
    fn test_trigger_override_takes_precedence(
        #[case] side: OrderSide,
        #[case] stored: &str,
        #[case] override_price: &str,
        #[case] last: &str,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::from(stored))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::from("0.01"),
            Some(Price::from(override_price)),
            &order,
            None,
            None,
            Some(Price::from(last)),
        )
        .unwrap();

        assert_eq!(result, (None, None));
    }

    #[rstest]
    #[case(OrderSide::Buy, "100.00", "105.00", None, Some("100.50"))]
    #[case(OrderSide::Buy, "105.00", "100.00", Some("101.00"), None)]
    #[case(OrderSide::Sell, "100.00", "95.00", None, Some("99.50"))]
    #[case(OrderSide::Sell, "95.00", "100.00", Some("99.00"), None)]
    fn test_trailing_limit_prices_improve_independently(
        #[case] side: OrderSide,
        #[case] trigger: &str,
        #[case] limit: &str,
        #[case] expected_trigger: Option<&str>,
        #[case] expected_limit: Option<&str>,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::from(trigger))
            .price(Price::from(limit))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1))
            .limit_offset(dec!(0.5))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::from("0.01"),
            None,
            &order,
            None,
            None,
            Some(Price::from("100.00")),
        )
        .unwrap();

        assert_eq!(
            result,
            (
                expected_trigger.map(Price::from),
                expected_limit.map(Price::from)
            )
        );
    }

    #[rstest]
    #[case(OrderSide::Buy, "110.00", "98.00", "99.00", "98.50")]
    #[case(OrderSide::Buy, "110.00", "103.00", "102.00", "101.50")]
    #[case(OrderSide::Sell, "90.00", "103.00", "102.00", "102.50")]
    #[case(OrderSide::Sell, "90.00", "98.00", "99.00", "99.50")]
    fn test_trailing_limit_last_or_bid_ask_keeps_best_prices(
        #[case] side: OrderSide,
        #[case] initial: &str,
        #[case] last: &str,
        #[case] expected_trigger: &str,
        #[case] expected_limit: &str,
    ) {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .trigger_price(Price::from(initial))
            .price(Price::from(initial))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1))
            .limit_offset(dec!(0.5))
            .trigger_type(TriggerType::LastOrBidAsk)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::from("0.01"),
            None,
            &order,
            Some(Price::from("100.00")),
            Some(Price::from("101.00")),
            Some(Price::from(last)),
        )
        .unwrap();

        assert_eq!(
            result,
            (
                Some(Price::from(expected_trigger)),
                Some(Price::from(expected_limit))
            )
        );
    }

    #[rstest]
    #[case(OrderSide::Buy, TriggerType::LastPrice, "99.00", "98.50")]
    #[case(OrderSide::Buy, TriggerType::MarkPrice, "99.00", "98.50")]
    #[case(OrderSide::Buy, TriggerType::Default, "102.00", "101.50")]
    #[case(OrderSide::Buy, TriggerType::BidAsk, "102.00", "101.50")]
    #[case(OrderSide::Buy, TriggerType::LastOrBidAsk, "99.00", "98.50")]
    #[case(OrderSide::Sell, TriggerType::LastPrice, "97.00", "97.50")]
    #[case(OrderSide::Sell, TriggerType::MarkPrice, "97.00", "97.50")]
    #[case(OrderSide::Sell, TriggerType::Default, "99.00", "99.50")]
    #[case(OrderSide::Sell, TriggerType::BidAsk, "99.00", "99.50")]
    #[case(OrderSide::Sell, TriggerType::LastOrBidAsk, "99.00", "99.50")]
    fn test_trailing_prices_initialize_from_market(
        #[case] side: OrderSide,
        #[case] trigger_type: TriggerType,
        #[case] expected_trigger: &str,
        #[case] expected_limit: &str,
        #[values(OrderType::TrailingStopMarket, OrderType::TrailingStopLimit)]
        order_type: OrderType,
    ) {
        let order = OrderTestBuilder::new(order_type)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(side)
            .activation_price(Price::from("150.00"))
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1))
            .limit_offset(dec!(0.5))
            .trigger_type(trigger_type)
            .quantity(Quantity::from(1))
            .build();

        let result = trailing_stop_calculate(
            Price::from("0.01"),
            None,
            &order,
            Some(Price::from("100.00")),
            Some(Price::from("101.00")),
            Some(Price::from("98.00")),
        )
        .unwrap();

        let limit =
            (order_type == OrderType::TrailingStopLimit).then(|| Price::from(expected_limit));

        assert_eq!(result, (Some(Price::from(expected_trigger)), limit));
        assert_eq!(result.0.map(|price| price.precision), Some(2));
        assert_eq!(result.1.map(|price| price.precision), limit.map(|_| 2));
    }

    #[rstest]
    #[case(OrderType::TrailingStopMarket)]
    #[case(OrderType::TrailingStopLimit)]
    fn test_last_or_bid_ask_checks_quote_calculation_before_last(#[case] order_type: OrderType) {
        let order = OrderTestBuilder::new(order_type)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trailing_offset_type(TrailingOffsetType::PriceTier)
            .trailing_offset(dec!(1))
            .limit_offset(dec!(0.5))
            .trigger_type(TriggerType::LastOrBidAsk)
            .quantity(Quantity::from(1))
            .build();

        let error = trailing_stop_calculate(
            Price::from("0.01"),
            None,
            &order,
            Some(Price::from("100.00")),
            Some(Price::from("101.00")),
            None,
        )
        .unwrap_err();

        assert_eq!(
            error,
            TrailingStopError::UnsupportedOffsetType(TrailingOffsetType::PriceTier)
        );
        assert_eq!(
            error.to_string(),
            "`TrailingOffsetType` PRICE_TIER not currently supported"
        );
    }

    #[rstest]
    #[case(OrderType::TrailingStopMarket)]
    #[case(OrderType::TrailingStopLimit)]
    fn test_last_or_bid_ask_requires_last(#[case] order_type: OrderType) {
        let order = OrderTestBuilder::new(order_type)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1))
            .limit_offset(dec!(0.5))
            .trigger_type(TriggerType::LastOrBidAsk)
            .quantity(Quantity::from(1))
            .build();

        let error = trailing_stop_calculate(
            Price::from("0.01"),
            None,
            &order,
            Some(Price::from("100.00")),
            Some(Price::from("101.00")),
            None,
        )
        .unwrap_err();

        assert_eq!(error, TrailingStopError::MissingLast);
        assert_eq!(error.to_string(), "Last required");
    }

    #[rstest]
    fn test_calculate_with_unsupported_offset_type() {
        let last_error = trailing_stop_calculate_with_last(
            Price::from("0.01"),
            TrailingOffsetType::PriceTier,
            OrderSide::Buy,
            dec!(1),
            Price::from("100.00"),
        )
        .unwrap_err();
        let quote_error = trailing_stop_calculate_with_bid_ask(
            Price::from("0.01"),
            TrailingOffsetType::PriceTier,
            OrderSide::Sell,
            dec!(2),
            Price::from("101.00"),
            Price::from("102.00"),
        )
        .unwrap_err();

        assert_eq!(
            last_error,
            TrailingStopError::UnsupportedOffsetType(TrailingOffsetType::PriceTier)
        );
        assert_eq!(quote_error, last_error);
    }

    #[rstest]
    fn test_calculate_price_error_preserves_source() {
        let increment = Price::from("0.01");
        let quote = Price::max(0);
        let source =
            Price::from_decimal_dp(quote.as_decimal() + dec!(1), increment.precision).unwrap_err();
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id("BTCUSDT-PERP.BINANCE".into())
            .side(OrderSide::Buy)
            .trailing_offset_type(TrailingOffsetType::Price)
            .trailing_offset(dec!(1))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        let errors = [
            trailing_stop_calculate(increment, None, &order, None, None, Some(quote)).unwrap_err(),
            trailing_stop_calculate_with_last(
                increment,
                TrailingOffsetType::Price,
                OrderSide::Buy,
                dec!(1),
                quote,
            )
            .unwrap_err(),
            trailing_stop_calculate_with_bid_ask(
                increment,
                TrailingOffsetType::Price,
                OrderSide::Buy,
                dec!(1),
                Price::from("100.00"),
                quote,
            )
            .unwrap_err(),
        ];

        for error in errors {
            assert_eq!(
                error,
                TrailingStopError::InvalidPrice(Box::new(source.clone()))
            );
            assert_eq!(error.to_string(), source.to_string());
            assert_eq!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<Box<CorrectnessError>>()
                    .map(Box::as_ref),
                Some(&source)
            );
        }
    }
}
