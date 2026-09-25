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

use anyhow::Context;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_MICROSECOND};
use nautilus_model::{
    data::BarSpecification,
    enums::{AggressorSide, BarAggregation, BookAction, OptionKind, OrderSide, PriceType},
    identifiers::{InstrumentId, Symbol, TradeId},
    types::{PRICE_MAX, PRICE_MIN, Price, fixed::check_fixed_precision},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de};
use ustr::Ustr;

use super::enums::{TardisExchange, TardisInstrumentType, TardisOptionType};

pub(crate) fn validate_non_zero_amount(value: f64, precision: u8) -> anyhow::Result<()> {
    anyhow::ensure!(value != 0.0, "value was zero");
    check_fixed_precision(precision)?;
    let rounded_value =
        (value * 10.0_f64.powi(i32::from(precision))).round() / 10.0_f64.powi(i32::from(precision));
    anyhow::ensure!(
        rounded_value != 0.0,
        "value {value} was zero after rounding to precision {precision}"
    );
    Ok(())
}

// FNV-1a 64-bit constants (see http://www.isthe.com/chongo/tech/comp/fnv/).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// Deserialize a string and convert to uppercase `Ustr`.
///
/// # Errors
///
/// Returns a deserialization error if the input is not a valid string.
pub(crate) fn deserialize_uppercase<'de, D>(deserializer: D) -> Result<Ustr, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(|s| Ustr::from(&s.to_uppercase()))
}

