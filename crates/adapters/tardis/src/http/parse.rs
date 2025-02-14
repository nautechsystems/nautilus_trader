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
        create_crypto_future, create_crypto_perpetual, create_currency_pair, create_option_contract,
    },
    models::InstrumentInfo,
};
use crate::{
    enums::InstrumentType,
    parse::{normalize_instrument_id, parse_instrument_id},
};

#[must_use]
pub fn parse_instrument_any(
    info: InstrumentInfo,
    effective: Option<UnixNanos>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    match info.instrument_type {
        InstrumentType::Spot => parse_spot_instrument(info, effective, ts_init, normalize_symbols),
        InstrumentType::Perpetual => {
            parse_perp_instrument(info, effective, ts_init, normalize_symbols)
        }
        InstrumentType::Future | InstrumentType::Combo => {
            parse_future_instrument(info, effective, ts_init, normalize_symbols)
        }
        InstrumentType::Option => {
            parse_option_instrument(info, effective, ts_init, normalize_symbols)
        }
    }
}

fn parse_spot_instrument(
    info: InstrumentInfo,
    effective: Option<UnixNanos>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, &info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };
    let raw_symbol = Symbol::new(info.id);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let mut price_increment = get_price_increment(info.price_increment);
    let mut size_increment = get_size_increment(info.amount_increment);
    let mut ts_event = UnixNanos::from(info.available_since);

    let mut instruments: Vec<InstrumentAny> = Vec::new();

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

    if let Some(changes) = &info.changes {
        for change in changes {
            price_increment = match change.price_increment {
                Some(value) => get_price_increment(value),
                None => price_increment,
            };
            size_increment = match change.amount_increment {
                Some(value) => get_size_increment(value),
                None => size_increment,
            };
            ts_event = UnixNanos::from(change.until);

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

    if let Some(effective) = effective {
        // Retain instruments up to and including the effective time
        instruments.retain(|i| i.ts_event() <= effective);
        if instruments.is_empty() {
            return Vec::new();
        }
        // Sort descending by ts_event to keep most recent
        instruments.sort_by_key(|i| std::cmp::Reverse(i.ts_event()));
        // Keep only most recent version at or before effective time
        instruments.truncate(1);
    }

    instruments
}

fn parse_perp_instrument(
    info: InstrumentInfo,
    effective: Option<UnixNanos>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    let instrument_id = if normalize_symbols {
        normalize_instrument_id(&info.exchange, info.id, &info.instrument_type, info.inverse)
    } else {
        parse_instrument_id(&info.exchange, info.id)
    };
    let raw_symbol = Symbol::new(info.id);
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let mut price_increment = get_price_increment(info.price_increment);
    let mut size_increment = get_size_increment(info.amount_increment);
    let mut multiplier = get_multiplier(info.contract_multiplier);
    let mut ts_event = UnixNanos::from(info.available_since);

    let mut instruments = Vec::new();

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

    if let Some(changes) = &info.changes {
        for change in changes {
            price_increment = match change.price_increment {
                Some(value) => get_price_increment(value),
                None => price_increment,
            };
            size_increment = match change.amount_increment {
                Some(value) => get_size_increment(value),
                None => size_increment,
            };
            multiplier = match change.contract_multiplier {
                Some(value) => Some(Quantity::from(value.to_string())),
                None => multiplier,
            };
            ts_event = UnixNanos::from(change.until);

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

    if let Some(effective) = effective {
        // Retain instruments up to and including the effective time
        instruments.retain(|i| i.ts_event() <= effective);
        if instruments.is_empty() {
            return Vec::new();
        }
        // Sort descending by ts_event to keep most recent
        instruments.sort_by_key(|i| std::cmp::Reverse(i.ts_event()));
        // Keep only most recent version at or before effective time
        instruments.truncate(1);
    }

    instruments
}

fn parse_future_instrument(
    info: InstrumentInfo,
    effective: Option<UnixNanos>,
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
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let mut price_increment = get_price_increment(info.price_increment);
    let mut size_increment = get_size_increment(info.amount_increment);
    let mut multiplier = get_multiplier(info.contract_multiplier);
    let mut ts_event = UnixNanos::from(info.available_since);

    let mut instruments = Vec::new();

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

    if let Some(changes) = &info.changes {
        for change in changes {
            price_increment = match change.price_increment {
                Some(value) => get_price_increment(value),
                None => price_increment,
            };
            size_increment = match change.amount_increment {
                Some(value) => get_size_increment(value),
                None => size_increment,
            };
            multiplier = match change.contract_multiplier {
                Some(value) => Some(Quantity::from(value.to_string())),
                None => multiplier,
            };
            ts_event = UnixNanos::from(change.until);

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

    if let Some(effective) = effective {
        // Retain instruments up to and including the effective time
        instruments.retain(|i| i.ts_event() <= effective);
        if instruments.is_empty() {
            return Vec::new();
        }
        // Sort descending by ts_event to keep most recent
        instruments.sort_by_key(|i| std::cmp::Reverse(i.ts_event()));
        // Keep only most recent version at or before effective time
        instruments.truncate(1);
    }

    instruments
}

fn parse_option_instrument(
    info: InstrumentInfo,
    effective: Option<UnixNanos>,
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
    let margin_init = dec!(0); // TBD
    let margin_maint = dec!(0); // TBD
    let maker_fee =
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value");
    let taker_fee =
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value");

    let mut price_increment = get_price_increment(info.price_increment);
    let mut multiplier = get_multiplier(info.contract_multiplier);
    let mut ts_event = UnixNanos::from(info.available_since);

    let mut instruments = Vec::new();

    instruments.push(create_option_contract(
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

    if let Some(changes) = &info.changes {
        for change in changes {
            price_increment = match change.price_increment {
                Some(value) => get_price_increment(value),
                None => price_increment,
            };

            multiplier = match change.contract_multiplier {
                Some(value) => Some(Quantity::from(value.to_string())),
                None => multiplier,
            };
            ts_event = UnixNanos::from(change.until);

            instruments.push(create_option_contract(
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

    if let Some(effective) = effective {
        // Retain instruments up to and including the effective time
        instruments.retain(|i| i.ts_event() <= effective);
        if instruments.is_empty() {
            return Vec::new();
        }
        // Sort descending by ts_event to keep most recent
        instruments.sort_by_key(|i| std::cmp::Reverse(i.ts_event()));
        // Keep only most recent version at or before effective time
        instruments.truncate(1);
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
    fn test_parse_instrument_spot() {
        let json_data = load_test_json("instrument_spot.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
            .first()
            .unwrap()
            .clone();

        assert_eq!(instrument.id(), InstrumentId::from("BTC_USDC.DERIBIT"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC_USDC"));
        assert_eq!(instrument.underlying(), None);
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USDC());
        assert_eq!(instrument.settlement_currency(), Currency::USDC());
        assert!(!instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 0);
        assert_eq!(instrument.size_precision(), 4);
        assert_eq!(instrument.price_increment(), Price::from("1"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.0001"));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(instrument.activation_ns(), None);
        assert_eq!(instrument.expiration_ns(), None);
        assert_eq!(instrument.min_quantity(), Some(Quantity::from("0.0001")));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0));
        assert_eq!(instrument.taker_fee(), dec!(0));
    }

    #[rstest]
    fn test_parse_instrument_perpetual() {
        let json_data = load_test_json("instrument_perpetual.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
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
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(1)));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(-0.00025));
        assert_eq!(instrument.taker_fee(), dec!(0.00075));
    }

    #[rstest]
    fn test_parse_instrument_future() {
        let json_data = load_test_json("instrument_future.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
            .first()
            .unwrap()
            .clone();

        assert_eq!(instrument.id(), InstrumentId::from("BTC-14FEB25.DERIBIT"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-14FEB25"));
        assert_eq!(instrument.underlying().unwrap().as_str(), "BTC");
        assert_eq!(instrument.base_currency(), None);
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 1); // from priceIncrement 2.5
        assert_eq!(instrument.size_precision(), 0); // from amountIncrement 10
        assert_eq!(instrument.price_increment(), Price::from("2.5"));
        assert_eq!(instrument.size_increment(), Quantity::from(10));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(
            instrument.activation_ns(),
            Some(UnixNanos::from(1738281600000000000))
        );
        assert_eq!(
            instrument.expiration_ns(),
            Some(UnixNanos::from(1739520000000000000))
        );
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(10)));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0));
        assert_eq!(instrument.taker_fee(), dec!(0));
    }

    #[rstest]
    fn test_parse_instrument_combo() {
        let json_data = load_test_json("instrument_combo.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
            .first()
            .unwrap()
            .clone();

        assert_eq!(
            instrument.id(),
            InstrumentId::from("BTC-FS-28MAR25_PERP.DERIBIT")
        );
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-FS-28MAR25_PERP"));
        assert_eq!(instrument.underlying().unwrap().as_str(), "BTC");
        assert_eq!(instrument.base_currency(), None);
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 1); // from priceIncrement 0.5
        assert_eq!(instrument.size_precision(), 0); // from amountIncrement 10
        assert_eq!(instrument.price_increment(), Price::from("0.5"));
        assert_eq!(instrument.size_increment(), Quantity::from(10));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(
            instrument.activation_ns(),
            Some(UnixNanos::from(1711670400000000000))
        );
        assert_eq!(
            instrument.expiration_ns(),
            Some(UnixNanos::from(1743148800000000000))
        );
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(10)));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0));
        assert_eq!(instrument.taker_fee(), dec!(0));
    }

    #[rstest]
    fn test_parse_instrument_option() {
        let json_data = load_test_json("instrument_option.json");
        let info: InstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
            .first()
            .unwrap()
            .clone();

        assert_eq!(
            instrument.id(),
            InstrumentId::from("BTC-25APR25-200000-P.DERIBIT")
        );
        assert_eq!(
            instrument.raw_symbol(),
            Symbol::from("BTC-25APR25-200000-P")
        );
        assert_eq!(instrument.underlying().unwrap().as_str(), "BTC");
        assert_eq!(instrument.base_currency(), None);
        assert_eq!(instrument.quote_currency(), Currency::BTC());
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        // assert!(instrument.is_inverse());  // TODO: Implement inverse options
        assert_eq!(instrument.price_precision(), 4);
        // assert_eq!(instrument.size_precision(), 1); // from amountIncrement 0.1
        assert_eq!(instrument.price_increment(), Price::from("0.0001"));
        // assert_eq!(instrument.size_increment(), Quantity::from("0.1"));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(
            instrument.activation_ns(),
            Some(UnixNanos::from(1738281600000000000))
        );
        assert_eq!(
            instrument.expiration_ns(),
            Some(UnixNanos::from(1745568000000000000))
        );
        assert_eq!(instrument.min_quantity(), Some(Quantity::from("0.1")));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0));
        assert_eq!(instrument.taker_fee(), dec!(0));
    }
}
