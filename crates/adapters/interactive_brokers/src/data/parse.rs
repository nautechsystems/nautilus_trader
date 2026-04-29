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

//! Parsing utilities for converting Interactive Brokers data to Nautilus types.

use ibapi::contracts::{OptionComputation, tick_types::TickType};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, IndexPriceUpdate, QuoteTick, TradeTick, greeks::OptionGreekValues,
        option_chain::OptionGreeks,
    },
    enums::{AggressorSide, BookAction, GreeksConvention},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

/// Parse IB tick price and size data into a QuoteTick.
///
/// This builds a quote from individual tick updates. You typically need to accumulate
/// bid/ask prices and sizes from multiple tick updates before creating a QuoteTick.
///
/// # Arguments
///
/// * `instrument_id` - The instrument identifier
/// * `bid_price` - Bid price (if available)
/// * `ask_price` - Ask price (if available)
/// * `bid_size` - Bid size (if available)
/// * `ask_size` - Ask size (if available)
/// * `price_precision` - Price precision for the instrument
/// * `size_precision` - Size precision for the instrument
/// * `ts_event` - Event timestamp
/// * `ts_init` - Initialization timestamp
///
/// # Errors
///
/// Returns an error if price or size conversion fails.
#[allow(clippy::too_many_arguments)]
pub fn parse_quote_tick(
    instrument_id: InstrumentId,
    bid_price: Option<f64>,
    ask_price: Option<f64>,
    bid_size: Option<f64>,
    ask_size: Option<f64>,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid = bid_price.map(|p| Price::new(p, price_precision));
    let ask = ask_price.map(|p| Price::new(p, price_precision));
    let bid_qty = bid_size.map(|s| Quantity::new(s, size_precision));
    let ask_qty = ask_size.map(|s| Quantity::new(s, size_precision));

    Ok(QuoteTick::new(
        instrument_id,
        bid.unwrap_or_else(|| Price::zero(price_precision)),
        ask.unwrap_or_else(|| Price::zero(price_precision)),
        bid_qty.unwrap_or_else(|| Quantity::zero(size_precision)),
        ask_qty.unwrap_or_else(|| Quantity::zero(size_precision)),
        ts_event,
        ts_init,
    ))
}

/// Parse IB trade tick data into a TradeTick.
///
/// # Arguments
///
/// * `instrument_id` - The instrument identifier
/// * `price` - Trade price
/// * `size` - Trade size
/// * `price_precision` - Price precision for the instrument
/// * `size_precision` - Size precision for the instrument
/// * `ts_event` - Event timestamp
/// * `ts_init` - Initialization timestamp
/// * `trade_id` - Optional trade ID (will be generated if not provided)
///
/// # Errors
///
/// Returns an error if price or size conversion fails.
#[allow(clippy::too_many_arguments)]
pub fn parse_trade_tick(
    instrument_id: InstrumentId,
    price: f64,
    size: f64,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    trade_id: Option<TradeId>,
) -> anyhow::Result<TradeTick> {
    let trade_price = Price::new(price, price_precision);
    let trade_size = Quantity::new(size, size_precision);
    let aggressor_side = AggressorSide::NoAggressor; // IB doesn't provide this directly
    let trade_id = trade_id
        .unwrap_or_else(|| crate::common::parse::generate_ib_trade_id(ts_event, price, size));

    Ok(TradeTick::new(
        instrument_id,
        trade_price,
        trade_size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ))
}

/// Parse IB index price data into an [`IndexPriceUpdate`].
///
/// # Errors
///
/// Returns an error if the price conversion fails.
pub fn parse_index_price(
    instrument_id: InstrumentId,
    price: f64,
    price_precision: u8,
    price_magnifier: i32,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    let converted_price = if price_magnifier > 0 {
        price / price_magnifier as f64
    } else {
        price
    };

    Ok(IndexPriceUpdate::new(
        instrument_id,
        Price::new(converted_price, price_precision),
        ts_event,
        ts_init,
    ))
}

