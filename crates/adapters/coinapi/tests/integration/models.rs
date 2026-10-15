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

use nautilus_coinapi::http::models::{
    Book, BookLevel, Exchange, Ohlcv, Quote, Symbol, SymbolRawKvp, Trade,
};
use rstest::rstest;
use rust_decimal::Decimal;

fn decimal(text: &str) -> Decimal {
    Decimal::from_str_exact(text).unwrap()
}

#[rstest]
fn exchanges_fixture_preserves_every_modeled_field() {
    let actual: Vec<Exchange> =
        serde_json::from_str(include_str!("../../test_data/exchanges.json")).unwrap();
    assert_eq!(
        actual,
        vec![Exchange {
            exchange_id: "COINBASE".into(),
            name: "Coinbase Pro (GDAX)".into(),
            website: Some("https://pro.coinbase.com/".into()),
            data_start: None,
            data_end: None,
            data_symbols_count: Some(914),
            volume_1hrs_usd: Some(decimal("69441223.44")),
            volume_1day_usd: Some(decimal("544657133.34")),
            volume_1mth_usd: Some(decimal("67196658542.20")),
        }]
    );
}

#[rstest]
fn symbols_fixture_preserves_every_modeled_field() {
    let actual: Vec<Symbol> =
        serde_json::from_str(include_str!("../../test_data/symbols.json")).unwrap();
    assert_eq!(
        actual,
        vec![
            Symbol {
                symbol_id: "COINBASE_SPOT_BTC_USDT".into(),
                exchange_id: "COINBASE".into(),
                symbol_type: "SPOT".into(),
                asset_id_base: Some("BTC".into()),
                asset_id_quote: Some("USDT".into()),
                asset_id_unit: None,
                symbol_id_exchange: Some("BTC-USDT".into()),
                asset_id_base_exchange: Some("BTC".into()),
                asset_id_quote_exchange: Some("USDT".into()),
                future_contract_unit_asset: None,
                data_start: Some("2021-05-04".into()),
                data_end: Some("2026-09-23".into()),
                future_delivery_time: None,
                data_quote_start: Some("2021-05-04T00:00:00.0000000Z".parse().unwrap()),
                data_quote_end: Some("2026-09-23T23:00:00.0000000Z".parse().unwrap()),
                data_trade_start: Some("2021-05-04T00:00:00.0000000Z".parse().unwrap()),
                data_trade_end: Some("2026-09-23T23:00:00.0000000Z".parse().unwrap()),
                data_orderbook_start: Some("2021-05-04T00:00:00.0000000Z".parse().unwrap()),
                data_orderbook_end: Some("2026-09-23T23:00:00.0000000Z".parse().unwrap()),
                future_contract_unit: None,
                price_precision: Some(decimal("0.01000000000000000000")),
                size_precision: Some(decimal("0.00000001000000000000")),
                price: Some(decimal("83632.76")),
                volume_1hrs: Some(decimal("3.22275323")),
                volume_1hrs_usd: Some(decimal("269439.86")),
                volume_1day: Some(decimal("8.4595482")),
                volume_1day_usd: Some(decimal("707264.67")),
                volume_1mth: Some(decimal("1274.56542302")),
                volume_1mth_usd: Some(decimal("106560665.82")),
                symbol_id_int: Some(5950967),
                raw_kvp: Some(SymbolRawKvp {
                    contract_type: None,
                    underlying_type: None,
                }),
            },
            Symbol {
                symbol_id: "COINBASE_SPOT_BTC_USD".into(),
                exchange_id: "COINBASE".into(),
                symbol_type: "SPOT".into(),
                asset_id_base: Some("BTC".into()),
                asset_id_quote: Some("USD".into()),
                asset_id_unit: None,
                symbol_id_exchange: Some("BTC-USD".into()),
                asset_id_base_exchange: Some("BTC".into()),
                asset_id_quote_exchange: Some("USD".into()),
                future_contract_unit_asset: None,
                data_start: Some("2015-01-14".into()),
                data_end: Some("2026-09-23".into()),
                future_delivery_time: None,
                data_quote_start: Some("2015-05-17T00:00:00.0000000Z".parse().unwrap()),
                data_quote_end: Some("2026-09-23T23:00:00.0000000Z".parse().unwrap()),
                data_trade_start: Some("2015-01-14T00:00:00.0000000Z".parse().unwrap()),
                data_trade_end: Some("2026-09-23T23:00:00.0000000Z".parse().unwrap()),
                data_orderbook_start: Some("2015-05-17T00:00:00.0000000Z".parse().unwrap()),
                data_orderbook_end: Some("2026-09-23T23:00:00.0000000Z".parse().unwrap()),
                future_contract_unit: None,
                price_precision: Some(decimal("0.01000000000000000000")),
                size_precision: Some(decimal("0.00000001000000000000")),
                price: Some(decimal("83614.995")),
                volume_1hrs: Some(decimal("358.66382585")),
                volume_1hrs_usd: Some(decimal("29989674.01")),
                volume_1day: Some(decimal("1613.42485967")),
                volume_1day_usd: Some(decimal("134906511.57")),
                volume_1mth: Some(decimal("203924.06855079")),
                volume_1mth_usd: Some(decimal("17051109972.25")),
                symbol_id_int: Some(27576),
                raw_kvp: Some(SymbolRawKvp {
                    contract_type: None,
                    underlying_type: None,
                }),
            }
        ]
    );
}

