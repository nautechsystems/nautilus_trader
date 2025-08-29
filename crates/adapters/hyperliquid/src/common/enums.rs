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

//! Enums for Hyperliquid trading operations.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use crate::common::types::Price;

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
pub enum Side {
    #[serde(rename = "B")]
    Buy,
    #[serde(rename = "A")]
    Sell,
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
pub enum TimeInForce {
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
pub enum OrderType {
    /// Limit order with time-in-force.
    #[serde(rename = "limit")]
    Limit { tif: TimeInForce },

    /// Trigger order (stop or take profit).
    #[serde(rename = "trigger")]
    Trigger {
        #[serde(rename = "isMarket")]
        is_market: bool,
        #[serde(rename = "triggerPx")]
        trigger_px: Price,
        tpsl: TpSl,
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
pub enum TpSl {
    /// Take Profit.
    Tp,
    /// Stop Loss.
    Sl,
}

/// Represents the reduce only flag wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReduceOnly(pub bool);

impl ReduceOnly {
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
pub enum LiquidityFlag {
    Maker,
    Taker,
}

impl From<bool> for LiquidityFlag {
    /// Converts from `crossed` field in fill responses.
    ///
    /// `true` (crossed) -> Taker, `false` -> Maker
    fn from(crossed: bool) -> Self {
        if crossed {
            LiquidityFlag::Taker
        } else {
            LiquidityFlag::Maker
        }
    }
}

/// Represents order reject codes from Hyperliquid.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RejectCode {
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
    /// Unknown reject reason.
    Unknown(String),
}

impl RejectCode {
    /// Parses reject code from error string.
    pub fn from_error_string(error: &str) -> Self {
        match error {
            s if s.contains("tick size") => RejectCode::Tick,
            s if s.contains("minimum value of $10") => RejectCode::MinTradeNtl,
            s if s.contains("minimum value of 10") => RejectCode::MinTradeSpotNtl,
            s if s.contains("Insufficient margin") => RejectCode::PerpMargin,
            s if s.contains("Reduce only order would increase") => RejectCode::ReduceOnly,
            s if s.contains("Post only order would have immediately matched") => {
                RejectCode::BadAloPx
            }
            s if s.contains("could not immediately match") => RejectCode::IocCancel,
            s if s.contains("Invalid TP/SL price") => RejectCode::BadTriggerPx,
            s if s.contains("No liquidity available for market order") => {
                RejectCode::MarketOrderNoLiquidity
            }
            s if s.contains("PositionIncreaseAtOpenInterestCap") => {
                RejectCode::PositionIncreaseAtOpenInterestCap
            }
            s if s.contains("PositionFlipAtOpenInterestCap") => {
                RejectCode::PositionFlipAtOpenInterestCap
            }
            s if s.contains("TooAggressiveAtOpenInterestCap") => {
                RejectCode::TooAggressiveAtOpenInterestCap
            }
            s if s.contains("OpenInterestIncrease") => RejectCode::OpenInterestIncrease,
            s if s.contains("Insufficient spot balance") => RejectCode::InsufficientSpotBalance,
            s if s.contains("Oracle") => RejectCode::Oracle,
            s if s.contains("max position") => RejectCode::PerpMaxPosition,
            s if s.contains("MissingOrder") => RejectCode::MissingOrder,
            s => RejectCode::Unknown(s.to_string()),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_side_serde() {
        // Arrange
        let buy_side = Side::Buy;
        let sell_side = Side::Sell;

        // Act & Assert - Serialization
        assert_eq!(serde_json::to_string(&buy_side).unwrap(), "\"B\"");
        assert_eq!(serde_json::to_string(&sell_side).unwrap(), "\"A\"");

        // Act & Assert - Deserialization
        assert_eq!(serde_json::from_str::<Side>("\"B\"").unwrap(), Side::Buy);
        assert_eq!(serde_json::from_str::<Side>("\"A\"").unwrap(), Side::Sell);
    }

    #[test]
    fn test_time_in_force_serde() {
        // Arrange
        let test_cases = [
            (TimeInForce::Alo, "\"Alo\""),
            (TimeInForce::Ioc, "\"Ioc\""),
            (TimeInForce::Gtc, "\"Gtc\""),
        ];

        // Act & Assert
        for (tif, expected_json) in test_cases {
            assert_eq!(serde_json::to_string(&tif).unwrap(), expected_json);
            assert_eq!(
                serde_json::from_str::<TimeInForce>(expected_json).unwrap(),
                tif
            );
        }
    }

    #[test]
    fn test_liquidity_flag_from_crossed() {
        // Arrange, Act & Assert
        assert_eq!(LiquidityFlag::from(true), LiquidityFlag::Taker);
        assert_eq!(LiquidityFlag::from(false), LiquidityFlag::Maker);
    }

    #[test]
    fn test_reject_code_from_error_string() {
        // Arrange
        let test_cases = [
            ("Price must be divisible by tick size.", RejectCode::Tick),
            (
                "Order must have minimum value of $10.",
                RejectCode::MinTradeNtl,
            ),
            (
                "Insufficient margin to place order.",
                RejectCode::PerpMargin,
            ),
            (
                "Post only order would have immediately matched, bbo was 1.23",
                RejectCode::BadAloPx,
            ),
            (
                "Some unknown error",
                RejectCode::Unknown("Some unknown error".to_string()),
            ),
        ];

        // Act & Assert
        for (error_str, expected_code) in test_cases {
            assert_eq!(RejectCode::from_error_string(error_str), expected_code);
        }
    }

    #[test]
    fn test_reduce_only() {
        // Arrange
        let reduce_only = ReduceOnly::new(true);

        // Act & Assert
        assert!(reduce_only.is_reduce_only());

        let json = serde_json::to_string(&reduce_only).unwrap();
        assert_eq!(json, "true");

        let parsed: ReduceOnly = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, reduce_only);
    }
}
