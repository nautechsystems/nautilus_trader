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

use nautilus_model::enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use crate::error::DydxError;

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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx", eq, eq_int)
)]
pub enum DydxOrderSide {
    /// Buy order.
    Buy,
    /// Sell order.
    Sell,
}

impl TryFrom<OrderSide> for DydxOrderSide {
    type Error = DydxError;

    fn try_from(value: OrderSide) -> Result<Self, Self::Error> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            _ => Err(DydxError::InvalidOrderSide(format!("{value:?}"))),
        }
    }
}

impl DydxOrderSide {
    /// Try to convert from Nautilus `OrderSide`.
    ///
    /// # Errors
    ///
    /// Returns an error if the order side is not `Buy` or `Sell`.
    pub fn try_from_order_side(value: OrderSide) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl From<DydxOrderSide> for OrderSide {
    fn from(side: DydxOrderSide) -> Self {
        match side {
            DydxOrderSide::Buy => Self::Buy,
            DydxOrderSide::Sell => Self::Sell,
        }
    }
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx", eq, eq_int)
)]
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

impl TryFrom<OrderType> for DydxOrderType {
    type Error = DydxError;

    fn try_from(value: OrderType) -> Result<Self, Self::Error> {
        match value {
            OrderType::Market => Ok(Self::Market),
            OrderType::Limit => Ok(Self::Limit),
            OrderType::StopMarket => Ok(Self::StopMarket),
            OrderType::StopLimit => Ok(Self::StopLimit),
            OrderType::MarketIfTouched => Ok(Self::TakeProfitMarket),
            OrderType::LimitIfTouched => Ok(Self::TakeProfitLimit),
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit => Ok(Self::TrailingStop),
            OrderType::MarketToLimit => Err(DydxError::UnsupportedOrderType(format!("{value:?}"))),
        }
    }
}

impl DydxOrderType {
    /// Try to convert from Nautilus `OrderType`.
    ///
    /// # Errors
    ///
    /// Returns an error if the order type is not supported by dYdX.
    pub fn try_from_order_type(value: OrderType) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl From<DydxOrderType> for OrderType {
    fn from(value: DydxOrderType) -> Self {
        match value {
            DydxOrderType::Market => Self::Market,
            DydxOrderType::Limit => Self::Limit,
            DydxOrderType::StopMarket => Self::StopMarket,
            DydxOrderType::StopLimit => Self::StopLimit,
            DydxOrderType::TakeProfitMarket => Self::MarketIfTouched,
            DydxOrderType::TakeProfitLimit => Self::LimitIfTouched,
            DydxOrderType::TrailingStop => Self::TrailingStopMarket,
        }
    }
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

    #[rstest]
    fn test_order_side_conversion_from_nautilus() {
        assert_eq!(
            DydxOrderSide::try_from(OrderSide::Buy).unwrap(),
            DydxOrderSide::Buy
        );
        assert_eq!(
            DydxOrderSide::try_from(OrderSide::Sell).unwrap(),
            DydxOrderSide::Sell
        );
        assert!(DydxOrderSide::try_from(OrderSide::NoOrderSide).is_err());
    }

    #[rstest]
    fn test_order_side_conversion_to_nautilus() {
        assert_eq!(OrderSide::from(DydxOrderSide::Buy), OrderSide::Buy);
        assert_eq!(OrderSide::from(DydxOrderSide::Sell), OrderSide::Sell);
    }

    #[rstest]
    fn test_order_type_conversion_from_nautilus() {
        assert_eq!(
            DydxOrderType::try_from(OrderType::Market).unwrap(),
            DydxOrderType::Market
        );
        assert_eq!(
            DydxOrderType::try_from(OrderType::Limit).unwrap(),
            DydxOrderType::Limit
        );
        assert_eq!(
            DydxOrderType::try_from(OrderType::StopMarket).unwrap(),
            DydxOrderType::StopMarket
        );
        assert_eq!(
            DydxOrderType::try_from(OrderType::StopLimit).unwrap(),
            DydxOrderType::StopLimit
        );
        assert!(DydxOrderType::try_from(OrderType::MarketToLimit).is_err());
    }

    #[rstest]
    fn test_order_type_conversion_to_nautilus() {
        assert_eq!(OrderType::from(DydxOrderType::Market), OrderType::Market);
        assert_eq!(OrderType::from(DydxOrderType::Limit), OrderType::Limit);
        assert_eq!(
            OrderType::from(DydxOrderType::StopMarket),
            OrderType::StopMarket
        );
        assert_eq!(
            OrderType::from(DydxOrderType::StopLimit),
            OrderType::StopLimit
        );
    }
}
