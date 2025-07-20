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

use nautilus_model::enums::{
    AggressorSide, LiquiditySide, OptionKind, OrderSide, OrderStatus, OrderType, PositionSide,
};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Represents the type of book action.
#[derive(
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
#[serde(rename_all = "lowercase")]
pub enum OKXBookAction {
    /// Incremental update.
    Update,
    /// Full snapshot.
    Snapshot,
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
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
pub enum OKXCandleConfirm {
    /// K-line is incomplete.
    #[serde(rename = "0")]
    Partial,
    /// K-line is completed.
    #[serde(rename = "1")]
    Closed,
}

/// Represents the side of an order or trade (Buy/Sell).
#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXSide {
    /// Buy side of a trade or order.
    Buy,
    /// Sell side of a trade or order.
    Sell,
}

impl From<OrderSide> for OKXSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Buy => Self::Buy,
            OrderSide::Sell => Self::Sell,
            _ => panic!("Invalid `OrderSide`"),
        }
    }
}

impl From<OKXSide> for AggressorSide {
    fn from(value: OKXSide) -> Self {
        match value {
            OKXSide::Buy => Self::Buyer,
            OKXSide::Sell => Self::Seller,
        }
    }
}

/// Represents the available order types on OKX.
#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXOrderType {
    /// Market order, executed immediately at current market price.
    Market,
    /// Limit order, executed only at specified price or better.
    Limit,
    PostOnly,        // limit only, requires "px" to be provided
    Fok,             // Market order if "px" is not provided, otherwise limit order
    Ioc,             // Market order if "px" is not provided, otherwise limit order
    OptimalLimitIoc, // Market order with immediate-or-cancel order
    Mmp,             // Market Maker Protection (only applicable to Option in Portfolio Margin mode)
    MmpAndPostOnly, // Market Maker Protection and Post-only order(only applicable to Option in Portfolio Margin mode)
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXOrderStatus {
    /// Order has been canceled by user or system.
    Canceled,
    Live,
    PartiallyFilled,
    Filled,
    MmpCanceled,
}

impl From<OrderStatus> for OKXOrderStatus {
    fn from(value: OrderStatus) -> Self {
        match value {
            OrderStatus::Canceled => Self::Canceled,
            OrderStatus::Accepted => Self::Live,
            OrderStatus::PartiallyFilled => Self::PartiallyFilled,
            OrderStatus::Filled => Self::Filled,
            _ => panic!("Invalid `OrderStatus`"),
        }
    }
}

/// Represents the type of execution that generated a trade.
#[derive(
    Clone,
    Debug,
    Default,
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
pub enum OKXExecType {
    #[serde(rename = "")]
    #[default]
    None,
    #[serde(rename = "T")]
    Taker,
    #[serde(rename = "M")]
    Maker,
}

impl From<LiquiditySide> for OKXExecType {
    fn from(value: LiquiditySide) -> Self {
        match value {
            LiquiditySide::NoLiquiditySide => Self::None,
            LiquiditySide::Taker => Self::Taker,
            LiquiditySide::Maker => Self::Maker,
        }
    }
}

/// Represents instrument types on OKX.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Default,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXInstrumentType {
    #[default]
    Any,
    /// Spot products.
    Spot,
    /// Margin products.
    Margin,
    /// Swap products.
    Swap,
    /// Futures products.
    Futures,
    /// Option products.
    Option,
}

/// Represents an instrument status on OKX.
#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXInstrumentStatus {
    Live,
    Suspend,
    Preopen,
    Test,
}

/// Represents an instrument contract type on OKX.
#[derive(
    Copy,
    Clone,
    Default,
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
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXContractType {
    #[serde(rename = "")]
    #[default]
    None,
    Linear,
    Inverse,
}

/// Represents an option type on OKX.
#[derive(
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
pub enum OKXOptionType {
    #[serde(rename = "")]
    None,
    #[serde(rename = "C")]
    Call,
    #[serde(rename = "P")]
    Put,
}

impl From<OKXOptionType> for OptionKind {
    fn from(option_type: OKXOptionType) -> Self {
        match option_type {
            OKXOptionType::Call => OptionKind::Call,
            OKXOptionType::Put => OptionKind::Put,
            _ => panic!("Invalid `option_type`, was None"),
        }
    }
}

