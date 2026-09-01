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

use jiff::{Timestamp, civil::Date, tz::Offset};
use nautilus_core::UnixNanos;
use rstest::fixture;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{
    CryptoOption, betting::BettingInstrument, binary_option::BinaryOption, cfd::Cfd,
    commodity::Commodity, futures_spread::FuturesSpread, index_instrument::IndexInstrument,
    option_spread::OptionSpread, perpetual_contract::PerpetualContract,
    synthetic::SyntheticInstrument, tokenized_asset::TokenizedAsset,
};
use crate::{
    enums::{AssetClass, OptionKind},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{
        CryptoFuture, CryptoFuturesSpread, CryptoOptionSpread, CryptoPerpetual, CurrencyPair,
        Equity, FuturesContract, OptionContract,
    },
    types::{Currency, Money, Price, Quantity},
};

fn timestamp(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> Timestamp {
    let datetime = Date::new(year, month, day)
        .expect("valid date")
        .at(hour, minute, second, 0);
    Offset::UTC.to_timestamp(datetime).expect("valid timestamp")
}

impl Default for SyntheticInstrument {
    /// Creates a new default [`SyntheticInstrument`] instance for testing.
    fn default() -> Self {
        let btc_binance = InstrumentId::from("BTC.BINANCE");
        let ltc_binance = InstrumentId::from("LTC.BINANCE");
        let formula = "(BTC.BINANCE + LTC.BINANCE) / 2.0";
        Self::builder()
            .symbol(Symbol::new("BTC-LTC"))
            .price_precision(2)
            .components(vec![btc_binance, ltc_binance])
            .formula(formula)
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap()
    }
}

#[fixture]
pub fn crypto_future_btcusdt(
    #[default(2)] price_precision: u8,
    #[default(6)] size_precision: u8,
    #[default(Price::from("0.01"))] price_increment: Price,
    #[default(Quantity::from("0.000001"))] size_increment: Quantity,
) -> CryptoFuture {
    let activation = timestamp(2014, 4, 8, 0, 0, 0);
    let expiration = timestamp(2014, 7, 8, 0, 0, 0);
    CryptoFuture::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-123.BINANCE"))
        .raw_symbol(Symbol::from("BTCUSDT"))
        .underlying(Currency::from("BTC"))
        .quote_currency(Currency::from("USDT"))
        .settlement_currency(Currency::from("USDT"))
        .is_inverse(false)
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .max_quantity(Quantity::from("9000.0"))
        .min_quantity(Quantity::from("0.000001"))
        .min_notional(Money::new(10.00, Currency::from("USDT")))
        .max_price(Price::from("1000000.00"))
        .min_price(Price::from("0.01"))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

#[fixture]
pub fn ethbtc_quanto(
    #[default(5)] price_precision: u8,
    #[default(3)] size_precision: u8,
    #[default(Price::from("0.00001"))] price_increment: Price,
    #[default(Quantity::from("0.001"))] size_increment: Quantity,
) -> CryptoFuture {
    let activation = timestamp(2014, 4, 8, 0, 0, 0);
    let expiration = timestamp(2014, 7, 8, 0, 0, 0);
    CryptoFuture::builder()
        .instrument_id(InstrumentId::from("ETHBTC-123.BINANCE"))
        .raw_symbol(Symbol::from("ETHBTC"))
        .underlying(Currency::from("ETH"))
        .quote_currency(Currency::from("BTC"))
        .settlement_currency(Currency::from("USDT"))
        .is_inverse(false)
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .max_quantity(Quantity::from("9000.0"))
        .min_quantity(Quantity::from("0.001"))
        .min_notional(Money::new(1.0, Currency::from("USDT")))
        .max_price(Price::from("1.0"))
        .min_price(Price::from("0.00001"))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

#[fixture]
pub fn xbtusd_inverse_perp(
    // One-decimal tick (0.5 USD) and integer contract size
    #[default(1)] price_precision: u8,
    #[default(0)] size_precision: u8,
    #[default(Price::from("0.5"))] price_increment: Price,
    #[default(Quantity::from("1"))] size_increment: Quantity,
) -> CryptoPerpetual {
    CryptoPerpetual::builder()
        // BitMEX uses XBT for BTC; keep the "-PERP" suffix for clarity
        .instrument_id(InstrumentId::from("XBTUSD-PERP.BITMEX"))
        .raw_symbol(Symbol::from("XBTUSD"))
        // base
        .base_currency(Currency::BTC())
        // quote
        .quote_currency(Currency::USD())
        // settlement (inverse)
        .settlement_currency(Currency::BTC())
        .is_inverse(true)
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .lot_size(Quantity::from("1"))
        .max_notional(Money::from("10000000 USD"))
        .min_notional(Money::from("1 USD"))
        .max_price(Price::from("10000000"))
        .min_price(Price::from("0.01"))
        .margin_init(dec!(0.01))
        .margin_maint(dec!(0.0035))
        // maker_fee (rebate)
        .maker_fee(dec!(-0.00025))
        .taker_fee(dec!(0.00075))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn crypto_option_btc_deribit(
    #[default(3)] price_precision: u8,
    #[default(1)] size_precision: u8,
    #[default(Price::from("0.001"))] price_increment: Price,
    #[default(Quantity::from("0.1"))] size_increment: Quantity,
) -> CryptoOption {
    let activation = UnixNanos::from(1_671_696_002_000_000_000);
    let expiration = UnixNanos::from(1_673_596_800_000_000_000);
    CryptoOption::builder()
        .instrument_id(InstrumentId::from("BTC-13JAN23-16000-P.DERIBIT"))
        .raw_symbol(Symbol::from("BTC-13JAN23-16000-P"))
        .underlying(Currency::from("BTC"))
        .quote_currency(Currency::from("USD"))
        .settlement_currency(Currency::from("BTC"))
        .is_inverse(false)
        .option_kind(OptionKind::Put)
        .strike_price(Price::from("16000.000"))
        .activation_ns(activation)
        .expiration_ns(expiration)
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .max_quantity(Quantity::from("9000.0"))
        .min_quantity(Quantity::from("0.1"))
        .min_notional(Money::new(10.00, Currency::from("USD")))
        .maker_fee(dec!(0.0003))
        .taker_fee(dec!(0.0003))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

#[fixture]
pub fn crypto_perpetual_ethusdt() -> CryptoPerpetual {
    CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .raw_symbol(Symbol::from("ETHUSDT"))
        .base_currency(Currency::from("ETH"))
        .quote_currency(Currency::from("USDT"))
        .settlement_currency(Currency::from("USDT"))
        .is_inverse(false)
        .price_precision(2)
        .size_precision(3)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("0.001"))
        .max_quantity(Quantity::from("10000.0"))
        .min_quantity(Quantity::from("0.001"))
        .min_notional(Money::new(10.00, Currency::from("USDT")))
        .max_price(Price::from("15000.00"))
        .min_price(Price::from("1.0"))
        .margin_init(dec!(1.0))
        .margin_maint(dec!(0.35))
        .maker_fee(dec!(0.0002))
        .taker_fee(dec!(0.0004))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn xbtusd_bitmex() -> CryptoPerpetual {
    CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("BTCUSDT.BITMEX"))
        .raw_symbol(Symbol::from("XBTUSD"))
        .base_currency(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::BTC())
        .is_inverse(true)
        .price_precision(1)
        .size_precision(0)
        .price_increment(Price::from("0.5"))
        .size_increment(Quantity::from("1"))
        .max_notional(Money::from("10000000 USD"))
        .min_notional(Money::from("1 USD"))
        .max_price(Price::from("10000000"))
        .min_price(Price::from("0.01"))
        .margin_init(dec!(0.01))
        .margin_maint(dec!(0.0035))
        .maker_fee(dec!(-0.00025))
        .taker_fee(dec!(0.00075))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn ethusdt_bitmex() -> CryptoPerpetual {
    CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("ETHUSD.BITMEX"))
        .raw_symbol(Symbol::from("ETHUSD"))
        .base_currency(Currency::ETH())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::ETH())
        .is_inverse(true)
        .price_precision(2)
        .size_precision(0)
        .price_increment(Price::from("0.05"))
        .size_increment(Quantity::from("1"))
        .max_price(Price::from("10000000"))
        .min_price(Price::from("0.01"))
        .margin_init(dec!(0.01))
        .margin_maint(dec!(0.0035))
        .maker_fee(dec!(-0.00025))
        .taker_fee(dec!(0.00075))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn currency_pair_btcusdt() -> CurrencyPair {
    CurrencyPair::builder()
        .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
        .raw_symbol(Symbol::from("BTCUSDT"))
        .base_currency(Currency::from("BTC"))
        .quote_currency(Currency::from("USDT"))
        .price_precision(2)
        .size_precision(6)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("0.000001"))
        .max_quantity(Quantity::from("9000"))
        .min_quantity(Quantity::from("0.000001"))
        .max_price(Price::from("1000000"))
        .min_price(Price::from("0.01"))
        .margin_init(dec!(0.001))
        .margin_maint(dec!(0.001))
        .maker_fee(dec!(0.001))
        .taker_fee(dec!(0.001))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn currency_pair_ethusdt() -> CurrencyPair {
    CurrencyPair::builder()
        .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
        .raw_symbol(Symbol::from("ETHUSDT"))
        .base_currency(Currency::from("ETH"))
        .quote_currency(Currency::from("USDT"))
        .price_precision(2)
        .size_precision(5)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("0.00001"))
        .max_quantity(Quantity::from("9000"))
        .min_quantity(Quantity::from("0.00001"))
        .max_price(Price::from("1000000"))
        .min_price(Price::from("0.01"))
        .margin_init(dec!(0.01))
        .margin_maint(dec!(0.0035))
        .maker_fee(dec!(0.0001))
        .taker_fee(dec!(0.0001))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
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
    let tick_scheme = if quote_currency == "JPY" {
        "FOREX_3DECIMAL"
    } else {
        "FOREX_5DECIMAL"
    };
    let price_increment = Price::new(
        1.0 / 10.0f64.powi(i32::from(price_precision)),
        price_precision,
    );
    CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(symbol)
        .base_currency(Currency::from(base_currency))
        .quote_currency(Currency::from(quote_currency))
        .price_precision(price_precision)
        .size_precision(0)
        .price_increment(price_increment)
        .size_increment(Quantity::from("1"))
        .lot_size(Quantity::from("1000"))
        .max_quantity(Quantity::from("1000000"))
        .min_quantity(Quantity::from("100"))
        .margin_init(dec!(0.03))
        .margin_maint(dec!(0.03))
        .maker_fee(dec!(0.00002))
        .taker_fee(dec!(0.00002))
        .tick_scheme(Ustr::from(tick_scheme))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
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

#[fixture]
pub fn equity_aapl() -> Equity {
    Equity::builder()
        .instrument_id(InstrumentId::from("AAPL.XNAS"))
        .raw_symbol(Symbol::from("AAPL"))
        .isin(Ustr::from("US0378331005"))
        .currency(Currency::from("USD"))
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

/// AAPL equity with ITCH-compatible precision (`price_precision=4`).
///
/// # Panics
///
/// Panics if the instrument definition is invalid.
#[must_use]
pub fn equity_aapl_itch() -> Equity {
    Equity::builder()
        .instrument_id(InstrumentId::from("AAPL.XNAS"))
        .raw_symbol(Symbol::from("AAPL"))
        .isin(Ustr::from("US0378331005"))
        .currency(Currency::from("USD"))
        .price_precision(4)
        .price_increment(Price::from("0.0001"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

/// # Panics
///
/// Panics if constructing the activation or expiration timestamp fails,
/// e.g., if the provided dates are invalid or timestamp conversion returns `None`.
#[must_use]
pub fn futures_contract_es(
    activation: Option<UnixNanos>,
    expiration: Option<UnixNanos>,
) -> FuturesContract {
    let activation = activation.unwrap_or(UnixNanos::from(timestamp(2021, 9, 10, 0, 0, 0)));
    let expiration = expiration.unwrap_or(UnixNanos::from(timestamp(2021, 12, 17, 0, 0, 0)));
    FuturesContract::builder()
        .instrument_id(InstrumentId::from("ESZ21.GLBX"))
        .raw_symbol(Symbol::from("ESZ21"))
        .asset_class(AssetClass::Index)
        .exchange(Ustr::from("XCME"))
        .underlying(Ustr::from("ES"))
        .activation_ns(activation)
        .expiration_ns(expiration)
        .currency(Currency::USD())
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn futures_spread_es() -> FuturesSpread {
    let activation = timestamp(2022, 6, 21, 13, 30, 0);
    let expiration = timestamp(2024, 6, 21, 13, 30, 0);
    FuturesSpread::builder()
        .instrument_id(InstrumentId::from("ESM4-ESU4.GLBX"))
        .raw_symbol(Symbol::from("ESM4-ESU4"))
        .asset_class(AssetClass::Index)
        .exchange(Ustr::from("XCME"))
        .underlying(Ustr::from("ES"))
        .strategy_type(Ustr::from("EQ"))
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .currency(Currency::USD())
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn option_contract_appl() -> OptionContract {
    let activation = timestamp(2021, 9, 17, 0, 0, 0);
    let expiration = timestamp(2021, 12, 17, 0, 0, 0);
    OptionContract::builder()
        .instrument_id(InstrumentId::from("AAPL211217C00150000.OPRA"))
        .raw_symbol(Symbol::from("AAPL211217C00150000"))
        .asset_class(AssetClass::Equity)
        .exchange(Ustr::from("GMNI"))
        .underlying(Ustr::from("AAPL"))
        .option_kind(OptionKind::Call)
        .strike_price(Price::from("149.0"))
        .currency(Currency::USD())
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn option_spread() -> OptionSpread {
    let activation = timestamp(2023, 11, 6, 20, 54, 7);
    let expiration = timestamp(2024, 2, 23, 22, 59, 0);
    OptionSpread::builder()
        .instrument_id(InstrumentId::from("UD:U$: GN 2534559.GLBX"))
        .raw_symbol(Symbol::from("UD:U$: GN 2534559"))
        .asset_class(AssetClass::FX)
        .exchange(Ustr::from("XCME"))
        .underlying(Ustr::from("SR3"))
        .strategy_type(Ustr::from("GN"))
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .currency(Currency::USD())
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn crypto_futures_spread_btc_deribit() -> CryptoFuturesSpread {
    let activation = timestamp(2026, 5, 12, 0, 0, 0);
    let expiration = timestamp(2026, 5, 19, 8, 0, 0);
    CryptoFuturesSpread::builder()
        .instrument_id(InstrumentId::from("BTC-FS-19MAY26_PERP.DERIBIT"))
        .raw_symbol(Symbol::from("BTC-FS-19MAY26_PERP"))
        .underlying(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::BTC())
        .is_inverse(false)
        .strategy_type(Ustr::from("FS"))
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .price_precision(1)
        .size_precision(0)
        .price_increment(Price::from("0.5"))
        .size_increment(Quantity::from("1"))
        .multiplier(Quantity::from("10"))
        .min_quantity(Quantity::from("1"))
        .maker_fee(dec!(0.0003))
        .taker_fee(dec!(0.0003))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

#[fixture]
pub fn crypto_option_spread_btc_deribit() -> CryptoOptionSpread {
    let activation = timestamp(2026, 5, 12, 0, 0, 0);
    let expiration = timestamp(2026, 5, 19, 8, 0, 0);
    CryptoOptionSpread::builder()
        .instrument_id(InstrumentId::from("BTC-CS-19MAY26-70000_75000.DERIBIT"))
        .raw_symbol(Symbol::from("BTC-CS-19MAY26-70000_75000"))
        .underlying(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::BTC())
        .is_inverse(false)
        .strategy_type(Ustr::from("CS"))
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .price_precision(4)
        .size_precision(1)
        .price_increment(Price::from("0.0001"))
        .size_increment(Quantity::from("0.1"))
        .multiplier(Quantity::from(1))
        .min_quantity(Quantity::from("0.1"))
        .maker_fee(dec!(0.0003))
        .taker_fee(dec!(0.0003))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

#[fixture]
pub fn betting() -> BettingInstrument {
    let raw_symbol = Symbol::new("1-123456789");
    let id = InstrumentId::from(format!("{raw_symbol}.BETFAIR"));
    let event_type_id = 6423;
    let event_type_name = Ustr::from("American Football");
    let competition_id = 12_282_733;
    let competition_name = Ustr::from("NFL");
    let event_id = 29_678_534;
    let event_name = Ustr::from("NFL");
    let event_country_code = Ustr::from("GB");
    let event_open_date = UnixNanos::from(timestamp(2022, 2, 7, 23, 30, 0));
    let betting_type = Ustr::from("ODDS");
    let market_id = Ustr::from("1-123456789");
    let market_name = Ustr::from("AFC Conference Winner");
    let market_type = Ustr::from("SPECIAL");
    let market_start_time = UnixNanos::from(timestamp(2022, 2, 7, 23, 30, 0));
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

    BettingInstrument::builder()
        .instrument_id(id)
        .raw_symbol(raw_symbol)
        .event_type_id(event_type_id)
        .event_type_name(event_type_name)
        .competition_id(competition_id)
        .competition_name(competition_name)
        .event_id(event_id)
        .event_name(event_name)
        .event_country_code(event_country_code)
        .event_open_date(event_open_date)
        .betting_type(betting_type)
        .market_id(market_id)
        .market_name(market_name)
        .market_type(market_type)
        .market_start_time(market_start_time)
        .selection_id(selection_id)
        .selection_name(selection_name)
        .selection_handicap(selection_handicap)
        .currency(currency)
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .maybe_max_quantity(max_quantity)
        .maybe_min_quantity(min_quantity)
        .maybe_max_notional(max_notional)
        .maybe_min_notional(min_notional)
        .maybe_max_price(max_price)
        .maybe_min_price(min_price)
        .maybe_margin_init(margin_init)
        .maybe_margin_maint(margin_maint)
        .maybe_maker_fee(maker_fee)
        .maybe_taker_fee(taker_fee)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()
        .unwrap()
}

#[fixture]
pub fn commodity_gold() -> Commodity {
    Commodity::builder()
        .instrument_id(InstrumentId::from("GOLD.COMEX"))
        .raw_symbol(Symbol::from("GOLD"))
        .asset_class(AssetClass::Commodity)
        .quote_currency(Currency::from("USD"))
        .price_precision(2)
        .size_precision(0)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("1"))
        .lot_size(Quantity::from("1"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn index_instrument_spx() -> IndexInstrument {
    IndexInstrument::builder()
        .instrument_id(InstrumentId::from("SPX.INDEX"))
        .raw_symbol(Symbol::from("SPX"))
        .currency(Currency::from("USD"))
        .price_precision(2)
        .size_precision(0)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("1"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn cfd_gold() -> Cfd {
    Cfd::builder()
        .instrument_id(InstrumentId::from("GOLD-CFD.SIM"))
        .raw_symbol(Symbol::from("GOLD-CFD"))
        .asset_class(AssetClass::Commodity)
        .quote_currency(Currency::from("USD"))
        .price_precision(2)
        .size_precision(0)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("1"))
        .lot_size(Quantity::from("1"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn perpetual_contract_eurusd() -> PerpetualContract {
    PerpetualContract::builder()
        .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
        .raw_symbol(Symbol::from("EURUSD-PERP"))
        .underlying(Ustr::from("EURUSD"))
        .asset_class(AssetClass::FX)
        .base_currency(Currency::from("EUR"))
        .quote_currency(Currency::from("USD"))
        .settlement_currency(Currency::from("USD"))
        .is_inverse(false)
        .price_precision(5)
        .size_precision(0)
        .price_increment(Price::from("0.00001"))
        .size_increment(Quantity::from("1"))
        .margin_init(dec!(0.03))
        .margin_maint(dec!(0.03))
        .maker_fee(dec!(0.00002))
        .taker_fee(dec!(0.00002))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn binary_option() -> BinaryOption {
    let raw_symbol = Symbol::new(
        "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905",
    );
    let activation = timestamp(2023, 11, 6, 20, 54, 7);
    let expiration = timestamp(2024, 2, 23, 22, 59, 0);
    let price_increment = Price::from("0.001");
    let size_increment = Quantity::from("0.01");
    BinaryOption::builder()
        .instrument_id(InstrumentId::from("{raw_symbol}.POLYMARKET"))
        .raw_symbol(raw_symbol)
        .asset_class(AssetClass::Alternative)
        .currency(Currency::USDC())
        .activation_ns(UnixNanos::from(activation))
        .expiration_ns(UnixNanos::from(expiration))
        .price_precision(price_increment.precision)
        .size_precision(size_increment.precision)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

#[fixture]
pub fn tokenized_asset_aaplx() -> TokenizedAsset {
    TokenizedAsset::builder()
        .instrument_id(InstrumentId::from("AAPLx/USD.KRAKEN"))
        .raw_symbol(Symbol::from("AAPLxUSD"))
        .asset_class(AssetClass::Equity)
        .base_currency(Currency::get_or_create_crypto("AAPLx"))
        .quote_currency(Currency::from("USD"))
        .price_precision(2)
        .size_precision(4)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("0.0001"))
        .min_quantity(Quantity::from("0.0001"))
        .maker_fee(dec!(-0.0002))
        .taker_fee(dec!(0.001))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}
