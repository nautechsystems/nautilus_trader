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

use anyhow::Context;
use ibapi::contracts::SecurityType;
use jiff::{
    Timestamp,
    civil::DateTime,
    tz::{AmbiguousOffset, Offset},
};
use nautilus_core::{
    DurationNanos, UnixNanos, datetime::get_timezone, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::{
        Cfd, Commodity, CurrencyPair, Equity, FuturesContract, FuturesSpread, IndexInstrument,
        InstrumentAny, OptionContract, OptionSpread,
    },
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::common::{
    contract_to_params,
    enums::{IbOptionRight, IbSecurityType},
};

const NINETY_DAYS: DurationNanos = DurationNanos::from_days(90);

/// Convert tick size to precision value.
#[must_use]
pub fn tick_size_to_precision(tick_size: f64) -> u8 {
    if tick_size <= 0.0 {
        return 8; // Default precision for zero or negative tick sizes
    }

    // Count decimal places
    let s = format!("{tick_size:.10}");
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

    let contract_timezone = details
        .map(|details| details.time_zone_id.as_str())
        .filter(|timezone| !timezone.is_empty())
        .unwrap_or("UTC");

    let (date, time, timezone) = if expiry.len() == 8 {
        let session_end = details.and_then(|details| {
            details
                .trading_hours
                .iter()
                .find(|session| session.starts_with(expiry))
                .and_then(|session| session.split_once('-'))
                .map(|(_, end)| end)
        });
        let (date, time) = match session_end {
            Some(end) => end.split_once(':').unwrap_or((expiry, end)),
            None => (expiry, "0000"),
        };
        let time = match time.len() {
            4 => format!("{}:{}:00", &time[0..2], &time[2..4]),
            _ => anyhow::bail!("Invalid expiry session end '{time}' for {expiry}"),
        };
        (date, time, contract_timezone)
    } else {
        let mut parts = expiry.split_whitespace();
        let date = parts.next().context("Expiry timestamp is missing a date")?;
        let time = parts
            .next()
            .context("Expiry timestamp is missing a time")?
            .to_string();
        let timezone = parts.next().unwrap_or(contract_timezone);
        if parts.next().is_some() {
            anyhow::bail!("Invalid expiry format: {expiry}");
        }
        (date, time, timezone)
    };

    let datetime = DateTime::strptime("%Y%m%d %H:%M:%S", format!("{date} {time}"))
        .with_context(|| format!("Invalid expiry timestamp: {expiry}"))?;
    let timestamp = localize_expiry(datetime, timezone, expiry)?;
    let nanos = u64::try_from(timestamp.as_nanosecond())
        .with_context(|| format!("Expiry timestamp precedes the Unix epoch: {expiry}"))?;
    Ok(UnixNanos::new(nanos))
}

fn localize_expiry(datetime: DateTime, timezone: &str, expiry: &str) -> anyhow::Result<Timestamp> {
    if timezone.eq_ignore_ascii_case("UTC") || timezone.eq_ignore_ascii_case("Z") {
        return Ok(Offset::UTC.to_timestamp(datetime)?);
    }

    let zone = get_timezone(timezone).with_context(|| {
        format!("Unknown IB contract timezone '{timezone}' for expiry {expiry}")
    })?;
    let ambiguous = zone.to_ambiguous_timestamp(datetime);
    match ambiguous.offset() {
        AmbiguousOffset::Unambiguous { .. } => Ok(ambiguous.unambiguous()?),
        AmbiguousOffset::Fold { .. } => Ok(ambiguous.earlier()?),
        AmbiguousOffset::Gap { .. } => {
            anyhow::bail!("Expiry {expiry} does not exist in timezone '{timezone}'")
        }
    }
}

/// Parse an IB ContractDetails to a Nautilus instrument.
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
        SecurityType::Future | SecurityType::ContinuousFuture => {
            parse_futures_contract(details, instrument_id)
        }
        SecurityType::Option => parse_option_contract(details, instrument_id),
        SecurityType::FuturesOption => parse_option_contract(details, instrument_id), // FOP uses same parsing as OPT
        SecurityType::Index => Ok(parse_index_contract(details, instrument_id)),
        SecurityType::CFD => Ok(parse_cfd_contract(details, instrument_id)),
        SecurityType::Commodity => Ok(parse_commodity_contract(details, instrument_id)),
        SecurityType::Bond => Ok(parse_bond_contract(details, instrument_id)),
        _ => anyhow::bail!("Unsupported security type: {sec_type:?}"),
    }
}

