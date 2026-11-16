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

use std::{fmt::Display, str::FromStr};

use nautilus_core::correctness::CorrectnessError;
use nautilus_model::identifiers::{
    AccountId, ActorId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, OrderListId,
    PositionId, StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
};
use rstest::rstest;

const NON_ASCII: &str = "é";

#[rstest]
#[case::account(parse::<AccountId>, "BROKER-123456", string_errors(), Err(non_ascii_error()))]
#[case::actor(parse::<ActorId>, "ACTOR-ALPHA", string_errors(), Err(non_ascii_error()))]
#[case::client(parse::<ClientId>, "CLIENT-7", string_errors(), Err(non_ascii_error()))]
#[case::client_order(parse::<ClientOrderId>, "ORDER-123", string_errors(), Err(non_ascii_error()))]
#[case::component(parse::<ComponentId>, "COMPONENT-8", string_errors(), Err(non_ascii_error()))]
#[case::exec_algorithm(parse::<ExecAlgorithmId>, "TWAP-9", string_errors(), Err(non_ascii_error()))]
#[case::order_list(parse::<OrderListId>, "LIST-10", string_errors(), Err(non_ascii_error()))]
#[case::position(parse::<PositionId>, "POSITION-11", string_errors(), Ok(NON_ASCII.to_owned()))]
#[case::strategy(parse::<StrategyId>, "STRATEGY-012", string_errors(), Err(non_ascii_error()))]
#[case::symbol(parse::<Symbol>, "BTC-USDT", string_errors(), Ok(NON_ASCII.to_owned()))]
#[case::trader(parse::<TraderId>, "TRADER-014", string_errors(), Err(non_ascii_error()))]
#[case::venue(parse::<Venue>, "XNAS", string_errors(), Err(non_ascii_error()))]
#[case::venue_order(parse::<VenueOrderId>, "VENUE-ORDER-15", string_errors(), Err(non_ascii_error()))]
fn from_str_preserves_values_and_validation(
    #[case] parse: Parser,
    #[case] value: &str,
    #[case] expected: [CorrectnessError; 2],
    #[case] expected_non_ascii: Result<String, CorrectnessError>,
) {
    let [expected_empty, expected_whitespace] = expected;
    let valid = parse(value);
    let empty = parse("");
    let whitespace = parse(" ");
    let non_ascii = parse(NON_ASCII);

    assert_eq!(valid, Ok(value.to_owned()));
    assert_eq!(empty, Err(expected_empty));
    assert_eq!(whitespace, Err(expected_whitespace));
    assert_eq!(non_ascii, expected_non_ascii);
}

#[rstest]
#[case::account(parse::<AccountId>, "BROKER")]
#[case::trader(parse::<TraderId>, "TRADER")]
#[case::strategy(parse::<StrategyId>, "STRATEGY")]
fn from_str_requires_identifier_hyphens(#[case] parse: Parser, #[case] value: &str) {
    let error = parse(value).unwrap_err();

    assert_eq!(
        error,
        CorrectnessError::MissingSubstring {
            param: "value".to_owned(),
            pattern: "-".to_owned(),
            value: value.to_owned()
        }
    );
}

#[rstest]
#[case::account_issuer(parse::<AccountId>, "-123", "`value` issuer part (before '-') cannot be empty")]
#[case::account_number(parse::<AccountId>, "BROKER-", "`value` account part (after '-') cannot be empty")]
#[case::trader_name(parse::<TraderId>, "-123", "`value` name part (before '-') cannot be empty")]
#[case::trader_tag(parse::<TraderId>, "TRADER-", "`value` tag part (after '-') cannot be empty")]
#[case::strategy_name(parse::<StrategyId>, "-123", "`value` name part (before '-') cannot be empty")]
#[case::strategy_tag(parse::<StrategyId>, "STRATEGY-", "`value` tag part (after '-') cannot be empty")]
fn from_str_rejects_empty_identifier_components(
    #[case] parse: Parser,
    #[case] value: &str,
    #[case] message: &str,
) {
    let error = parse(value).unwrap_err();

    assert_eq!(
        error,
        CorrectnessError::PredicateViolation {
            message: message.to_owned()
        }
    );
}

#[rstest]
fn from_str_accepts_external_strategy() {
    let parsed = "EXTERNAL".parse::<StrategyId>();

    assert_eq!(parsed, Ok(StrategyId::external()));
}

#[rstest]
fn from_str_preserves_trade_id_validation() {
    let value = "TRADE-13";
    let valid = parse::<TradeId>(value);
    let empty = parse::<TradeId>("");
    let whitespace = parse::<TradeId>(" ");
    let non_ascii = parse::<TradeId>(NON_ASCII);

    assert_eq!(valid, Ok(value.to_owned()));
    assert_eq!(
        empty,
        Err(CorrectnessError::PredicateViolation {
            message: "String is empty".to_owned()
        })
    );
    assert_eq!(
        whitespace,
        Err(CorrectnessError::PredicateViolation {
            message: "String contains only whitespace".to_owned()
        })
    );
    assert_eq!(
        non_ascii,
        Err(CorrectnessError::PredicateViolation {
            message: "String contains non-ASCII character".to_owned()
        })
    );
}

#[rstest]
fn from_str_preserves_trade_id_length_limit() {
    let maximum = "123456789012345678901234567890123456";
    let excessive = format!("{maximum}7");
    let valid = maximum.parse::<TradeId>();
    let error = excessive.parse::<TradeId>().unwrap_err();

    assert_eq!(valid.map(|id| id.to_string()), Ok(maximum.to_owned()));
    assert_eq!(
        error,
        CorrectnessError::PredicateViolation {
            message: "String exceeds maximum length of 36 characters, was 37".to_owned()
        }
    );
}

#[rstest]
fn from_str_rejects_trade_id_nul() {
    let error = "trade\0id".parse::<TradeId>().unwrap_err();

    assert_eq!(
        error,
        CorrectnessError::PredicateViolation {
            message: "String contains interior NUL byte".to_owned()
        }
    );
}

fn string_errors() -> [CorrectnessError; 2] {
    [
        CorrectnessError::EmptyString {
            param: "value".to_owned(),
        },
        CorrectnessError::WhitespaceString {
            param: "value".to_owned(),
        },
    ]
}

fn non_ascii_error() -> CorrectnessError {
    CorrectnessError::NonAsciiString {
        param: "value".to_owned(),
        value: NON_ASCII.to_owned(),
    }
}

type Parser = fn(&str) -> Result<String, CorrectnessError>;

fn parse<T: FromStr<Err = CorrectnessError> + Display>(
    value: &str,
) -> Result<String, CorrectnessError> {
    value.parse::<T>().map(|value| value.to_string())
}
