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

use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_MICROSECOND};
use nautilus_model::{
    data::BarSpecification,
    enums::{AggressorSide, BarAggregation, BookAction, OptionKind, OrderSide, PriceType},
    identifiers::{InstrumentId, Symbol},
    types::{PRICE_MAX, PRICE_MIN, Price},
};
use serde::{Deserialize, Deserializer};
use ustr::Ustr;
use uuid::Uuid;

use super::enums::{TardisExchange, TardisInstrumentType, TardisOptionType};

/// Deserialize a string and convert to uppercase `Ustr`.
///
/// # Errors
///
/// Returns a deserialization error if the input is not a valid string.
pub fn deserialize_uppercase<'de, D>(deserializer: D) -> Result<Ustr, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(|s| Ustr::from(&s.to_uppercase()))
}
// Errors
//
// Returns a deserialization error if the input is not a valid string.

/// Deserialize a trade ID or generate a new UUID if empty.
///
/// # Errors
///
/// Returns a deserialization error if the input cannot be deserialized as a string.
pub fn deserialize_trade_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if s.is_empty() {
        return Ok(Uuid::new_v4().to_string());
    }

    Ok(s)
}

#[must_use]
#[inline]
pub fn normalize_symbol_str(
    symbol: Ustr,
    exchange: &TardisExchange,
    instrument_type: &TardisInstrumentType,
    is_inverse: Option<bool>,
) -> Ustr {
    match exchange {
        TardisExchange::Binance
        | TardisExchange::BinanceFutures
        | TardisExchange::BinanceUs
        | TardisExchange::BinanceDex
        | TardisExchange::BinanceJersey
            if instrument_type == &TardisInstrumentType::Perpetual =>
        {
            append_suffix(symbol, "-PERP")
        }

        TardisExchange::Bybit | TardisExchange::BybitSpot | TardisExchange::BybitOptions => {
            match instrument_type {
                TardisInstrumentType::Spot => append_suffix(symbol, "-SPOT"),
                TardisInstrumentType::Perpetual if !is_inverse.unwrap_or(false) => {
                    append_suffix(symbol, "-LINEAR")
                }
                TardisInstrumentType::Future if !is_inverse.unwrap_or(false) => {
                    append_suffix(symbol, "-LINEAR")
                }
                TardisInstrumentType::Perpetual if is_inverse == Some(true) => {
                    append_suffix(symbol, "-INVERSE")
                }
                TardisInstrumentType::Future if is_inverse == Some(true) => {
                    append_suffix(symbol, "-INVERSE")
                }
                TardisInstrumentType::Option => append_suffix(symbol, "-OPTION"),
                _ => symbol,
            }
        }

        TardisExchange::Dydx if instrument_type == &TardisInstrumentType::Perpetual => {
            append_suffix(symbol, "-PERP")
        }

        TardisExchange::GateIoFutures if instrument_type == &TardisInstrumentType::Perpetual => {
            append_suffix(symbol, "-PERP")
        }

        _ => symbol,
    }
}

fn append_suffix(symbol: Ustr, suffix: &str) -> Ustr {
    let mut symbol = symbol.to_string();
    symbol.push_str(suffix);
    Ustr::from(&symbol)
}

/// Parses a Nautilus instrument ID from the given Tardis `exchange` and `symbol` values.
#[must_use]
pub fn parse_instrument_id(exchange: &TardisExchange, symbol: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), exchange.as_venue())
}

/// Parses a Nautilus instrument ID with a normalized symbol from the given Tardis `exchange` and `symbol` values.
#[must_use]
pub fn normalize_instrument_id(
    exchange: &TardisExchange,
    symbol: Ustr,
    instrument_type: &TardisInstrumentType,
    is_inverse: Option<bool>,
) -> InstrumentId {
    let symbol = normalize_symbol_str(symbol, exchange, instrument_type, is_inverse);
    parse_instrument_id(exchange, symbol)
}

