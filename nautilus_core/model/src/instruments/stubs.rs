// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::{TimeZone, Utc};
use nautilus_core::time::UnixNanos;
use rstest::fixture;
use rust_decimal::Decimal;

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
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Decimal::from_str("0.001").unwrap(),
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
        0,
        Price::from("0.01"),
        Quantity::from("0.001"),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Decimal::from_str("0.001").unwrap(),
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
        Decimal::from_str("0.01").unwrap(),
        Decimal::from_str("0.0035").unwrap(),
        Decimal::from_str("-0.00025").unwrap(),
        Decimal::from_str("0.00075").unwrap(),
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
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        None,
        Some(Quantity::from("9000")),
        Some(Quantity::from("0.000001")),
        Some(Price::from("1000000")),
        Some(Price::from("0.01")),
        0,
        0,
    )
    .unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// Equity
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn equity_aapl() -> Equity {
    Equity::new(
        InstrumentId::from("AAPL.NASDAQ"),
        Symbol::from("AAPL"),
        Some(String::from("US0378331005")),
        Currency::from("USD"),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Some(Quantity::from(1)),
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
pub fn futures_contract_es() -> FuturesContract {
    let activation = Utc.with_ymd_and_hms(2021, 4, 8, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2021, 7, 8, 0, 0, 0).unwrap();
    FuturesContract::new(
        InstrumentId::new(Symbol::from("ESZ21"), Venue::from("CME")),
        Symbol::from("ESZ21"),
        AssetClass::Index,
        String::from("ES"),
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        Currency::USD(),
        2,
        Price::from("0.01"),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Quantity::from("1.0"),
        Some(Quantity::from("1.0")),
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
        String::from("AAPL"),
        OptionKind::Call,
        activation.timestamp_nanos_opt().unwrap() as UnixNanos,
        expiration.timestamp_nanos_opt().unwrap() as UnixNanos,
        Price::from("149.0"),
        Currency::USD(),
        2,
        Price::from("0.01"),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.0").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Decimal::from_str("0.001").unwrap(),
        Some(Quantity::from("1.0")),
        None,
        None,
        None,
        None,
        0,
        0,
    )
    .unwrap()
}
