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

use std::str::FromStr;

use nautilus_core::{datetime::NANOSECONDS_IN_MICROSECOND, nanos::UnixNanos};
use nautilus_model::{
    enums::{AggressorSide, BookAction, OrderSide},
    identifiers::InstrumentId,
};

/// Parse an instrument ID from the given venue and symbol values.
pub fn parse_instrument_id(exchange: &str, symbol: &str) -> InstrumentId {
    let venue = exchange.split('-').next().unwrap_or(exchange);
    InstrumentId::from_str(&format!("{}.{}", symbol, venue))
        .expect("Failed to parse `instrument_id`")
}

/// Parse an order side from the given string.
pub fn parse_order_side(value: &str) -> OrderSide {
    match value {
        "bid" => OrderSide::Buy,
        "ask" => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    }
}

/// Parse an aggressor side from the given string.
pub fn parse_aggressor_side(value: &str) -> AggressorSide {
    match value {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    }
}

/// Parse a Tardis timestamp in UNIX microseconds to UNIX nanoseconds.
pub fn parse_timestamp(value: u64) -> UnixNanos {
    UnixNanos::from(value * NANOSECONDS_IN_MICROSECOND)
}

/// Parse book action inferred from the given values.
pub fn parse_book_action(is_snapshot: bool, amount: f64) -> BookAction {
    if is_snapshot {
        BookAction::Add
    } else if amount == 0.0 {
        BookAction::Delete
    } else {
        BookAction::Update
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::enums::AggressorSide;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("okex-futures", "BTC-USD-200313", "BTC-USD-200313.okex")]
    #[case("binance", "ETH-USDT", "ETH-USDT.binance")]
    #[case("bitmex-perp", "XBTUSD", "XBTUSD.bitmex")]
    #[case("exchange-with-hyphen", "FOO-BAR", "FOO-BAR.exchange")]
    fn test_parse_instrument_id(
        #[case] exchange: &str,
        #[case] symbol: &str,
        #[case] expected: &str,
    ) {
        let instrument_id = parse_instrument_id(exchange, symbol);
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
}