#[rstest]
fn book_fixture_preserves_every_modeled_field() {
    let actual: Book = serde_json::from_str(include_str!("../../test_data/book.json")).unwrap();
    assert_eq!(
        actual,
        Book {
            symbol_id: "COINBASE_SPOT_BTC_USD".into(),
            time_exchange: "2026-09-24T09:10:36.0854790Z".parse().unwrap(),
            time_coinapi: "2026-09-24T09:10:36.1734713Z".parse().unwrap(),
            asks: vec![BookLevel {
                id: None,
                price: decimal("83556.6"),
                size: decimal("0.01675362"),
            }],
            bids: vec![BookLevel {
                id: None,
                price: decimal("83556.59"),
                size: decimal("0.02515277"),
            }],
        }
    );
}

#[rstest]
fn book_l3_fixture_preserves_every_modeled_field() {
    let actual: Book = serde_json::from_str(include_str!("../../test_data/book_l3.json")).unwrap();
    assert_eq!(
        actual,
        Book {
            symbol_id: "COINBASE_SPOT_BTC_USD".into(),
            time_exchange: "2026-09-24T09:10:41.9950960Z".parse().unwrap(),
            time_coinapi: "2026-09-24T09:10:42.0847793Z".parse().unwrap(),
            asks: vec![
                BookLevel {
                    id: Some("6269c54e-ff4e-429b-9e86-a598a4f91da8".into()),
                    price: decimal("83553.46"),
                    size: decimal("0.00611078"),
                },
                BookLevel {
                    id: Some("e8320711-8843-4eb6-b173-90ce76be94dd".into()),
                    price: decimal("83553.46"),
                    size: decimal("0.00658862"),
                },
                BookLevel {
                    id: Some("1eacde51-4b1e-4573-b0c2-691b13d24ab8".into()),
                    price: decimal("83553.46"),
                    size: decimal("0.00360328"),
                }
            ],
            bids: vec![
                BookLevel {
                    id: Some("d3c1d60b-3ac9-4275-9232-48a3c25c4a7b".into()),
                    price: decimal("83553.45"),
                    size: decimal("0.09532659"),
                },
                BookLevel {
                    id: Some("7b355f41-4aee-4058-8c4f-550077378203".into()),
                    price: decimal("83553.45"),
                    size: decimal("0.01"),
                },
                BookLevel {
                    id: Some("6de6192e-6785-4187-8b58-6b7b8d8dee0f".into()),
                    price: decimal("83553.45"),
                    size: decimal("0.00008868"),
                },
                BookLevel {
                    id: Some("4fdda8d4-e533-4809-a60a-1982ceed79f1".into()),
                    price: decimal("83553.45"),
                    size: decimal("0.05421852"),
                }
            ],
        }
    );
}

