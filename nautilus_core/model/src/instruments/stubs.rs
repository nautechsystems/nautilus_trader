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

use chrono::{TimeZone, Utc};
use nautilus_core::time::UnixNanos;
use rstest::fixture;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{futures_spread::FuturesSpread, options_spread::OptionsSpread};
use crate::{
    enums::{AssetClass, OptionKind},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    instruments::{
        crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair, equity::Equity, futures_contract::FuturesContract,
        options_contract::OptionsContract,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

////////////////////////////////////////////////////////////////////////////////
// CryptoFuture
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn crypto_future_btcusdt() -> CryptoFuture {
    let activation = Utc.with_ymd_and_hms(2014, 4, 8, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2014, 7, 8, 0, 0, 0).unwrap();
    CryptoFuture::new(
        InstrumentId::from("ETHUSDT-123.BINANCE"),
        Symbol::from("BTCUSDT"),
        Currency::from("BTC"),
        Currency::from("USDT"),
        Currency::from("USDT"),
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        2,
        6,
        Price::from("0.01"),
        Quantity::from("0.000001"),
        dec!(0),
        dec!(0),
        dec!(0),
        dec!(0),
        None,
        Some(Quantity::from("9000.0")),
        Some(Quantity::from("0.000001")),
        None,
        Some(Money::new(10.00, Currency::from("USDT")).unwrap()),
        Some(Price::from("1000000.00")),
        Some(Price::from("0.01")),
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// CryptoPerpetual
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn crypto_perpetual_ethusdt() -> CryptoPerpetual {
    CryptoPerpetual::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Symbol::from("ETHUSDT"),
        Currency::from("ETH"),
        Currency::from("USDT"),
        Currency::from("USDT"),
        false,
        2,
        3,
        Price::from("0.01"),
        Quantity::from("0.001"),
        dec!(0.0002),
        dec!(0.0004),
        dec!(1.0),
        dec!(0.35),
        None,
        Some(Quantity::from("10000.0")),
        Some(Quantity::from("0.001")),
        None,
        Some(Money::new(10.00, Currency::from("USDT")).unwrap()),
        Some(Price::from("15000.00")),
        Some(Price::from("1.0")),
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn xbtusd_bitmex() -> CryptoPerpetual {
    CryptoPerpetual::new(
        InstrumentId::from("BTCUSDT.BITMEX"),
        Symbol::from("XBTUSD"),
        Currency::BTC(),
        Currency::USD(),
        Currency::BTC(),
        true,
        1,
        0,
        Price::from("0.5"),
        Quantity::from("1"),
        dec!(-0.00025),
        dec!(0.00075),
        dec!(0.01),
        dec!(0.0035),
        None,
        None,
        None,
        Some(Money::from("10000000 USD")),
        Some(Money::from("1 USD")),
        Some(Price::from("10000000")),
        Some(Price::from("0.01")),
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn ethusdt_bitmex() -> CryptoPerpetual {
    CryptoPerpetual::new(
        InstrumentId::from("ETHUSD.BITMEX"),
        Symbol::from("ETHUSD"),
        Currency::ETH(),
        Currency::USD(),
        Currency::ETH(),
        true,
        2,
        0,
        Price::from("0.05"),
        Quantity::from("1"),
        dec!(-0.00025),
        dec!(0.00075),
        dec!(0.01),
        dec!(0.0035),
        None,
        None,
        None,
        None,
        None,
        Some(Price::from("10000000")),
        Some(Price::from("0.01")),
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// CurrencyPair
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn currency_pair_btcusdt() -> CurrencyPair {
    CurrencyPair::new(
        InstrumentId::from("BTCUSDT.BINANCE"),
        Symbol::from("BTCUSDT"),
        Currency::from("BTC"),
        Currency::from("USDT"),
        2,
        6,
        Price::from("0.01"),
        Quantity::from("0.000001"),
        dec!(0.001),
        dec!(0.001),
        dec!(0.001),
        dec!(0.001),
        None,
        Some(Quantity::from("9000")),
        Some(Quantity::from("0.000001")),
        None,
        None,
        Some(Price::from("1000000")),
        Some(Price::from("0.01")),
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn currency_pair_ethusdt() -> CurrencyPair {
    CurrencyPair::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        Symbol::from("ETHUSDT"),
        Currency::from("ETH"),
        Currency::from("USDT"),
        2,
        5,
        Price::from("0.01"),
        Quantity::from("0.00001"),
        dec!(0.0001),
        dec!(0.0001),
        dec!(0.01),
        dec!(0.0035),
        None,
        Some(Quantity::from("9000")),
        Some(Quantity::from("0.00001")),
        None,
        None,
        Some(Price::from("1000000")),
        Some(Price::from("0.01")),
        0,
        0,
    )
    .unwrap()
}

#[must_use]
pub fn default_fx_ccy(symbol: Symbol, venue: Option<Venue>) -> CurrencyPair {
    let target_venue = venue.unwrap_or(Venue::from("SIM"));
    let instrument_id = InstrumentId::new(symbol, target_venue);
    let base_currency = symbol.value.split('/').next().unwrap();
    let quote_currency = symbol.value.split('/').last().unwrap();
    let price_precision = if quote_currency == "JPY" { 3 } else { 5 };
    let price_increment = Price::new(1.0 / 10.0f64, price_precision).unwrap();
    CurrencyPair::new(
        instrument_id,
        symbol,
        Currency::from(base_currency),
        Currency::from(quote_currency),
        price_precision,
        0,
        price_increment,
        Quantity::from("1"),
        dec!(0.00002),
        dec!(0.00002),
        dec!(0.03),
        dec!(0.03),
        Some(Quantity::from("1000")),
        Some(Quantity::from("1000000")),
        Some(Quantity::from("100")),
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}
#[fixture]
pub fn audusd_sim() -> CurrencyPair {
    default_fx_ccy(Symbol::from("AUD/USD"), Some(Venue::from("SIM")))
}

#[fixture]
pub fn usdjpy_idealpro() -> CurrencyPair {
    default_fx_ccy(Symbol::from("USD/JPY"), Some(Venue::from("IDEALPRO")))
}

////////////////////////////////////////////////////////////////////////////////
// Equity
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn equity_aapl() -> Equity {
    Equity::new(
        InstrumentId::from("AAPL.XNAS"),
        Symbol::from("AAPL"),
        Some(Ustr::from("US0378331005")),
        Currency::from("USD"),
        2,
        Price::from("0.01"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// FuturesContract
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn futures_contract_es() -> FuturesContract {
    let activation = Utc.with_ymd_and_hms(2021, 4, 8, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2021, 7, 8, 0, 0, 0).unwrap();
    FuturesContract::new(
        InstrumentId::from("ESZ1.GLBX"),
        Symbol::from("ESZ1"),
        AssetClass::Index,
        Some(Ustr::from("XCME")),
        Ustr::from("ES"),
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        Currency::USD(),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// FuturesSpread
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn futures_spread_es() -> FuturesSpread {
    let activation = Utc.with_ymd_and_hms(2022, 6, 21, 13, 30, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2024, 6, 21, 13, 30, 0).unwrap();
    FuturesSpread::new(
        InstrumentId::from("ESM4-ESU4.GLBX"),
        Symbol::from("ESM4-ESU4"),
        AssetClass::Index,
        Some(Ustr::from("XCME")),
        Ustr::from("ES"),
        Ustr::from("EQ"),
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        Currency::USD(),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// OptionsContract
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn options_contract_appl() -> OptionsContract {
    let activation = Utc.with_ymd_and_hms(2021, 9, 17, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2021, 12, 17, 0, 0, 0).unwrap();
    OptionsContract::new(
        InstrumentId::from("AAPL211217C00150000.OPRA"),
        Symbol::from("AAPL211217C00150000"),
        AssetClass::Equity,
        Some(Ustr::from("GMNI")), // Nasdaq GEMX
        Ustr::from("AAPL"),
        OptionKind::Call,
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        Price::from("149.0"),
        Currency::USD(),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// OptionsSpread
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn options_spread() -> OptionsSpread {
    let activation = Utc.with_ymd_and_hms(2023, 11, 6, 20, 54, 7).unwrap();
    let expiration = Utc.with_ymd_and_hms(2024, 2, 23, 22, 59, 0).unwrap();
    OptionsSpread::new(
        InstrumentId::from("UD:U$: GN 2534559.GLBX"),
        Symbol::from("UD:U$: GN 2534559"),
        AssetClass::FX,
        Some(Ustr::from("XCME")),
        Ustr::from("SR3"), // British Pound futures (option on futures)
        Ustr::from("GN"),
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        Currency::USD(),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}
