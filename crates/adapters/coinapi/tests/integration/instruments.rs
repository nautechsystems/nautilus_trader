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

use nautilus_coinapi::{
    common::symbol::{CoinApiSymbolType, SymbolError, VenueSymbol},
    http::{
        models::{Symbol, SymbolRawKvp},
        parse::{CoinApiInstrument, InstrumentError, parse_instrument, parse_instruments},
    },
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use ustr::Ustr;

const TS_INIT: UnixNanos = UnixNanos::new(1_790_000_000_000_000_000);

#[derive(Debug, PartialEq)]
struct Definition {
    kind: &'static str,
    asset_class: AssetClass,
    symbol_id: String,
    instrument_id: String,
    raw_symbol: String,
    base: String,
    quote: String,
    settlement: String,
    is_inverse: bool,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Quantity,
    activation: Option<UnixNanos>,
    expiration: Option<UnixNanos>,
}

fn definition(parsed: &CoinApiInstrument) -> Definition {
    let instrument = &parsed.instrument;

    let kind = match instrument {
        InstrumentAny::CurrencyPair(_) => "CurrencyPair",
        InstrumentAny::CryptoPerpetual(_) => "CryptoPerpetual",
        InstrumentAny::CryptoFuture(_) => "CryptoFuture",
        InstrumentAny::PerpetualContract(_) => "PerpetualContract",
        _ => "other",
    };

    assert_eq!(
        instrument.price_precision(),
        instrument.price_increment().precision
    );
    assert_eq!(
        instrument.size_precision(),
        instrument.size_increment().precision
    );
    assert_eq!(instrument.ts_event(), TS_INIT);
    assert_eq!(instrument.ts_init(), TS_INIT);

    Definition {
        kind,
        asset_class: instrument.asset_class(),
        symbol_id: parsed.symbol_id.to_string(),
        instrument_id: instrument.id().to_string(),
        raw_symbol: instrument.raw_symbol().to_string(),
        base: instrument
            .base_currency()
            .map_or_else(|| instrument.underlying().unwrap(), |base| base.code)
            .to_string(),
        quote: instrument.quote_currency().code.to_string(),
        settlement: instrument.settlement_currency().code.to_string(),
        is_inverse: instrument.is_inverse(),
        price_increment: instrument.price_increment(),
        size_increment: instrument.size_increment(),
        multiplier: instrument.multiplier(),
        activation: instrument.activation_ns(),
        expiration: instrument.expiration_ns(),
    }
}

fn nanos(timestamp: &str) -> UnixNanos {
    let timestamp: jiff::Timestamp = timestamp.parse().unwrap();
    UnixNanos::from(u64::try_from(timestamp.as_nanosecond()).unwrap())
}

fn load(fixture: &str) -> Vec<Symbol> {
    serde_json::from_str(fixture).unwrap()
}

fn parse_all(fixture: &str) -> Vec<Definition> {
    let (instruments, errors) = parse_instruments(&load(fixture), TS_INIT);
    assert_eq!(errors, vec![]);
    instruments.iter().map(definition).collect()
}

fn spot(
    symbol_id: &str,
    instrument_id: &str,
    raw_symbol: &str,
    quote: &str,
    price_increment: &str,
    size_increment: &str,
) -> Definition {
    Definition {
        kind: "CurrencyPair",
        asset_class: AssetClass::Cryptocurrency,
        symbol_id: symbol_id.to_string(),
        instrument_id: instrument_id.to_string(),
        raw_symbol: raw_symbol.to_string(),
        base: "BTC".to_string(),
        quote: quote.to_string(),
        settlement: quote.to_string(),
        is_inverse: false,
        price_increment: Price::from(price_increment),
        size_increment: Quantity::from(size_increment),
        multiplier: Quantity::from(1),
        activation: None,
        expiration: None,
    }
}

#[rstest]
fn spot_fixture_parses_complete_currency_pairs() {
    let actual = parse_all(include_str!("../../test_data/symbols_spot.json"));

    assert_eq!(
        actual,
        vec![
            spot(
                "BINANCE_SPOT_BTC_USDT",
                "BTCUSDT.BINANCE",
                "BTCUSDT",
                "USDT",
                "0.01",
                "0.00001"
            ),
            spot(
                "BYBITSPOT_SPOT_BTC_USDT",
                "BTCUSDT-SPOT.BYBIT",
                "BTCUSDT",
                "USDT",
                "0.1",
                "0.000001"
            ),
            spot(
                "OKEX_SPOT_BTC_USDT",
                "BTC-USDT.OKX",
                "BTC-USDT",
                "USDT",
                "0.1",
                "0.00000001"
            ),
            spot(
                "COINBASE_SPOT_BTC_USD",
                "BTC-USD.COINBASE",
                "BTC-USD",
                "USD",
                "0.01",
                "0.00000001"
            ),
            spot(
                "KRAKEN_SPOT_BTC_USD",
                "BTC/USD.KRAKEN",
                "XBT/USD",
                "USD",
                "0.1",
                "0.00000001"
            ),
        ]
    );
}

#[expect(clippy::too_many_arguments)]
fn derivative(
    kind: &'static str,
    symbol_id: &str,
    instrument_id: &str,
    raw_symbol: &str,
    quote: &str,
    is_inverse: bool,
    increments: (&str, &str, &str),
    dates: (Option<UnixNanos>, Option<UnixNanos>),
) -> Definition {
    Definition {
        kind,
        asset_class: AssetClass::Cryptocurrency,
        symbol_id: symbol_id.to_string(),
        instrument_id: instrument_id.to_string(),
        raw_symbol: raw_symbol.to_string(),
        base: "BTC".to_string(),
        quote: quote.to_string(),
        settlement: if is_inverse { "BTC" } else { quote }.to_string(),
        is_inverse,
        price_increment: Price::from(increments.0),
        size_increment: Quantity::from(increments.1),
        multiplier: Quantity::from(increments.2),
        activation: dates.0,
        expiration: dates.1,
    }
}

#[rstest]
fn perpetual_fixture_parses_complete_perpetuals() {
    let actual = parse_all(include_str!("../../test_data/symbols_perpetual.json"));

    let perp = |symbol_id, instrument_id, raw_symbol, quote, is_inverse, increments| {
        derivative(
            "CryptoPerpetual",
            symbol_id,
            instrument_id,
            raw_symbol,
            quote,
            is_inverse,
            increments,
            (None, None),
        )
    };

    assert_eq!(
        actual,
        vec![
            perp(
                "BINANCEFTS_PERP_BTC_USDT",
                "BTCUSDT-PERP.BINANCE",
                "BTCUSDT",
                "USDT",
                false,
                ("0.1", "0.001", "1")
            ),
            Definition {
                kind: "PerpetualContract",
                asset_class: AssetClass::Commodity,
                base: "XAU".to_string(),
                ..perp(
                    "BINANCEFTS_PERP_XAU_USDT",
                    "XAUUSDT-PERP.BINANCE",
                    "XAUUSDT",
                    "USDT",
                    false,
                    ("0.01", "0.001", "1")
                )
            },
            perp(
                "BYBIT_PERP_BTC_USDT",
                "BTCUSDT-LINEAR.BYBIT",
                "BTCUSDT",
                "USDT",
                false,
                ("0.1", "0.001", "1")
            ),
            perp(
                "BYBITINVERSE_PERP_BTC_USD",
                "BTCUSD-INVERSE.BYBIT",
                "BTCUSD",
                "USD",
                true,
                ("0.1", "1", "1")
            ),
            perp(
                "OKEX_PERP_BTC_USDT",
                "BTC-USDT-SWAP.OKX",
                "BTC-USDT-SWAP",
                "USDT",
                false,
                ("0.1", "0.01", "0.01")
            ),
            perp(
                "OKEX_PERP_BTC_USD",
                "BTC-USD-SWAP.OKX",
                "BTC-USD-SWAP",
                "USD",
                true,
                ("0.1", "0.1", "100")
            ),
            perp(
                "KRAKENFTS_PERP_BTC_USD",
                "PI_XBTUSD.KRAKEN",
                "PI_XBTUSD",
                "USD",
                true,
                ("0.1", "1", "1")
            ),
            perp(
                "KRAKENFTS_PERP_BTC_USD_22F0062",
                "PF_XBTUSD.KRAKEN",
                "PF_XBTUSD",
                "USD",
                false,
                ("1", "0.0001", "1")
            ),
        ]
    );
}

#[rstest]
fn futures_fixture_parses_complete_futures() {
    let actual = parse_all(include_str!("../../test_data/symbols_futures.json"));

    let future =
        |symbol_id, instrument_id, raw_symbol, quote, is_inverse, increments, start, end| {
            derivative(
                "CryptoFuture",
                symbol_id,
                instrument_id,
                raw_symbol,
                quote,
                is_inverse,
                increments,
                (Some(nanos(start)), Some(nanos(end))),
            )
        };

    assert_eq!(
        actual,
        vec![
            future(
                "BINANCEFTS_FTS_BTC_USDT_261225",
                "BTCUSDT_261225.BINANCE",
                "BTCUSDT_261225",
                "USDT",
                false,
                ("0.1", "0.001", "1"),
                "2026-06-26T00:00:00Z",
                "2026-12-25T08:00:00Z"
            ),
            future(
                "BYBIT_FTS_BTC_USDT_261030",
                "BTCUSDT-30OCT26-LINEAR.BYBIT",
                "BTCUSDT-30OCT26",
                "USDT",
                false,
                ("0.1", "0.001", "1"),
                "2026-08-07T00:00:00Z",
                "2026-10-30T08:00:00Z"
            ),
            future(
                "BYBITINVERSE_FTS_BTC_USD_261225",
                "BTCUSDZ26-INVERSE.BYBIT",
                "BTCUSDZ26",
                "USD",
                true,
                ("0.5", "1", "1"),
                "2026-06-12T00:00:00Z",
                "2026-12-25T08:00:00Z"
            ),
            future(
                "OKEX_FTS_BTC_USD_261225",
                "BTC-USD-261225.OKX",
                "BTC-USD-261225",
                "USD",
                true,
                ("0.1", "0.1", "100"),
                "2026-04-10T00:00:00Z",
                "2026-12-25T08:00:00Z"
            ),
        ]
    );
}

#[rstest]
fn unmapped_symbols_are_rejected_and_other_types_skipped() {
    let mut symbols = load(include_str!("../../test_data/symbols_rejected.json"));
    let mut option = load(include_str!("../../test_data/symbols_spot.json")).remove(0);
    option.symbol_type = Ustr::from("OPTION");
    symbols.push(option);

    let (instruments, errors) = parse_instruments(&symbols, TS_INIT);

    assert_eq!(instruments, vec![]);
    assert_eq!(
        errors,
        vec![
            InstrumentError::Symbol {
                symbol_id: Ustr::from("BINANCEFTSC_PERP_BTC_USD"),
                source: SymbolError::Unsupported {
                    exchange_id: Ustr::from("BINANCEFTSC"),
                    symbol_type: CoinApiSymbolType::Perpetual,
                },
            },
            InstrumentError::Symbol {
                symbol_id: Ustr::from("OKEX_FTS_BTC_USD_261225_2D57B63"),
                source: SymbolError::Unresolved {
                    exchange_id: Ustr::from("OKEX"),
                    symbol_type: CoinApiSymbolType::Futures,
                    symbol: Ustr::from("BTC-USD_UM-261225"),
                },
            },
            InstrumentError::Symbol {
                symbol_id: Ustr::from("KRAKENFTS_FTS_BTC_USD_260925"),
                source: SymbolError::Unsupported {
                    exchange_id: Ustr::from("KRAKENFTS"),
                    symbol_type: CoinApiSymbolType::Futures,
                },
            },
        ]
    );
    assert_eq!(
        errors[1].to_string(),
        "CoinAPI symbol OKEX_FTS_BTC_USD_261225_2D57B63: CoinAPI OKEX FUTURES symbol \
         BTC-USD_UM-261225 does not match its native form for the given assets and delivery date"
    );
}

#[rstest]
fn duplicate_native_ids_are_rejected_as_ambiguous() {
    let mut symbols = load(include_str!("../../test_data/symbols_perpetual.json"));
    let mut relisted = symbols[0].clone();
    relisted.symbol_id = Ustr::from("BINANCEFTS_PERP_BTC_USDT_2A1B3C4");
    symbols.push(relisted);

    let (instruments, errors) = parse_instruments(&symbols, TS_INIT);

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    assert_eq!(
        instruments
            .iter()
            .map(|parsed| parsed.symbol_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "BINANCEFTS_PERP_XAU_USDT",
            "BYBIT_PERP_BTC_USDT",
            "BYBITINVERSE_PERP_BTC_USD",
            "OKEX_PERP_BTC_USDT",
            "OKEX_PERP_BTC_USD",
            "KRAKENFTS_PERP_BTC_USD",
            "KRAKENFTS_PERP_BTC_USD_22F0062",
        ]
    );
    assert_eq!(
        errors,
        vec![
            InstrumentError::Symbol {
                symbol_id: Ustr::from("BINANCEFTS_PERP_BTC_USDT"),
                source: SymbolError::Ambiguous(instrument_id),
            },
            InstrumentError::Symbol {
                symbol_id: Ustr::from("BINANCEFTS_PERP_BTC_USDT_2A1B3C4"),
                source: SymbolError::Ambiguous(instrument_id),
            },
        ]
    );
}

#[rstest]
#[case::missing_contract_type(None, Some("COMMODITY"), "raw_kvp.contractType", "unsupported None")]
#[case::delivery_contract_type(
    Some("NEXT_QUARTER"),
    None,
    "raw_kvp.contractType",
    "unsupported Some(\"NEXT_QUARTER\")"
)]
#[case::unknown_underlying(
    Some("TRADIFI_PERPETUAL"),
    Some("INDEX"),
    "raw_kvp.underlyingType",
    "unsupported Some(\"INDEX\")"
)]
fn binance_perpetual_classification_fails_with_field(
    #[case] contract_type: Option<&str>,
    #[case] underlying_type: Option<&str>,
    #[case] field: &'static str,
    #[case] reason: &str,
) {
    let mut symbol = load(include_str!("../../test_data/symbols_perpetual.json")).remove(1);
    symbol.raw_kvp = Some(SymbolRawKvp {
        contract_type: contract_type.map(Ustr::from),
        underlying_type: underlying_type.map(Ustr::from),
    });

    let result = parse_instrument(&symbol, TS_INIT);

    assert_eq!(
        result,
        Err(InstrumentError::Field {
            symbol_id: Ustr::from("BINANCEFTS_PERP_XAU_USDT"),
            field,
            reason: reason.to_owned(),
        })
    );
}

