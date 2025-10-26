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

use nautilus_model::enums::{AggressorSide, OrderSide, OrderStatus};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.hyperliquid")
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidTpSl {
    /// Take Profit.
    Tp,
    /// Stop Loss.
    Sl,
}

/// Represents trigger price types for conditional orders.
///
/// Hyperliquid supports different price references for trigger evaluation:
/// - Last: Last traded price (most common)
/// - Mark: Mark price (for risk management)
/// - Oracle: Oracle/index price (for some perpetuals)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.hyperliquid")
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidTriggerPriceType {
    /// Last traded price.
    Last,
    /// Mark price.
    Mark,
    /// Oracle/index price.
    Oracle,
}

impl From<HyperliquidTriggerPriceType> for nautilus_model::enums::TriggerType {
    fn from(value: HyperliquidTriggerPriceType) -> Self {
        match value {
            HyperliquidTriggerPriceType::Last => Self::LastPrice,
            HyperliquidTriggerPriceType::Mark => Self::MarkPrice,
            HyperliquidTriggerPriceType::Oracle => Self::IndexPrice,
        }
    }
}

impl From<nautilus_model::enums::TriggerType> for HyperliquidTriggerPriceType {
    fn from(value: nautilus_model::enums::TriggerType) -> Self {
        match value {
            nautilus_model::enums::TriggerType::LastPrice => Self::Last,
            nautilus_model::enums::TriggerType::MarkPrice => Self::Mark,
            nautilus_model::enums::TriggerType::IndexPrice => Self::Oracle,
            _ => Self::Last, // Default fallback
        }
    }
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.hyperliquid")
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

impl From<HyperliquidConditionalOrderType> for nautilus_model::enums::OrderType {
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

impl From<nautilus_model::enums::OrderType> for HyperliquidConditionalOrderType {
    fn from(value: nautilus_model::enums::OrderType) -> Self {
        match value {
            nautilus_model::enums::OrderType::StopMarket => Self::StopMarket,
            nautilus_model::enums::OrderType::StopLimit => Self::StopLimit,
            nautilus_model::enums::OrderType::MarketIfTouched => Self::TakeProfitMarket,
            nautilus_model::enums::OrderType::LimitIfTouched => Self::TakeProfitLimit,
            nautilus_model::enums::OrderType::TrailingStopMarket => Self::TrailingStopMarket,
            nautilus_model::enums::OrderType::TrailingStopLimit => Self::TrailingStopLimit,
            _ => panic!("Unsupported OrderType for conditional orders: {:?}", value),
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.hyperliquid")
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
            s if s.contains("post only order would have immediately matched")
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
                tracing::warn!(
                    "Unknown Hyperliquid error pattern (consider updating error parsing): {}",
                    error // Use original error, not normalized
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

/// Represents Hyperliquid order status from API responses
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
pub enum HyperliquidOrderStatus {
    /// Order has been accepted and is open
    Open,
    /// Order has been accepted and is open (alternative representation)
    Accepted,
    /// Order has been partially filled
    PartiallyFilled,
    /// Order has been completely filled
    Filled,
    /// Order has been canceled
    Canceled,
    /// Order has been canceled (alternative spelling)
    Cancelled,
    /// Order was rejected by the exchange
    Rejected,
    /// Order has expired
    Expired,
}

impl From<HyperliquidOrderStatus> for OrderStatus {
    fn from(status: HyperliquidOrderStatus) -> Self {
        match status {
            HyperliquidOrderStatus::Open | HyperliquidOrderStatus::Accepted => Self::Accepted,
            HyperliquidOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            HyperliquidOrderStatus::Filled => Self::Filled,
            HyperliquidOrderStatus::Canceled | HyperliquidOrderStatus::Cancelled => Self::Canceled,
            HyperliquidOrderStatus::Rejected => Self::Rejected,
            HyperliquidOrderStatus::Expired => Self::Expired,
        }
    }
}

pub fn hyperliquid_status_to_order_status(status: &str) -> OrderStatus {
    match status {
        "open" | "accepted" => OrderStatus::Accepted,
        "partially_filled" => OrderStatus::PartiallyFilled,
        "filled" => OrderStatus::Filled,
        "canceled" | "cancelled" => OrderStatus::Canceled,
        "rejected" => OrderStatus::Rejected,
        "expired" => OrderStatus::Expired,
        _ => OrderStatus::Rejected,
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::enums::{OrderType, TriggerType};
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
        // Test HyperliquidOrderStatus to OrderState conversion
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Open),
            OrderStatus::Accepted
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Accepted),
            OrderStatus::Accepted
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::PartiallyFilled),
            OrderStatus::PartiallyFilled
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
            OrderStatus::from(HyperliquidOrderStatus::Cancelled),
            OrderStatus::Canceled
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Rejected),
            OrderStatus::Rejected
        );
        assert_eq!(
            OrderStatus::from(HyperliquidOrderStatus::Expired),
            OrderStatus::Expired
        );
    }

