// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{datetime::NANOSECONDS_IN_MICROSECOND, nanos::UnixNanos};
use nautilus_model::{
    data::bar::BarSpecification,
    enums::{AggressorSide, BarAggregation, BookAction, OptionKind, OrderSide, PriceType},
    identifiers::{InstrumentId, Symbol},
};
use serde::{Deserialize, Deserializer};

use super::enums::{Exchange, InstrumentType, OptionType};

pub fn deserialize_uppercase<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(|s| s.to_uppercase())
}

#[must_use]
#[inline]
pub fn normalize_symbol_str(
    symbol: &str,
    exchange: &Exchange,
    instrument_type: InstrumentType,
    is_inverse: Option<bool>,
) -> String {
    let mut symbol = symbol.to_string();

    match exchange {
        Exchange::BinanceFutures
        | Exchange::BinanceUs
        | Exchange::BinanceDex
        | Exchange::BinanceJersey
            if instrument_type == InstrumentType::Perpetual =>
        {
            symbol.push_str("-PERP");
        }

        Exchange::Bybit => match instrument_type {
            InstrumentType::Spot => {
                symbol.push_str("-SPOT");
            }
            InstrumentType::Perpetual if !is_inverse.unwrap_or(false) => {
                symbol.push_str("-LINEAR");
            }
            InstrumentType::Future if !is_inverse.unwrap_or(false) => {
                symbol.push_str("-LINEAR");
            }
            InstrumentType::Perpetual if is_inverse == Some(true) => {
                symbol.push_str("-INVERSE");
            }
            InstrumentType::Future if is_inverse == Some(true) => {
                symbol.push_str("-INVERSE");
            }
            InstrumentType::Option => {
                symbol.push_str("-OPTION");
            }
            _ => {}
        },

        Exchange::Dydx if instrument_type == InstrumentType::Perpetual => {
            symbol.push_str("-PERP");
        }

        _ => {}
    }

    symbol
}

/// Parses a Nautilus instrument ID from the given Tardis `exchange` and `symbol` values.
#[must_use]
pub fn parse_instrument_id(exchange: &Exchange, symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from_str_unchecked(symbol), exchange.as_venue())
}