fn ib_contract_info(details: &ibapi::contracts::ContractDetails) -> nautilus_core::Params {
    let mut info = ib_contract_info_for_contract(&details.contract);
    info.insert(
        "priceMagnifier".to_string(),
        serde_json::Value::from(details.price_magnifier),
    );
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
    match IbSecurityType::from_str(sec_type).ok() {
        Some(IbSecurityType::Stock) => AssetClass::Equity,
        Some(IbSecurityType::Index) => AssetClass::Index,
        Some(IbSecurityType::ForexPair) => AssetClass::FX,
        Some(IbSecurityType::Bond) => AssetClass::Debt,
        Some(IbSecurityType::Commodity) => AssetClass::Commodity,
        Some(IbSecurityType::Future) => AssetClass::Index,
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

    let instrument = Equity::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        // Standard lot size for stocks
        .lot_size(Quantity::new(100.0, 0))
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    InstrumentAny::from(instrument)
}

/// Parse forex contract (CASH).
fn parse_forex_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let (size_precision, size_increment, min_quantity) = parse_contract_size_rules(details, 1.0);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .base_currency(Currency::from(details.contract.symbol.to_string()))
        .quote_currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .size_increment(size_increment)
        .maybe_min_quantity(min_quantity)
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    InstrumentAny::from(instrument)
}

/// Parse crypto contract (CRYPTO).
fn parse_crypto_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let (size_precision, size_increment, min_quantity) =
        parse_contract_size_rules(details, 0.00000001);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .base_currency(Currency::from(details.contract.symbol.to_string()))
        .quote_currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .size_increment(size_increment)
        .maybe_min_quantity(min_quantity)
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    InstrumentAny::from(instrument)
}

fn parse_contract_multiplier(multiplier: &str, default: f64) -> Quantity {
    if multiplier.is_empty() {
        return Quantity::new(default, 0);
    }

    Quantity::from_str(multiplier).unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to parse IB contract multiplier '{multiplier}', using default {default}: {e}"
        );
        Quantity::new(default, 0)
    })
}

/// Parse futures contract (FUT).
fn parse_futures_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> anyhow::Result<InstrumentAny> {
    let price_precision = tick_size_to_precision(details.min_tick);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let expiry = &details.contract.last_trade_date_or_contract_month;
    let expiration_ns = match expiry_timestring_to_unix_nanos(expiry, Some(details)) {
        Ok(expiration_ns) => expiration_ns,
        // Continuous futures can report contract details without a last-trade date.
        Err(e)
            if matches!(
                details.contract.security_type,
                SecurityType::ContinuousFuture
            ) =>
        {
            tracing::warn!(
                "Continuous future {} reported expiry '{expiry}', defaulting to 90 days out: {e}",
                details.contract.symbol.as_str(),
            );
            timestamp + NINETY_DAYS
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to parse futures expiry '{expiry}'"));
        }
    };

    let activation_ns = activation_from_expiration(expiration_ns);

    let multiplier = parse_contract_multiplier(&details.contract.multiplier, 1.0);

    let raw_symbol = if matches!(
        details.contract.security_type,
        SecurityType::ContinuousFuture
    ) && !details.contract.symbol.as_str().is_empty()
    {
        details.contract.symbol.as_str()
    } else {
        details.contract.local_symbol.as_str()
    };

    let instrument = FuturesContract::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(raw_symbol))
        .asset_class(sec_type_to_asset_class(
            details.under_security_type.as_str(),
        ))
        .underlying(Ustr::from(details.under_symbol.as_str()))
        .activation_ns(activation_ns)
        .expiration_ns(expiration_ns)
        .currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .multiplier(multiplier)
        .lot_size(Quantity::new(1.0, 0))
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    Ok(InstrumentAny::from(instrument))
}