/// Parse an IB option computation into Nautilus option greeks when the tick carries model greeks.
#[must_use]
pub fn parse_option_computation_to_option_greeks(
    instrument_id: InstrumentId,
    computation: &OptionComputation,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<OptionGreeks> {
    match computation.field {
        TickType::ModelOption | TickType::DelayedModelOption => Some(OptionGreeks {
            instrument_id,
            greeks: OptionGreekValues {
                delta: computation.delta.unwrap_or_default(),
                gamma: computation.gamma.unwrap_or_default(),
                vega: computation.vega.unwrap_or_default(),
                theta: computation.theta.unwrap_or_default(),
                rho: 0.0, // IB does not publish rho in tickOptionComputation
            },
            convention: GreeksConvention::BlackScholes,
            mark_iv: computation.implied_volatility,
            bid_iv: None,
            ask_iv: None,
            underlying_price: computation.underlying_price,
            open_interest: None,
            ts_event,
            ts_init,
        }),
        _ => None,
    }
}

/// Parse open interest from an IB tick type when present.
#[must_use]
pub fn parse_option_open_interest(tick_type: &TickType, value: f64) -> Option<f64> {
    match tick_type {
        TickType::OpenInterest
        | TickType::OptionCallOpenInterest
        | TickType::OptionPutOpenInterest => Some(value),
        _ => None,
    }
}

/// Parse IB real-time bar data into a Bar.
///
/// # Arguments
///
/// * `bar_type` - The bar type specification
/// * `open` - Opening price
/// * `high` - High price
/// * `low` - Low price
/// * `close` - Closing price
/// * `volume` - Volume
/// * `price_precision` - Price precision for the instrument
/// * `size_precision` - Size precision for the instrument
/// * `ts_event` - Event timestamp
/// * `ts_init` - Initialization timestamp
///
/// # Errors
///
/// Returns an error if price or size conversion fails.
#[allow(clippy::too_many_arguments)]
pub fn parse_realtime_bar(
    bar_type: BarType,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open_price = Price::new(open, price_precision);
    let high_price = Price::new(high, price_precision);
    let low_price = Price::new(low, price_precision);
    let close_price = Price::new(close, price_precision);
    let bar_volume = Quantity::new(volume, size_precision);

    Ok(Bar::new(
        bar_type,
        open_price,
        high_price,
        low_price,
        close_price,
        bar_volume,
        ts_event,
        ts_init,
    ))
}

/// Parse IB market depth operation to BookAction.
///
/// # Arguments
///
/// * `operation` - IB market depth operation (0=insert, 1=update, 2=delete)
///
/// # Returns
///
/// Returns the corresponding BookAction.
#[must_use]
pub fn parse_market_depth_operation(operation: i32) -> BookAction {
    match operation {
        0 => BookAction::Add,
        1 => BookAction::Update,
        2 => BookAction::Delete,
        _ => BookAction::Add, // Default to Add for unknown operations
    }
}

// Note: ib_timestamp_to_unix_nanos moved to convert.rs

#[cfg(test)]
mod tests {
    use ibapi::contracts::{OptionComputation, tick_types::TickType};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::{InstrumentId, Symbol, Venue},
    };
    use rstest::rstest;

    use super::*;

    fn create_test_instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("AAPL"), Venue::from("NASDAQ"))
    }

    #[rstest]
    fn test_parse_quote_tick_with_all_fields() {
        let instrument_id = create_test_instrument_id();
        let result = parse_quote_tick(
            instrument_id,
            Some(150.25),
            Some(150.30),
            Some(100.0),
            Some(200.0),
            2,
            0,
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.bid_price.as_f64(), 150.25);
        assert_eq!(quote.ask_price.as_f64(), 150.30);
        assert_eq!(quote.bid_size.as_f64(), 100.0);
        assert_eq!(quote.ask_size.as_f64(), 200.0);
    }

    #[rstest]
    fn test_parse_quote_tick_with_partial_fields() {
        let instrument_id = create_test_instrument_id();
        let result = parse_quote_tick(
            instrument_id,
            Some(150.25),
            None,
            Some(100.0),
            None,
            2,
            0,
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.bid_price.as_f64(), 150.25);
        assert_eq!(quote.ask_price.as_f64(), 0.0); // Should default to zero
        assert_eq!(quote.bid_size.as_f64(), 100.0);
        assert_eq!(quote.ask_size.as_f64(), 0.0); // Should default to zero
    }

    #[rstest]
    fn test_parse_quote_tick_with_no_fields() {
        let instrument_id = create_test_instrument_id();
        let result = parse_quote_tick(
            instrument_id,
            None,
            None,
            None,
            None,
            2,
            0,
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.bid_price.as_f64(), 0.0);
        assert_eq!(quote.ask_price.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_trade_tick_with_trade_id() {
        let instrument_id = create_test_instrument_id();
        let trade_id = TradeId::from("TRADE-001");
        let result = parse_trade_tick(
            instrument_id,
            150.25,
            100.0,
            2,
            0,
            UnixNanos::new(0),
            UnixNanos::new(0),
            Some(trade_id),
        );
        assert!(result.is_ok());
        let trade = result.unwrap();
        assert_eq!(trade.price.as_f64(), 150.25);
        assert_eq!(trade.size.as_f64(), 100.0);
        assert_eq!(trade.trade_id, trade_id);
    }

    #[rstest]
    fn test_parse_trade_tick_without_trade_id() {
        let instrument_id = create_test_instrument_id();
        let result = parse_trade_tick(
            instrument_id,
            150.25,
            100.0,
            2,
            0,
            UnixNanos::new(1000),
            UnixNanos::new(1000),
            None,
        );
        assert!(result.is_ok());
        let trade = result.unwrap();
        assert_eq!(trade.price.as_f64(), 150.25);
        assert_eq!(trade.size.as_f64(), 100.0);
        // Trade ID should be auto-generated
        assert!(!trade.trade_id.to_string().is_empty());
    }

    #[rstest]
    fn test_parse_index_price_with_price_magnifier() {
        let instrument_id = create_test_instrument_id();
        let result = parse_index_price(
            instrument_id,
            452525.0,
            2,
            100,
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let index_price = result.unwrap();
        assert_eq!(index_price.value.as_f64(), 4525.25);
    }

    #[rstest]
    fn test_parse_index_price_without_price_magnifier() {
        let instrument_id = create_test_instrument_id();
        let result = parse_index_price(
            instrument_id,
            4525.25,
            2,
            1,
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let index_price = result.unwrap();
        assert_eq!(index_price.value.as_f64(), 4525.25);
    }

    #[rstest]
    fn test_parse_realtime_bar() {
        let instrument_id = create_test_instrument_id();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let result = parse_realtime_bar(
            bar_type,
            150.0,
            151.0,
            149.0,
            150.5,
            1000.0,
            2,
            0,
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let bar = result.unwrap();
        assert_eq!(bar.open.as_f64(), 150.0);
        assert_eq!(bar.high.as_f64(), 151.0);
        assert_eq!(bar.low.as_f64(), 149.0);
        assert_eq!(bar.close.as_f64(), 150.5);
        assert_eq!(bar.volume.as_f64(), 1000.0);
    }

    #[rstest]
    fn test_parse_market_depth_operation_insert() {
        let action = parse_market_depth_operation(0);
        assert_eq!(action, BookAction::Add);
    }

    #[rstest]
    fn test_parse_market_depth_operation_update() {
        let action = parse_market_depth_operation(1);
        assert_eq!(action, BookAction::Update);
    }

    #[rstest]
    fn test_parse_market_depth_operation_delete() {
        let action = parse_market_depth_operation(2);
        assert_eq!(action, BookAction::Delete);
    }

    #[rstest]
    fn test_parse_market_depth_operation_unknown() {
        let action = parse_market_depth_operation(99);
        // Should default to Add for unknown operations
        assert_eq!(action, BookAction::Add);
    }

    #[rstest]
    fn test_parse_quote_tick_precision() {
        let instrument_id = create_test_instrument_id();
        let result = parse_quote_tick(
            instrument_id,
            Some(150.255),
            Some(150.305),
            Some(100.5),
            Some(200.5),
            2, // Price precision: 2 decimal places
            0, // Size precision: 0 decimal places
            UnixNanos::new(0),
            UnixNanos::new(0),
        );
        assert!(result.is_ok());
        let quote = result.unwrap();
        // Prices should be rounded to 2 decimal places
        assert_eq!(quote.bid_price.as_f64(), 150.26); // Rounded up
        assert_eq!(quote.ask_price.as_f64(), 150.31); // Rounded up
    }

    #[rstest]
    fn test_parse_option_computation_to_option_greeks_from_model_tick() {
        let instrument_id = create_test_instrument_id();
        let greeks = parse_option_computation_to_option_greeks(
            instrument_id,
            &OptionComputation {
                field: TickType::ModelOption,
                implied_volatility: Some(0.25),
                delta: Some(0.55),
                gamma: Some(0.02),
                vega: Some(0.15),
                theta: Some(-0.05),
                underlying_price: Some(155.0),
                ..Default::default()
            },
            UnixNanos::new(10),
            UnixNanos::new(11),
        )
        .unwrap();

        assert_eq!(greeks.instrument_id, instrument_id);
        assert_eq!(greeks.delta, 0.55);
        assert_eq!(greeks.gamma, 0.02);
        assert_eq!(greeks.vega, 0.15);
        assert_eq!(greeks.theta, -0.05);
        assert_eq!(greeks.rho, 0.0);
        assert_eq!(greeks.mark_iv, Some(0.25));
        assert_eq!(greeks.underlying_price, Some(155.0));
        assert_eq!(greeks.open_interest, None);
        assert_eq!(greeks.ts_event, UnixNanos::new(10));
        assert_eq!(greeks.ts_init, UnixNanos::new(11));
    }

    #[rstest]
    fn test_parse_option_computation_to_option_greeks_ignores_non_model_tick() {
        let instrument_id = create_test_instrument_id();
        let greeks = parse_option_computation_to_option_greeks(
            instrument_id,
            &OptionComputation {
                field: TickType::BidOption,
                implied_volatility: Some(0.24),
                ..Default::default()
            },
            UnixNanos::new(10),
            UnixNanos::new(11),
        );

        assert!(greeks.is_none());
    }

    #[rstest]
    fn test_parse_option_open_interest_supports_option_interest_tick_types() {
        assert_eq!(
            parse_option_open_interest(&TickType::OptionCallOpenInterest, 1234.0),
            Some(1234.0)
        );
        assert_eq!(
            parse_option_open_interest(&TickType::OptionPutOpenInterest, 5678.0),
            Some(5678.0)
        );
        assert_eq!(
            parse_option_open_interest(&TickType::OpenInterest, 42.0),
            Some(42.0)
        );
        assert_eq!(parse_option_open_interest(&TickType::Bid, 42.0), None);
    }
}
