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

use std::{fmt::Display, str::FromStr};

use nautilus_model::enums::{AggressorSide, OrderSide, OrderStatus, OrderType};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use super::consts::HYPERLIQUID_POST_ONLY_WOULD_MATCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HyperliquidBarInterval {
    #[serde(rename = "1m")]
    OneMinute,
    #[serde(rename = "3m")]
    ThreeMinutes,
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "15m")]
    FifteenMinutes,
    #[serde(rename = "30m")]
    ThirtyMinutes,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "2h")]
    TwoHours,
    #[serde(rename = "4h")]
    FourHours,
    #[serde(rename = "8h")]
    EightHours,
    #[serde(rename = "12h")]
    TwelveHours,
    #[serde(rename = "1d")]
    OneDay,
    #[serde(rename = "3d")]
    ThreeDays,
    #[serde(rename = "1w")]
    OneWeek,
    #[serde(rename = "1M")]
    OneMonth,
}

impl HyperliquidBarInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::ThreeMinutes => "3m",
            Self::FiveMinutes => "5m",
            Self::FifteenMinutes => "15m",
            Self::ThirtyMinutes => "30m",
            Self::OneHour => "1h",
            Self::TwoHours => "2h",
            Self::FourHours => "4h",
            Self::EightHours => "8h",
            Self::TwelveHours => "12h",
            Self::OneDay => "1d",
            Self::ThreeDays => "3d",
            Self::OneWeek => "1w",
            Self::OneMonth => "1M",
        }
    }
}

impl FromStr for HyperliquidBarInterval {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1m" => Ok(Self::OneMinute),
            "3m" => Ok(Self::ThreeMinutes),
            "5m" => Ok(Self::FiveMinutes),
            "15m" => Ok(Self::FifteenMinutes),
            "30m" => Ok(Self::ThirtyMinutes),
            "1h" => Ok(Self::OneHour),
            "2h" => Ok(Self::TwoHours),
            "4h" => Ok(Self::FourHours),
            "8h" => Ok(Self::EightHours),
            "12h" => Ok(Self::TwelveHours),
            "1d" => Ok(Self::OneDay),
            "3d" => Ok(Self::ThreeDays),
            "1w" => Ok(Self::OneWeek),
            "1M" => Ok(Self::OneMonth),
            _ => anyhow::bail!("Invalid Hyperliquid bar interval: {s}"),
        }
    }
}

impl Display for HyperliquidBarInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Represents the order side (Buy or Sell).
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
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
pub enum HyperliquidSide {
    #[serde(rename = "B")]
    Buy,
    #[serde(rename = "A")]
    Sell,
}

impl From<OrderSide> for HyperliquidSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Buy => Self::Buy,
            OrderSide::Sell => Self::Sell,
            _ => panic!("Invalid `OrderSide`"),
        }
    }
}

impl From<HyperliquidSide> for OrderSide {
    fn from(value: HyperliquidSide) -> Self {
        match value {
            HyperliquidSide::Buy => Self::Buy,
            HyperliquidSide::Sell => Self::Sell,
        }
    }
}

impl From<HyperliquidSide> for AggressorSide {
    fn from(value: HyperliquidSide) -> Self {
        match value {
            HyperliquidSide::Buy => Self::Buyer,
            HyperliquidSide::Sell => Self::Seller,
        }
    }
}

/// Represents the time in force for limit orders.
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
#[serde(rename_all = "PascalCase")]
#[strum(serialize_all = "PascalCase")]
pub enum HyperliquidTimeInForce {
    /// Add Liquidity Only - post-only order.
    Alo,
    /// Immediate or Cancel - fill immediately or cancel.
    Ioc,
    /// Good Till Cancel - remain on book until filled or cancelled.
    Gtc,
}

/// Represents the order type configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HyperliquidOrderType {
    /// Limit order with time-in-force.
    #[serde(rename = "limit")]
    Limit { tif: HyperliquidTimeInForce },

    /// Trigger order (stop or take profit).
    #[serde(rename = "trigger")]
    Trigger {
        #[serde(rename = "isMarket")]
        is_market: bool,
        #[serde(rename = "triggerPx")]
        trigger_px: String,
        tpsl: HyperliquidTpSl,
    },
}

/// Represents the take profit / stop loss type.
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidTpSl {
    /// Take Profit.
    Tp,
    /// Stop Loss.
    Sl,
}

/// Represents conditional/trigger order types.
///
/// Hyperliquid supports various conditional order types that trigger
/// based on market conditions. These map to Nautilus OrderType variants.
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum HyperliquidConditionalOrderType {
    /// Stop market order (protective stop with market execution).
    StopMarket,
    /// Stop limit order (protective stop with limit price).
    StopLimit,
    /// Take profit market order (profit-taking with market execution).
    TakeProfitMarket,
    /// Take profit limit order (profit-taking with limit price).
    TakeProfitLimit,
    /// Trailing stop market order (dynamic stop with market execution).
    TrailingStopMarket,
    /// Trailing stop limit order (dynamic stop with limit price).
    TrailingStopLimit,
}

