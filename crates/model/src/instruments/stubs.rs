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

use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use rstest::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{
    CryptoOption, betting::BettingInstrument, binary_option::BinaryOption,
    futures_spread::FuturesSpread, option_spread::OptionSpread, synthetic::SyntheticInstrument,
};
use crate::{
    enums::{AssetClass, OptionKind},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{
        CryptoFuture, CryptoPerpetual, CurrencyPair, Equity, FuturesContract, OptionContract,
    },
    types::{Currency, Money, Price, Quantity},
};

impl Default for SyntheticInstrument {
    /// Creates a new default [`SyntheticInstrument`] instance for testing.
    fn default() -> Self {
        let btc_binance = InstrumentId::from("BTC.BINANCE");
        let ltc_binance = InstrumentId::from("LTC.BINANCE");
        let formula = "(BTC.BINANCE + LTC.BINANCE) / 2.0".to_string();
        Self::new(
            Symbol::new("BTC-LTC"),
            2,
            vec![btc_binance, ltc_binance],
            formula,
            0.into(),
            0.into(),
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// CryptoFuture
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn crypto_future_btcusdt(
    #[default(2)] price_precision: u8,
    #[default(6)] size_precision: u8,
    #[default(Price::from("0.01"))] price_increment: Price,
    #[default(Quantity::from("0.000001"))] size_increment: Quantity,
) -> CryptoFuture {
    let activation = Utc.with_ymd_and_hms(2014, 4, 8, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2014, 7, 8, 0, 0, 0).unwrap();
    CryptoFuture::new(
        InstrumentId::from("ETHUSDT-123.BINANCE"),
        Symbol::from("BTCUSDT"),
        Currency::from("BTC"),
        Currency::from("USDT"),
        Currency::from("USDT"),
        false,
        UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
        UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,
        None,
        Some(Quantity::from("9000.0")),
        Some(Quantity::from("0.000001")),
        None,
        Some(Money::new(10.00, Currency::from("USDT"))),
        Some(Price::from("1000000.00")),
        Some(Price::from("0.01")),
        None,
        None,
        None,
        None,
        0.into(),
        0.into(),
    )
}

#[fixture]
pub fn ethbtc_quanto(
    #[default(5)] price_precision: u8,
    #[default(3)] size_precision: u8,
    #[default(Price::from("0.00001"))] price_increment: Price,
    #[default(Quantity::from("0.001"))] size_increment: Quantity,
) -> CryptoFuture {
    let activation = Utc.with_ymd_and_hms(2014, 4, 8, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2014, 7, 8, 0, 0, 0).unwrap();
    CryptoFuture::new(
        InstrumentId::from("ETHBTC-123.BINANCE"),
        Symbol::from("ETHBTC"),
        Currency::from("ETH"),
        Currency::from("BTC"),
        Currency::from("BTC"),
        false,
        UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
        UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,
        None,
        Some(Quantity::from("9000.0")),
        Some(Quantity::from("0.001")),
        None,
        Some(Money::new(1.0, Currency::from("BTC"))),
        Some(Price::from("1.0")),
        Some(Price::from("0.00001")),
        None,
        None,
        None,
        None,
        0.into(),
        0.into(),
    )
}

////////////////////////////////////////////////////////////////////////////////
// CryptoPerpetual – BitMEX inverse (XBTUSD)
// ////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn xbtusd_inverse_perp(
    // One-decimal tick (0.5 USD) and integer contract size
    #[default(1)] price_precision: u8,
    #[default(0)] size_precision: u8,
    #[default(Price::from("0.5"))] price_increment: Price,
    #[default(Quantity::from("1"))] size_increment: Quantity,
) -> CryptoPerpetual {
    CryptoPerpetual::new(
        // BitMEX uses XBT for BTC; keep the “-PERP” suffix for clarity
        InstrumentId::from("XBTUSD-PERP.BITMEX"),
        Symbol::from("XBTUSD"),
        Currency::BTC(), // base
        Currency::USD(), // quote
        Currency::BTC(), // settlement (inverse)
        true,            // is_inverse
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,                              // lot_size
        Some(Quantity::from("1")),         // multiplier: 1 USD/contract
        None,                              // max_quantity
        None,                              // min_quantity
        Some(Money::from("10000000 USD")), // max_notional
        Some(Money::from("1 USD")),        // min_notional
        Some(Price::from("10000000")),     // max_price
        Some(Price::from("0.01")),         // min_price
        Some(dec!(0.01)),                  // margin_init
        Some(dec!(0.0035)),                // margin_maint
        Some(dec!(-0.00025)),              // maker_fee (rebate)
        Some(dec!(0.00075)),               // taker_fee
        UnixNanos::default(),              // ts_event
        UnixNanos::default(),              // ts_init
    )
}

////////////////////////////////////////////////////////////////////////////////
// CryptoOption
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn crypto_option_btc_deribit(
    #[default(3)] price_precision: u8,
    #[default(1)] size_precision: u8,
    #[default(Price::from("0.001"))] price_increment: Price,
    #[default(Quantity::from("0.1"))] size_increment: Quantity,
) -> CryptoOption {
    let activation = UnixNanos::from(1_671_696_002_000_000_000);
    let expiration = UnixNanos::from(1_673_596_800_000_000_000);
    CryptoOption::new(
        InstrumentId::from("BTC-13JAN23-16000-P.DERIBIT"),
        Symbol::from("BTC-13JAN23-16000-P"),
        Currency::from("BTC"),
        Currency::from("USD"),
        Currency::from("BTC"),
        false,
        OptionKind::Put,
        Price::from("16000.000"),
        activation,
        expiration,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        Some(Quantity::from(1)),
        Some(Quantity::from(1)),
        Some(Quantity::from("9000.0")),
        Some(Quantity::from("0.1")),
        None,
        Some(Money::new(10.00, Currency::from("USD"))),
        None,
        None,
        None,
        None,
        Some(dec!(0.0003)),
        Some(dec!(0.0003)),
        0.into(),
        0.into(),
    )
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
        None,
        None,
        Some(Quantity::from("10000.0")),
        Some(Quantity::from("0.001")),
        None,
        Some(Money::new(10.00, Currency::from("USDT"))),
        Some(Price::from("15000.00")),
        Some(Price::from("1.0")),
        Some(dec!(1.0)),
        Some(dec!(0.35)),
        Some(dec!(0.0002)),
        Some(dec!(0.0004)),
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        None,
        None,
        None,
        None,
        Some(Money::from("10000000 USD")),
        Some(Money::from("1 USD")),
        Some(Price::from("10000000")),
        Some(Price::from("0.01")),
        Some(dec!(0.01)),
        Some(dec!(0.0035)),
        Some(dec!(-0.00025)),
        Some(dec!(0.00075)),
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        None,
        None,
        None,
        None,
        None,
        None,
        Some(Price::from("10000000")),
        Some(Price::from("0.01")),
        Some(dec!(0.01)),
        Some(dec!(0.0035)),
        Some(dec!(-0.00025)),
        Some(dec!(0.00075)),
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        None,
        None,
        Some(Quantity::from("9000")),
        Some(Quantity::from("0.000001")),
        None,
        None,
        Some(Price::from("1000000")),
        Some(Price::from("0.01")),
        Some(dec!(0.001)),
        Some(dec!(0.001)),
        Some(dec!(0.001)),
        Some(dec!(0.001)),
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        None,
        None,
        Some(Quantity::from("9000")),
        Some(Quantity::from("0.00001")),
        None,
        None,
        Some(Price::from("1000000")),
        Some(Price::from("0.01")),
        Some(dec!(0.01)),
        Some(dec!(0.0035)),
        Some(dec!(0.0001)),
        Some(dec!(0.0001)),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

/// # Panics
///
/// Panics if `symbol` does not contain a '/' delimiter.
#[must_use]
pub fn default_fx_ccy(symbol: Symbol, venue: Option<Venue>) -> CurrencyPair {
    let target_venue = venue.unwrap_or(Venue::from("SIM"));
    let instrument_id = InstrumentId::new(symbol, target_venue);
    let base_currency = symbol.as_str().split('/').next().unwrap();
    let quote_currency = symbol.as_str().split('/').next_back().unwrap();
    let price_precision = if quote_currency == "JPY" { 3 } else { 5 };
    let price_increment = Price::new(1.0 / 10.0f64.powi(price_precision as i32), price_precision);
    CurrencyPair::new(
        instrument_id,
        symbol,
        Currency::from(base_currency),
        Currency::from(quote_currency),
        price_precision,
        0,
        price_increment,
        Quantity::from("1"),
        None,
        Some(Quantity::from("1000")),
        Some(Quantity::from("1000000")),
        Some(Quantity::from("100")),
        None,
        None,
        None,
        None,
        Some(dec!(0.03)),
        Some(dec!(0.03)),
        Some(dec!(0.00002)),
        Some(dec!(0.00002)),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

#[fixture]
pub fn audusd_sim() -> CurrencyPair {
    default_fx_ccy(Symbol::from("AUD/USD"), Some(Venue::from("SIM")))
}

#[fixture]
pub fn gbpusd_sim() -> CurrencyPair {
    default_fx_ccy(Symbol::from("GBP/USD"), Some(Venue::from("SIM")))
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
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

////////////////////////////////////////////////////////////////////////////////
// FuturesContract
////////////////////////////////////////////////////////////////////////////////

/// # Panics
///
/// Panics if constructing the activation or expiration timestamp fails,
/// e.g., if the provided dates are invalid or timestamp conversion returns `None`.
pub fn futures_contract_es(
    activation: Option<UnixNanos>,
    expiration: Option<UnixNanos>,
) -> FuturesContract {
    let activation = activation.unwrap_or(UnixNanos::from(
        Utc.with_ymd_and_hms(2021, 9, 10, 0, 0, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    ));
    let expiration = expiration.unwrap_or(UnixNanos::from(
        Utc.with_ymd_and_hms(2021, 12, 17, 0, 0, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    ));
    FuturesContract::new(
        InstrumentId::from("ESZ21.GLBX"),
        Symbol::from("ESZ21"),
        AssetClass::Index,
        Some(Ustr::from("XCME")),
        Ustr::from("ES"),
        activation,
        expiration,
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
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
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
        UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
        UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
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
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

////////////////////////////////////////////////////////////////////////////////
// OptionContract
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn option_contract_appl() -> OptionContract {
    let activation = Utc.with_ymd_and_hms(2021, 9, 17, 0, 0, 0).unwrap();
    let expiration = Utc.with_ymd_and_hms(2021, 12, 17, 0, 0, 0).unwrap();
    OptionContract::new(
        InstrumentId::from("AAPL211217C00150000.OPRA"),
        Symbol::from("AAPL211217C00150000"),
        AssetClass::Equity,
        Some(Ustr::from("GMNI")),
        Ustr::from("AAPL"),
        OptionKind::Call,
        Price::from("149.0"),
        Currency::USD(),
        UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
        UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
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
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

////////////////////////////////////////////////////////////////////////////////
// OptionSpread
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn option_spread() -> OptionSpread {
    let activation = Utc.with_ymd_and_hms(2023, 11, 6, 20, 54, 7).unwrap();
    let expiration = Utc.with_ymd_and_hms(2024, 2, 23, 22, 59, 0).unwrap();
    OptionSpread::new(
        InstrumentId::from("UD:U$: GN 2534559.GLBX"),
        Symbol::from("UD:U$: GN 2534559"),
        AssetClass::FX,
        Some(Ustr::from("XCME")),
        Ustr::from("SR3"),
        Ustr::from("GN"),
        UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
        UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
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
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

////////////////////////////////////////////////////////////////////////////////
// BettingInstrument
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn betting() -> BettingInstrument {
    let raw_symbol = Symbol::new("1-123456789");
    let id = InstrumentId::from(format!("{raw_symbol}.BETFAIR").as_str());
    let event_type_id = 6423;
    let event_type_name = Ustr::from("American Football");
    let competition_id = 12282733;
    let competition_name = Ustr::from("NFL");
    let event_id = 29678534;
    let event_name = Ustr::from("NFL");
    let event_country_code = Ustr::from("GB");
    let event_open_date = UnixNanos::from(
        Utc.with_ymd_and_hms(2022, 2, 7, 23, 30, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    );
    let betting_type = Ustr::from("ODDS");
    let market_id = Ustr::from("1-123456789");
    let market_name = Ustr::from("AFC Conference Winner");
    let market_type = Ustr::from("SPECIAL");
    let market_start_time = UnixNanos::from(
        Utc.with_ymd_and_hms(2022, 2, 7, 23, 30, 0)
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap() as u64,
    );
    let selection_id = 50214;
    let selection_name = Ustr::from("Kansas City Chiefs");
    let selection_handicap = 0.0;
    let currency = Currency::GBP();
    let price_increment = Price::from("0.01");
    let size_increment = Quantity::from("0.01");
    let max_quantity = Some(Quantity::from("1000"));
    let min_quantity = Some(Quantity::from("1"));
    let max_notional = Some(Money::from("10000 GBP"));
    let min_notional = Some(Money::from("10 GBP"));
    let max_price = Some(Price::from("100.00"));
    let min_price = Some(Price::from("1.00"));
    let margin_init = Some(Decimal::from(1));
    let margin_maint = Some(Decimal::from(1));
    let maker_fee = Some(Decimal::from(0));
    let taker_fee = Some(Decimal::from(0));
    let ts_event = UnixNanos::default();
    let ts_init = UnixNanos::default();

    BettingInstrument::new(
        id,
        raw_symbol,
        event_type_id,
        event_type_name,
        competition_id,
        competition_name,
        event_id,
        event_name,
        event_country_code,
        event_open_date,
        betting_type,
        market_id,
        market_name,
        market_type,
        market_start_time,
        selection_id,
        selection_name,
        selection_handicap,
        currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        ts_event,
        ts_init,
    )
}

////////////////////////////////////////////////////////////////////////////////
// BinaryOption
////////////////////////////////////////////////////////////////////////////////

#[fixture]
pub fn binary_option() -> BinaryOption {
    let raw_symbol = Symbol::new(
        "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905",
    );
    let activation = Utc.with_ymd_and_hms(2023, 11, 6, 20, 54, 7).unwrap();
    let expiration = Utc.with_ymd_and_hms(2024, 2, 23, 22, 59, 0).unwrap();
    let price_increment = Price::from("0.001");
    let size_increment = Quantity::from("0.01");
    BinaryOption::new(
        InstrumentId::from("{raw_symbol}.POLYMARKET"),
        raw_symbol,
        AssetClass::Alternative,
        Currency::USDC(),
        UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
        UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}