/// Represents the trading mode for OKX orders.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Default,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXTradeMode {
    #[default]
    Cash,
    Isolated,
    Cross,
    SpotIsolated,
}

/// Represents an OKX account mode.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#overview-account-mode>
#[derive(
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
pub enum OKXAccountMode {
    #[serde(rename = "Spot mode")]
    Spot,
    #[serde(rename = "Spot and futures mode")]
    SpotAndFutures,
    #[serde(rename = "Multi-currency margin mode")]
    MultiCurrencyMarginMode,
    #[serde(rename = "Portfolio margin mode")]
    PortfolioMarginMode,
}

/// Represents the margin mode for OKX accounts.
///
/// # Reference
///
/// - <https://www.okx.com/en-au/help/iv-isolated-margin-mode>
/// - <https://www.okx.com/en-au/help/iii-single-currency-margin-cross-margin-trading>
/// - <https://www.okx.com/en-au/help/iv-multi-currency-margin-mode-cross-margin-trading>
#[derive(
    Copy,
    Clone,
    Default,
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
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXMarginMode {
    #[serde(rename = "")]
    #[default]
    None,
    Isolated,
    Cross,
}

/// Represents the position mode for OKX accounts.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-set-position-mode>
#[derive(
    Copy,
    Clone,
    Default,
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXPositionMode {
    #[default]
    #[serde(rename = "net_mode")]
    NetMode,
    #[serde(rename = "long_short_mode")]
    LongShortMode,
}

#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXPositionSide {
    #[serde(rename = "")]
    None,
    Net,
    Long,
    Short,
}

#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXSelfTradePreventionMode {
    #[serde(rename = "")]
    None,
    CancelMaker,
    CancelTaker,
    CancelBoth,
}

#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXTakeProfitKind {
    #[serde(rename = "")]
    None,
    Condition,
    Limit,
}

#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXTriggerType {
    #[serde(rename = "")]
    None,
    Last,
    Index,
    Mark,
}

impl From<OKXSide> for OrderSide {
    fn from(side: OKXSide) -> Self {
        match side {
            OKXSide::Buy => Self::Buy,
            OKXSide::Sell => Self::Sell,
        }
    }
}

impl From<OKXExecType> for LiquiditySide {
    fn from(exec: OKXExecType) -> Self {
        match exec {
            OKXExecType::Maker => Self::Maker,
            OKXExecType::Taker => Self::Taker,
            OKXExecType::None => Self::NoLiquiditySide,
        }
    }
}

impl From<OKXPositionSide> for PositionSide {
    fn from(side: OKXPositionSide) -> Self {
        match side {
            OKXPositionSide::Long => Self::Long,
            OKXPositionSide::Short => Self::Short,
            _ => Self::Flat,
        }
    }
}

impl From<OKXOrderStatus> for OrderStatus {
    fn from(status: OKXOrderStatus) -> Self {
        match status {
            OKXOrderStatus::Live => Self::Accepted,
            OKXOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            OKXOrderStatus::Filled => Self::Filled,
            OKXOrderStatus::Canceled | OKXOrderStatus::MmpCanceled => Self::Canceled,
        }
    }
}

impl From<OKXOrderType> for OrderType {
    fn from(ord_type: OKXOrderType) -> Self {
        match ord_type {
            OKXOrderType::Market => Self::Market,
            OKXOrderType::Limit
            | OKXOrderType::PostOnly
            | OKXOrderType::OptimalLimitIoc
            | OKXOrderType::Mmp
            | OKXOrderType::MmpAndPostOnly => Self::Limit,
            OKXOrderType::Fok | OKXOrderType::Ioc => Self::MarketToLimit,
        }
    }
}

impl From<OrderType> for OKXOrderType {
    fn from(value: OrderType) -> Self {
        match value {
            OrderType::Market => Self::Market,
            OrderType::Limit => Self::Limit,
            OrderType::MarketToLimit => Self::Ioc,
            _ => panic!("Invalid `OrderType` cannot be represented on OKX"),
        }
    }
}