impl From<HyperliquidConditionalOrderType> for OrderType {
    fn from(value: HyperliquidConditionalOrderType) -> Self {
        match value {
            HyperliquidConditionalOrderType::StopMarket => Self::StopMarket,
            HyperliquidConditionalOrderType::StopLimit => Self::StopLimit,
            HyperliquidConditionalOrderType::TakeProfitMarket => Self::MarketIfTouched,
            HyperliquidConditionalOrderType::TakeProfitLimit => Self::LimitIfTouched,
            HyperliquidConditionalOrderType::TrailingStopMarket => Self::TrailingStopMarket,
            HyperliquidConditionalOrderType::TrailingStopLimit => Self::TrailingStopLimit,
        }
    }
}

impl From<OrderType> for HyperliquidConditionalOrderType {
    fn from(value: OrderType) -> Self {
        match value {
            OrderType::StopMarket => Self::StopMarket,
            OrderType::StopLimit => Self::StopLimit,
            OrderType::MarketIfTouched => Self::TakeProfitMarket,
            OrderType::LimitIfTouched => Self::TakeProfitLimit,
            OrderType::TrailingStopMarket => Self::TrailingStopMarket,
            OrderType::TrailingStopLimit => Self::TrailingStopLimit,
            _ => panic!("Unsupported OrderType for conditional orders: {value:?}"),
        }
    }
}

/// Represents trailing offset types for trailing stop orders.
///
/// Trailing stops adjust dynamically based on market movement:
/// - Price: Fixed price offset (e.g., $100)
/// - Percentage: Percentage offset (e.g., 5%)
/// - BasisPoints: Basis points offset (e.g., 250 bps = 2.5%)
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidTrailingOffsetType {
    /// Fixed price offset.
    Price,
    /// Percentage offset.
    Percentage,
    /// Basis points offset (1 bp = 0.01%).
    #[serde(rename = "basispoints")]
    #[strum(serialize = "basispoints")]
    BasisPoints,
}

/// Represents the reduce only flag wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HyperliquidReduceOnly(pub bool);

impl HyperliquidReduceOnly {
    /// Creates a new reduce only flag.
    pub fn new(reduce_only: bool) -> Self {
        Self(reduce_only)
    }

    /// Returns whether this is a reduce only order.
    pub fn is_reduce_only(&self) -> bool {
        self.0
    }
}

/// Represents the liquidity flag indicating maker or taker.
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
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidLiquidityFlag {
    Maker,
    Taker,
}

impl From<bool> for HyperliquidLiquidityFlag {
    /// Converts from `crossed` field in fill responses.
    ///
    /// `true` (crossed) -> Taker, `false` -> Maker
    fn from(crossed: bool) -> Self {
        if crossed { Self::Taker } else { Self::Maker }
    }
}

/// Hyperliquid liquidation method.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidLiquidationMethod {
    Market,
    Backstop,
}

/// Hyperliquid position type/mode.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, EnumString,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum HyperliquidPositionType {
    OneWay,
}

/// Hyperliquid TWAP order status.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidTwapStatus {
    Activated,
    Terminated,
    Finished,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidRejectCode {
    /// Price must be divisible by tick size.
    Tick,
    /// Order must have minimum value of $10.
    MinTradeNtl,
    /// Order must have minimum value of 10 {quote_token}.
    MinTradeSpotNtl,
    /// Insufficient margin to place order.
    PerpMargin,
    /// Reduce only order would increase position.
    ReduceOnly,
    /// Post only order would have immediately matched.
    BadAloPx,
    /// Order could not immediately match.
    IocCancel,
    /// Invalid TP/SL price.
    BadTriggerPx,
    /// No liquidity available for market order.
    MarketOrderNoLiquidity,
    /// Position increase at open interest cap.
    PositionIncreaseAtOpenInterestCap,
    /// Position flip at open interest cap.
    PositionFlipAtOpenInterestCap,
    /// Too aggressive at open interest cap.
    TooAggressiveAtOpenInterestCap,
    /// Open interest increase.
    OpenInterestIncrease,
    /// Insufficient spot balance.
    InsufficientSpotBalance,
    /// Oracle issue.
    Oracle,
    /// Perp max position.
    PerpMaxPosition,
    /// Missing order.
    MissingOrder,
    /// Unknown reject reason with raw error message.
    Unknown(String),
}

impl HyperliquidRejectCode {
    /// Parse reject code from Hyperliquid API error message.
    pub fn from_api_error(error_message: &str) -> Self {
        Self::from_error_string_internal(error_message)
    }