/// Parse option contract (OPT).
fn parse_option_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> anyhow::Result<InstrumentAny> {
    let price_precision = tick_size_to_precision(details.min_tick);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let expiry = &details.contract.last_trade_date_or_contract_month;
    let expiration_ns = expiry_timestring_to_unix_nanos(expiry, Some(details))
        .with_context(|| format!("Failed to parse option expiry '{expiry}'"))?;

    let activation_ns = activation_from_expiration(expiration_ns);

    // Parse option kind (CALL or PUT)
    let option_kind = details
        .contract
        .right
        .map(|right| IbOptionRight::from_str(right.as_str()))
        .transpose()?
        .context("Option contract missing right")?
        .option_kind();

    let multiplier = parse_contract_multiplier(&details.contract.multiplier, 100.0);
    let asset_class = sec_type_to_asset_class(details.under_security_type.as_str());
    let underlying =
        if details.under_security_type == "IND" && !details.under_symbol.starts_with('^') {
            format!("^{}", details.under_symbol)
        } else {
            details.under_symbol.clone()
        };

    let instrument = OptionContract::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .asset_class(asset_class)
        .underlying(Ustr::from(underlying.as_str()))
        .option_kind(option_kind)
        .strike_price(Price::new(details.contract.strike, price_precision))
        .currency(Currency::from(details.contract.currency.to_string()))
        .activation_ns(activation_ns)
        .expiration_ns(expiration_ns)
        .price_precision(price_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .multiplier(multiplier)
        .lot_size(multiplier)
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    Ok(InstrumentAny::from(instrument))
}

fn activation_from_expiration(expiration_ns: UnixNanos) -> UnixNanos {
    expiration_ns.checked_sub(NINETY_DAYS).unwrap_or_default()
}

fn parse_contract_size_rules(
    details: &ibapi::contracts::ContractDetails,
    default_increment: f64,
) -> (u8, Quantity, Option<Quantity>) {
    let size_increment = details
        .size_increment
        .or(details.min_size)
        .unwrap_or(default_increment);
    let size_precision = details.min_size.map_or_else(
        || tick_size_to_precision(size_increment),
        |min_size| tick_size_to_precision(min_size).max(tick_size_to_precision(size_increment)),
    );
    let min_quantity = details
        .min_size
        .map(|min_size| Quantity::new(min_size, size_precision));

    (
        size_precision,
        Quantity::new(size_increment, size_precision),
        min_quantity,
    )
}

#[cfg(test)]
#[allow(
    clippy::items_after_test_module,
    reason = "private parser tests remain beside the contract helpers they exercise"
)]
mod tests {
    use ibapi::contracts::{
        Contract, ContractDetails, Currency, Exchange, OptionRight, SecurityType, Symbol,
    };
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol as NautilusSymbol, Venue},
        instruments::{Instrument, InstrumentAny},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::{
        expiry_timestring_to_unix_nanos, parse_contract_multiplier,
        parse_ib_contract_to_instrument, parse_option_spread_instrument_id,
    };

    #[rstest]
    fn test_parse_crypto_contract_creates_spot_currency_pair() {
        let details = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("BTC"),
                security_type: SecurityType::Crypto,
                exchange: Exchange::from("PAXOS"),
                currency: Currency::from("USD"),
                local_symbol: String::from("BTC.USD"),
                ..Default::default()
            },
            min_tick: 0.01,
            min_size: Some(0.0001),
            size_increment: Some(0.0001),
            ..Default::default()
        };
        let instrument_id = InstrumentId::from("BTC/USD.PAXOS");

        let instrument = parse_ib_contract_to_instrument(&details, instrument_id).unwrap();
        let InstrumentAny::CurrencyPair(pair) = instrument else {
            panic!("expected spot currency pair");
        };

        assert_eq!(pair.base_currency.code.as_str(), "BTC");
        assert_eq!(pair.quote_currency.code.as_str(), "USD");
        assert_eq!(pair.size_precision, 4);
        assert_eq!(pair.size_increment, Quantity::from("0.0001"));
    }

    #[rstest]
    fn test_expiry_session_end_uses_contract_timezone() {
        let details = ContractDetails {
            time_zone_id: String::from("America/New_York"),
            trading_hours: vec![String::from("20260313:0930-20260313:1600")],
            ..Default::default()
        };

        let expiry = expiry_timestring_to_unix_nanos("20260313", Some(&details)).unwrap();
        let expected = "2026-03-13T20:00:00Z".parse::<jiff::Timestamp>().unwrap();

        assert_eq!(
            expiry.as_u64(),
            u64::try_from(expected.as_nanosecond()).unwrap()
        );
    }

    #[rstest]
    fn test_parse_future_rejects_missing_expiry() {
        let details = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("ES"),
                security_type: SecurityType::Future,
                exchange: Exchange::from("CME"),
                currency: Currency::from("USD"),
                local_symbol: String::from("ESZ6"),
                ..Default::default()
            },
            min_tick: 0.25,
            ..Default::default()
        };

        let result = parse_ib_contract_to_instrument(&details, InstrumentId::from("ESZ6.CME"));

        assert_eq!(
            result.unwrap_err().to_string(),
            "Failed to parse futures expiry ''"
        );
    }

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
                right: Some(OptionRight::Put),
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

    #[rstest]
    fn test_parse_contract_preserves_price_magnifier_in_info() {
        let details = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("AAPL"),
                security_type: SecurityType::Stock,
                exchange: Exchange::from("SMART"),
                primary_exchange: Exchange::from("NASDAQ"),
                currency: Currency::from("USD"),
                local_symbol: String::from("AAPL"),
                ..Default::default()
            },
            min_tick: 0.01,
            price_magnifier: 100,
            ..Default::default()
        };
        let instrument_id = InstrumentId::new(NautilusSymbol::from("AAPL"), Venue::from("XNAS"));

        let instrument = parse_ib_contract_to_instrument(&details, instrument_id).unwrap();
        let InstrumentAny::Equity(equity) = instrument else {
            panic!("expected equity");
        };

        assert_eq!(
            equity.info.unwrap().get("priceMagnifier"),
            Some(&serde_json::Value::from(100))
        );
    }

    #[rstest]
    #[case("100", 100.0)]
    #[case("", 1.0)]
    #[case("not-a-number", 1.0)]
    fn test_parse_contract_multiplier_uses_quantity_parser(
        #[case] multiplier: &str,
        #[case] expected: f64,
    ) {
        assert_eq!(
            parse_contract_multiplier(multiplier, 1.0),
            Quantity::new(expected, 0)
        );
    }

    #[rstest]
    fn test_parse_continuous_future_contract_uses_symbol_as_raw_symbol() {
        let details = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("ES"),
                security_type: SecurityType::ContinuousFuture,
                exchange: Exchange::from("CME"),
                currency: Currency::from("USD"),
                local_symbol: String::new(),
                multiplier: "50".to_string(),
                ..Default::default()
            },
            min_tick: 0.25,
            under_symbol: "ES".to_string(),
            under_security_type: "IND".to_string(),
            ..Default::default()
        };
        let instrument_id = InstrumentId::new(NautilusSymbol::from("ES"), Venue::from("CME"));

        let instrument = parse_ib_contract_to_instrument(&details, instrument_id).unwrap();

        let InstrumentAny::FuturesContract(future) = instrument else {
            panic!("expected futures contract");
        };

        assert_eq!(future.raw_symbol().as_str(), "ES");
    }

    #[rstest]
    fn test_parse_option_spread_uses_minimum_leg_tick() {
        let leg1 = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("SPY"),
                security_type: SecurityType::Option,
                exchange: Exchange::from("SMART"),
                currency: Currency::from("USD"),
                local_symbol: "SPY   260120C00400000".to_string(),
                multiplier: "100".to_string(),
                ..Default::default()
            },
            min_tick: 0.05,
            under_symbol: "SPY".to_string(),
            ..Default::default()
        };
        let leg2 = ContractDetails {
            contract: Contract {
                symbol: Symbol::from("SPY"),
                security_type: SecurityType::Option,
                exchange: Exchange::from("SMART"),
                currency: Currency::from("USD"),
                local_symbol: "SPY   260120C00410000".to_string(),
                multiplier: "100".to_string(),
                ..Default::default()
            },
            min_tick: 0.01,
            under_symbol: "SPY".to_string(),
            ..Default::default()
        };
        let instrument_id =
            InstrumentId::from("(1)SPY   260120C00400000_((-1))SPY   260120C00410000.SMART");

        let spread = parse_option_spread_instrument_id(
            instrument_id,
            &[(&leg1, 1), (&leg2, -1)],
            None,
            None,
        )
        .unwrap();

        assert_eq!(spread.price_precision(), 2);
        assert_eq!(spread.price_increment(), Price::from("0.01"));
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
    let (size_precision, size_increment, _) = parse_contract_size_rules(details, 1.0);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = IndexInstrument::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .size_increment(size_increment)
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    InstrumentAny::from(instrument)
}

