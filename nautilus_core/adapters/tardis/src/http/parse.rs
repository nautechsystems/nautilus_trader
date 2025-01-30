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
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::Symbol,
    instruments::InstrumentAny,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{
    instruments::{
        create_crypto_future, create_crypto_perpetual, create_currency_pair,
        create_options_contract,
    },
    types::InstrumentInfo,
};
use crate::{
    enums::InstrumentType,
    parse::{normalize_instrument_id, parse_instrument_id},
};

#[must_use]
pub fn parse_instrument_any(
    info: InstrumentInfo,
    start: Option<u64>,
    end: Option<u64>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    match info.instrument_type {
        InstrumentType::Spot => parse_spot_instrument(info, start, end, ts_init, normalize_symbols),
        InstrumentType::Perpetual => {
            parse_perp_instrument(info, start, end, ts_init, normalize_symbols)
        }
        InstrumentType::Future | InstrumentType::Combo => {
            parse_future_instrument(info, start, end, ts_init, normalize_symbols)
        }
        InstrumentType::Option => {
            parse_option_instrument(info, start, end, ts_init, normalize_symbols)
        }
    }
}

fn parse_spot_instrument(
    info: InstrumentInfo,
    start: Option<u64>,
    end: Option<u64>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, &info.instrument_type, info.inverse)
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

    // Filters
    let start = start.unwrap_or(0);
    let end = end.unwrap_or(u64::MAX);

    let mut instruments: Vec<InstrumentAny> = Vec::new();

    if info.changes.is_none() {
        let ts_init = ts_init.unwrap_or(UnixNanos::from(
            Utc::now().timestamp_nanos_opt().unwrap() as u64
        ));
        instruments.push(create_currency_pair(
            &info,
            instrument_id,
            raw_symbol,
            price_increment,
            size_increment,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
            ts_init,
        ));
    }

    if let Some(changes) = &info.changes {
        for change in changes {
            let until_ns = change.until.timestamp_nanos_opt().unwrap() as u64;
            if until_ns < start || until_ns > end {
                continue;
            }

            let price_increment =
                get_price_increment(change.price_increment.unwrap_or(info.price_increment));
            let size_increment =
                get_size_increment(change.amount_increment.unwrap_or(info.amount_increment));
            let ts_event = UnixNanos::from(until_ns);

            instruments.push(create_currency_pair(
                &info,
                instrument_id,
                raw_symbol,
                price_increment,
                size_increment,
                margin_init,
                margin_maint,
                maker_fee,
                taker_fee,
                ts_event,
                ts_init.unwrap_or(ts_event),
            ));
        }
    }
    instruments
}

fn parse_perp_instrument(
    info: InstrumentInfo,
    start: Option<u64>,
    end: Option<u64>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, &info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };
    let raw_symbol = Symbol::new(info.id);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);
    let multiplier = get_multiplier(info.contract_multiplier);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    // Filters
    let start = start.unwrap_or(0);
    let end = end.unwrap_or(u64::MAX);
    let mut instruments = Vec::new();

    if info.changes.is_none() {
        let ts_init = ts_init.unwrap_or(UnixNanos::from(
            Utc::now().timestamp_nanos_opt().unwrap() as u64
        ));
        instruments.push(create_crypto_perpetual(
            &info,
            instrument_id,
            raw_symbol,
            price_increment,
            size_increment,
            multiplier,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
            ts_init,
        ));
    }

    if let Some(changes) = &info.changes {
        for change in changes {
            let until_ns = change.until.timestamp_nanos_opt().unwrap() as u64;
            if until_ns < start || until_ns > end {
                continue;
            }

            let price_increment =
                get_price_increment(change.price_increment.unwrap_or(info.price_increment));
            let size_increment =
                get_size_increment(change.amount_increment.unwrap_or(info.amount_increment));
            let multiplier = get_multiplier(info.contract_multiplier);
            let ts_event = UnixNanos::from(until_ns);

            instruments.push(create_crypto_perpetual(
                &info,
                instrument_id,
                raw_symbol,
                price_increment,
                size_increment,
                multiplier,
                margin_init,
                margin_maint,
                maker_fee,
                taker_fee,
                ts_event,
                ts_init.unwrap_or(ts_event),
            ));
        }
    }
    instruments
}