    fn from_error_string_internal(error: &str) -> Self {
        // Normalize: trim whitespace and convert to lowercase for robust matching
        let normalized = error.trim().to_lowercase();

        match normalized.as_str() {
            // Tick size validation errors
            s if s.contains("tick size") => Self::Tick,

            // Minimum notional value errors (perp: $10, spot: 10 USDC)
            s if s.contains("minimum value of $10") => Self::MinTradeNtl,
            s if s.contains("minimum value of 10") => Self::MinTradeSpotNtl,

            // Margin errors
            s if s.contains("insufficient margin") => Self::PerpMargin,

            // Reduce-only order violations
            s if s.contains("reduce only order would increase")
                || s.contains("reduce-only order would increase") =>
            {
                Self::ReduceOnly
            }

            // Post-only order matching errors
            s if s.contains(&HYPERLIQUID_POST_ONLY_WOULD_MATCH.to_lowercase())
                || s.contains("post-only order would have immediately matched") =>
            {
                Self::BadAloPx
            }

            // IOC (Immediate-or-Cancel) order errors
            s if s.contains("could not immediately match") => Self::IocCancel,

            // TP/SL trigger price errors
            s if s.contains("invalid tp/sl price") => Self::BadTriggerPx,

            // Market order liquidity errors
            s if s.contains("no liquidity available for market order") => {
                Self::MarketOrderNoLiquidity
            }

            // Open interest cap errors (various types)
            // Note: These patterns are case-insensitive due to normalization
            s if s.contains("positionincreaseatopeninterestcap") => {
                Self::PositionIncreaseAtOpenInterestCap
            }
            s if s.contains("positionflipatopeninterestcap") => Self::PositionFlipAtOpenInterestCap,
            s if s.contains("tooaggressiveatopeninterestcap") => {
                Self::TooAggressiveAtOpenInterestCap
            }
            s if s.contains("openinterestincrease") => Self::OpenInterestIncrease,

            // Spot balance errors
            s if s.contains("insufficient spot balance") => Self::InsufficientSpotBalance,

            // Oracle errors
            s if s.contains("oracle") => Self::Oracle,

            // Position size limit errors
            s if s.contains("max position") => Self::PerpMaxPosition,

            // Missing order errors (cancel/modify non-existent order)
            s if s.contains("missingorder") => Self::MissingOrder,

            // Unknown error - log for monitoring and return with original message
            _ => {
                log::warn!(
                    "Unknown Hyperliquid error pattern (consider updating error parsing): {error}" // Use original error, not normalized
                );
                Self::Unknown(error.to_string())
            }
        }
    }

    /// Parses reject code from error string.
    ///
    /// **Deprecated**: This method uses substring matching which is fragile and not robust.
    /// Use `from_api_error()` instead, which provides a migration path for structured error handling.
    #[deprecated(
        since = "0.50.0",
        note = "String parsing is fragile; use HyperliquidRejectCode::from_api_error() instead"
    )]
    pub fn from_error_string(error: &str) -> Self {
        Self::from_error_string_internal(error)
    }
}

/// Represents Hyperliquid order status from API responses.
///
/// Hyperliquid uses lowercase status values with camelCase for compound words.
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
pub enum HyperliquidOrderStatus {
    /// Order has been accepted and is open.
    #[serde(rename = "open")]
    Open,
    /// Order has been accepted and is open (alternative representation).
    #[serde(rename = "accepted")]
    Accepted,
    /// Order has been triggered (for conditional orders).
    #[serde(rename = "triggered")]
    Triggered,
    /// Order has been completely filled.
    #[serde(rename = "filled")]
    Filled,
    /// Order has been canceled.
    #[serde(rename = "canceled")]
    Canceled,
    /// Order was rejected by the exchange.
    #[serde(rename = "rejected")]
    Rejected,
    // Specific cancel reasons - all map to CANCELED status
    /// Order canceled due to margin requirements.
    #[serde(rename = "marginCanceled")]
    MarginCanceled,
    /// Order canceled due to vault withdrawal.
    #[serde(rename = "vaultWithdrawalCanceled")]
    VaultWithdrawalCanceled,
    /// Order canceled due to open interest cap.
    #[serde(rename = "openInterestCapCanceled")]
    OpenInterestCapCanceled,
    /// Order canceled due to self trade prevention.
    #[serde(rename = "selfTradeCanceled")]
    SelfTradeCanceled,
    /// Order canceled due to reduce only constraint.
    #[serde(rename = "reduceOnlyCanceled")]
    ReduceOnlyCanceled,
    /// Order canceled because sibling order was filled.
    #[serde(rename = "siblingFilledCanceled")]
    SiblingFilledCanceled,
    /// Order canceled due to delisting.
    #[serde(rename = "delistedCanceled")]
    DelistedCanceled,
    /// Order canceled due to liquidation.
    #[serde(rename = "liquidatedCanceled")]
    LiquidatedCanceled,
    /// Order was scheduled for cancel.
    #[serde(rename = "scheduledCancel")]
    ScheduledCancel,
    // Specific reject reasons - all map to REJECTED status
    /// Order rejected due to tick size.
    #[serde(rename = "tickRejected")]
    TickRejected,
    /// Order rejected due to minimum trade notional.
    #[serde(rename = "minTradeNtlRejected")]
    MinTradeNtlRejected,
    /// Order rejected due to perp margin.
    #[serde(rename = "perpMarginRejected")]
    PerpMarginRejected,
    /// Order rejected due to reduce only constraint.
    #[serde(rename = "reduceOnlyRejected")]
    ReduceOnlyRejected,
    /// Order rejected due to bad ALO price.
    #[serde(rename = "badAloPxRejected")]
    BadAloPxRejected,
    /// IOC order canceled and rejected.
    #[serde(rename = "iocCancelRejected")]
    IocCancelRejected,
    /// Order rejected due to bad trigger price.
    #[serde(rename = "badTriggerPxRejected")]
    BadTriggerPxRejected,
    /// Market order rejected due to no liquidity.
    #[serde(rename = "marketOrderNoLiquidityRejected")]
    MarketOrderNoLiquidityRejected,
    /// Order rejected due to open interest cap.
    #[serde(rename = "positionIncreaseAtOpenInterestCapRejected")]
    PositionIncreaseAtOpenInterestCapRejected,
    /// Order rejected due to position flip at open interest cap.
    #[serde(rename = "positionFlipAtOpenInterestCapRejected")]
    PositionFlipAtOpenInterestCapRejected,
    /// Order rejected due to too aggressive at open interest cap.
    #[serde(rename = "tooAggressiveAtOpenInterestCapRejected")]
    TooAggressiveAtOpenInterestCapRejected,
    /// Order rejected due to open interest increase.
    #[serde(rename = "openInterestIncreaseRejected")]
    OpenInterestIncreaseRejected,
    /// Order rejected due to insufficient spot balance.
    #[serde(rename = "insufficientSpotBalanceRejected")]
    InsufficientSpotBalanceRejected,
    /// Order rejected by oracle.
    #[serde(rename = "oracleRejected")]
    OracleRejected,
    /// Order rejected due to perp max position.
    #[serde(rename = "perpMaxPositionRejected")]
    PerpMaxPositionRejected,
}

