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

use std::str::FromStr;

use chrono::{DateTime, Utc};
use nautilus_core::{parsing::precision_from_str, UnixNanos};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    enums::{AssetClass, CurrencyType},
    identifiers::Symbol,
    instruments::{CryptoFuture, CryptoPerpetual, CurrencyPair, InstrumentAny, OptionsContract},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::types::InstrumentInfo;
use crate::{
    enums::InstrumentType,
    parse::{normalize_instrument_id, parse_instrument_id, parse_option_kind},
};

#[must_use]
pub fn parse_instrument_any(
    info: InstrumentInfo,
    ts_init: UnixNanos,
    normalize_symbols: bool,
) -> InstrumentAny {
    match info.instrument_type {
        InstrumentType::Spot => parse_spot_instrument(info, ts_init, normalize_symbols),
        InstrumentType::Perpetual => parse_perp_instrument(info, ts_init, normalize_symbols),
        InstrumentType::Future => parse_future_instrument(info, ts_init, normalize_symbols),
        InstrumentType::Option => parse_option_instrument(info, ts_init, normalize_symbols),
    }
}

fn parse_spot_instrument(
    info: InstrumentInfo,
    ts_init: UnixNanos,
    normalize_symbols: bool,
) -> InstrumentAny {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };

    let raw_symbol = Symbol::new(info.id);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CurrencyPair(instrument)
}

fn parse_perp_instrument(
    info: InstrumentInfo,
    ts_init: UnixNanos,
    normalize_symbols: bool,
) -> InstrumentAny {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };

    let raw_symbol = Symbol::new(info.id);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
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
        None, // TBD
        None, // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CryptoPerpetual(instrument)
}

fn parse_future_instrument(
    info: InstrumentInfo,
    ts_init: UnixNanos,
    normalize_symbols: bool,
) -> InstrumentAny {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };

    let raw_symbol = Symbol::new(info.id);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);
    let activation = parse_datetime_to_unix_nanos(Some(&info.available_since), "available_since");
    let expiration = parse_datetime_to_unix_nanos(info.expiry.as_deref(), "expiry");
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let instrument = CryptoFuture::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(info.base_currency.to_uppercase().as_str()),
        info.inverse.expect("Future should have `inverse` field"),
        activation,
        expiration,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // TBD
        None, // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CryptoFuture(instrument)
}

fn parse_option_instrument(
    info: InstrumentInfo,
    ts_init: UnixNanos,
    normalize_symbols: bool,
) -> InstrumentAny {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };

    let raw_symbol = Symbol::new(info.id);
    let price_increment = get_price_increment(info.price_increment);
    let activation = parse_datetime_to_unix_nanos(Some(&info.available_since), "available_since");
    let expiration = parse_datetime_to_unix_nanos(info.expiry.as_deref(), "expiry");
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let instrument = OptionsContract::new(
        instrument_id,
        raw_symbol,
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
        activation,
        expiration,
        price_increment.precision,
        price_increment,
        Quantity::from(1),
        Quantity::from(1),
        None, // TBD
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None, // TBD
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::OptionsContract(instrument)
}

/// Returns the price increment from the given `value` limiting to a maximum precision of 9.
fn get_price_increment(value: f64) -> Price {
    let value_str = value.to_string();
    let precision = precision_from_str(&value_str);
    match precision {
        ..9 => Price::from(value_str.as_str()),
        _ => Price::from("0.000000001"),
    }
}

/// Returns the size increment from the given `value` limiting to a maximum precision of 9.
fn get_size_increment(value: f64) -> Quantity {
    let value_str = value.to_string();
    let precision = precision_from_str(&value_str);
    match precision {
        ..9 => Quantity::from(value_str.as_str()),
        _ => Quantity::from("0.000000001"),
    }
}

/// Returns the currency either from the internal currency map or creates a default crypto.
fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(code)
        .copied()
        .unwrap_or(Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

/// Parses the given RFC 3339 datetime string (UTC) into a `UnixNanos` timestamp.
/// If `value` is `None`, then defaults to the UNIX epoch (0 nanoseconds).
fn parse_datetime_to_unix_nanos(value: Option<&str>, field: &str) -> UnixNanos {
    value
        .map(|dt| {
            UnixNanos::from(
                DateTime::parse_from_rfc3339(dt)
                    .unwrap_or_else(|_| panic!("Failed to parse `{field}`"))
                    .with_timezone(&Utc)
                    .timestamp_nanos_opt()
                    .unwrap_or(0) as u64,
            )
        })
        .unwrap_or_default()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;
    use crate::tests::load_test_json;

    #[rstest]
    fn test_parse_instrument_crypto_perpetual() {
        let json_data = load_test_json("instrument_perpetual.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, UnixNanos::default(), false);

        assert_eq!(instrument.id(), InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("XBTUSD"));
        assert_eq!(instrument.underlying(), None);
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.settlement_currency(), Currency::USD());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.5"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(instrument.activation_ns(), None);
        assert_eq!(instrument.expiration_ns(), None);
    }

    // TODO: test_parse_instrument_currency_pair
    // TODO: test_parse_instrument_crypto_future
    // TODO: test_parse_instrument_crypto_option
}