/// Normalizes the given amount by truncating it to the specified decimal precision.
///
/// Uses rounding to the nearest integer before truncation to avoid floating-point
/// precision issues (e.g., `0.1 * 10` becoming `0.9999999999`).
#[must_use]
pub fn normalize_amount(amount: f64, precision: u8) -> f64 {
    let factor = 10_f64.powi(i32::from(precision));
    // Round to nearest integer first to handle floating-point precision issues,
    // then truncate toward zero to maintain the original truncation semantics
    let scaled = amount * factor;
    let rounded = scaled.round();
    // If the rounded value is very close to scaled, use it; otherwise use trunc
    // This handles edge cases like 0.1 * 10 = 0.9999999999... -> 1.0
    let result = if (rounded - scaled).abs() < 1e-9 {
        rounded.trunc()
    } else {
        scaled.trunc()
    };
    result / factor
}

/// Parses a Nautilus price from the given `value`.
///
/// Values outside the representable range are capped to min/max price.
#[must_use]
pub fn parse_price(value: f64, precision: u8) -> Price {
    match value {
        v if (PRICE_MIN..=PRICE_MAX).contains(&v) => Price::new(value, precision),
        v if v < PRICE_MIN => Price::min(precision),
        _ => Price::max(precision),
    }
}

/// Parses a Nautilus order side from the given Tardis string `value`.
#[must_use]
pub fn parse_order_side(value: &str) -> OrderSide {
    match value {
        "bid" => OrderSide::Buy,
        "ask" => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    }
}

/// Parses a Nautilus aggressor side from the given Tardis string `value`.
#[must_use]
pub fn parse_aggressor_side(value: &str) -> AggressorSide {
    match value {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    }
}

/// Parses a Nautilus option kind from the given Tardis enum `value`.
#[must_use]
pub const fn parse_option_kind(value: TardisOptionType) -> OptionKind {
    match value {
        TardisOptionType::Call => OptionKind::Call,
        TardisOptionType::Put => OptionKind::Put,
    }
}

/// Parses a UNIX nanoseconds timestamp from the given Tardis microseconds `value_us`.
#[must_use]
pub fn parse_timestamp(value_us: u64) -> UnixNanos {
    value_us
        .checked_mul(NANOSECONDS_IN_MICROSECOND)
        .map_or_else(|| {
            tracing::error!("Timestamp overflow: {value_us} microseconds exceeds maximum representable value");
            UnixNanos::max()
        }, UnixNanos::from)
}

/// Parses a Nautilus book action inferred from the given Tardis values.
#[must_use]
pub fn parse_book_action(is_snapshot: bool, amount: f64) -> BookAction {
    if amount == 0.0 {
        BookAction::Delete
    } else if is_snapshot {
        BookAction::Add
    } else {
        BookAction::Update
    }
}

/// Parses a Nautilus bar specification from the given Tardis string `value`.
///
/// The [`PriceType`] is always `LAST` for Tardis trade bars.
///
/// # Errors
///
/// Returns an error if the specification format is invalid or if the aggregation suffix is unsupported.
pub fn parse_bar_spec(value: &str) -> anyhow::Result<BarSpecification> {
    let parts: Vec<&str> = value.split('_').collect();
    let last_part = parts
        .last()
        .ok_or_else(|| anyhow::anyhow!("Invalid bar spec: empty string"))?;
    let split_idx = last_part
        .chars()
        .position(|c| !c.is_ascii_digit())
        .ok_or_else(|| anyhow::anyhow!("Invalid bar spec: no aggregation suffix in '{value}'"))?;

    let (step_str, suffix) = last_part.split_at(split_idx);
    let step: usize = step_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid step in bar spec '{value}': {e}"))?;

    let aggregation = match suffix {
        "ms" => BarAggregation::Millisecond,
        "s" => BarAggregation::Second,
        "m" => BarAggregation::Minute,
        "ticks" => BarAggregation::Tick,
        "vol" => BarAggregation::Volume,
        _ => anyhow::bail!("Unsupported bar aggregation type: '{suffix}'"),
    };

    Ok(BarSpecification::new(step, aggregation, PriceType::Last))
}