impl From<HyperliquidOrderStatus> for OrderStatus {
    fn from(status: HyperliquidOrderStatus) -> Self {
        match status {
            HyperliquidOrderStatus::Open | HyperliquidOrderStatus::Accepted => Self::Accepted,
            HyperliquidOrderStatus::Triggered => Self::Triggered,
            HyperliquidOrderStatus::Filled => Self::Filled,
            // All cancel variants map to CANCELED
            HyperliquidOrderStatus::Canceled
            | HyperliquidOrderStatus::MarginCanceled
            | HyperliquidOrderStatus::VaultWithdrawalCanceled
            | HyperliquidOrderStatus::OpenInterestCapCanceled
            | HyperliquidOrderStatus::SelfTradeCanceled
            | HyperliquidOrderStatus::ReduceOnlyCanceled
            | HyperliquidOrderStatus::SiblingFilledCanceled
            | HyperliquidOrderStatus::DelistedCanceled
            | HyperliquidOrderStatus::LiquidatedCanceled
            | HyperliquidOrderStatus::ScheduledCancel => Self::Canceled,
            // All reject variants map to REJECTED
            HyperliquidOrderStatus::Rejected
            | HyperliquidOrderStatus::TickRejected
            | HyperliquidOrderStatus::MinTradeNtlRejected
            | HyperliquidOrderStatus::PerpMarginRejected
            | HyperliquidOrderStatus::ReduceOnlyRejected
            | HyperliquidOrderStatus::BadAloPxRejected
            | HyperliquidOrderStatus::IocCancelRejected
            | HyperliquidOrderStatus::BadTriggerPxRejected
            | HyperliquidOrderStatus::MarketOrderNoLiquidityRejected
            | HyperliquidOrderStatus::PositionIncreaseAtOpenInterestCapRejected
            | HyperliquidOrderStatus::PositionFlipAtOpenInterestCapRejected
            | HyperliquidOrderStatus::TooAggressiveAtOpenInterestCapRejected
            | HyperliquidOrderStatus::OpenInterestIncreaseRejected
            | HyperliquidOrderStatus::InsufficientSpotBalanceRejected
            | HyperliquidOrderStatus::OracleRejected
            | HyperliquidOrderStatus::PerpMaxPositionRejected => Self::Rejected,
        }
    }
}

/// Represents the direction of a fill (open/close position).
///
/// For perpetuals:
/// - OpenLong: Opening a long position
/// - OpenShort: Opening a short position
/// - CloseLong: Closing an existing long position
/// - CloseShort: Closing an existing short position
///
/// For spot:
/// - Sell: Selling an asset
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
#[serde(rename_all = "PascalCase")]
#[strum(serialize_all = "PascalCase")]
pub enum HyperliquidFillDirection {
    /// Opening a long position.
    #[serde(rename = "Open Long")]
    #[strum(serialize = "Open Long")]
    OpenLong,
    /// Opening a short position.
    #[serde(rename = "Open Short")]
    #[strum(serialize = "Open Short")]
    OpenShort,
    /// Closing an existing long position.
    #[serde(rename = "Close Long")]
    #[strum(serialize = "Close Long")]
    CloseLong,
    /// Closing an existing short position.
    #[serde(rename = "Close Short")]
    #[strum(serialize = "Close Short")]
    CloseShort,
    /// Flipping from long to short (position reversal).
    #[serde(rename = "Long > Short")]
    #[strum(serialize = "Long > Short")]
    LongToShort,
    /// Flipping from short to long (position reversal).
    #[serde(rename = "Short > Long")]
    #[strum(serialize = "Short > Long")]
    ShortToLong,
    /// Buying an asset (spot only).
    Buy,
    /// Selling an asset (spot only).
    Sell,
}

