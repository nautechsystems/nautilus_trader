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

//! Cap'n Proto serialization integration tests for instrument types.

#![cfg(feature = "capnp")]

use nautilus_core::Params;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{
        CurrencyPair, InstrumentAny, SyntheticInstrument,
        stubs::{
            betting, binary_option, cfd_gold, commodity_gold, crypto_future_btcusdt,
            crypto_futures_spread_btc_deribit, crypto_option_btc_deribit,
            crypto_option_spread_btc_deribit, crypto_perpetual_ethusdt, currency_pair_btcusdt,
            equity_aapl, futures_contract_es, futures_spread_es, index_instrument_spx,
            option_contract_appl, option_spread, perpetual_contract_eurusd, tokenized_asset_aaplx,
        },
    },
    types::{Money, Price, Quantity, fixed::check_fixed_precision},
};
use nautilus_serialization::capnp::{
    FromCapnp, ToCapnp,
    instruments::{deserialize_instrument_any, serialize_instrument_any},
    instruments_capnp,
};
use rstest::rstest;
use serde::Serialize;
use ustr::Ustr;

fn assert_json_eq<T: Serialize>(expected: &T, actual: &T) {
    assert_eq!(
        serde_json::to_value(expected).unwrap(),
        serde_json::to_value(actual).unwrap(),
    );
}

fn roundtrip_instrument_any(instrument: &InstrumentAny) -> InstrumentAny {
    let bytes = serialize_instrument_any(instrument).unwrap();
    deserialize_instrument_any(&bytes).unwrap()
}

macro_rules! assert_typed_roundtrip {
    ($value:expr, $builder:path, $reader:path, $ty:ty) => {{
        let value: $ty = $value;
        let mut message = capnp::message::Builder::new_default();
        {
            let builder = message.init_root::<$builder>();
            value.to_capnp(builder);
        }
        let reader = message
            .get_root_as_reader::<$reader>()
            .expect("capnp reader");
        let decoded = <$ty>::from_capnp(reader).expect("capnp decode");
        assert_json_eq(&value, &decoded);
    }};
}

#[rstest]
#[case::betting(InstrumentAny::Betting(betting()))]
#[case::binary_option(InstrumentAny::BinaryOption(binary_option()))]
#[case::cfd(InstrumentAny::Cfd(cfd_gold()))]
#[case::commodity(InstrumentAny::Commodity(commodity_gold()))]
#[case::crypto_future(InstrumentAny::CryptoFuture(crypto_future_btcusdt(
    2,
    6,
    Price::from("0.01"),
    Quantity::from("0.000001"),
)))]
#[case::crypto_futures_spread(InstrumentAny::CryptoFuturesSpread(
    crypto_futures_spread_btc_deribit()
))]
#[case::crypto_option(InstrumentAny::CryptoOption(crypto_option_btc_deribit(
    3,
    1,
    Price::from("0.001"),
    Quantity::from("0.1"),
)))]
#[case::crypto_option_spread(InstrumentAny::CryptoOptionSpread(crypto_option_spread_btc_deribit()))]
#[case::crypto_perpetual(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))]
#[case::currency_pair(InstrumentAny::CurrencyPair(currency_pair_btcusdt()))]
#[case::equity(InstrumentAny::Equity(equity_aapl()))]
#[case::futures_contract(InstrumentAny::FuturesContract(futures_contract_es(None, None)))]
#[case::futures_spread(InstrumentAny::FuturesSpread(futures_spread_es()))]
#[case::index_instrument(InstrumentAny::IndexInstrument(index_instrument_spx()))]
#[case::option_contract(InstrumentAny::OptionContract(option_contract_appl()))]
#[case::option_spread(InstrumentAny::OptionSpread(option_spread()))]
#[case::perpetual_contract(InstrumentAny::PerpetualContract(perpetual_contract_eurusd()))]
#[case::tokenized_asset(InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()))]
fn test_instrument_any_roundtrip(#[case] instrument: InstrumentAny) {
    let decoded = roundtrip_instrument_any(&instrument);
    assert_eq!(
        std::mem::discriminant(&decoded),
        std::mem::discriminant(&instrument)
    );
    assert_json_eq(&instrument, &decoded);
}

#[rstest]
fn test_currency_pair_optional_absence_roundtrip() {
    let mut pair = currency_pair_btcusdt();
    pair.lot_size = None;
    pair.max_quantity = None;
    pair.min_quantity = None;
    pair.max_notional = None;
    pair.min_notional = None;
    pair.max_price = None;
    pair.min_price = None;
    pair.tick_scheme = None;
    pair.info = None;

    assert_typed_roundtrip!(
        pair,
        instruments_capnp::currency_pair::Builder,
        instruments_capnp::currency_pair::Reader,
        CurrencyPair
    );
}

#[rstest]
fn test_currency_pair_info_and_bounds_roundtrip() {
    let mut info = Params::new();
    info.insert("venue_symbol".to_string(), serde_json::json!("BTCUSDT"));
    info.insert("gamma".to_string(), serde_json::json!("0.123456789"));

    let mut pair = currency_pair_btcusdt();
    pair.lot_size = Some(Quantity::from("0.001"));
    pair.max_quantity = Some(Quantity::from("1000"));
    pair.min_quantity = Some(Quantity::from("0.001"));
    pair.max_notional = Some(Money::from("1000000 USDT"));
    pair.min_notional = Some(Money::from("10 USDT"));
    pair.max_price = Some(Price::from("1000000.00"));
    pair.min_price = Some(Price::from("0.01"));
    pair.tick_scheme = Some(Ustr::from("CRYPTO_2DECIMAL"));
    pair.info = Some(info);

    assert_typed_roundtrip!(
        pair,
        instruments_capnp::currency_pair::Builder,
        instruments_capnp::currency_pair::Reader,
        CurrencyPair
    );
}

#[rstest]
fn test_synthetic_instrument_roundtrip() {
    let instrument = SyntheticInstrument::default();
    assert_typed_roundtrip!(
        instrument,
        instruments_capnp::synthetic_instrument::Builder,
        instruments_capnp::synthetic_instrument::Reader,
        SyntheticInstrument
    );
}

#[rstest]
fn test_synthetic_instrument_preserves_stored_id_and_increment() {
    let mut instrument = SyntheticInstrument::default();
    instrument.id = InstrumentId::from("BTC-LTC.BINANCE");
    instrument.price_increment = Price::from("0.05");

    assert_typed_roundtrip!(
        instrument,
        instruments_capnp::synthetic_instrument::Builder,
        instruments_capnp::synthetic_instrument::Reader,
        SyntheticInstrument
    );
}

#[rstest]
fn test_invalid_nested_price_precision_returns_error() {
    let pair = currency_pair_btcusdt();
    let mut message = capnp::message::Builder::new_default();
    {
        let mut builder = message.init_root::<instruments_capnp::currency_pair::Builder>();
        pair.to_capnp(builder.reborrow());
        builder
            .reborrow()
            .get_price_increment()
            .unwrap()
            .set_precision(u8::MAX);
    }

    let reader = message
        .get_root_as_reader::<instruments_capnp::currency_pair::Reader>()
        .unwrap();
    let error = CurrencyPair::from_capnp(reader).unwrap_err();
    let expected_error = check_fixed_precision(u8::MAX).unwrap_err();

    assert_eq!(error.to_string(), expected_error.to_string());
}
