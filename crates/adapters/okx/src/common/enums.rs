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

//! Enumerations mapping OKX concepts onto idiomatic Nautilus variants.

use nautilus_model::enums::{
    AggressorSide, LiquiditySide, OptionKind, OrderSide, OrderStatus, OrderType, PositionSide,
    TriggerType,
};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use crate::common::consts::OKX_CONDITIONAL_ORDER_TYPES;

/// Represents the type of book action.
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
#[serde(rename_all = "lowercase")]
pub enum OKXBookAction {
    /// Incremental update.
    Update,
    /// Full snapshot.
    Snapshot,
}

/// Represents the possible states of an order throughout its lifecycle.
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
    Trigger,        // Conditional/algo order (stop orders, etc.)
}

/// Represents the possible states of an order throughout its lifecycle.
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
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXOrderStatus {
    Canceled,
    Live,
    Effective,
    PartiallyFilled,
    Filled,
    MmpCanceled,
    OrderPlaced,
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
    Copy,
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
            OKXOptionType::Call => Self::Call,
            OKXOptionType::Put => Self::Put,
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
#[strum(ascii_case_insensitive)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.okx")
)]
pub enum OKXTradeMode {
    #[default]
    Cash,
    Isolated,
    Cross,
    #[strum(serialize = "spot_isolated")]
    SpotIsolated,
}

/// Represents an OKX account mode.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#overview-account-mode>
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
#[serde(rename_all = "snake_case")]
pub enum OKXPositionSide {
    #[serde(rename = "")]
    None,
    Net,
    Long,
    Short,
}

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
#[serde(rename_all = "snake_case")]
pub enum OKXSelfTradePreventionMode {
    #[serde(rename = "")]
    None,
    CancelMaker,
    CancelTaker,
    CancelBoth,
}

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
#[serde(rename_all = "snake_case")]
pub enum OKXTakeProfitKind {
    #[serde(rename = "")]
    None,
    Condition,
    Limit,
}

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
#[serde(rename_all = "snake_case")]
pub enum OKXTriggerType {
    #[serde(rename = "")]
    None,
    Last,
    Index,
    Mark,
}

impl From<TriggerType> for OKXTriggerType {
    fn from(value: TriggerType) -> Self {
        match value {
            TriggerType::LastPrice => Self::Last,
            TriggerType::MarkPrice => Self::Mark,
            TriggerType::IndexPrice => Self::Index,
            _ => Self::Last,
        }
    }
}

/// Represents the target currency for order quantity.
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OKXTargetCurrency {
    /// Base currency.
    BaseCcy,
    /// Quote currency.
    QuoteCcy,
}

/// Represents an OKX order book channel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OKXBookChannel {
    /// Standard depth-first book channel (`books`).
    Book,
    /// Low-latency 400-depth channel (`books-l2-tbt`).
    BookL2Tbt,
    /// Low-latency 50-depth channel (`books50-l2-tbt`).
    Books50L2Tbt,
}

/// Represents OKX VIP level tiers for trading fee structure and API limits.
///
/// VIP levels determine:
/// - Trading fee discounts.
/// - API rate limits.
/// - Access to advanced order book channels (L2/L3 depth).
///
/// Higher VIP levels (VIP4+) get access to:
/// - "books50-l2-tbt" channel (50 depth, 10ms updates).
/// - "bbo-tbt" channel (1 depth, 10ms updates).
///
/// VIP5+ get access to:
/// - "books-l2-tbt" channel (400 depth, 10ms updates).
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub enum OKXVipLevel {
    /// VIP level 0 (default tier).
    #[serde(rename = "0")]
    #[strum(serialize = "0")]
    Vip0 = 0,
    /// VIP level 1.
    #[serde(rename = "1")]
    #[strum(serialize = "1")]
    Vip1 = 1,
    /// VIP level 2.
    #[serde(rename = "2")]
    #[strum(serialize = "2")]
    Vip2 = 2,
    /// VIP level 3.
    #[serde(rename = "3")]
    #[strum(serialize = "3")]
    Vip3 = 3,
    /// VIP level 4 (can access books50-l2-tbt channel).
    #[serde(rename = "4")]
    #[strum(serialize = "4")]
    Vip4 = 4,
    /// VIP level 5 (can access books-l2-tbt channel).
    #[serde(rename = "5")]
    #[strum(serialize = "5")]
    Vip5 = 5,
    /// VIP level 6.
    #[serde(rename = "6")]
    #[strum(serialize = "6")]
    Vip6 = 6,
    /// VIP level 7.
    #[serde(rename = "7")]
    #[strum(serialize = "7")]
    Vip7 = 7,
    /// VIP level 8.
    #[serde(rename = "8")]
    #[strum(serialize = "8")]
    Vip8 = 8,
    /// VIP level 9 (highest tier).
    #[serde(rename = "9")]
    #[strum(serialize = "9")]
    Vip9 = 9,
}