fn parse_future_instrument(
    info: InstrumentInfo,
    start: Option<u64>,
    end: Option<u64>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, &info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };
    let raw_symbol = Symbol::new(info.id);
    let price_increment = get_price_increment(info.price_increment);
    let size_increment = get_size_increment(info.amount_increment);
    let multiplier = get_multiplier(info.contract_multiplier);
    let activation = parse_datetime_to_unix_nanos(Some(info.available_since));
    let expiration = parse_datetime_to_unix_nanos(info.expiry);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    // Filters
    let start = start.unwrap_or(0);
    let end = end.unwrap_or(u64::MAX);
    let mut instruments = Vec::new();

    if info.changes.is_none() {
        let ts_init = ts_init.unwrap_or(UnixNanos::from(
            Utc::now().timestamp_nanos_opt().unwrap() as u64
        ));
        instruments.push(create_crypto_future(
            &info,
            instrument_id,
            raw_symbol,
            activation,
            expiration,
            price_increment,
            size_increment,
            multiplier,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
            ts_init,
        ));
    }

    if let Some(changes) = &info.changes {
        for change in changes {
            let until_ns = change.until.timestamp_nanos_opt().unwrap() as u64;
            if until_ns < start || until_ns > end {
                continue;
            }

            let price_increment =
                get_price_increment(change.price_increment.unwrap_or(info.price_increment));
            let size_increment =
                get_size_increment(change.amount_increment.unwrap_or(info.amount_increment));
            let multiplier = get_multiplier(info.contract_multiplier);
            let ts_event = UnixNanos::from(until_ns);

            instruments.push(create_crypto_future(
                &info,
                instrument_id,
                raw_symbol,
                activation,
                expiration,
                price_increment,
                size_increment,
                multiplier,
                margin_init,
                margin_maint,
                maker_fee,
                taker_fee,
                ts_event,
                ts_init.unwrap_or(ts_event),
            ));
        }
    }
    instruments
}

fn parse_option_instrument(
    info: InstrumentInfo,
    start: Option<u64>,
    end: Option<u64>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, &info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };
    let raw_symbol = Symbol::new(info.id);
    let activation = parse_datetime_to_unix_nanos(Some(info.available_since));
    let expiration = parse_datetime_to_unix_nanos(info.expiry);
    let price_increment = get_price_increment(info.price_increment);
    let multiplier = get_multiplier(info.contract_multiplier);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    // Filters
    let start = start.unwrap_or(0);
    let end = end.unwrap_or(u64::MAX);
    let mut instruments = Vec::new();

    if info.changes.is_none() {
        let ts_init = ts_init.unwrap_or(UnixNanos::from(
            Utc::now().timestamp_nanos_opt().unwrap() as u64
        ));
        instruments.push(create_options_contract(
            &info,
            instrument_id,
            raw_symbol,
            activation,
            expiration,
            price_increment,
            multiplier,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
            ts_init,
        ));
    }

    if let Some(changes) = &info.changes {
        for change in changes {
            let until_ns = change.until.timestamp_nanos_opt().unwrap() as u64;
            if until_ns < start || until_ns > end {
                continue;
            }

            let price_increment =
                get_price_increment(change.price_increment.unwrap_or(info.price_increment));
            let multiplier = get_multiplier(info.contract_multiplier);
            let ts_event = UnixNanos::from(until_ns);

            instruments.push(create_options_contract(
                &info,
                instrument_id,
                raw_symbol,
                activation,
                expiration,
                price_increment,
                multiplier,
                margin_init,
                margin_maint,
                maker_fee,
                taker_fee,
                ts_event,
                ts_init.unwrap_or(ts_event),
            ));
        }
    }
    instruments
}

/// Returns the price increment from the given `value`.
fn get_price_increment(value: f64) -> Price {
    Price::from(value.to_string())
}

/// Returns the size increment from the given `value`.
fn get_size_increment(value: f64) -> Quantity {
    Quantity::from(value.to_string())
}

fn get_multiplier(value: Option<f64>) -> Option<Quantity> {
    value.map(|x| Quantity::from(x.to_string()))
}

/// Parses the given RFC 3339 datetime string (UTC) into a `UnixNanos` timestamp.
/// If `value` is `None`, then defaults to the UNIX epoch (0 nanoseconds).
fn parse_datetime_to_unix_nanos(value: Option<DateTime<Utc>>) -> UnixNanos {
    value
        .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64))
        .unwrap_or_default()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{identifiers::InstrumentId, types::Currency};
    use rstest::rstest;

    use super::*;
    use crate::tests::load_test_json;

    #[rstest]
    fn test_parse_instrument_crypto_perpetual() {
        let json_data = load_test_json("instrument_perpetual.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, None, Some(UnixNanos::default()), false)
            .first()
            .unwrap()
            .clone();

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