/// Parses a Nautilus instrument ID with a normalized symbol from the given Tardis `exchange` and `symbol` values.
#[must_use]
pub fn normalize_instrument_id(
    exchange: &Exchange,
    symbol: &str,
    instrument_type: InstrumentType,
    is_inverse: Option<bool>,
) -> InstrumentId {
    let symbol = normalize_symbol_str(symbol, exchange, instrument_type, is_inverse);
    parse_instrument_id(exchange, &symbol)
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
pub const fn parse_option_kind(value: OptionType) -> OptionKind {
    match value {
        OptionType::Call => OptionKind::Call,
        OptionType::Put => OptionKind::Put,
    }
}

/// Parses a UNIX nanoseconds timestamp from the given Tardis microseconds `value_us`.
#[must_use]
pub fn parse_timestamp(value_us: u64) -> UnixNanos {
    UnixNanos::from(value_us * NANOSECONDS_IN_MICROSECOND)
}

/// Parses a Nautilus book action inferred from the given Tardis values.
#[must_use]
pub fn parse_book_action(is_snapshot: bool, amount: f64) -> BookAction {
    if is_snapshot {
        BookAction::Add
    } else if amount == 0.0 {
        BookAction::Delete
    } else {
        BookAction::Update
    }
}

/// Parses a Nautilus bar specification from the given Tardis string `value`.
///
/// The [`PriceType`] is always `LAST` for Tardis trade bars.
#[must_use]
pub fn parse_bar_spec(value: &str) -> BarSpecification {
    let parts: Vec<&str> = value.split('_').collect();
    let last_part = parts.last().expect("Invalid bar spec");
    let split_idx = last_part
        .chars()
        .position(|c| !c.is_ascii_digit())
        .expect("Invalid bar spec");

    let (step_str, suffix) = last_part.split_at(split_idx);
    let step: usize = step_str.parse().expect("Invalid step");

    let aggregation = match suffix {
        "ms" => BarAggregation::Millisecond,
        "s" => BarAggregation::Second,
        "m" => BarAggregation::Minute,
        "ticks" => BarAggregation::Tick,
        "vol" => BarAggregation::Volume,
        _ => panic!("Unsupported bar aggregation type"),
    };

    BarSpecification {
        step,
        aggregation,
        price_type: PriceType::Last,
    }
}

/// Converts a Nautilus `BarSpecification` to the Tardis trade bar string convention.
#[must_use]
pub fn bar_spec_to_tardis_trade_bar_string(bar_spec: &BarSpecification) -> String {
    let suffix = match bar_spec.aggregation {
        BarAggregation::Millisecond => "ms",
        BarAggregation::Second => "s",
        BarAggregation::Minute => "m",
        BarAggregation::Tick => "ticks",
        BarAggregation::Volume => "vol",
        _ => panic!("Unsupported bar aggregation type {}", bar_spec.aggregation),
    };
    format!("trade_bar_{}{}", bar_spec.step, suffix)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::enums::AggressorSide;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Exchange::Binance, "ETHUSDT", "ETHUSDT.BINANCE")]
    #[case(Exchange::Bitmex, "XBTUSD", "XBTUSD.BITMEX")]
    #[case(Exchange::Bybit, "BTCUSDT", "BTCUSDT.BYBIT")]
    #[case(Exchange::OkexFutures, "BTC-USD-200313", "BTC-USD-200313.OKEX")]
    #[case(Exchange::HuobiDmLinearSwap, "FOO-BAR", "FOO-BAR.HUOBI")]
    fn test_parse_instrument_id(
        #[case] exchange: Exchange,
        #[case] symbol: &str,
        #[case] expected: &str,
    ) {
        let instrument_id = parse_instrument_id(&exchange, symbol);
        let expected_instrument_id = InstrumentId::from_str(expected).unwrap();
        assert_eq!(instrument_id, expected_instrument_id);
    }

    #[rstest]
    #[case(
        Exchange::Binance,
        "SOLUSDT",
        InstrumentType::Spot,
        None,
        "SOLUSDT.BINANCE"
    )]
    #[case(
        Exchange::BinanceFutures,
        "SOLUSDT",
        InstrumentType::Perpetual,
        None,
        "SOLUSDT-PERP.BINANCE"
    )]
    #[case(
        Exchange::Bybit,
        "BTCUSDT",
        InstrumentType::Spot,
        None,
        "BTCUSDT-SPOT.BYBIT"
    )]
    #[case(
        Exchange::Bybit,
        "BTCUSDT",
        InstrumentType::Perpetual,
        None,
        "BTCUSDT-LINEAR.BYBIT"
    )]
    #[case(
        Exchange::Bybit,
        "BTCUSDT",
        InstrumentType::Perpetual,
        Some(true),
        "BTCUSDT-INVERSE.BYBIT"
    )]
    #[case(
        Exchange::Dydx,
        "BTC-USD",
        InstrumentType::Perpetual,
        None,
        "BTC-USD-PERP.DYDX"
    )]
    fn test_normalize_instrument_id(
        #[case] exchange: Exchange,
        #[case] symbol: &str,
        #[case] instrument_type: InstrumentType,
        #[case] is_inverse: Option<bool>,
        #[case] expected: &str,
    ) {
        let instrument_id = normalize_instrument_id(&exchange, symbol, instrument_type, is_inverse);
        let expected_instrument_id = InstrumentId::from_str(expected).unwrap();
        assert_eq!(instrument_id, expected_instrument_id);
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
        let spec = parse_bar_spec(value);
        assert_eq!(spec.step, expected_step);
        assert_eq!(spec.aggregation, expected_aggregation);
        assert_eq!(spec.price_type, PriceType::Last);
    }

    #[rstest]
    #[case("trade_bar_10unknown")]
    #[should_panic(expected = "Unsupported bar aggregation type")]
    fn test_parse_bar_spec_invalid_suffix(#[case] value: &str) {
        let _ = parse_bar_spec(value);
    }

    #[rstest]
    #[case("")]
    #[should_panic(expected = "Invalid bar spec")]
    fn test_parse_bar_spec_empty(#[case] value: &str) {
        let _ = parse_bar_spec(value);
    }

    #[rstest]
    #[case("trade_bar_notanumberms")]
    #[should_panic(expected = "Invalid step")]
    fn test_parse_bar_spec_invalid_step(#[case] value: &str) {
        let _ = parse_bar_spec(value);
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
        assert_eq!(bar_spec_to_tardis_trade_bar_string(&bar_spec), expected);
    }
}
