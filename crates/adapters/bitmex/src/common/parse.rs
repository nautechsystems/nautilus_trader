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

use chrono::{DateTime, Utc};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::PositionSide,
    identifiers::{InstrumentId, Symbol},
};

use crate::common::{
    consts::BITMEX_VENUE,
    enums::{
        BitmexContingencyType, BitmexLiquidityIndicator, BitmexOrderStatus, BitmexOrderType,
        BitmexSide, BitmexTimeInForce,
    },
};

/// Parses a Nautilus instrument ID from the given BitMEX `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from_str_unchecked(symbol), *BITMEX_VENUE)
}

/// Parses the given datetime (UTC) into a `UnixNanos` timestamp.
/// If `value` is `None`, then defaults to the UNIX epoch (0 nanoseconds).
///
/// # Panics
///
/// Panics if the timestamp cannot be converted to nanoseconds (should never happen with valid timestamps).
pub fn parse_optional_datetime_to_unix_nanos(
    value: &Option<DateTime<Utc>>,
    field: &str,
) -> UnixNanos {
    value
        .map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| panic!("Invalid timestamp for `{field}`"))
                    as u64,
            )
        })
        .unwrap_or_default()
}

pub fn parse_aggressor_side(side: &Option<BitmexSide>) -> nautilus_model::enums::AggressorSide {
    match side {
        Some(BitmexSide::Buy) => nautilus_model::enums::AggressorSide::Buyer,
        Some(BitmexSide::Sell) => nautilus_model::enums::AggressorSide::Seller,
        None => nautilus_model::enums::AggressorSide::NoAggressor,
    }
}

pub fn parse_liquidity_side(
    liquidity: &Option<BitmexLiquidityIndicator>,
) -> nautilus_model::enums::LiquiditySide {
    liquidity
        .map(|l| l.into())
        .unwrap_or(nautilus_model::enums::LiquiditySide::NoLiquiditySide)
}

pub fn parse_position_side(current_qty: Option<i64>) -> nautilus_model::enums::PositionSide {
    match current_qty {
        Some(qty) if qty > 0 => PositionSide::Long,
        Some(qty) if qty < 0 => PositionSide::Short,
        _ => PositionSide::Flat,
    }
}

/// Parse a BitMEX time in force into a Nautilus time in force.
///
/// # Panics
///
/// Panics if an unsupported `TimeInForce` variant is encountered.
pub fn parse_time_in_force(tif: &BitmexTimeInForce) -> nautilus_model::enums::TimeInForce {
    (*tif).into()
}

pub fn parse_order_type(order_type: &BitmexOrderType) -> nautilus_model::enums::OrderType {
    (*order_type).into()
}

pub fn parse_order_status(order_status: &BitmexOrderStatus) -> nautilus_model::enums::OrderStatus {
    (*order_status).into()
}

pub fn parse_contingency_type(
    contingency_type: &BitmexContingencyType,
) -> nautilus_model::enums::ContingencyType {
    (*contingency_type).into()
}