/// Parse a spread instrument ID into an OptionSpread instrument.
///
/// This implements the same logic as Python's `parse_spread_instrument_id`.
/// Uses contract details from the first leg to determine spread properties.
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
    let underlying = if first_details.under_symbol.is_empty() {
        Ustr::from(first_contract.symbol.as_str())
    } else {
        Ustr::from(first_details.under_symbol.as_str())
    };

    // Parse multiplier
    let multiplier_str = first_contract.multiplier.clone();
    let multiplier =
        Quantity::from_str(&multiplier_str).unwrap_or_else(|_| Quantity::new(100.0, 0)); // Default to 100 for options

    // Determine asset class based on security type
    let asset_class = match first_contract.security_type {
        ibapi::contracts::SecurityType::FuturesOption => AssetClass::Index, // Futures options
        _ => AssetClass::Equity,                                            // Equity options
    };

    // Calculate price precision and increment from the finest leg tick.
    let min_tick = leg_contract_details
        .iter()
        .map(|(details, _)| details.min_tick)
        .fold(first_details.min_tick, f64::min);
    let price_precision = tick_size_to_precision(min_tick);
    let price_increment = Price::new(min_tick, price_precision);

    // Use provided timestamp or current time
    let timestamp = timestamp_ns.unwrap_or_else(|| get_atomic_clock_realtime().get_time_ns());

    // For options spreads, lot size equals multiplier (same as individual option contracts)
    let lot_size = multiplier;

    // Create the spread instrument
    let spread = OptionSpread::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(instrument_id.symbol.as_str()))
        .asset_class(asset_class)
        .underlying(underlying)
        .strategy_type(Ustr::from("SPREAD"))
        // activation_ns (spreads don't have single activation dates)
        .activation_ns(UnixNanos::new(0))
        // expiration_ns (spreads don't have single expiration dates)
        .expiration_ns(UnixNanos::new(0))
        .currency(currency)
        .price_precision(price_precision)
        .price_increment(price_increment)
        .multiplier(multiplier)
        .lot_size(lot_size)
        .margin_init(Decimal::ZERO)
        .margin_maint(Decimal::ZERO)
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()?;

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
    let underlying = if first_details.under_symbol.is_empty() {
        Ustr::from(first_contract.symbol.as_str())
    } else {
        Ustr::from(first_details.under_symbol.as_str())
    };
    let multiplier =
        Quantity::from_str(&first_contract.multiplier).unwrap_or_else(|_| Quantity::new(1.0, 0));
    let min_tick = leg_contract_details
        .iter()
        .map(|(details, _)| details.min_tick)
        .fold(first_details.min_tick, f64::min);
    let price_precision = tick_size_to_precision(min_tick);
    let price_increment = Price::new(min_tick, price_precision);
    let timestamp = timestamp_ns.unwrap_or_else(|| get_atomic_clock_realtime().get_time_ns());

    Ok(FuturesSpread::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(instrument_id.symbol.as_str()))
        .asset_class(AssetClass::Index)
        .underlying(underlying)
        .strategy_type(Ustr::from("SPREAD"))
        .activation_ns(UnixNanos::new(0))
        .expiration_ns(UnixNanos::new(0))
        .currency(currency)
        .price_precision(price_precision)
        .price_increment(price_increment)
        .multiplier(multiplier)
        .lot_size(Quantity::new(1.0, 0))
        .margin_init(Decimal::ZERO)
        .margin_maint(Decimal::ZERO)
        .maybe_info(bag_contract.map(ib_contract_info_for_contract))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()?)
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
    let (size_precision, size_increment, min_quantity) = parse_contract_size_rules(details, 1.0);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let base_currency = details
        .contract
        .local_symbol
        .contains('.')
        .then(|| Currency::from(details.contract.symbol.to_string()));

    let instrument = Cfd::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .asset_class(sec_type_to_asset_class(
            details.under_security_type.as_str(),
        ))
        .maybe_base_currency(base_currency)
        .quote_currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .size_increment(size_increment)
        .maybe_min_quantity(min_quantity)
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    InstrumentAny::from(instrument)
}

/// Parse commodity contract (CMDTY).
fn parse_commodity_contract(
    details: &ibapi::contracts::ContractDetails,
    instrument_id: InstrumentId,
) -> InstrumentAny {
    let price_precision = tick_size_to_precision(details.min_tick);
    let (size_precision, size_increment, min_quantity) = parse_contract_size_rules(details, 1.0);
    let timestamp = get_atomic_clock_realtime().get_time_ns();

    let instrument = Commodity::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .asset_class(AssetClass::Commodity)
        .quote_currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .size_precision(size_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        .size_increment(size_increment)
        .maybe_min_quantity(min_quantity)
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

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

    // ISIN could be extracted from `security_id` if needed
    let instrument = Equity::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(details.contract.local_symbol.as_str()))
        .currency(Currency::from(details.contract.currency.to_string()))
        .price_precision(price_precision)
        .price_increment(Price::new(details.min_tick, price_precision))
        // Standard lot size for bonds
        .lot_size(Quantity::new(1.0, 0))
        .info(ib_contract_info(details))
        .ts_event(timestamp)
        .ts_init(timestamp)
        .build()
        .unwrap();

    InstrumentAny::from(instrument)
}
