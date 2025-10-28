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
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{
    instruments::{
        create_crypto_future, create_crypto_option, create_crypto_perpetual, create_currency_pair,
        get_currency,
    },
    models::TardisInstrumentInfo,
};
use crate::{
    enums::TardisInstrumentType,
    parse::{normalize_instrument_id, parse_instrument_id},
};

#[must_use]
pub fn parse_instrument_any(
    info: TardisInstrumentInfo,
    effective: Option<UnixNanos>,
    ts_init: Option<UnixNanos>,
    normalize_symbols: bool,
) -> Vec<InstrumentAny> {
    match info.instrument_type {
        TardisInstrumentType::Spot => {
            parse_spot_instrument(info, effective, ts_init, normalize_symbols)
        }
        TardisInstrumentType::Perpetual => {
            parse_perp_instrument(info, effective, ts_init, normalize_symbols)
        }
        TardisInstrumentType::Future | TardisInstrumentType::Combo => {
            parse_future_instrument(info, effective, ts_init, normalize_symbols)
        }
        TardisInstrumentType::Option => {
            parse_option_instrument(info, effective, ts_init, normalize_symbols)
        }
    }
}

fn parse_spot_instrument(
    info: TardisInstrumentInfo,
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

    let mut price_increment = parse_price_increment(info.price_increment);
    let base_currency = get_currency(info.base_currency.to_uppercase().as_str());
    let mut size_increment = parse_spot_size_increment(info.amount_increment, base_currency);
    let mut multiplier = parse_multiplier(info.contract_multiplier);
    let mut maker_fee = parse_fee_rate(info.maker_fee);
    let mut taker_fee = parse_fee_rate(info.taker_fee);
    let mut ts_event = info
        .changes
        .as_ref()
        .and_then(|changes| changes.last().map(|c| UnixNanos::from(c.until)))
        .unwrap_or_else(|| UnixNanos::from(info.available_since));

    // Current instrument definition
    let mut instruments = vec![create_currency_pair(
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
    )];

    if let Some(changes) = &info.changes {
        // Sort changes newest to oldest
        let mut sorted_changes = changes.clone();
        sorted_changes.sort_by(|a, b| b.until.cmp(&a.until));

        if let Some(effective_time) = effective {
            // Apply changes where change.until >= effective_time
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                ts_event = UnixNanos::from(change.until);

                if ts_event < effective_time {
                    break; // Early exit since changes are sorted newest to oldest
                } else if i == sorted_changes.len() - 1 {
                    ts_event = UnixNanos::from(info.available_since);
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change.amount_increment.map_or(size_increment, |value| {
                    parse_spot_size_increment(value, base_currency)
                });
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);
            }

            // Replace with single instrument reflecting effective state
            instruments = vec![create_currency_pair(
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
            )];
        } else {
            // Historical sequence with all states
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change.amount_increment.map_or(size_increment, |value| {
                    parse_spot_size_increment(value, base_currency)
                });
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);

                // Get the timestamp for when the change occurred
                ts_event = if i == sorted_changes.len() - 1 {
                    UnixNanos::from(info.available_since)
                } else {
                    UnixNanos::from(change.until)
                };

                instruments.push(create_currency_pair(
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

            // Sort in ascending (chronological) order
            instruments.reverse();
        }
    }

    instruments
}

fn parse_perp_instrument(
    info: TardisInstrumentInfo,
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

    let mut price_increment = parse_price_increment(info.price_increment);
    let mut size_increment = parse_size_increment(info.amount_increment);
    let mut multiplier = parse_multiplier(info.contract_multiplier);
    let mut maker_fee = parse_fee_rate(info.maker_fee);
    let mut taker_fee = parse_fee_rate(info.taker_fee);
    let mut ts_event = info
        .changes
        .as_ref()
        .and_then(|changes| changes.last().map(|c| UnixNanos::from(c.until)))
        .unwrap_or_else(|| UnixNanos::from(info.available_since));

    // Current instrument definition
    let mut instruments = vec![create_crypto_perpetual(
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
    )];

    if let Some(changes) = &info.changes {
        // Sort changes newest to oldest
        let mut sorted_changes = changes.clone();
        sorted_changes.sort_by(|a, b| b.until.cmp(&a.until));

        if let Some(effective_time) = effective {
            // Apply changes where change.until >= effective_time
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                ts_event = UnixNanos::from(change.until);

                if ts_event < effective_time {
                    break; // Early exit since changes are sorted newest to oldest
                } else if i == sorted_changes.len() - 1 {
                    ts_event = UnixNanos::from(info.available_since);
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change
                    .amount_increment
                    .map_or(size_increment, parse_size_increment);
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);
            }

            // Replace with single instrument reflecting effective state
            instruments = vec![create_crypto_perpetual(
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
            )];
        } else {
            // Historical view with all states
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change
                    .amount_increment
                    .map_or(size_increment, parse_size_increment);
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);

                // Get the timestamp for when the change occurred
                ts_event = if i == sorted_changes.len() - 1 {
                    UnixNanos::from(info.available_since)
                } else {
                    UnixNanos::from(change.until)
                };

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

            // Sort in ascending (chronological) order
            instruments.reverse();
        }
    }

    instruments
}