#[rstest]
fn binance_equity_perpetual_is_equity_contract() {
    let mut symbol = load(include_str!("../../test_data/symbols_perpetual.json")).remove(1);
    symbol.raw_kvp = Some(SymbolRawKvp {
        contract_type: Some(Ustr::from("TRADIFI_PERPETUAL")),
        underlying_type: Some(Ustr::from("CN_EQUITY")),
    });

    let parsed = parse_instrument(&symbol, TS_INIT).unwrap();

    assert!(matches!(
        parsed.instrument,
        InstrumentAny::PerpetualContract(_)
    ));
    assert_eq!(parsed.instrument.asset_class(), AssetClass::Equity);
}

fn futures_symbol() -> Symbol {
    load(include_str!("../../test_data/symbols_futures.json")).remove(0)
}

#[rstest]
#[case::missing_exchange_symbol(|s: &mut Symbol| s.symbol_id_exchange = None, "symbol_id_exchange", "missing")]
#[case::empty_base(|s: &mut Symbol| s.asset_id_base = Some(Ustr::from("")), "asset_id_base", "missing")]
#[case::missing_quote(|s: &mut Symbol| s.asset_id_quote = None, "asset_id_quote", "missing")]
#[case::missing_base_exchange(|s: &mut Symbol| s.asset_id_base_exchange = None, "asset_id_base_exchange", "missing")]
#[case::missing_quote_exchange(|s: &mut Symbol| s.asset_id_quote_exchange = None, "asset_id_quote_exchange", "missing")]
#[case::unknown_type(|s: &mut Symbol| s.symbol_type = Ustr::from("INDEX"), "symbol_type", "unsupported INDEX")]
#[case::missing_delivery(|s: &mut Symbol| s.future_delivery_time = None, "future_delivery_time", "missing")]
#[case::missing_price(|s: &mut Symbol| s.price_precision = None, "price_precision", "missing")]
#[case::zero_price(|s: &mut Symbol| s.price_precision = Some(Decimal::ZERO), "price_precision", "0 is not positive")]
#[case::negative_size(|s: &mut Symbol| s.size_precision = Some(Decimal::NEGATIVE_ONE), "size_precision", "-1 is not positive")]
#[case::missing_contract_unit(|s: &mut Symbol| s.future_contract_unit = None, "future_contract_unit", "missing")]
#[case::missing_start(|s: &mut Symbol| s.data_start = None, "data_start", "missing")]
fn invalid_definitions_fail_with_field(
    #[case] mutate: fn(&mut Symbol),
    #[case] field: &'static str,
    #[case] reason: &str,
) {
    let mut symbol = futures_symbol();
    mutate(&mut symbol);

    let result = parse_instrument(&symbol, TS_INIT);

    assert_eq!(
        result,
        Err(InstrumentError::Field {
            symbol_id: Ustr::from("BINANCEFTS_FTS_BTC_USDT_261225"),
            field,
            reason: reason.to_owned(),
        })
    );
}

