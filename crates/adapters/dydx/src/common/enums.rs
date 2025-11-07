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

//! Enumerations mapping dYdX v4 concepts onto idiomatic Nautilus variants.

use nautilus_model::enums::{LiquiditySide, OrderStatus, PositionSide};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// dYdX order status throughout its lifecycle.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxOrderStatus {
    /// Order is open and active.
    Open,
    /// Order is filled completely.
    Filled,
    /// Order is canceled.
    Canceled,
    /// Order is best effort canceled (short-term orders).
    BestEffortCanceled,
    /// Order is partially filled.
    PartiallyFilled,
    /// Order is best effort opened (pending confirmation).
    BestEffortOpened,
    /// Order is untriggered (conditional orders).
    Untriggered,
}

impl From<DydxOrderStatus> for OrderStatus {
    fn from(value: DydxOrderStatus) -> Self {
        match value {
            DydxOrderStatus::Open | DydxOrderStatus::BestEffortOpened => Self::Accepted,
            DydxOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            DydxOrderStatus::Filled => Self::Filled,
            DydxOrderStatus::Canceled | DydxOrderStatus::BestEffortCanceled => Self::Canceled,
            DydxOrderStatus::Untriggered => Self::PendingUpdate,
        }
    }
}

/// dYdX time-in-force specifications.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxTimeInForce {
    /// Good-Til-Time (GTT) - order expires at specified time.
    Gtt,
    /// Fill-Or-Kill (FOK) - must fill completely immediately or cancel.
    Fok,
    /// Immediate-Or-Cancel (IOC) - fill immediately, cancel remainder.
    Ioc,
}

/// dYdX order side.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxOrderSide {
    /// Buy order.
    Buy,
    /// Sell order.
    Sell,
}

/// dYdX order type.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxOrderType {
    /// Limit order with specified price.
    Limit,
    /// Market order (executed at best available price).
    Market,
    /// Stop-limit order (triggered at stop price, executed as limit).
    StopLimit,
    /// Stop-market order (triggered at stop price, executed as market).
    StopMarket,
    /// Take-profit order (limit).
    TakeProfitLimit,
    /// Take-profit order (market).
    TakeProfitMarket,
    /// Trailing stop order.
    TrailingStop,
}

/// dYdX position status.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxPositionStatus {
    /// Position is open.
    Open,
    /// Position is closed.
    Closed,
    /// Position was liquidated.
    Liquidated,
}

impl From<DydxPositionStatus> for PositionSide {
    fn from(value: DydxPositionStatus) -> Self {
        match value {
            DydxPositionStatus::Open => Self::Long, // Default, actual side from position size
            DydxPositionStatus::Closed => Self::Flat,
            DydxPositionStatus::Liquidated => Self::Flat,
        }
    }
}

/// dYdX perpetual market status.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxMarketStatus {
    /// Market is active and trading.
    Active,
    /// Market is paused (no trading).
    Paused,
    /// Cancel-only mode (no new orders).
    CancelOnly,
    /// Post-only mode (only maker orders).
    PostOnly,
    /// Market is initializing.
    Initializing,
    /// Market is in final settlement.
    FinalSettlement,
}

/// dYdX fill type.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxFillType {
    /// Normal limit order fill.
    Limit,
    /// Liquidation (taker side).
    Liquidated,
    /// Liquidation (maker side).
    Liquidation,
    /// Deleveraging (deleveraged account).
    Deleveraged,
    /// Deleveraging (offsetting account).
    Offsetting,
}

/// dYdX liquidity side (maker/taker).
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxLiquidity {
    /// Maker (provides liquidity).
    Maker,
    /// Taker (removes liquidity).
    Taker,
}

impl From<DydxLiquidity> for LiquiditySide {
    fn from(value: DydxLiquidity) -> Self {
        match value {
            DydxLiquidity::Maker => Self::Maker,
            DydxLiquidity::Taker => Self::Taker,
        }
    }
}

impl From<LiquiditySide> for DydxLiquidity {
    fn from(value: LiquiditySide) -> Self {
        match value {
            LiquiditySide::Maker => Self::Maker,
            LiquiditySide::Taker => Self::Taker,
            LiquiditySide::NoLiquiditySide => Self::Taker, // Default fallback
        }
    }
}

/// dYdX ticker type for market data.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DydxTickerType {
    /// Perpetual market ticker.
    Perpetual,
}

/// dYdX candlestick resolution.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum DydxCandleResolution {
    /// 1 minute candles.
    #[serde(rename = "1MIN")]
    #[strum(serialize = "1MIN")]
    #[default]
    OneMinute,
    /// 5 minute candles.
    #[serde(rename = "5MINS")]
    #[strum(serialize = "5MINS")]
    FiveMinutes,
    /// 15 minute candles.
    #[serde(rename = "15MINS")]
    #[strum(serialize = "15MINS")]
    FifteenMinutes,
    /// 30 minute candles.
    #[serde(rename = "30MINS")]
    #[strum(serialize = "30MINS")]
    ThirtyMinutes,
    /// 1 hour candles.
    #[serde(rename = "1HOUR")]
    #[strum(serialize = "1HOUR")]
    OneHour,
    /// 4 hour candles.
    #[serde(rename = "4HOURS")]
    #[strum(serialize = "4HOURS")]
    FourHours,
    /// 1 day candles.
    #[serde(rename = "1DAY")]
    #[strum(serialize = "1DAY")]
    OneDay,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_order_status_conversion() {
        assert_eq!(
            OrderStatus::from(DydxOrderStatus::Open),
            OrderStatus::Accepted
        );
        assert_eq!(
            OrderStatus::from(DydxOrderStatus::Filled),
            OrderStatus::Filled
        );
        assert_eq!(
            OrderStatus::from(DydxOrderStatus::Canceled),
            OrderStatus::Canceled
        );
    }

    #[rstest]
    fn test_liquidity_conversion() {
        assert_eq!(
            LiquiditySide::from(DydxLiquidity::Maker),
            LiquiditySide::Maker
        );
        assert_eq!(
            LiquiditySide::from(DydxLiquidity::Taker),
            LiquiditySide::Taker
        );
    }
}
