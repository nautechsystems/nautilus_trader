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

use std::{num::NonZero, str::FromStr};

use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    data::BarSpecification,
    enums::{
        AggressorSide, BarAggregation, CurrencyType, LiquiditySide, OrderSide, PositionSide,
        PriceType,
    },
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

pub const BAR_SPEC_1_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
pub const BAR_SPEC_5_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(5).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
pub const BAR_SPEC_30_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(30).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
pub const BAR_SPEC_2_HOUR: BarSpecification = BarSpecification {
    step: NonZero::new(2).unwrap(),
    aggregation: BarAggregation::Hour,
    price_type: PriceType::Last,
};
pub const BAR_SPEC_1_DAY: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Day,
    price_type: PriceType::Last,
};

/// Custom deserializer for strings to u64.
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

/// Returns the currency either from the internal currency map or creates a default crypto.
pub fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(code)
        .copied()
        .unwrap_or(Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

/// Parses a Nautilus instrument ID from the given Coinbase `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *COINBASE_INTX_VENUE)
}

pub fn parse_millisecond_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let millis: u64 = timestamp.parse()?;
    Ok(UnixNanos::from(millis * NANOSECONDS_IN_MILLISECOND))
}

pub fn parse_rfc3339_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)?;
    Ok(UnixNanos::from(dt.timestamp_nanos_opt().unwrap() as u64))
}

pub fn parse_price(value: &str) -> anyhow::Result<Price> {
    Price::from_str(value).map_err(|e| anyhow::anyhow!(e))
}

pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    Quantity::new_checked(value.parse::<f64>()?, precision)
}

pub fn parse_notional(value: &str, currency: Currency) -> anyhow::Result<Option<Money>> {
    let parsed = value.trim().parse::<f64>()?;
    Ok(if parsed == 0.0 {
        None
    } else {
        Some(Money::new(parsed, currency))
    })
}

pub fn parse_aggressor_side(side: &Option<CoinbaseIntxSide>) -> AggressorSide {
    match side {
        Some(CoinbaseIntxSide::Buy) => nautilus_model::enums::AggressorSide::Buyer,
        Some(CoinbaseIntxSide::Sell) => nautilus_model::enums::AggressorSide::Seller,
        None => nautilus_model::enums::AggressorSide::NoAggressor,
    }
}

pub fn parse_execution_type(liquidity: &Option<CoinbaseIntxExecType>) -> LiquiditySide {
    match liquidity {
        Some(CoinbaseIntxExecType::Maker) => nautilus_model::enums::LiquiditySide::Maker,
        Some(CoinbaseIntxExecType::Taker) => nautilus_model::enums::LiquiditySide::Taker,
        _ => nautilus_model::enums::LiquiditySide::NoLiquiditySide,
    }
}

pub fn parse_position_side(current_qty: Option<f64>) -> PositionSide {
    match current_qty {
        Some(qty) if qty.is_sign_positive() => PositionSide::Long,
        Some(qty) if qty.is_sign_negative() => PositionSide::Short,
        _ => PositionSide::Flat,
    }
}

pub fn parse_order_side(order_side: &Option<CoinbaseIntxSide>) -> OrderSide {
    match order_side {
        Some(CoinbaseIntxSide::Buy) => OrderSide::Buy,
        Some(CoinbaseIntxSide::Sell) => OrderSide::Sell,
        None => OrderSide::NoOrderSide,
    }
}

pub fn bar_spec_as_coinbase_channel(
    bar_spec: BarSpecification,
) -> anyhow::Result<CoinbaseIntxWsChannel> {
    let channel = match bar_spec {
        BAR_SPEC_1_MINUTE => CoinbaseIntxWsChannel::CandlesOneMinute,
        BAR_SPEC_5_MINUTE => CoinbaseIntxWsChannel::CandlesFiveMinute,
        BAR_SPEC_30_MINUTE => CoinbaseIntxWsChannel::CandlesThirtyMinute,
        BAR_SPEC_2_HOUR => CoinbaseIntxWsChannel::CandlesTwoHour,
        BAR_SPEC_1_DAY => CoinbaseIntxWsChannel::CandlesOneDay,
        _ => anyhow::bail!("Invalid `BarSpecification` for channel, was {bar_spec}"),
    };
    Ok(channel)
}

pub fn coinbase_channel_as_bar_spec(
    channel: &CoinbaseIntxWsChannel,
) -> anyhow::Result<BarSpecification> {
    let bar_spec = match channel {
        CoinbaseIntxWsChannel::CandlesOneMinute => BAR_SPEC_1_MINUTE,
        CoinbaseIntxWsChannel::CandlesFiveMinute => BAR_SPEC_5_MINUTE,
        CoinbaseIntxWsChannel::CandlesThirtyMinute => BAR_SPEC_30_MINUTE,
        CoinbaseIntxWsChannel::CandlesTwoHour => BAR_SPEC_2_HOUR,
        CoinbaseIntxWsChannel::CandlesOneDay => BAR_SPEC_1_DAY,
        // TODO: Complete remainder
        _ => anyhow::bail!("Invalid channel for `BarSpecification`, was {channel}"),
    };
    Ok(bar_spec)
}
