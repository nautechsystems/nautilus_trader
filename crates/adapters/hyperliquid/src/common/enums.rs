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

use nautilus_model::enums::{AggressorSide, OrderSide};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Represents the order side (Buy or Sell).
///
/// Hyperliquid uses "B" for Buy and "A" for Sell in API responses.
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
            _ => panic!("Invalid `OrderSide`: {value:?}"),
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
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum HyperliquidTpSl {
    /// Take Profit.
    Tp,
    /// Stop Loss.
    Sl,
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
        if crossed {
            HyperliquidLiquidityFlag::Taker
        } else {
            HyperliquidLiquidityFlag::Maker
        }
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
    pub fn from_api_error(error_message: &str) -> Self {
        // TODO: Research Hyperliquid's actual error response format
        // Check if they provide:
        // - Numeric error codes
        // - Error type/category fields
        // - Structured error objects
        // If so, parse those instead of string matching

        // For now, we still fall back to string matching, but this method provides
        // a clear migration path when better error information becomes available
        Self::from_error_string_internal(error_message)
    }

    /// Internal string parsing method - not exposed publicly.
    ///
    /// This encapsulates the fragile string matching logic and makes it clear
    /// that it should only be used internally until we have better error handling.
    fn from_error_string_internal(error: &str) -> Self {
        match error {
            s if s.contains("tick size") => HyperliquidRejectCode::Tick,
            s if s.contains("minimum value of $10") => HyperliquidRejectCode::MinTradeNtl,
            s if s.contains("minimum value of 10") => HyperliquidRejectCode::MinTradeSpotNtl,
            s if s.contains("Insufficient margin") => HyperliquidRejectCode::PerpMargin,
            s if s.contains("Reduce only order would increase") => {
                HyperliquidRejectCode::ReduceOnly
            }
            s if s.contains("Post only order would have immediately matched") => {
                HyperliquidRejectCode::BadAloPx
            }
            s if s.contains("could not immediately match") => HyperliquidRejectCode::IocCancel,
            s if s.contains("Invalid TP/SL price") => HyperliquidRejectCode::BadTriggerPx,
            s if s.contains("No liquidity available for market order") => {
                HyperliquidRejectCode::MarketOrderNoLiquidity
            }
            s if s.contains("PositionIncreaseAtOpenInterestCap") => {
                HyperliquidRejectCode::PositionIncreaseAtOpenInterestCap
            }
            s if s.contains("PositionFlipAtOpenInterestCap") => {
                HyperliquidRejectCode::PositionFlipAtOpenInterestCap
            }
            s if s.contains("TooAggressiveAtOpenInterestCap") => {
                HyperliquidRejectCode::TooAggressiveAtOpenInterestCap
            }
            s if s.contains("OpenInterestIncrease") => HyperliquidRejectCode::OpenInterestIncrease,
            s if s.contains("Insufficient spot balance") => {
                HyperliquidRejectCode::InsufficientSpotBalance
            }
            s if s.contains("Oracle") => HyperliquidRejectCode::Oracle,
            s if s.contains("max position") => HyperliquidRejectCode::PerpMaxPosition,
            s if s.contains("MissingOrder") => HyperliquidRejectCode::MissingOrder,
            s => HyperliquidRejectCode::Unknown(s.to_string()),
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
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
}