    #[rstest]
    fn test_order_status_string_mapping() {
        // Test direct string to OrderState conversion
        assert_eq!(
            hyperliquid_status_to_order_status("open"),
            OrderStatus::Accepted
        );
        assert_eq!(
            hyperliquid_status_to_order_status("accepted"),
            OrderStatus::Accepted
        );
        assert_eq!(
            hyperliquid_status_to_order_status("partially_filled"),
            OrderStatus::PartiallyFilled
        );
        assert_eq!(
            hyperliquid_status_to_order_status("filled"),
            OrderStatus::Filled
        );
        assert_eq!(
            hyperliquid_status_to_order_status("canceled"),
            OrderStatus::Canceled
        );
        assert_eq!(
            hyperliquid_status_to_order_status("cancelled"),
            OrderStatus::Canceled
        );
        assert_eq!(
            hyperliquid_status_to_order_status("rejected"),
            OrderStatus::Rejected
        );
        assert_eq!(
            hyperliquid_status_to_order_status("expired"),
            OrderStatus::Expired
        );
        assert_eq!(
            hyperliquid_status_to_order_status("unknown_status"),
            OrderStatus::Rejected
        );
    }

    // ========================================================================
    // Conditional Order Tests
    // ========================================================================

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
    fn test_hyperliquid_trigger_price_type_serialization() {
        let last = HyperliquidTriggerPriceType::Last;
        let mark = HyperliquidTriggerPriceType::Mark;
        let oracle = HyperliquidTriggerPriceType::Oracle;

        assert_eq!(serde_json::to_string(&last).unwrap(), r#""last""#);
        assert_eq!(serde_json::to_string(&mark).unwrap(), r#""mark""#);
        assert_eq!(serde_json::to_string(&oracle).unwrap(), r#""oracle""#);
    }

    #[rstest]
    fn test_hyperliquid_trigger_price_type_to_nautilus() {
        assert_eq!(
            TriggerType::from(HyperliquidTriggerPriceType::Last),
            TriggerType::LastPrice
        );
        assert_eq!(
            TriggerType::from(HyperliquidTriggerPriceType::Mark),
            TriggerType::MarkPrice
        );
        assert_eq!(
            TriggerType::from(HyperliquidTriggerPriceType::Oracle),
            TriggerType::IndexPrice
        );
    }

    #[rstest]
    fn test_nautilus_trigger_type_to_hyperliquid() {
        assert_eq!(
            HyperliquidTriggerPriceType::from(TriggerType::LastPrice),
            HyperliquidTriggerPriceType::Last
        );
        assert_eq!(
            HyperliquidTriggerPriceType::from(TriggerType::MarkPrice),
            HyperliquidTriggerPriceType::Mark
        );
        assert_eq!(
            HyperliquidTriggerPriceType::from(TriggerType::IndexPrice),
            HyperliquidTriggerPriceType::Oracle
        );
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

    #[rstest]
    fn test_all_trigger_price_types() {
        let trigger_types = vec![
            HyperliquidTriggerPriceType::Last,
            HyperliquidTriggerPriceType::Mark,
            HyperliquidTriggerPriceType::Oracle,
        ];

        for trigger_type in trigger_types {
            let nautilus_type = TriggerType::from(trigger_type);
            let back_to_hl = HyperliquidTriggerPriceType::from(nautilus_type);
            assert_eq!(trigger_type, back_to_hl, "Trigger type roundtrip failed");
        }
    }
}