#[rstest]
fn quotes_fixture_preserves_every_modeled_field() {
    let actual: Vec<Quote> =
        serde_json::from_str(include_str!("../../test_data/quotes.json")).unwrap();
    assert_eq!(
        actual,
        vec![Quote {
            symbol_id: Some("COINBASE_SPOT_BTC_USD".into()),
            time_exchange: "2026-09-23T09:09:00.0822750Z".parse().unwrap(),
            time_coinapi: "2026-09-23T09:09:00.2385208Z".parse().unwrap(),
            ask_price: decimal("85988.49"),
            ask_size: decimal("0.00075591"),
            bid_price: decimal("85988.48"),
            bid_size: decimal("0.05799828"),
        }]
    );
}

#[rstest]
fn trades_fixture_preserves_every_modeled_field() {
    let actual: Vec<Trade> =
        serde_json::from_str(include_str!("../../test_data/trades.json")).unwrap();
    assert_eq!(
        actual,
        vec![Trade {
            symbol_id: Some("COINBASE_SPOT_BTC_USD".into()),
            time_exchange: "2026-09-23T09:09:00.0822750Z".parse().unwrap(),
            time_coinapi: "2026-09-23T09:09:00.2385208Z".parse().unwrap(),
            uuid: "f84a79ad-ea86-4527-b1f1-e9d8bd7ed004".into(),
            price: decimal("85988.49"),
            size: decimal("0.01"),
            taker_side: "BUY".into(),
            id_trade: Some("1097218181".into()),
        }]
    );
}

#[rstest]
fn trades_precision_fixture_preserves_every_modeled_field() {
    let actual: Vec<Trade> =
        serde_json::from_str(include_str!("../../test_data/trades_precision.json")).unwrap();
    assert_eq!(
        actual,
        vec![Trade {
            symbol_id: Some("TEST_SPOT_BTC_USD".into()),
            time_exchange: "2026-01-01T00:00:01.1234567Z".parse().unwrap(),
            time_coinapi: "2026-01-01T00:00:01.1234568Z".parse().unwrap(),
            uuid: "trade-1".into(),
            price: decimal("123.12345678901234567890123456"),
            size: decimal("0.00001234"),
            taker_side: "BUY".into(),
            id_trade: Some("venue-1".into()),
        }]
    );
}

#[rstest]
fn ohlcv_fixture_preserves_every_modeled_field() {
    let actual: Vec<Ohlcv> =
        serde_json::from_str(include_str!("../../test_data/ohlcv.json")).unwrap();
    assert_eq!(
        actual,
        vec![Ohlcv {
            time_period_start: "2026-09-23T09:09:00.0000000Z".parse().unwrap(),
            time_period_end: "2026-09-23T09:10:00.0000000Z".parse().unwrap(),
            time_open: "2026-09-23T09:09:00.0822750Z".parse().unwrap(),
            time_close: "2026-09-23T09:09:59.9983600Z".parse().unwrap(),
            price_open: decimal("85988.49"),
            price_high: decimal("86007.23"),
            price_low: decimal("85986.91"),
            price_close: decimal("85998.695"),
            volume_traded: decimal("0.9707556800000001"),
            trades_count: 261,
        }]
    );
}

#[rstest]
#[case("null")]
#[case("true")]
#[case("0.12345678901234567890123456789")]
#[case("0.12345678901234567890123456789e0")]
#[case("1e-29")]
fn invalid_or_inexact_financial_values_fail(#[case] value: &str) {
    let body = include_str!("../../test_data/trades_precision.json")
        .replace("123.12345678901234567890123456", value);
    assert!(serde_json::from_str::<Vec<Trade>>(&body).is_err());
}
