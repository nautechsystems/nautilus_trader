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

use nautilus_core::{nanos::UnixNanos, parsing::precision_from_str};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    enums::{AssetClass, CurrencyType},
    instruments::{
        any::InstrumentAny, crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair, options_contract::OptionsContract,
    },
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::types::InstrumentInfo;
use crate::tardis::{
    enums::InstrumentType,
    parse::{parse_instrument_id_with_enum, parse_option_kind},
};

#[must_use]
pub fn parse_instrument_any(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    match info.instrument_type {
        InstrumentType::Spot => parse_spot_instrument(info, ts_init),
        InstrumentType::Perpetual => parse_perp_instrument(info, ts_init),
        InstrumentType::Future => parse_future_instrument(info, ts_init),
        InstrumentType::Option => parse_option_instrument(info, ts_init),
    }
}

fn parse_spot_instrument(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id_with_enum(&info.id, &info.exchange);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);

    let instrument = CurrencyPair::new(
        instrument_id,
        instrument_id.symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value"),
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value"),
        dec!(0), // TBD
        dec!(0), // TBD
        None,    // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CurrencyPair(instrument)
}

fn parse_perp_instrument(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id_with_enum(&info.id, &info.exchange);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        instrument_id.symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(
            info.settlement_currency
                .unwrap_or(info.quote_currency)
                .to_uppercase()
                .as_str(),
        ),
        info.inverse.expect("Perpetual should have `inverse` field"),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value"),
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value"),
        dec!(0), // TBD
        dec!(0), // TBD
        None,    // TBD
        None,    // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CryptoPerpetual(instrument)
}

fn parse_future_instrument(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id_with_enum(&info.id, &info.exchange);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);

    let instrument = CryptoFuture::new(
        instrument_id,
        instrument_id.symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(info.base_currency.to_uppercase().as_str()),
        info.inverse.expect("Future should have `inverse` field"),
        UnixNanos::default(), // TODO: Parse activation
        UnixNanos::default(), // TODO: Parse expiration
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value"),
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value"),
        dec!(0), // TBD
        dec!(0), // TBD
        None,    // TBD
        None,    // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CryptoFuture(instrument)
}

fn parse_option_instrument(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id_with_enum(&info.id, &info.exchange);
    let price_increment = get_price_increment(info.price_increment);

    let instrument = OptionsContract::new(
        instrument_id,
        instrument_id.symbol,
        AssetClass::Cryptocurrency,
        Some(Ustr::from(instrument_id.venue.as_str())),
        Ustr::from(info.base_currency.to_string().to_uppercase().as_str()),
        parse_option_kind(
            info.option_type
                .expect("Option should have `option_type` field"),
        ),
        Price::new(
            info.strike_price
                .expect("Option should have `strike_price` field"),
            price_increment.precision,
        ),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        UnixNanos::default(), // TODO: Parse activation
        UnixNanos::default(), // TODO: Parse expiration
        price_increment.precision,
        price_increment,
        Quantity::from(1),
        Quantity::from(1),
        None, // TBD
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None, // TBD
        None,
        None,
        None,
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::OptionsContract(instrument)
}

// TODO: Temporary function to handle price increments beyond max precision
fn get_price_increment(value: f64) -> Price {
    let value_str = value.to_string();
    let precision = precision_from_str(&value_str);
    match precision {
        ..9 => Price::from(value_str.as_str()),
        _ => Price::from("0.000000001"),
    }
}

// TODO: Temporary function to handle size increments beyond max precision
fn get_size_increment(value: f64) -> Quantity {
    let value_str = value.to_string();
    let precision = precision_from_str(&value_str);
    match precision {
        ..9 => Quantity::from(value_str.as_str()),
        _ => Quantity::from("0.000000001"),
    }
}

// TODO: Temporary function to handle "unknown" crypto currencies
fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(code)
        .copied()
        .unwrap_or(Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;
    use crate::tardis::tests::load_test_json;

    #[rstest]
    fn test_parse_instrument_crypto_perpetual() {
        let json_data = load_test_json("instrument_perpetual.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, UnixNanos::default());

        assert_eq!(instrument.id(), InstrumentId::from("XBTUSD.BITMEX"));
        // TODO: Assert remaining fields on InstrumentAny
    }

    // TODO: test_parse_instrument_currency_pair
    // TODO: test_parse_instrument_crypto_future
    // TODO: test_parse_instrument_crypto_option
}