#[rstest]
#[case::blank_base(|s: &mut Symbol| s.asset_id_base = Some(Ustr::from("  ")), "asset_id_base")]
#[case::excess_price_scale(|s: &mut Symbol| s.price_precision = Some(Decimal::new(1, 28)), "price_precision")]
#[case::malformed_start(|s: &mut Symbol| s.data_start = Some("2026-13-01".into()), "data_start")]
#[case::expiry_before_activation(|s: &mut Symbol| s.data_start = Some("2027-01-01".into()), "data_start")]
fn invalid_values_fail_with_field(#[case] mutate: fn(&mut Symbol), #[case] field: &str) {
    let mut symbol = futures_symbol();
    mutate(&mut symbol);

    let result = parse_instrument(&symbol, TS_INIT);

    assert!(
        matches!(&result, Err(InstrumentError::Field { symbol_id, field: actual, .. })
            if *symbol_id == "BINANCEFTS_FTS_BTC_USDT_261225" && *actual == field),
        "{result:?}"
    );
}

#[rstest]
fn spot_symbol_type_mismatch_is_unresolved() {
    let mut symbol = futures_symbol();
    symbol.symbol_type = Ustr::from("SPOT");

    let result = parse_instrument(&symbol, TS_INIT);

    assert_eq!(
        result,
        Err(InstrumentError::Symbol {
            symbol_id: Ustr::from("BINANCEFTS_FTS_BTC_USDT_261225"),
            source: SymbolError::Unsupported {
                exchange_id: Ustr::from("BINANCEFTS"),
                symbol_type: CoinApiSymbolType::Spot,
            },
        })
    );
}

#[rstest]
fn native_id_reverses_to_coinapi_exchange_symbol() {
    let parsed = parse_instrument(&futures_symbol(), TS_INIT).unwrap();

    let reverse = VenueSymbol::from_native(parsed.instrument.id()).unwrap();

    assert_eq!(
        parsed.instrument.id(),
        InstrumentId::from("BTCUSDT_261225.BINANCE")
    );
    assert_eq!(reverse.exchange_id, "BINANCEFTS");
    assert_eq!(reverse.symbol_type, CoinApiSymbolType::Futures);
    assert_eq!(reverse.symbol, "BTCUSDT_261225");
}