impl From<PositionSide> for OKXPositionSide {
    fn from(value: PositionSide) -> Self {
        match value {
            PositionSide::Long => Self::Long,
            PositionSide::Short => Self::Short,
            _ => Self::None,
        }
    }
}

#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXAlgoOrderType {
    Conditional,
    Oco,
    Trigger,
    MoveOrderStop,
    Iceberg,
    Twap,
}

#[derive(
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
#[serde(rename_all = "snake_case")]
pub enum OKXAlgoOrderStatus {
    Live,
    Pause,
    PartiallyEffective,
    Effective,
    Canceled,
    OrderFailed,
    PartiallyFailed,
}

#[derive(
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
pub enum OKXTransactionType {
    #[serde(rename = "1")]
    Buy,
    #[serde(rename = "2")]
    Sell,
    #[serde(rename = "3")]
    OpenLong,
    #[serde(rename = "4")]
    OpenShort,
    #[serde(rename = "5")]
    CloseLong,
    #[serde(rename = "6")]
    CloseShort,
    #[serde(rename = "100")]
    PartialLiquidationCloseLong,
    #[serde(rename = "101")]
    PartialLiquidationCloseShort,
    #[serde(rename = "102")]
    PartialLiquidationBuy,
    #[serde(rename = "103")]
    PartialLiquidationSell,
    #[serde(rename = "104")]
    LiquidationLong,
    #[serde(rename = "105")]
    LiquidationShort,
    #[serde(rename = "106")]
    LiquidationBuy,
    #[serde(rename = "107")]
    LiquidationSell,
    #[serde(rename = "110")]
    LiquidationTransferIn,
    #[serde(rename = "111")]
    LiquidationTransferOut,
    #[serde(rename = "118")]
    SystemTokenConversionTransferIn,
    #[serde(rename = "119")]
    SystemTokenConversionTransferOut,
    #[serde(rename = "125")]
    AdlCloseLong,
    #[serde(rename = "126")]
    AdlCloseShort,
    #[serde(rename = "127")]
    AdlBuy,
    #[serde(rename = "128")]
    AdlSell,
    #[serde(rename = "212")]
    AutoBorrowOfQuickMargin,
    #[serde(rename = "213")]
    AutoRepayOfQuickMargin,
    #[serde(rename = "204")]
    BlockTradeBuy,
    #[serde(rename = "205")]
    BlockTradeSell,
    #[serde(rename = "206")]
    BlockTradeOpenLong,
    #[serde(rename = "207")]
    BlockTradeOpenShort,
    #[serde(rename = "208")]
    BlockTradeCloseOpen,
    #[serde(rename = "209")]
    BlockTradeCloseShort,
    #[serde(rename = "270")]
    SpreadTradingBuy,
    #[serde(rename = "271")]
    SpreadTradingSell,
    #[serde(rename = "272")]
    SpreadTradingOpenLong,
    #[serde(rename = "273")]
    SpreadTradingOpenShort,
    #[serde(rename = "274")]
    SpreadTradingCloseLong,
    #[serde(rename = "275")]
    SpreadTradingCloseShort,
}

#[derive(
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
pub enum OKXBarSize {
    #[serde(rename = "1s")]
    Second1,
    #[serde(rename = "1m")]
    Minute1,
    #[serde(rename = "3m")]
    Minute3,
    #[serde(rename = "5m")]
    Minute5,
    #[serde(rename = "15m")]
    Minute15,
    #[serde(rename = "30m")]
    Minute30,
    #[serde(rename = "1H")]
    Hour1,
    #[serde(rename = "2H")]
    Hour2,
    #[serde(rename = "4H")]
    Hour4,
    #[serde(rename = "6H")]
    Hour6,
    #[serde(rename = "12H")]
    Hour12,
    #[serde(rename = "1D")]
    Day1,
    #[serde(rename = "2D")]
    Day2,
    #[serde(rename = "3D")]
    Day3,
    #[serde(rename = "5D")]
    Day5,
    #[serde(rename = "1W")]
    Week1,
    #[serde(rename = "1M")]
    Month1,
    #[serde(rename = "3M")]
    Month3,
}