/// Represents info request types for the Hyperliquid info endpoint.
///
/// These correspond to the "type" field in info endpoint requests.
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
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum HyperliquidInfoRequestType {
    /// Get metadata about available markets.
    Meta,
    /// Get spot metadata (tokens and pairs).
    SpotMeta,
    /// Get metadata with asset contexts (for price precision).
    MetaAndAssetCtxs,
    /// Get spot metadata with asset contexts.
    SpotMetaAndAssetCtxs,
    /// Get L2 order book for a coin.
    L2Book,
    /// Get all mid prices.
    AllMids,
    /// Get user fills.
    UserFills,
    /// Get user fills by time range.
    UserFillsByTime,
    /// Get order status for a user.
    OrderStatus,
    /// Get all open orders for a user.
    OpenOrders,
    /// Get frontend open orders (includes more detail).
    FrontendOpenOrders,
    /// Get user state (balances, positions, margin).
    ClearinghouseState,
    /// Get spot clearinghouse state.
    SpotClearinghouseState,
    /// Get exchange status.
    ExchangeStatus,
    /// Get candle/bar data snapshot.
    CandleSnapshot,
    /// Get candle/bar data (WS post).
    Candle,
    /// Get recent trades.
    RecentTrades,
    /// Get historical orders.
    HistoricalOrders,
    /// Get funding history.
    FundingHistory,
    /// Get user funding.
    UserFunding,
    /// Get non-user funding updates.
    NonUserFundingUpdates,
    /// Get TWAP history.
    TwapHistory,
    /// Get user TWAP slice fills.
    UserTwapSliceFills,
    /// Get user TWAP slice fills by time range.
    UserTwapSliceFillsByTime,
    /// Get user rate limit.
    UserRateLimit,
    /// Get user role.
    UserRole,
    /// Get delegator history.
    DelegatorHistory,
    /// Get delegator rewards.
    DelegatorRewards,
    /// Get validator stats.
    ValidatorStats,
    /// Get user fee schedule and effective rates.
    UserFees,
}

impl HyperliquidInfoRequestType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::SpotMeta => "spotMeta",
            Self::MetaAndAssetCtxs => "metaAndAssetCtxs",
            Self::SpotMetaAndAssetCtxs => "spotMetaAndAssetCtxs",
            Self::L2Book => "l2Book",
            Self::AllMids => "allMids",
            Self::UserFills => "userFills",
            Self::UserFillsByTime => "userFillsByTime",
            Self::OrderStatus => "orderStatus",
            Self::OpenOrders => "openOrders",
            Self::FrontendOpenOrders => "frontendOpenOrders",
            Self::ClearinghouseState => "clearinghouseState",
            Self::SpotClearinghouseState => "spotClearinghouseState",
            Self::ExchangeStatus => "exchangeStatus",
            Self::CandleSnapshot => "candleSnapshot",
            Self::Candle => "candle",
            Self::RecentTrades => "recentTrades",
            Self::HistoricalOrders => "historicalOrders",
            Self::FundingHistory => "fundingHistory",
            Self::UserFunding => "userFunding",
            Self::NonUserFundingUpdates => "nonUserFundingUpdates",
            Self::TwapHistory => "twapHistory",
            Self::UserTwapSliceFills => "userTwapSliceFills",
            Self::UserTwapSliceFillsByTime => "userTwapSliceFillsByTime",
            Self::UserRateLimit => "userRateLimit",
            Self::UserRole => "userRole",
            Self::DelegatorHistory => "delegatorHistory",
            Self::DelegatorRewards => "delegatorRewards",
            Self::ValidatorStats => "validatorStats",
            Self::UserFees => "userFees",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidLeverageType {
    Cross,
    Isolated,
    #[serde(other)]
    Unknown,
}

/// Hyperliquid product type.
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
pub enum HyperliquidProductType {
    /// Perpetual futures.
    Perp,
    /// Spot markets.
    Spot,
}

