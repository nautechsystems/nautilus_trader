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
    enums::{OrderSide, PositionSide},
    identifiers::{InstrumentId, Symbol},
};

use crate::{
    consts::BITMEX_VENUE,
    enums::{ContingencyType, LiquidityIndicator, OrderStatus, OrderType, Side, TimeInForce},
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

pub fn parse_aggressor_side(side: &Option<Side>) -> nautilus_model::enums::AggressorSide {
    match side {
        Some(Side::Buy) => nautilus_model::enums::AggressorSide::Buyer,
        Some(Side::Sell) => nautilus_model::enums::AggressorSide::Seller,
        None => nautilus_model::enums::AggressorSide::NoAggressor,
    }
}

pub fn parse_liquidity_side(
    liquidity: &Option<LiquidityIndicator>,
) -> nautilus_model::enums::LiquiditySide {
    match liquidity {
        Some(LiquidityIndicator::Maker) => nautilus_model::enums::LiquiditySide::Maker,
        Some(LiquidityIndicator::Taker) => nautilus_model::enums::LiquiditySide::Taker,
        _ => nautilus_model::enums::LiquiditySide::NoLiquiditySide,
    }
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
pub fn parse_time_in_force(tif: &TimeInForce) -> nautilus_model::enums::TimeInForce {
    match tif {
        TimeInForce::Day => nautilus_model::enums::TimeInForce::Day,
        TimeInForce::GoodTillCancel => nautilus_model::enums::TimeInForce::Gtc,
        TimeInForce::GoodTillDate => nautilus_model::enums::TimeInForce::Gtd,
        TimeInForce::ImmediateOrCancel => nautilus_model::enums::TimeInForce::Ioc,
        TimeInForce::FillOrKill => nautilus_model::enums::TimeInForce::Fok,
        TimeInForce::AtTheOpening => nautilus_model::enums::TimeInForce::AtTheOpen,
        TimeInForce::AtTheClose => nautilus_model::enums::TimeInForce::AtTheClose,
        _ => panic!("Unsupported `TimeInForce`, was {tif}"),
    }
}

pub fn parse_order_side(order_side: &Option<Side>) -> OrderSide {
    match order_side {
        Some(Side::Buy) => OrderSide::Buy,
        Some(Side::Sell) => OrderSide::Sell,
        None => OrderSide::NoOrderSide,
    }
}

pub fn parse_order_type(order_type: &OrderType) -> nautilus_model::enums::OrderType {
    match order_type {
        OrderType::Market => nautilus_model::enums::OrderType::Market,
        OrderType::Limit => nautilus_model::enums::OrderType::Limit,
        OrderType::Stop => nautilus_model::enums::OrderType::StopMarket,
        OrderType::StopLimit => nautilus_model::enums::OrderType::StopLimit,
        OrderType::MarketIfTouched => nautilus_model::enums::OrderType::MarketIfTouched,
        OrderType::LimitIfTouched => nautilus_model::enums::OrderType::LimitIfTouched,
        OrderType::Pegged => nautilus_model::enums::OrderType::Limit,
    }
}

pub fn parse_order_status(order_status: &OrderStatus) -> nautilus_model::enums::OrderStatus {
    match order_status {
        OrderStatus::New => nautilus_model::enums::OrderStatus::Accepted,
        OrderStatus::PartiallyFilled => nautilus_model::enums::OrderStatus::PartiallyFilled,
        OrderStatus::Filled => nautilus_model::enums::OrderStatus::Filled,
        OrderStatus::Canceled => nautilus_model::enums::OrderStatus::Canceled,
        OrderStatus::Rejected => nautilus_model::enums::OrderStatus::Rejected,
        OrderStatus::Expired => nautilus_model::enums::OrderStatus::Expired,
    }
}

pub fn parse_contingency_type(
    contingency_type: &ContingencyType,
) -> nautilus_model::enums::ContingencyType {
    match contingency_type {
        ContingencyType::OneCancelsTheOther => nautilus_model::enums::ContingencyType::Oco,
        ContingencyType::OneTriggersTheOther => nautilus_model::enums::ContingencyType::Oto,
        ContingencyType::OneUpdatesTheOtherProportional => {
            nautilus_model::enums::ContingencyType::Ouo
        }
        ContingencyType::OneUpdatesTheOtherAbsolute => nautilus_model::enums::ContingencyType::Ouo,
        ContingencyType::Unknown => nautilus_model::enums::ContingencyType::NoContingency,
    }
}