impl From<u8> for OKXVipLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Vip0,
            1 => Self::Vip1,
            2 => Self::Vip2,
            3 => Self::Vip3,
            4 => Self::Vip4,
            5 => Self::Vip5,
            6 => Self::Vip6,
            7 => Self::Vip7,
            8 => Self::Vip8,
            9 => Self::Vip9,
            _ => {
                tracing::warn!("Invalid VIP level {value}, defaulting to Vip0");
                Self::Vip0
            }
        }
    }
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
            OKXOrderStatus::Effective => Self::Triggered,
            OKXOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            OKXOrderStatus::Filled => Self::Filled,
            OKXOrderStatus::Canceled | OKXOrderStatus::MmpCanceled => Self::Canceled,
            OKXOrderStatus::OrderPlaced => Self::Triggered,
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
            | OKXOrderType::MmpAndPostOnly
            | OKXOrderType::Fok
            | OKXOrderType::Ioc => Self::Limit,
            OKXOrderType::Trigger => Self::StopMarket,
        }
    }
}

impl From<OrderType> for OKXOrderType {
    fn from(value: OrderType) -> Self {
        match value {
            OrderType::Market => Self::Market,
            OrderType::Limit => Self::Limit,
            OrderType::MarketToLimit => Self::Ioc,
            // Conditional orders will be handled separately via algo orders
            OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => {
                panic!("Conditional order types must use OKXAlgoOrderType")
            }
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
#[serde(rename_all = "snake_case")]
pub enum OKXAlgoOrderType {
    Conditional,
    Oco,
    Trigger,
    MoveOrderStop,
    Iceberg,
    Twap,
}

/// Helper to determine if an order type requires algo order handling.
pub fn is_conditional_order(order_type: OrderType) -> bool {
    OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
}

/// Converts Nautilus conditional order types to OKX algo order type.
///
/// # Errors
///
/// Returns an error if the provided `order_type` is not a conditional order type.
pub fn conditional_order_to_algo_type(order_type: OrderType) -> anyhow::Result<OKXAlgoOrderType> {
    match order_type {
        OrderType::StopMarket
        | OrderType::StopLimit
        | OrderType::MarketIfTouched
        | OrderType::LimitIfTouched => Ok(OKXAlgoOrderType::Trigger),
        _ => anyhow::bail!("Not a conditional order type: {order_type:?}"),
    }
}

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

/// Represents the category of an order on OKX.
///
/// The category field indicates whether an order is a normal trade, liquidation,
/// auto-deleveraging (ADL) event, or algorithmic order type. This is critical for
/// risk management and proper handling of exchange-generated orders.
///
/// # References
///
/// <https://www.okx.com/docs-v5/en/#order-book-trading-ws-order-channel>
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
#[serde(rename_all = "snake_case")]
pub enum OKXOrderCategory {
    /// Normal trading order.
    Normal,
    /// Full liquidation order (position completely closed by exchange).
    FullLiquidation,
    /// Partial liquidation order (position partially closed by exchange).
    PartialLiquidation,
    /// Auto-deleveraging order (position closed to offset counterparty liquidation).
    Adl,
    /// Time-Weighted Average Price algorithmic order.
    Twap,
    /// Iceberg algorithmic order (hidden quantity).
    Iceberg,
    /// One-Cancels-the-Other algorithmic order.
    Oco,
    /// Conditional/trigger order.
    Conditional,
    /// Move order stop algorithmic order.
    MoveOrderStop,
    /// Delivery and exercise (for futures/options settlement).
    Ddh,
    /// Unknown or future category (graceful fallback).
    #[serde(other)]
    Other,
}

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