impl HyperliquidProductType {
    /// Extract product type from an instrument symbol.
    ///
    /// # Errors
    ///
    /// Returns error if symbol doesn't match expected format.
    pub fn from_symbol(symbol: &str) -> anyhow::Result<Self> {
        if symbol.ends_with("-PERP") {
            Ok(Self::Perp)
        } else if symbol.ends_with("-SPOT") {
            Ok(Self::Spot)
        } else {
            anyhow::bail!("Invalid Hyperliquid symbol format: {symbol}")
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::OrderType;
    use rstest::rstest;
    use serde_json;

    use super::*;

    #[rstest]
    fn test_side_serde() {
        let buy_side = HyperliquidSide::Buy;
        let sell_side = HyperliquidSide::Sell;

        assert_eq!(serde_json::to_string(&buy_side).unwrap(), "\"B\"");
        assert_eq!(serde_json::to_string(&sell_side).unwrap(), "\"A\"");

        assert_eq!(
            serde_json::from_str::<HyperliquidSide>("\"B\"").unwrap(),
            HyperliquidSide::Buy
        );
        assert_eq!(
            serde_json::from_str::<HyperliquidSide>("\"A\"").unwrap(),
            HyperliquidSide::Sell
        );
    }

    #[rstest]
    fn test_side_from_order_side() {
        // Test conversion from OrderSide to HyperliquidSide
        assert_eq!(HyperliquidSide::from(OrderSide::Buy), HyperliquidSide::Buy);
        assert_eq!(
            HyperliquidSide::from(OrderSide::Sell),
            HyperliquidSide::Sell
        );
    }

    #[rstest]
    fn test_order_side_from_hyperliquid_side() {
        // Test conversion from HyperliquidSide to OrderSide
        assert_eq!(OrderSide::from(HyperliquidSide::Buy), OrderSide::Buy);
        assert_eq!(OrderSide::from(HyperliquidSide::Sell), OrderSide::Sell);
    }

    #[rstest]
    fn test_aggressor_side_from_hyperliquid_side() {
        // Test conversion from HyperliquidSide to AggressorSide
        assert_eq!(
            AggressorSide::from(HyperliquidSide::Buy),
            AggressorSide::Buyer
        );
        assert_eq!(
            AggressorSide::from(HyperliquidSide::Sell),
            AggressorSide::Seller
        );
    }

    #[rstest]
    fn test_time_in_force_serde() {
        let test_cases = [
            (HyperliquidTimeInForce::Alo, "\"Alo\""),
            (HyperliquidTimeInForce::Ioc, "\"Ioc\""),
            (HyperliquidTimeInForce::Gtc, "\"Gtc\""),
        ];

        for (tif, expected_json) in test_cases {
            assert_eq!(serde_json::to_string(&tif).unwrap(), expected_json);
            assert_eq!(
                serde_json::from_str::<HyperliquidTimeInForce>(expected_json).unwrap(),
                tif
            );
        }
    }

    #[rstest]
    fn test_liquidity_flag_from_crossed() {
        assert_eq!(
            HyperliquidLiquidityFlag::from(true),
            HyperliquidLiquidityFlag::Taker
        );
        assert_eq!(
            HyperliquidLiquidityFlag::from(false),
            HyperliquidLiquidityFlag::Maker
        );
    }

    #[rstest]
    #[allow(deprecated)]
    fn test_reject_code_from_error_string() {
        let test_cases = [
            (
                "Price must be divisible by tick size.",
                HyperliquidRejectCode::Tick,
            ),
            (
                "Order must have minimum value of $10.",
                HyperliquidRejectCode::MinTradeNtl,
            ),
            (
                "Insufficient margin to place order.",
                HyperliquidRejectCode::PerpMargin,
            ),
            (
                "Post only order would have immediately matched, bbo was 1.23",
                HyperliquidRejectCode::BadAloPx,
            ),
            (
                "Some unknown error",
                HyperliquidRejectCode::Unknown("Some unknown error".to_string()),
            ),
        ];

        for (error_str, expected_code) in test_cases {
            assert_eq!(
                HyperliquidRejectCode::from_error_string(error_str),
                expected_code
            );
        }
    }

    #[rstest]
    fn test_reject_code_from_api_error() {
        let test_cases = [
            (
                "Price must be divisible by tick size.",
                HyperliquidRejectCode::Tick,
            ),
            (
                "Order must have minimum value of $10.",
                HyperliquidRejectCode::MinTradeNtl,
            ),
            (
                "Insufficient margin to place order.",
                HyperliquidRejectCode::PerpMargin,
            ),
            (
                "Post only order would have immediately matched, bbo was 1.23",
                HyperliquidRejectCode::BadAloPx,
            ),
            (
                "Some unknown error",
                HyperliquidRejectCode::Unknown("Some unknown error".to_string()),
            ),
        ];

        for (error_str, expected_code) in test_cases {
            assert_eq!(
                HyperliquidRejectCode::from_api_error(error_str),
                expected_code
            );
        }
    }

    #[rstest]
    fn test_reduce_only() {
        let reduce_only = HyperliquidReduceOnly::new(true);

        assert!(reduce_only.is_reduce_only());

        let json = serde_json::to_string(&reduce_only).unwrap();
        assert_eq!(json, "true");

        let parsed: HyperliquidReduceOnly = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, reduce_only);
    }

    #[rstest]
    fn test_order_status_conversion() {
        // Test HyperliquidOrderStatus to OrderStatus conversion
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Open),
            OrderStatus::Accepted
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Accepted),
            OrderStatus::Accepted
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Triggered),
            OrderStatus::Triggered
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Filled),
            OrderStatus::Filled
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Canceled),
            OrderStatus::Canceled
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Rejected),
            OrderStatus::Rejected
        );

        // Test specific cancel reasons map to Canceled
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::MarginCanceled),
            OrderStatus::Canceled
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::SelfTradeCanceled),
            OrderStatus::Canceled
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::ReduceOnlyCanceled),
            OrderStatus::Canceled
        );

        // Test specific reject reasons map to Rejected
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::TickRejected),
            OrderStatus::Rejected
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::PerpMarginRejected),
            OrderStatus::Rejected
        );
    }

    #[rstest]
    fn test_order_status_serde_deserialization() {
        // Test that camelCase status values deserialize correctly
        let open: HyperliquidOrderStatus = serde_json::from_str(r#""open""#).unwrap();
        assert_eq!(open, HyperliquidOrderStatus::Open);

        let canceled: HyperliquidOrderStatus = serde_json::from_str(r#""canceled""#).unwrap();
        assert_eq!(canceled, HyperliquidOrderStatus::Canceled);

        let margin_canceled: HyperliquidOrderStatus =
            serde_json::from_str(r#""marginCanceled""#).unwrap();
        assert_eq!(margin_canceled, HyperliquidOrderStatus::MarginCanceled);

        let self_trade_canceled: HyperliquidOrderStatus =
            serde_json::from_str(r#""selfTradeCanceled""#).unwrap();
        assert_eq!(
            self_trade_canceled,
            HyperliquidOrderStatus::SelfTradeCanceled
        );

        let reduce_only_canceled: HyperliquidOrderStatus =
            serde_json::from_str(r#""reduceOnlyCanceled""#).unwrap();
        assert_eq!(
            reduce_only_canceled,
            HyperliquidOrderStatus::ReduceOnlyCanceled
        );

        let tick_rejected: HyperliquidOrderStatus =
            serde_json::from_str(r#""tickRejected""#).unwrap();
        assert_eq!(tick_rejected, HyperliquidOrderStatus::TickRejected);
    }

    #[rstest]
    fn test_hyperliquid_tpsl_serialization() {
        let tp = HyperliquidTpSl::Tp;
        let sl = HyperliquidTpSl::Sl;

        assert_eq!(serde_json::to_string(&tp).unwrap(), r#""tp""#);
        assert_eq!(serde_json::to_string(&sl).unwrap(), r#""sl""#);
    }

    #[rstest]
    fn test_hyperliquid_tpsl_deserialization() {
        let tp: HyperliquidTpSl = serde_json::from_str(r#""tp""#).unwrap();
        let sl: HyperliquidTpSl = serde_json::from_str(r#""sl""#).unwrap();

        assert_eq!(tp, HyperliquidTpSl::Tp);
        assert_eq!(sl, HyperliquidTpSl::Sl);
    }

    #[rstest]
    fn test_conditional_order_type_conversions() {
        // Test all conditional order types
        assert_eq!(
            OrderType::from(HyperliquidConditionalOrderType::StopMarket),
            OrderType::StopMarket
        );
        assert_eq!(
            OrderType::from(HyperliquidConditionalOrderType::StopLimit),
            OrderType::StopLimit
        );
        assert_eq!(
            OrderType::from(HyperliquidConditionalOrderType::TakeProfitMarket),
            OrderType::MarketIfTouched
        );
        assert_eq!(
            OrderType::from(HyperliquidConditionalOrderType::TakeProfitLimit),
            OrderType::LimitIfTouched
        );
        assert_eq!(
            OrderType::from(HyperliquidConditionalOrderType::TrailingStopMarket),
            OrderType::TrailingStopMarket
        );
    }

    // Tests for error parsing with real and simulated error messages
    mod error_parsing_tests {
        use super::*;

        #[rstest]
        fn test_parse_tick_size_error() {
            let error = "Price must be divisible by tick size 0.01";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::Tick);
        }

        #[rstest]
        fn test_parse_tick_size_error_case_insensitive() {
            let error = "PRICE MUST BE DIVISIBLE BY TICK SIZE 0.01";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::Tick);
        }

        #[rstest]
        fn test_parse_min_notional_perp() {
            let error = "Order must have minimum value of $10";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::MinTradeNtl);
        }

        #[rstest]
        fn test_parse_min_notional_spot() {
            let error = "Order must have minimum value of 10 USDC";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::MinTradeSpotNtl);
        }

        #[rstest]
        fn test_parse_insufficient_margin() {
            let error = "Insufficient margin to place order";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::PerpMargin);
        }

        #[rstest]
        fn test_parse_insufficient_margin_case_variations() {
            let variations = vec![
                "insufficient margin to place order",
                "INSUFFICIENT MARGIN TO PLACE ORDER",
                "  Insufficient margin to place order  ", // with whitespace
            ];

            for error in variations {
                let code = HyperliquidRejectCode::from_api_error(error);
                assert_eq!(code, HyperliquidRejectCode::PerpMargin);
            }
        }

        #[rstest]
        fn test_parse_reduce_only_violation() {
            let error = "Reduce only order would increase position";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::ReduceOnly);
        }

        #[rstest]
        fn test_parse_reduce_only_with_hyphen() {
            let error = "Reduce-only order would increase position";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::ReduceOnly);
        }

        #[rstest]
        fn test_parse_post_only_match() {
            let error = "Post only order would have immediately matched";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::BadAloPx);
        }

        #[rstest]
        fn test_parse_post_only_with_hyphen() {
            let error = "Post-only order would have immediately matched";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::BadAloPx);
        }

        #[rstest]
        fn test_parse_ioc_no_match() {
            let error = "Order could not immediately match";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::IocCancel);
        }

        #[rstest]
        fn test_parse_invalid_trigger_price() {
            let error = "Invalid TP/SL price";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::BadTriggerPx);
        }

        #[rstest]
        fn test_parse_no_liquidity() {
            let error = "No liquidity available for market order";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::MarketOrderNoLiquidity);
        }

        #[rstest]
        fn test_parse_position_increase_at_oi_cap() {
            let error = "PositionIncreaseAtOpenInterestCap";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(
                code,
                HyperliquidRejectCode::PositionIncreaseAtOpenInterestCap
            );
        }

        #[rstest]
        fn test_parse_position_flip_at_oi_cap() {
            let error = "PositionFlipAtOpenInterestCap";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::PositionFlipAtOpenInterestCap);
        }

        #[rstest]
        fn test_parse_too_aggressive_at_oi_cap() {
            let error = "TooAggressiveAtOpenInterestCap";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::TooAggressiveAtOpenInterestCap);
        }

        #[rstest]
        fn test_parse_open_interest_increase() {
            let error = "OpenInterestIncrease";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::OpenInterestIncrease);
        }

        #[rstest]
        fn test_parse_insufficient_spot_balance() {
            let error = "Insufficient spot balance";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::InsufficientSpotBalance);
        }

        #[rstest]
        fn test_parse_oracle_error() {
            let error = "Oracle price unavailable";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::Oracle);
        }

        #[rstest]
        fn test_parse_max_position() {
            let error = "Exceeds max position size";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::PerpMaxPosition);
        }

        #[rstest]
        fn test_parse_missing_order() {
            let error = "MissingOrder";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert_eq!(code, HyperliquidRejectCode::MissingOrder);
        }

        #[rstest]
        fn test_parse_unknown_error() {
            let error = "This is a completely new error message";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert!(matches!(code, HyperliquidRejectCode::Unknown(_)));

            // Verify the original message is preserved
            if let HyperliquidRejectCode::Unknown(msg) = code {
                assert_eq!(msg, error);
            }
        }

        #[rstest]
        fn test_parse_empty_error() {
            let error = "";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert!(matches!(code, HyperliquidRejectCode::Unknown(_)));
        }

        #[rstest]
        fn test_parse_whitespace_only() {
            let error = "   ";
            let code = HyperliquidRejectCode::from_api_error(error);
            assert!(matches!(code, HyperliquidRejectCode::Unknown(_)));
        }

        #[rstest]
        fn test_normalization_preserves_original_in_unknown() {
            let error = "  UNKNOWN ERROR MESSAGE  ";
            let code = HyperliquidRejectCode::from_api_error(error);

            // Should be Unknown, and should contain original message (not normalized)
            if let HyperliquidRejectCode::Unknown(msg) = code {
                assert_eq!(msg, error);
            } else {
                panic!("Expected Unknown variant");
            }
        }
    }

    #[rstest]
    fn test_conditional_order_type_round_trip() {
        assert_eq!(
            OrderType::from(HyperliquidConditionalOrderType::TrailingStopLimit),
            OrderType::TrailingStopLimit
        );

        // Test reverse conversions
        assert_eq!(
            HyperliquidConditionalOrderType::from(OrderType::StopMarket),
            HyperliquidConditionalOrderType::StopMarket
        );
        assert_eq!(
            HyperliquidConditionalOrderType::from(OrderType::StopLimit),
            HyperliquidConditionalOrderType::StopLimit
        );
    }

    #[rstest]
    fn test_trailing_offset_type_serialization() {
        let price = HyperliquidTrailingOffsetType::Price;
        let percentage = HyperliquidTrailingOffsetType::Percentage;
        let basis_points = HyperliquidTrailingOffsetType::BasisPoints;

        assert_eq!(serde_json::to_string(&price).unwrap(), r#""price""#);
        assert_eq!(
            serde_json::to_string(&percentage).unwrap(),
            r#""percentage""#
        );
        assert_eq!(
            serde_json::to_string(&basis_points).unwrap(),
            r#""basispoints""#
        );
    }

    #[rstest]
    fn test_conditional_order_type_serialization() {
        assert_eq!(
            serde_json::to_string(&HyperliquidConditionalOrderType::StopMarket).unwrap(),
            r#""STOP_MARKET""#
        );
        assert_eq!(
            serde_json::to_string(&HyperliquidConditionalOrderType::StopLimit).unwrap(),
            r#""STOP_LIMIT""#
        );
        assert_eq!(
            serde_json::to_string(&HyperliquidConditionalOrderType::TakeProfitMarket).unwrap(),
            r#""TAKE_PROFIT_MARKET""#
        );
        assert_eq!(
            serde_json::to_string(&HyperliquidConditionalOrderType::TakeProfitLimit).unwrap(),
            r#""TAKE_PROFIT_LIMIT""#
        );
        assert_eq!(
            serde_json::to_string(&HyperliquidConditionalOrderType::TrailingStopMarket).unwrap(),
            r#""TRAILING_STOP_MARKET""#
        );
        assert_eq!(
            serde_json::to_string(&HyperliquidConditionalOrderType::TrailingStopLimit).unwrap(),
            r#""TRAILING_STOP_LIMIT""#
        );
    }

    #[rstest]
    fn test_order_type_enum_coverage() {
        // Ensure all conditional order types roundtrip correctly
        let conditional_types = vec![
            HyperliquidConditionalOrderType::StopMarket,
            HyperliquidConditionalOrderType::StopLimit,
            HyperliquidConditionalOrderType::TakeProfitMarket,
            HyperliquidConditionalOrderType::TakeProfitLimit,
            HyperliquidConditionalOrderType::TrailingStopMarket,
            HyperliquidConditionalOrderType::TrailingStopLimit,
        ];

        for cond_type in conditional_types {
            let order_type = OrderType::from(cond_type);
            let back_to_cond = HyperliquidConditionalOrderType::from(order_type);
            assert_eq!(cond_type, back_to_cond, "Roundtrip conversion failed");
        }
    }
}