fn parse_future_instrument(
    info: TardisInstrumentInfo,
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

    let mut price_increment = parse_price_increment(info.price_increment);
    let mut size_increment = parse_size_increment(info.amount_increment);
    let mut multiplier = parse_multiplier(info.contract_multiplier);
    let mut maker_fee = parse_fee_rate(info.maker_fee);
    let mut taker_fee = parse_fee_rate(info.taker_fee);
    let mut ts_event = info
        .changes
        .as_ref()
        .and_then(|changes| changes.last().map(|c| UnixNanos::from(c.until)))
        .unwrap_or_else(|| UnixNanos::from(info.available_since));

    // Current instrument definition
    let mut instruments = vec![create_crypto_future(
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
    )];

    if let Some(changes) = &info.changes {
        // Sort changes newest to oldest
        let mut sorted_changes = changes.clone();
        sorted_changes.sort_by(|a, b| b.until.cmp(&a.until));

        if let Some(effective_time) = effective {
            // Apply changes where change.until >= effective_time
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                ts_event = UnixNanos::from(change.until);

                if ts_event < effective_time {
                    break; // Early exit since changes are sorted newest to oldest
                } else if i == sorted_changes.len() - 1 {
                    ts_event = UnixNanos::from(info.available_since);
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change
                    .amount_increment
                    .map_or(size_increment, parse_size_increment);
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);
            }

            // Replace with single instrument reflecting effective state
            instruments = vec![create_crypto_future(
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
            )];
        } else {
            // Historical view with all states
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change
                    .amount_increment
                    .map_or(size_increment, parse_size_increment);
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);

                // Get the timestamp for when the change occurred
                ts_event = if i == sorted_changes.len() - 1 {
                    UnixNanos::from(info.available_since)
                } else {
                    UnixNanos::from(change.until)
                };

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

            // Sort in ascending (chronological) order
            instruments.reverse();
        }
    }

    instruments
}

fn parse_option_instrument(
    info: TardisInstrumentInfo,
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

    let mut price_increment = parse_price_increment(info.price_increment);
    let mut size_increment = parse_size_increment(info.amount_increment);
    let mut multiplier = parse_multiplier(info.contract_multiplier);
    let mut maker_fee = parse_fee_rate(info.maker_fee);
    let mut taker_fee = parse_fee_rate(info.taker_fee);
    let mut ts_event = info
        .changes
        .as_ref()
        .and_then(|changes| changes.last().map(|c| UnixNanos::from(c.until)))
        .unwrap_or_else(|| UnixNanos::from(info.available_since));

    // Current instrument definition
    let mut instruments = vec![create_crypto_option(
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
    )];

    if let Some(changes) = &info.changes {
        // Sort changes newest to oldest
        let mut sorted_changes = changes.clone();
        sorted_changes.sort_by(|a, b| b.until.cmp(&a.until));

        if let Some(effective_time) = effective {
            // Apply changes where change.until >= effective_time
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                ts_event = UnixNanos::from(change.until);

                if ts_event < effective_time {
                    break; // Early exit since changes are sorted newest to oldest
                } else if i == sorted_changes.len() - 1 {
                    ts_event = UnixNanos::from(info.available_since);
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change
                    .amount_increment
                    .map_or(size_increment, parse_size_increment);
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);
            }

            // Replace with single instrument reflecting effective state
            instruments = vec![create_crypto_option(
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
            )];
        } else {
            // Historical view with all states
            for (i, change) in sorted_changes.iter().enumerate() {
                if change.price_increment.is_none()
                    && change.amount_increment.is_none()
                    && change.contract_multiplier.is_none()
                {
                    continue; // No changes to apply (already pushed current definition)
                }

                price_increment = change
                    .price_increment
                    .map_or(price_increment, parse_price_increment);
                size_increment = change
                    .amount_increment
                    .map_or(size_increment, parse_size_increment);
                multiplier = match change.contract_multiplier {
                    Some(value) => Some(Quantity::from(value.to_string())),
                    None => multiplier,
                };
                maker_fee = change.maker_fee.map_or(maker_fee, parse_fee_rate);
                taker_fee = change.taker_fee.map_or(taker_fee, parse_fee_rate);

                // Get the timestamp for when the change occurred
                ts_event = if i == sorted_changes.len() - 1 {
                    UnixNanos::from(info.available_since)
                } else {
                    UnixNanos::from(change.until)
                };

                instruments.push(create_crypto_option(
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

            // Sort in ascending (chronological) order
            instruments.reverse();
        }
    }

    instruments
}

/// Parses the price increment from the given `value`.
fn parse_price_increment(value: f64) -> Price {
    Price::from(value.to_string())
}

/// Parses the size increment from the given `value`.
fn parse_size_increment(value: f64) -> Quantity {
    Quantity::from(value.to_string())
}

/// Parses the spot size increment from the given `value`.
fn parse_spot_size_increment(value: f64, currency: Currency) -> Quantity {
    if value == 0.0 {
        let exponent = -i32::from(currency.precision);
        Quantity::from(format!("{}", 10.0_f64.powi(exponent)))
    } else {
        Quantity::from(value.to_string())
    }
}

/// Parses the multiplier from the given `value`.
fn parse_multiplier(value: Option<f64>) -> Option<Quantity> {
    value.map(|x| Quantity::from(x.to_string()))
}

/// Parses the fee rate from the given `value`.
fn parse_fee_rate(value: f64) -> Decimal {
    Decimal::from_str(&value.to_string()).expect("Invalid decimal value")
}

/// Parses the given RFC 3339 datetime string (UTC) into a `UnixNanos` timestamp.
/// If `value` is `None`, then defaults to the UNIX epoch (0 nanoseconds).
fn parse_datetime_to_unix_nanos(value: Option<DateTime<Utc>>) -> UnixNanos {
    value
        .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64))
        .unwrap_or_default()
}