/// Converts a Nautilus `BarSpecification` to the Tardis trade bar string convention.
///
/// # Errors
///
/// Returns an error if the bar aggregation kind is unsupported.
pub fn bar_spec_to_tardis_trade_bar_string(bar_spec: &BarSpecification) -> anyhow::Result<String> {
    let suffix = match bar_spec.aggregation {
        BarAggregation::Millisecond => "ms",
        BarAggregation::Second => "s",
        BarAggregation::Minute => "m",
        BarAggregation::Tick => "ticks",
        BarAggregation::Volume => "vol",
        _ => anyhow::bail!("Unsupported bar aggregation type: {}", bar_spec.aggregation),
    };
    Ok(format!("trade_bar_{}{}", bar_spec.step, suffix))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(TardisExchange::Binance, "ETHUSDT", "ETHUSDT.BINANCE")]
    #[case(TardisExchange::Bitmex, "XBTUSD", "XBTUSD.BITMEX")]
    #[case(TardisExchange::Bybit, "BTCUSDT", "BTCUSDT.BYBIT")]
    #[case(TardisExchange::OkexFutures, "BTC-USD-200313", "BTC-USD-200313.OKEX")]
    #[case(TardisExchange::HuobiDmLinearSwap, "FOO-BAR", "FOO-BAR.HUOBI")]
    fn test_parse_instrument_id(
        #[case] exchange: TardisExchange,
        #[case] symbol: Ustr,
        #[case] expected: &str,
    ) {
        let instrument_id = parse_instrument_id(&exchange, symbol);
        let expected_instrument_id = InstrumentId::from_str(expected).unwrap();
        assert_eq!(instrument_id, expected_instrument_id);
    }

    #[rstest]
    #[case(
        TardisExchange::Binance,
        "SOLUSDT",
        TardisInstrumentType::Spot,
        None,
        "SOLUSDT.BINANCE"
    )]
    #[case(
        TardisExchange::BinanceFutures,
        "SOLUSDT",
        TardisInstrumentType::Perpetual,
        None,
        "SOLUSDT-PERP.BINANCE"
    )]
    #[case(
        TardisExchange::Bybit,
        "BTCUSDT",
        TardisInstrumentType::Spot,
        None,
        "BTCUSDT-SPOT.BYBIT"
    )]
    #[case(
        TardisExchange::Bybit,
        "BTCUSDT",
        TardisInstrumentType::Perpetual,
        None,
        "BTCUSDT-LINEAR.BYBIT"
    )]
    #[case(
        TardisExchange::Bybit,
        "BTCUSDT",
        TardisInstrumentType::Perpetual,
        Some(true),
        "BTCUSDT-INVERSE.BYBIT"
    )]
    #[case(
        TardisExchange::Dydx,
        "BTC-USD",
        TardisInstrumentType::Perpetual,
        None,
        "BTC-USD-PERP.DYDX"
    )]
    fn test_normalize_instrument_id(
        #[case] exchange: TardisExchange,
        #[case] symbol: Ustr,
        #[case] instrument_type: TardisInstrumentType,
        #[case] is_inverse: Option<bool>,
        #[case] expected: &str,
    ) {
        let instrument_id =
            normalize_instrument_id(&exchange, symbol, &instrument_type, is_inverse);
        let expected_instrument_id = InstrumentId::from_str(expected).unwrap();
        assert_eq!(instrument_id, expected_instrument_id);
    }

    #[rstest]
    #[case(0.00001, 4, 0.0)]
    #[case(1.2345, 3, 1.234)]
    #[case(1.2345, 2, 1.23)]
    #[case(-1.2345, 3, -1.234)]
    #[case(123.456, 0, 123.0)]
    fn test_normalize_amount(#[case] amount: f64, #[case] precision: u8, #[case] expected: f64) {
        let result = normalize_amount(amount, precision);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_normalize_amount_floating_point_edge_cases() {
        // Test that floating-point edge cases are handled correctly
        // 0.1 * 10 can become 0.9999999... due to IEEE 754
        let result = normalize_amount(0.1, 1);
        assert_eq!(result, 0.1);

        // Test with values that could have precision issues
        let result = normalize_amount(0.7, 1);
        assert_eq!(result, 0.7);

        // Test large precision
        let result = normalize_amount(1.123456789, 9);
        assert_eq!(result, 1.123456789);

        // Test zero
        let result = normalize_amount(0.0, 8);
        assert_eq!(result, 0.0);

        // Test negative values
        let result = normalize_amount(-0.1, 1);
        assert_eq!(result, -0.1);
    }

    #[rstest]
    #[case("bid", OrderSide::Buy)]
    #[case("ask", OrderSide::Sell)]
    #[case("unknown", OrderSide::NoOrderSide)]
    #[case("", OrderSide::NoOrderSide)]
    #[case("random", OrderSide::NoOrderSide)]
    fn test_parse_order_side(#[case] input: &str, #[case] expected: OrderSide) {
        assert_eq!(parse_order_side(input), expected);
    }

    #[rstest]
    #[case("buy", AggressorSide::Buyer)]
    #[case("sell", AggressorSide::Seller)]
    #[case("unknown", AggressorSide::NoAggressor)]
    #[case("", AggressorSide::NoAggressor)]
    #[case("random", AggressorSide::NoAggressor)]
    fn test_parse_aggressor_side(#[case] input: &str, #[case] expected: AggressorSide) {
        assert_eq!(parse_aggressor_side(input), expected);
    }

    #[rstest]
    fn test_parse_timestamp() {
        let input_timestamp: u64 = 1583020803145000;
        let expected_nanos: UnixNanos =
            UnixNanos::from(input_timestamp * NANOSECONDS_IN_MICROSECOND);

        assert_eq!(parse_timestamp(input_timestamp), expected_nanos);
    }

    #[rstest]
    #[case(true, 10.0, BookAction::Add)]
    #[case(false, 0.0, BookAction::Delete)]
    #[case(false, 10.0, BookAction::Update)]
    fn test_parse_book_action(
        #[case] is_snapshot: bool,
        #[case] amount: f64,
        #[case] expected: BookAction,
    ) {
        assert_eq!(parse_book_action(is_snapshot, amount), expected);
    }

    #[rstest]
    #[case("trade_bar_10ms", 10, BarAggregation::Millisecond)]
    #[case("trade_bar_5m", 5, BarAggregation::Minute)]
    #[case("trade_bar_100ticks", 100, BarAggregation::Tick)]
    #[case("trade_bar_100000vol", 100000, BarAggregation::Volume)]
    fn test_parse_bar_spec(
        #[case] value: &str,
        #[case] expected_step: usize,
        #[case] expected_aggregation: BarAggregation,
    ) {
        let spec = parse_bar_spec(value).unwrap();
        assert_eq!(spec.step.get(), expected_step);
        assert_eq!(spec.aggregation, expected_aggregation);
        assert_eq!(spec.price_type, PriceType::Last);
    }

    #[rstest]
    #[case("trade_bar_10unknown", "Unsupported bar aggregation type")]
    #[case("", "no aggregation suffix")]
    #[case("trade_bar_notanumberms", "Invalid step")]
    fn test_parse_bar_spec_errors(#[case] value: &str, #[case] expected_error: &str) {
        let result = parse_bar_spec(value);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains(expected_error),
            "Expected error containing '{expected_error}'"
        );
    }

    #[rstest]
    #[case(
        BarSpecification::new(10, BarAggregation::Millisecond, PriceType::Last),
        "trade_bar_10ms"
    )]
    #[case(
        BarSpecification::new(5, BarAggregation::Minute, PriceType::Last),
        "trade_bar_5m"
    )]
    #[case(
        BarSpecification::new(100, BarAggregation::Tick, PriceType::Last),
        "trade_bar_100ticks"
    )]
    #[case(
        BarSpecification::new(100_000, BarAggregation::Volume, PriceType::Last),
        "trade_bar_100000vol"
    )]
    fn test_to_tardis_string(#[case] bar_spec: BarSpecification, #[case] expected: &str) {
        assert_eq!(
            bar_spec_to_tardis_trade_bar_string(&bar_spec).unwrap(),
            expected
        );
    }
}
