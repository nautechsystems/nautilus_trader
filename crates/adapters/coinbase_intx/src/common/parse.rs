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

use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{
        BarSpecification,
        bar::{
            BAR_SPEC_1_DAY_LAST, BAR_SPEC_1_MINUTE_LAST, BAR_SPEC_2_HOUR_LAST,
            BAR_SPEC_5_MINUTE_LAST, BAR_SPEC_30_MINUTE_LAST,
        },
    },
    enums::{AggressorSide, LiquiditySide, PositionSide},
    identifiers::{InstrumentId, Symbol},
    types::{Currency, Money, Price, Quantity},
};
use serde::{Deserialize, Deserializer};
use ustr::Ustr;

use crate::{
    common::{
        consts::COINBASE_INTX_VENUE,
        enums::{CoinbaseIntxExecType, CoinbaseIntxSide},
    },
    websocket::enums::CoinbaseIntxWsChannel,
};

/// Custom deserializer for strings to u64.
///
/// # Errors
///
/// Returns a deserialization error if the JSON string is invalid or cannot be parsed to u64.
pub fn deserialize_optional_string_to_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Returns a currency from the internal map or creates a new crypto currency.
///
/// Uses [`Currency::get_or_create_crypto`] to handle unknown currency codes,
/// which automatically registers newly listed Coinbase International Exchange assets.
pub fn get_currency(code: &str) -> Currency {
    Currency::get_or_create_crypto(code)
}

/// Parses a Nautilus instrument ID from the given Coinbase `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *COINBASE_INTX_VENUE)
}

/// Parses a timestamp in milliseconds since epoch into `UnixNanos`.
///
/// # Errors
///
/// Returns an error if the input string is not a valid unsigned integer.
pub fn parse_millisecond_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let millis: u64 = timestamp.parse()?;
    Ok(UnixNanos::from(millis * NANOSECONDS_IN_MILLISECOND))
}

/// Parses an RFC3339 timestamp string into `UnixNanos`.
///
/// # Errors
///
/// Returns an error if the input string is not a valid RFC3339 timestamp or is out of range.
pub fn parse_rfc3339_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)?;
    let nanos = dt
        .timestamp_nanos_opt()
        .ok_or_else(|| anyhow::anyhow!("RFC3339 timestamp out of range: {timestamp}"))?;
    Ok(UnixNanos::from(nanos as u64))
}

/// Parses a string into a `Price`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a floating point value.
pub fn parse_price(value: &str) -> anyhow::Result<Price> {
    Price::from_str(value).map_err(|e| anyhow::anyhow!(e))
}

/// Parses a string into a `Quantity` with the given precision.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a floating point value.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    Quantity::new_checked(value.parse::<f64>()?, precision)
}

/// Parses a notional string into `Money`, returning `None` if the value is zero.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a floating point value.
pub fn parse_notional(value: &str, currency: Currency) -> anyhow::Result<Option<Money>> {
    let parsed = value.trim().parse::<f64>()?;
    Ok(if parsed == 0.0 {
        None
    } else {
        Some(Money::new(parsed, currency))
    })
}

#[must_use]
pub const fn parse_aggressor_side(side: &Option<CoinbaseIntxSide>) -> AggressorSide {
    match side {
        Some(CoinbaseIntxSide::Buy) => nautilus_model::enums::AggressorSide::Buyer,
        Some(CoinbaseIntxSide::Sell) => nautilus_model::enums::AggressorSide::Seller,
        None => nautilus_model::enums::AggressorSide::NoAggressor,
    }
}

#[must_use]
pub const fn parse_execution_type(liquidity: &Option<CoinbaseIntxExecType>) -> LiquiditySide {
    match liquidity {
        Some(CoinbaseIntxExecType::Maker) => nautilus_model::enums::LiquiditySide::Maker,
        Some(CoinbaseIntxExecType::Taker) => nautilus_model::enums::LiquiditySide::Taker,
        _ => nautilus_model::enums::LiquiditySide::NoLiquiditySide,
    }
}

#[must_use]
pub const fn parse_position_side(current_qty: Option<f64>) -> PositionSide {
    match current_qty {
        Some(qty) if qty.is_sign_positive() => PositionSide::Long,
        Some(qty) if qty.is_sign_negative() => PositionSide::Short,
        _ => PositionSide::Flat,
    }
}

/// Converts a `BarSpecification` into the corresponding Coinbase WebSocket channel.
///
/// # Errors
///
/// Returns an error if the specification is not one of the supported candle intervals.
pub fn bar_spec_as_coinbase_channel(
    bar_spec: BarSpecification,
) -> anyhow::Result<CoinbaseIntxWsChannel> {
    let channel = match bar_spec {
        BAR_SPEC_1_MINUTE_LAST => CoinbaseIntxWsChannel::CandlesOneMinute,
        BAR_SPEC_5_MINUTE_LAST => CoinbaseIntxWsChannel::CandlesFiveMinute,
        BAR_SPEC_30_MINUTE_LAST => CoinbaseIntxWsChannel::CandlesThirtyMinute,
        BAR_SPEC_2_HOUR_LAST => CoinbaseIntxWsChannel::CandlesTwoHour,
        BAR_SPEC_1_DAY_LAST => CoinbaseIntxWsChannel::CandlesOneDay,
        _ => anyhow::bail!("Invalid `BarSpecification` for channel, was {bar_spec}"),
    };
    Ok(channel)
}

/// Converts a Coinbase WebSocket channel into the corresponding `BarSpecification`.
///
/// # Errors
///
/// Returns an error if the channel is not one of the supported candle channels.
pub fn coinbase_channel_as_bar_spec(
    channel: &CoinbaseIntxWsChannel,
) -> anyhow::Result<BarSpecification> {
    let bar_spec = match channel {
        CoinbaseIntxWsChannel::CandlesOneMinute => BAR_SPEC_1_MINUTE_LAST,
        CoinbaseIntxWsChannel::CandlesFiveMinute => BAR_SPEC_5_MINUTE_LAST,
        CoinbaseIntxWsChannel::CandlesThirtyMinute => BAR_SPEC_30_MINUTE_LAST,
        CoinbaseIntxWsChannel::CandlesTwoHour => BAR_SPEC_2_HOUR_LAST,
        CoinbaseIntxWsChannel::CandlesOneDay => BAR_SPEC_1_DAY_LAST,
        _ => anyhow::bail!("Invalid channel for `BarSpecification`, was {channel}"),
    };
    Ok(bar_spec)
}