/// Parses the settlement currency for the given Tardis instrument definition.
#[must_use]
pub fn parse_settlement_currency(info: &TardisInstrumentInfo, is_inverse: bool) -> String {
    info.settlement_currency
        .unwrap_or({
            if is_inverse {
                info.base_currency
            } else {
                info.quote_currency
            }
        })
        .to_uppercase()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{identifiers::InstrumentId, instruments::Instrument};
    use rstest::rstest;

    use super::*;
    use crate::tests::load_test_json;

    #[rstest]
    fn test_parse_instrument_spot() {
        let json_data = load_test_json("instrument_spot.json");
        let info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instruments = parse_instrument_any(info, None, None, false);
        let inst0 = instruments[0].clone();
        let inst1 = instruments[1].clone();

        assert_eq!(inst0.id(), InstrumentId::from("BTC_USDC.DERIBIT"));
        assert_eq!(inst0.raw_symbol(), Symbol::from("BTC_USDC"));
        assert_eq!(inst0.underlying(), None);
        assert_eq!(inst0.base_currency(), Some(Currency::BTC()));
        assert_eq!(inst0.quote_currency(), Currency::USDC());
        assert_eq!(inst0.settlement_currency(), Currency::USDC());
        assert!(!inst0.is_inverse());
        assert_eq!(inst0.price_precision(), 2);
        assert_eq!(inst0.size_precision(), 4);
        assert_eq!(inst0.price_increment(), Price::from("0.01"));
        assert_eq!(inst0.size_increment(), Quantity::from("0.0001"));
        assert_eq!(inst0.multiplier(), Quantity::from(1));
        assert_eq!(inst0.activation_ns(), None);
        assert_eq!(inst0.expiration_ns(), None);
        assert_eq!(inst0.lot_size(), Some(Quantity::from("0.0001")));
        assert_eq!(inst0.min_quantity(), Some(Quantity::from("0.0001")));
        assert_eq!(inst0.max_quantity(), None);
        assert_eq!(inst0.min_notional(), None);
        assert_eq!(inst0.max_notional(), None);
        assert_eq!(inst0.maker_fee(), dec!(0));
        assert_eq!(inst0.taker_fee(), dec!(0));
        assert_eq!(inst0.ts_event().to_rfc3339(), "2023-04-24T00:00:00+00:00");
        assert_eq!(inst0.ts_init().to_rfc3339(), "2023-04-24T00:00:00+00:00");

        assert_eq!(inst1.id(), InstrumentId::from("BTC_USDC.DERIBIT"));
        assert_eq!(inst1.raw_symbol(), Symbol::from("BTC_USDC"));
        assert_eq!(inst1.underlying(), None);
        assert_eq!(inst1.base_currency(), Some(Currency::BTC()));
        assert_eq!(inst1.quote_currency(), Currency::USDC());
        assert_eq!(inst1.settlement_currency(), Currency::USDC());
        assert!(!inst1.is_inverse());
        assert_eq!(inst1.price_precision(), 0); // Changed
        assert_eq!(inst1.size_precision(), 4);
        assert_eq!(inst1.price_increment(), Price::from("1")); // <-- Changed
        assert_eq!(inst1.size_increment(), Quantity::from("0.0001"));
        assert_eq!(inst1.multiplier(), Quantity::from(1));
        assert_eq!(inst1.activation_ns(), None);
        assert_eq!(inst1.expiration_ns(), None);
        assert_eq!(inst1.lot_size(), Some(Quantity::from("0.0001")));
        assert_eq!(inst1.min_quantity(), Some(Quantity::from("0.0001")));
        assert_eq!(inst1.max_quantity(), None);
        assert_eq!(inst1.min_notional(), None);
        assert_eq!(inst1.max_notional(), None);
        assert_eq!(inst1.maker_fee(), dec!(0));
        assert_eq!(inst1.taker_fee(), dec!(0));
        assert_eq!(inst1.ts_event().to_rfc3339(), "2024-04-02T12:10:00+00:00");
        assert_eq!(inst1.ts_init().to_rfc3339(), "2024-04-02T12:10:00+00:00");
    }

    #[rstest]
    fn test_parse_instrument_perpetual() {
        let json_data = load_test_json("instrument_perpetual.json");
        let info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let effective = UnixNanos::from("2020-08-01T08:00:00+00:00");
        let instrument =
            parse_instrument_any(info, Some(effective), Some(UnixNanos::default()), false)
                .first()
                .unwrap()
                .clone();

        assert_eq!(instrument.id(), InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("XBTUSD"));
        assert_eq!(instrument.underlying(), None);
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.5"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(instrument.activation_ns(), None);
        assert_eq!(instrument.expiration_ns(), None);
        assert_eq!(instrument.lot_size(), Some(Quantity::from(1)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(100)));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0.00050));
        assert_eq!(instrument.taker_fee(), dec!(0.00050));
    }

    #[rstest]
    fn test_parse_instrument_future() {
        let json_data = load_test_json("instrument_future.json");
        let info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
            .first()
            .unwrap()
            .clone();

        assert_eq!(instrument.id(), InstrumentId::from("BTC-14FEB25.DERIBIT"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-14FEB25"));
        assert_eq!(instrument.underlying().unwrap().as_str(), "BTC");
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
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
            Some(UnixNanos::from(1_738_281_600_000_000_000))
        );
        assert_eq!(
            instrument.expiration_ns(),
            Some(UnixNanos::from(1_739_520_000_000_000_000))
        );
        assert_eq!(instrument.lot_size(), Some(Quantity::from(10)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(10)));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0));
        assert_eq!(instrument.taker_fee(), dec!(0));
    }

    #[rstest]
    fn test_parse_instrument_perpetual_current() {
        let json_data = load_test_json("instrument_perpetual.json");
        let info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();

        let instrument = parse_instrument_any(info, None, Some(UnixNanos::default()), false)
            .last()
            .unwrap()
            .clone();

        assert_eq!(instrument.id(), InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("XBTUSD"));
        assert_eq!(instrument.size_increment(), Quantity::from(100));
        assert_eq!(instrument.lot_size(), Some(Quantity::from(100)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(100)));
    }

    #[rstest]
    fn test_parse_instrument_combo() {
        let json_data = load_test_json("instrument_combo.json");
        let info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();

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
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
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
            Some(UnixNanos::from(1_711_670_400_000_000_000))
        );
        assert_eq!(
            instrument.expiration_ns(),
            Some(UnixNanos::from(1_743_148_800_000_000_000))
        );
        assert_eq!(instrument.lot_size(), Some(Quantity::from(10)));
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
        let info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();

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
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::BTC());
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 4);
        assert_eq!(instrument.size_precision(), 1); // from amountIncrement 0.1
        assert_eq!(instrument.price_increment(), Price::from("0.0001"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.1"));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(
            instrument.activation_ns(),
            Some(UnixNanos::from(1_738_281_600_000_000_000))
        );
        assert_eq!(
            instrument.expiration_ns(),
            Some(UnixNanos::from(1_745_568_000_000_000_000))
        );
        assert_eq!(instrument.lot_size(), Some(Quantity::from("0.1")));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from("0.1")));
        assert_eq!(instrument.max_quantity(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.maker_fee(), dec!(0));
        assert_eq!(instrument.taker_fee(), dec!(0));
    }
}