/// Deserializes an `f64` from a JSON number or numeric string.
///
/// # Errors
///
/// Returns a deserialization error if the input is not numeric or if the string cannot be parsed
/// as `f64`.
pub(crate) fn deserialize_f64_or_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    struct F64OrString;
    impl<'de> de::Visitor<'de> for F64OrString {
        type Value = f64;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("f64 or string-encoded f64")
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<f64, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<f64, E> {
            Ok(v as f64)
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<f64, E> {
            Ok(v as f64)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<f64, E> {
            v.parse().map_err(de::Error::custom)
        }
        fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<f64, M::Error> {
            serde_json::Number::deserialize(de::value::MapAccessDeserializer::new(map))?
                .as_f64()
                .ok_or_else(|| de::Error::custom("number is outside f64 bounds"))
        }
    }
    deserializer.deserialize_any(F64OrString)
}

/// Deserializes an optional `f64` from null, a JSON number, or a numeric string.
///
/// # Errors
///
/// Returns a deserialization error if a non-null input is not numeric or if the string cannot be
/// parsed as `f64`.
pub(crate) fn deserialize_opt_f64_or_string<'de, D>(
    deserializer: D,
) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptF64OrString;
    impl<'de> de::Visitor<'de> for OptF64OrString {
        type Value = Option<f64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("null, f64, or string-encoded f64")
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<f64>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<f64>, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Option<f64>, E> {
            Ok(Some(v))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Option<f64>, E> {
            Ok(Some(v as f64))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Option<f64>, E> {
            Ok(Some(v as f64))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Option<f64>, E> {
            v.parse().map(Some).map_err(de::Error::custom)
        }
        fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<Option<f64>, M::Error> {
            deserialize_f64_or_string(de::value::MapAccessDeserializer::new(map)).map(Some)
        }
    }
    deserializer.deserialize_any(OptF64OrString)
}

/// Derives a deterministic [`TradeId`] from trade fields.
///
/// Tardis records do not always carry a venue-provided trade ID (some venues
/// publish empty strings or omit the field entirely). This hash combines the
/// symbol, timestamp, price, amount, and side so replayed data yields the same
/// identifier across runs. FNV-1a is stable across architectures and crate
/// versions; the 0x1f delimiter keeps variable-length fields from colliding.
///
/// `price` and `amount` are plain decimal text without trailing zeros, such as the `Display`
/// output of a normalized `Decimal` or of an `f64`, so equal values derive the same identifier
/// from machine messages and CSV records.
#[must_use]
pub fn derive_trade_id(
    symbol: Ustr,
    ts_event_ns: u64,
    price: &str,
    amount: &str,
    side: &str,
) -> TradeId {
    let mut hash: u64 = FNV_OFFSET_BASIS;

    for bytes in [
        symbol.as_bytes(),
        b"\x1f",
        &ts_event_ns.to_le_bytes(),
        b"\x1f",
        price.as_bytes(),
        b"\x1f",
        amount.as_bytes(),
        b"\x1f",
        side.as_bytes(),
    ] {
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    TradeId::new(format!("{hash:016x}"))
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

        TardisExchange::MexcFutures if instrument_type == &TardisInstrumentType::Perpetual => {
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
/// Amounts within 1e-9 of a step at `precision` (in units of that step) snap to the step
/// instead, so upstream floating-point artifacts such as `2.9999999999999996` keep their
/// intended size.
///
/// # Panics
///
/// Panics if `precision` exceeds 19, beyond which the tolerance is not representable.
#[must_use]
pub fn normalize_amount(amount: Decimal, precision: u8) -> Decimal {
    let precision = u32::from(precision);
    let rounded = amount.round_dp(precision);

    if (rounded - amount).abs() < Decimal::new(1, 9 + precision) {
        rounded
    } else {
        amount.trunc_with_scale(precision)
    }
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
pub fn parse_order_side(value: &str) -> Option<OrderSide> {
    match value {
        "bid" => Some(OrderSide::Buy),
        "ask" => Some(OrderSide::Sell),
        _ => None,
    }
}

/// Parses a Nautilus aggressor side from the given Tardis string `value`.
#[must_use]
pub fn parse_aggressor_side(value: &str) -> AggressorSide {
    match value {
        "buy" => AggressorSide::Buy,
        "sell" => AggressorSide::Sell,
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
            log::error!("Timestamp overflow: {value_us} microseconds exceeds maximum representable value");
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

    parse_canonical_bar_spec(step, aggregation)
        .with_context(|| format!("Invalid bar spec '{value}'"))
}

fn parse_canonical_bar_spec(
    step: usize,
    aggregation: BarAggregation,
) -> anyhow::Result<BarSpecification> {
    match aggregation {
        BarAggregation::Millisecond if step.is_multiple_of(1000) => {
            parse_canonical_bar_spec(step / 1000, BarAggregation::Second)
        }
        BarAggregation::Second if step.is_multiple_of(60) => {
            parse_canonical_bar_spec(step / 60, BarAggregation::Minute)
        }
        BarAggregation::Minute if step.is_multiple_of(60) => {
            parse_canonical_bar_spec(step / 60, BarAggregation::Hour)
        }
        BarAggregation::Hour if step.is_multiple_of(24) => {
            parse_canonical_bar_spec(step / 24, BarAggregation::Day)
        }
        _ => BarSpecification::new_checked(step, aggregation, PriceType::Last),
    }
}

/// Converts a Nautilus `BarSpecification` to the Tardis trade bar string convention.
///
/// # Errors
///
/// Returns an error if the bar aggregation kind is unsupported.
pub fn bar_spec_to_tardis_trade_bar_string(bar_spec: &BarSpecification) -> anyhow::Result<String> {
    match bar_spec.aggregation {
        BarAggregation::Hour => {
            let minutes = bar_spec
                .step
                .get()
                .checked_mul(60)
                .context("bar specification step overflow")?;
            return Ok(format!("trade_bar_{minutes}m"));
        }
        BarAggregation::Day => {
            let minutes = bar_spec
                .step
                .get()
                .checked_mul(1440)
                .context("bar specification step overflow")?;
            return Ok(format!("trade_bar_{minutes}m"));
        }
        _ => {}
    }

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
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[case("1e400")]
    #[case("-1e400")]
    fn test_deserialize_numeric_out_of_range(#[case] input: &str) {
        let required = deserialize_f64_or_string(&mut serde_json::Deserializer::from_str(input));
        let optional =
            deserialize_opt_f64_or_string(&mut serde_json::Deserializer::from_str(input));

        assert!(required.is_err());
        assert!(optional.is_err());
    }

    #[rstest]
    #[case(serde_json::json!(1.25), 1.25)]
    #[case(serde_json::json!(25), 25.0)]
    #[case(serde_json::json!(-3), -3.0)]
    #[case(serde_json::json!("2.5e2"), 250.0)]
    fn test_deserialize_numeric_fields(#[case] input: serde_json::Value, #[case] expected: f64) {
        let json = input.to_string();
        let required = deserialize_f64_or_string(&mut serde_json::Deserializer::from_str(&json));
        let optional =
            deserialize_opt_f64_or_string(&mut serde_json::Deserializer::from_str(&json));

        assert_eq!(required.unwrap(), expected);
        assert_eq!(optional.unwrap(), Some(expected));
    }

    #[rstest]
    fn test_deserialize_numeric_null() {
        let required = deserialize_f64_or_string(serde_json::Value::Null);
        let optional = deserialize_opt_f64_or_string(serde_json::Value::Null);

        assert!(required.is_err());
        assert_eq!(optional.unwrap(), None);
    }

    #[rstest]
    #[case(serde_json::json!({"number": "1.5"}))]
    #[case(serde_json::json!(true))]
    #[case(serde_json::json!("invalid"))]
    fn test_deserialize_numeric_invalid(#[case] input: serde_json::Value) {
        assert!(deserialize_f64_or_string(input.clone()).is_err());
        assert!(deserialize_opt_f64_or_string(input).is_err());
    }

    #[rstest]
    #[case(0.0, 0, false)]
    #[case(0.0004, 3, false)]
    #[case(0.0005, 3, true)]
    #[case(123.456, 3, true)]
    #[case(1.0, 255, false)]
    fn test_validate_non_zero_amount(
        #[case] amount: f64,
        #[case] precision: u8,
        #[case] expected: bool,
    ) {
        assert_eq!(
            validate_non_zero_amount(amount, precision).is_ok(),
            expected
        );
    }

    #[rstest]
    #[case(TardisExchange::Binance, "ETHUSDT", "ETHUSDT.BINANCE")]
    #[case(TardisExchange::Bitmex, "XBTUSD", "XBTUSD.BITMEX")]
    #[case(TardisExchange::Bybit, "BTCUSDT", "BTCUSDT.BYBIT")]
    #[case(TardisExchange::OkexFutures, "BTC-USD-200313", "BTC-USD-200313.OKEX")]
    #[case(TardisExchange::HuobiDmLinearSwap, "FOO-BAR", "FOO-BAR.HUOBI")]
    #[case(TardisExchange::Mexc, "BTCUSDT", "BTCUSDT.MEXC")]
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
    #[case(
        TardisExchange::MexcFutures,
        "BTC_USDT",
        TardisInstrumentType::Perpetual,
        None,
        "BTC_USDT-PERP.MEXC"
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
    #[case(dec!(0.00001), 4, dec!(0))]
    #[case(dec!(1.2345), 3, dec!(1.234))]
    #[case(dec!(1.2345), 2, dec!(1.23))]
    #[case(dec!(-1.2345), 3, dec!(-1.234))]
    #[case(dec!(123.456), 0, dec!(123))]
    #[case(dec!(0.1), 1, dec!(0.1))]
    #[case(dec!(1.123456789), 9, dec!(1.123456789))]
    #[case(dec!(0), 8, dec!(0))]
    #[case(dec!(-0.1), 1, dec!(-0.1))]
    #[case(dec!(2.9999999999999996), 0, dec!(3))]
    #[case(dec!(0.29999999999999998), 1, dec!(0.3))]
    #[case(dec!(2.999999998), 0, dec!(2))]
    #[case(dec!(100000000.123456789), 8, dec!(100000000.12345678))]
    fn test_normalize_amount(
        #[case] amount: Decimal,
        #[case] precision: u8,
        #[case] expected: Decimal,
    ) {
        let result = normalize_amount(amount, precision);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("bid", Some(OrderSide::Buy))]
    #[case("ask", Some(OrderSide::Sell))]
    #[case("unknown", None)]
    #[case("", None)]
    #[case("random", None)]
    fn test_parse_order_side(#[case] input: &str, #[case] expected: Option<OrderSide>) {
        assert_eq!(parse_order_side(input), expected);
    }

    #[rstest]
    #[case("buy", AggressorSide::Buy)]
    #[case("sell", AggressorSide::Sell)]
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
    #[case("trade_bar_10000ms", 10, BarAggregation::Second)]
    #[case("trade_bar_5m", 5, BarAggregation::Minute)]
    #[case("trade_bar_60m", 1, BarAggregation::Hour)]
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
        BarSpecification::new(1, BarAggregation::Hour, PriceType::Last),
        "trade_bar_60m"
    )]
    #[case(
        BarSpecification::new(2, BarAggregation::Day, PriceType::Last),
        "trade_bar_2880m"
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

    #[rstest]
    fn test_derive_trade_id_is_deterministic_and_16_hex_chars() {
        let first = derive_trade_id(Ustr::from("XBTUSD"), 1_700_000_000, "7996", "50", "sell");
        let second = derive_trade_id(Ustr::from("XBTUSD"), 1_700_000_000, "7996", "50", "sell");
        assert_eq!(first, second);
        assert_eq!(first.as_str().len(), 16);
    }

    #[rstest]
    #[case::symbol_changed(derive_trade_id(Ustr::from("ETHUSD"), 1, "1", "1", "buy"))]
    #[case::ts_changed(derive_trade_id(Ustr::from("XBTUSD"), 2, "1", "1", "buy"))]
    #[case::price_changed(derive_trade_id(Ustr::from("XBTUSD"), 1, "2", "1", "buy"))]
    #[case::amount_changed(derive_trade_id(Ustr::from("XBTUSD"), 1, "1", "2", "buy"))]
    #[case::side_changed(derive_trade_id(Ustr::from("XBTUSD"), 1, "1", "1", "sell"))]
    fn test_derive_trade_id_each_field_affects_output(#[case] altered: TradeId) {
        let baseline = derive_trade_id(Ustr::from("XBTUSD"), 1, "1", "1", "buy");
        assert_ne!(baseline, altered);
    }

    #[rstest]
    fn test_derive_trade_id_field_delimiter_prevents_collision() {
        // Without the 0x1f delimiter, concatenated bytes for these two inputs
        // would collapse into the same stream.
        let a = derive_trade_id(Ustr::from("A"), 1, "0", "0", "buy");
        let b = derive_trade_id(Ustr::from("A\x00"), 256, "0", "0", "buy");
        assert_ne!(a, b);
    }

    #[rstest]
    fn test_derive_trade_id_matches_csv_and_machine_text() {
        let price = dec!(7996.50).normalize().to_string();
        let amount = dec!(0.000000150).normalize().to_string();

        let machine = derive_trade_id(Ustr::from("XBTUSD"), 1, &price, &amount, "buy");
        let csv = derive_trade_id(
            Ustr::from("XBTUSD"),
            1,
            &7996.5_f64.to_string(),
            &0.000_000_15_f64.to_string(),
            "buy",
        );

        assert_eq!(machine, csv);
    }
}
