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

//! Instrument parsing utilities for converting IB ContractDetails to Nautilus instruments.

use std::str::FromStr;

use ibapi::contracts::SecurityType;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{AssetClass, OptionKind},
    identifiers::{InstrumentId, Symbol},
    instruments::{
        Cfd, Commodity, CryptoPerpetual, CurrencyPair, Equity, FuturesContract, FuturesSpread,
        IndexInstrument, InstrumentAny, OptionContract, OptionSpread,
    },
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::common::contract_to_params;

/// Convert tick size to precision value.
///
/// # Arguments
///
/// * `tick_size` - The tick size to convert
///
/// # Returns
///
/// Returns the precision value (number of decimal places).
#[must_use]
pub fn tick_size_to_precision(tick_size: f64) -> u8 {
    if tick_size <= 0.0 {
        return 8; // Default precision for zero or negative tick sizes
    }

    // Count decimal places
    let s = format!("{:.10}", tick_size);
    let s = s.trim_end_matches('0');
    let parts: Vec<&str> = s.split('.').collect();

    if parts.len() == 2 {
        parts[1].len().min(8) as u8
    } else {
        0
    }
}

/// Convert timestamp string to UnixNanos.
///
/// Handles formats like "20230101" or "20230101 00:00:00 UTC".
///
/// # Arguments
///
/// * `details` - The IB contract details
///
/// # Errors
///
/// Returns an error if the timestamp cannot be parsed.
pub fn expiry_timestring_to_unix_nanos(
    expiry: &str,
    details: Option<&ibapi::contracts::ContractDetails>,
) -> anyhow::Result<UnixNanos> {
    if expiry.is_empty() {
        anyhow::bail!("Empty expiry string");
    }

    // Parse timestamp string - Most contract expirations are %Y%m%d format
    // Some exchanges have expirations in %Y%m%d %H:%M:%S %Z
    let dt = if expiry.len() == 8 {
        // Format: YYYYMMDD
        let year = &expiry[0..4];
        let month = &expiry[4..6];
        let day = &expiry[6..8];
        let date = time::Date::from_calendar_date(
            year.parse()?,
            time::Month::try_from(month.parse::<u8>()?)?,
            day.parse()?,
        )?;

        // If we have trading hours, try to extract the last trade time
        // Trading hours format: "20240411:0000-20240411:1800;..."
        let mut expiry_time = time::Time::MIDNIGHT;

        if let Some(details) = details {
            if !details.trading_hours.is_empty()
                && !details.trading_hours.contains(&"CLOSED".to_string())
            {
                // Find the session for this date
                let expiry_str: &str = expiry;
                for session in &details.trading_hours {
                    if session.as_str().starts_with(expiry_str) && session.as_str().contains('-') {
                        let parts: Vec<&str> = session.as_str().split('-').collect();
                        if let Some(end_part) = parts.get(1) {
                            let inner_parts: Vec<&str> = end_part.split(':').collect();
                            if let Some(time_part) = inner_parts.get(1) {
                                if time_part.len() >= 4 {
                                    let hour = time_part
                                        .get(0..2)
                                        .and_then(|s: &str| s.parse::<u8>().ok())
                                        .unwrap_or(0);
                                    let minute = time_part
                                        .get(2..4)
                                        .and_then(|s: &str| s.parse::<u8>().ok())
                                        .unwrap_or(0);
                                    expiry_time = time::Time::from_hms(hour, minute, 0)
                                        .unwrap_or(time::Time::MIDNIGHT);
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
        time::PrimitiveDateTime::new(date, expiry_time)
    } else {
        // Format: YYYYMMDD HH:MM:SS TZ
        let parts: Vec<&str> = expiry.split(' ').collect();
        if parts.len() >= 3 {
            let date_part = parts[0];
            let time_part = parts[1];
            let year = &date_part[0..4];
            let month = &date_part[4..6];
            let day = &date_part[6..8];

            let time_parts: Vec<&str> = time_part.split(':').collect();
            let hour = time_parts.first().unwrap_or(&"0").parse::<u8>()?;
            let minute = time_parts.get(1).unwrap_or(&"0").parse::<u8>()?;
            let second = time_parts.get(2).unwrap_or(&"0").parse::<u8>()?;

            let date = time::Date::from_calendar_date(
                year.parse()?,
                time::Month::try_from(month.parse::<u8>()?)?,
                day.parse()?,
            )?;
            let time_obj = time::Time::from_hms(hour, minute, second)?;
            time::PrimitiveDateTime::new(date, time_obj)
        } else {
            anyhow::bail!("Invalid expiry format: {}", expiry);
        }
    };

    // Treat the parsed expiry timestamp as UTC. NautilusTrader expects IB timestamps
    // to be configured and interpreted in UTC.
    let offset_dt = dt.assume_utc();
    let nanos = offset_dt.unix_timestamp_nanos();
    Ok(UnixNanos::new(nanos as u64))
}

/// Parse an IB ContractDetails to a Nautilus instrument.
///
/// # Arguments
///
/// * `details` - The IB contract details
/// * `instrument_id` - The instrument ID to use
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_ib_contract_to_instrument(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> anyhow::Result<InstrumentAny> {
    let sec_type = &details.contract.security_type;

    match sec_type {
        SecurityType::Stock => Ok(parse_equity_contract(details, instrument_id)),
        SecurityType::ForexPair => Ok(parse_forex_contract(details, instrument_id)),
        SecurityType::Crypto => Ok(parse_crypto_contract(details, instrument_id)),
        SecurityType::Future => Ok(parse_futures_contract(details, instrument_id)),
        SecurityType::Option => parse_option_contract(details, instrument_id),
        SecurityType::FuturesOption => parse_option_contract(details, instrument_id), // FOP uses same parsing as OPT
        SecurityType::Index => Ok(parse_index_contract(details, instrument_id)),
        SecurityType::CFD => Ok(parse_cfd_contract(details, instrument_id)),
        SecurityType::Commodity => Ok(parse_commodity_contract(details, instrument_id)),
        SecurityType::Bond => Ok(parse_bond_contract(details, instrument_id)),
        _ => anyhow::bail!("Unsupported security type: {:?}", sec_type),
    }
}

fn ib_contract_info(details: &ibapi::contracts::ContractDetails) -> nautilus_core::Params {
    let mut info = nautilus_core::Params::new();
    let mut contract = serde_json::Map::new();

    let contract_params = contract_to_params(&details.contract);
    for (key, value) in &contract_params {
        contract.insert(key.clone(), value.clone());
    }

    info.insert("contract".to_string(), serde_json::Value::Object(contract));
    info
}

fn ib_contract_info_for_contract(contract: &ibapi::contracts::Contract) -> nautilus_core::Params {
    let mut info = nautilus_core::Params::new();
    let mut contract_map = serde_json::Map::new();
    let contract_params = contract_to_params(contract);

    for (key, value) in &contract_params {
        contract_map.insert(key.clone(), value.clone());
    }

    info.insert(
        "contract".to_string(),
        serde_json::Value::Object(contract_map),
    );
    info
}

fn sec_type_to_asset_class(sec_type: &str) -> AssetClass {
    match sec_type {
        "STK" => AssetClass::Equity,
        "IND" => AssetClass::Index,
        "CASH" => AssetClass::FX,
        "BOND" => AssetClass::Debt,
        "CMDTY" => AssetClass::Commodity,
        "FUT" => AssetClass::Index,
        _ => AssetClass::Equity,
    }
}

/// Parse equity contract (STK).
fn parse_equity_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = Equity::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        None, // isin
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        Price::new(details.min_tick, price_precision),
        Some(Quantity::new(100.0, 0)),   // Standard lot size for stocks
        None,                            // max_quantity
        None,                            // min_quantity
        None,                            // max_price
        None,                            // min_price
        None,                            // margin_init
        None,                            // margin_maint
        None,                            // maker_fee
        None,                            // taker_fee
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Parse forex contract (CASH).
fn parse_forex_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let size_precision = tick_size_to_precision(details.min_size);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = CurrencyPair::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        Currency::from(details.contract.symbol.to_string()),
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        size_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(details.size_increment, size_precision),
        None,                            // multiplier
        None,                            // lot_size
        None,                            // max_quantity
        None,                            // min_quantity
        None,                            // max_notional
        None,                            // min_notional
        None,                            // max_price
        None,                            // min_price
        None,                            // margin_init
        None,                            // margin_maint
        None,                            // maker_fee
        None,                            // taker_fee
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Parse crypto contract (CRYPTO).
fn parse_crypto_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let size_precision = tick_size_to_precision(details.min_size);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = CryptoPerpetual::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        Currency::from(details.contract.symbol.to_string()),
        Currency::from(details.contract.currency.to_string()),
        Currency::from(details.contract.currency.to_string()),
        true, // is_inverse
        price_precision,
        size_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(details.size_increment, size_precision),
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        Some(Quantity::new(details.min_size, size_precision)),
        None,                            // max_notional
        None,                            // min_notional
        None,                            // max_price
        None,                            // min_price
        None,                            // margin_init
        None,                            // margin_maint
        None,                            // maker_fee
        None,                            // taker_fee
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Parse futures contract (FUT).
fn parse_futures_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    // Parse expiration
    let expiration_ns = if !details
        .contract
        .last_trade_date_or_contract_month
        .is_empty()
    {
        expiry_timestring_to_unix_nanos(
            &details.contract.last_trade_date_or_contract_month,
            Some(details),
        )
        .unwrap_or_else(|_| UnixNanos::from(timestamp.as_u64() + 90 * 24 * 60 * 60 * 1_000_000_000))
    // Default to +90 days on error
    } else {
        UnixNanos::from(timestamp.as_u64() + 90 * 24 * 60 * 60 * 1_000_000_000) // Default to +90 days if empty
    };

    let ninety_days_ns: u64 = 90 * 24 * 60 * 60 * 1_000_000_000;
    let activation_ns = expiration_ns
        .checked_sub(ninety_days_ns)
        .unwrap_or(UnixNanos::from(0)); // -90 days or 0 if underflow

    let multiplier = details.contract.multiplier.parse::<f64>().unwrap_or(1.0);

    let instrument = FuturesContract::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        sec_type_to_asset_class(details.under_security_type.as_str()),
        None, // exchange
        Ustr::from(details.under_symbol.as_str()),
        activation_ns,
        expiration_ns,
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(multiplier, 0),
        Quantity::new(1.0, 0),
        None,                            // max_quantity
        None,                            // min_quantity
        None,                            // max_price
        None,                            // min_price
        None,                            // margin_init
        None,                            // margin_maint
        None,                            // maker_fee
        None,                            // taker_fee
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Parse option contract (OPT).
fn parse_option_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> anyhow::Result<InstrumentAny> {
    let price_precision = tick_size_to_precision(details.min_tick);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    // Parse expiration
    let expiration_ns = if !details
        .contract
        .last_trade_date_or_contract_month
        .is_empty()
    {
        expiry_timestring_to_unix_nanos(
            &details.contract.last_trade_date_or_contract_month,
            Some(details),
        )
        .unwrap_or_else(|_| UnixNanos::from(timestamp.as_u64() + 90 * 24 * 60 * 60 * 1_000_000_000))
    // Default to +90 days on error
    } else {
        UnixNanos::from(timestamp.as_u64() + 90 * 24 * 60 * 60 * 1_000_000_000) // Default to +90 days if empty
    };

    let ninety_days_ns: u64 = 90 * 24 * 60 * 60 * 1_000_000_000;
    let activation_ns = expiration_ns
        .checked_sub(ninety_days_ns)
        .unwrap_or(UnixNanos::from(0)); // -90 days or 0 if underflow

    // Parse option kind (CALL or PUT)
    let option_kind = match details.contract.right.as_str() {
        "C" => OptionKind::Call,
        "P" => OptionKind::Put,
        _ => anyhow::bail!("Unknown option kind: {}", details.contract.right),
    };

    let multiplier = details.contract.multiplier.parse::<f64>().unwrap_or(100.0);
    let asset_class = match details.under_security_type.as_str() {
        "IND" => AssetClass::Index,
        _ => AssetClass::Equity,
    };
    let underlying =
        if details.under_security_type == "IND" && !details.under_symbol.starts_with('^') {
            format!("^{}", details.under_symbol)
        } else {
            details.under_symbol.clone()
        };

    let instrument = OptionContract::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        asset_class,
        None, // exchange
        Ustr::from(underlying.as_str()),
        option_kind,
        Price::new(details.contract.strike, price_precision),
        Currency::from(details.contract.currency.to_string()),
        activation_ns,
        expiration_ns,
        price_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(multiplier, 0),
        Quantity::new(multiplier, 0),
        None,                            // max_quantity
        None,                            // min_quantity
        None,                            // max_price
        None,                            // min_price
        None,                            // margin_init
        None,                            // margin_maint
        None,                            // maker_fee
        None,                            // taker_fee
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    Ok(InstrumentAny::from(instrument))
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use ibapi::contracts::{Contract, ContractDetails, Currency, Exchange, SecurityType, Symbol};
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol as NautilusSymbol, Venue},
        instruments::{Instrument, InstrumentAny},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::parse_ib_contract_to_instrument;

    #[rstest]
    fn test_parse_option_contract_prefixes_index_underlying() {
        let details = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("SPXW"),
                security_type: SecurityType::Option,
                exchange: Exchange::from("SMART"),
                currency: Currency::from("USD"),
                local_symbol: "SPXW  260313P06630000".to_string(),
                last_trade_date_or_contract_month: "20260313".to_string(),
                right: "P".to_string(),
                strike: 6630.0,
                multiplier: "100".to_string(),
                ..Default::default()
            },
            min_tick: 0.05,
            under_symbol: "SPX".to_string(),
            under_security_type: "IND".to_string(),
            ..Default::default()
        };
        let instrument_id = InstrumentId::new(
            NautilusSymbol::from("SPXW  260313P06630000"),
            Venue::from("SMART"),
        );

        let instrument = parse_ib_contract_to_instrument(&details, instrument_id).unwrap();

        let InstrumentAny::OptionContract(option) = instrument else {
            panic!("expected option contract");
        };

        assert_eq!(option.asset_class(), AssetClass::Index);
        assert_eq!(option.underlying(), Some(Ustr::from("^SPX")));
    }
}

/// Parse index contract (IND).
///
/// Note: Indices are typically not directly tradable. This creates a CurrencyPair
/// representation as a placeholder until IndexInstrument type is available.
fn parse_index_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let size_precision = tick_size_to_precision(details.min_size);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = IndexInstrument::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        size_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(details.size_increment, size_precision),
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Create a spread instrument ID from leg tuples.
///
/// This implements the same logic as Python's `InstrumentId.new_spread`:
/// - Creates a symbol string like `(1)SYMBOL1_(-2)SYMBOL2`
/// - Positive ratios: `(ratio)SYMBOL`
/// - Negative ratios: `((abs(ratio)))SYMBOL`
/// - Sorts legs alphabetically by symbol
/// - All legs must have the same venue
///
/// # Arguments
///
/// * `leg_tuples` - Vector of (instrument_id, ratio) tuples
///
/// # Errors
///
/// Returns an error if:
/// - Less than 2 legs provided
/// - Any ratio is zero
/// - Venues don't match across legs
pub fn create_spread_instrument_id(
    leg_tuples: &[(InstrumentId, i32)],
) -> anyhow::Result<InstrumentId> {
    if leg_tuples.len() < 2 {
        anyhow::bail!("instrument_ratios list needs to have at least 2 legs");
    }

    // Validate all ratios are non-zero and venues match
    let first_venue = leg_tuples[0].0.venue;

    for (instrument_id, ratio) in leg_tuples {
        if *ratio == 0 {
            anyhow::bail!("ratio cannot be zero");
        }

        if instrument_id.venue != first_venue {
            anyhow::bail!(
                "All venues must match. Expected {}, was {}",
                first_venue,
                instrument_id.venue
            );
        }
    }

    // Sort instrument ratios alphabetically by symbol
    let mut sorted_ratios = leg_tuples.to_vec();
    sorted_ratios.sort_by(|a, b| a.0.symbol.as_str().cmp(b.0.symbol.as_str()));

    // Build the composite symbol
    let mut symbol_parts = Vec::new();

    for (instrument_id, ratio) in &sorted_ratios {
        let symbol_part = if *ratio > 0 {
            format!("({}){}", ratio, instrument_id.symbol.as_str())
        } else {
            format!("(({})){}", ratio.abs(), instrument_id.symbol.as_str())
        };
        symbol_parts.push(symbol_part);
    }

    let composite_symbol = symbol_parts.join("_");
    let symbol = Symbol::from(composite_symbol.as_str());

    Ok(InstrumentId::new(symbol, first_venue))
}

/// Parse a spread instrument ID into an OptionSpread instrument.
///
/// This implements the same logic as Python's `parse_spread_instrument_id`.
/// Uses contract details from the first leg to determine spread properties.
///
/// # Arguments
///
/// * `instrument_id` - The spread instrument ID
/// * `leg_contract_details` - Vector of (contract_details, ratio) tuples
/// * `timestamp_ns` - Optional timestamp (uses current time if None)
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_spread_instrument_id(
    instrument_id: InstrumentId,
    leg_contract_details: &[(&ibapi::contracts::ContractDetails, i32)],
    timestamp_ns: Option<UnixNanos>,
) -> anyhow::Result<OptionSpread> {
    if leg_contract_details.is_empty() {
        anyhow::bail!("leg_contract_details must be provided");
    }

    // Use contract details from first leg
    let (first_details, _) = leg_contract_details[0];
    let first_contract = &first_details.contract;

    // Extract properties from the first leg contract details
    let currency = Currency::from(first_contract.currency.to_string());
    let underlying = if !first_details.under_symbol.is_empty() {
        Ustr::from(first_details.under_symbol.as_str())
    } else {
        Ustr::from(first_contract.symbol.as_str())
    };

    // Parse multiplier
    let multiplier_str = first_contract.multiplier.to_string();
    let multiplier =
        Quantity::from_str(&multiplier_str).unwrap_or_else(|_| Quantity::new(100.0, 0)); // Default to 100 for options

    // Determine asset class based on security type
    let asset_class = match first_contract.security_type {
        ibapi::contracts::SecurityType::FuturesOption => AssetClass::Index, // Futures options
        _ => AssetClass::Equity,                                            // Equity options
    };

    // Calculate price precision and increment
    let price_precision = tick_size_to_precision(first_details.min_tick);
    let price_increment = Price::new(first_details.min_tick, price_precision);

    // Use provided timestamp or current time
    let timestamp = timestamp_ns.unwrap_or_else(|| get_atomic_clock_realtime().get_time_ns());

    // For options spreads, lot size equals multiplier (same as individual option contracts)
    let lot_size = multiplier;

    // Create the spread instrument
    let spread = OptionSpread::new_checked(
        instrument_id,
        Symbol::from(instrument_id.symbol.as_str()), // raw_symbol
        asset_class,
        None, // exchange (optional)
        underlying,
        Ustr::from("SPREAD"), // strategy_type
        UnixNanos::new(0),    // activation_ns (spreads don't have single activation dates)
        UnixNanos::new(0),    // expiration_ns (spreads don't have single expiration dates)
        currency,
        price_precision,
        price_increment,
        multiplier,
        lot_size,
        None,                // max_quantity
        None,                // min_quantity
        None,                // max_price
        None,                // min_price
        Some(Decimal::ZERO), // margin_init
        Some(Decimal::ZERO), // margin_maint
        Some(Decimal::ZERO), // maker_fee
        Some(Decimal::ZERO), // taker_fee
        None,                // info
        timestamp,
        timestamp,
    )?;

    Ok(spread)
}

pub fn parse_option_spread_instrument_id(
    instrument_id: InstrumentId,
    leg_contract_details: &[(&ibapi::contracts::ContractDetails, i32)],
    bag_contract: Option<&ibapi::contracts::Contract>,
    timestamp_ns: Option<UnixNanos>,
) -> anyhow::Result<OptionSpread> {
    let mut spread = parse_spread_instrument_id(instrument_id, leg_contract_details, timestamp_ns)?;
    spread.info = bag_contract.map(ib_contract_info_for_contract);
    Ok(spread)
}

pub fn parse_futures_spread_instrument_id(
    instrument_id: InstrumentId,
    leg_contract_details: &[(&ibapi::contracts::ContractDetails, i32)],
    bag_contract: Option<&ibapi::contracts::Contract>,
    timestamp_ns: Option<UnixNanos>,
) -> anyhow::Result<FuturesSpread> {
    if leg_contract_details.is_empty() {
        anyhow::bail!("leg_contract_details must be provided");
    }

    let (first_details, _) = leg_contract_details[0];
    let first_contract = &first_details.contract;
    let currency = Currency::from(first_contract.currency.to_string());
    let underlying = if !first_details.under_symbol.is_empty() {
        Ustr::from(first_details.under_symbol.as_str())
    } else {
        Ustr::from(first_contract.symbol.as_str())
    };
    let multiplier = Quantity::from_str(&first_contract.multiplier.to_string())
        .unwrap_or_else(|_| Quantity::new(1.0, 0));
    let price_precision = tick_size_to_precision(first_details.min_tick);
    let price_increment = Price::new(first_details.min_tick, price_precision);
    let timestamp = timestamp_ns.unwrap_or_else(|| get_atomic_clock_realtime().get_time_ns());

    Ok(FuturesSpread::new_checked(
        instrument_id,
        Symbol::from(instrument_id.symbol.as_str()),
        AssetClass::Index,
        None,
        underlying,
        Ustr::from("SPREAD"),
        UnixNanos::new(0),
        UnixNanos::new(0),
        currency,
        price_precision,
        price_increment,
        multiplier,
        Quantity::new(1.0, 0),
        None,
        None,
        None,
        None,
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        bag_contract.map(ib_contract_info_for_contract),
        timestamp,
        timestamp,
    )?)
}

pub fn parse_spread_instrument_any(
    instrument_id: InstrumentId,
    leg_contract_details: &[(&ibapi::contracts::ContractDetails, i32)],
    bag_contract: Option<&ibapi::contracts::Contract>,
    timestamp_ns: Option<UnixNanos>,
) -> anyhow::Result<InstrumentAny> {
    let has_future = leg_contract_details.iter().any(|(details, _)| {
        matches!(
            details.contract.security_type,
            SecurityType::Future | SecurityType::ContinuousFuture
        )
    });

    if has_future {
        Ok(InstrumentAny::from(parse_futures_spread_instrument_id(
            instrument_id,
            leg_contract_details,
            bag_contract,
            timestamp_ns,
        )?))
    } else {
        Ok(InstrumentAny::from(parse_option_spread_instrument_id(
            instrument_id,
            leg_contract_details,
            bag_contract,
            timestamp_ns,
        )?))
    }
}

/// Parse CFD contract (CFD).
fn parse_cfd_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let size_precision = tick_size_to_precision(details.min_size);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let base_currency = details
        .contract
        .local_symbol
        .contains('.')
        .then(|| Currency::from(details.contract.symbol.to_string()));

    let instrument = Cfd::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        sec_type_to_asset_class(details.under_security_type.as_str()),
        base_currency,
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        size_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(details.size_increment, size_precision),
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
        Some(ib_contract_info(details)),
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Parse commodity contract (CMDTY).
fn parse_commodity_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let size_precision = tick_size_to_precision(details.min_size);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = Commodity::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        AssetClass::Commodity,
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        size_precision,
        Price::new(details.min_tick, price_precision),
        Quantity::new(details.size_increment, size_precision),
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
        Some(ib_contract_info(details)),
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}

/// Parse bond contract (BOND).
fn parse_bond_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    // Use Equity as a placeholder until Bond type is available in Rust model
    // Note: This is a limitation of the current Nautilus Rust model, not the IB adapter
    let price_precision = tick_size_to_precision(details.min_tick);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = Equity::new(
        instrument_id,
        Symbol::from(details.contract.local_symbol.as_str()),
        None, // isin - could extract from security_id if available
        Currency::from(details.contract.currency.to_string()),
        price_precision,
        Price::new(details.min_tick, price_precision),
        Some(Quantity::new(1.0, 0)),     // Standard lot size for bonds
        None,                            // max_quantity
        None,                            // min_quantity
        None,                            // max_price
        None,                            // min_price
        None,                            // margin_init
        None,                            // margin_maint
        None,                            // maker_fee
        None,                            // taker_fee
        Some(ib_contract_info(details)), // info
        timestamp,
        timestamp,
    );

    InstrumentAny::from(instrument)
}
