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

//! Venue-specific enums for the Polymarket CLOB API.

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{Display, EnumString};

/// EIP-712 signature type for order signing.
///
/// Serialized as a numeric value (0/1/2) on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum SignatureType {
    Eoa = 0,
    PolyProxy = 1,
    PolyGnosisSafe = 2,
}

/// Binary outcome for a Polymarket prediction market.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
pub enum PolymarketOutcome {
    Yes,
    No,
}

/// Order side on the Polymarket CLOB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PolymarketOrderSide {
    Buy,
    Sell,
}

/// Liquidity side for fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PolymarketLiquiditySide {
    Maker,
    Taker,
}

/// Order type (time-in-force variant) on the Polymarket CLOB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
pub enum PolymarketOrderType {
    FOK,
    /// Immediate or cancel.
    FAK,
    GTC,
    GTD,
}

/// WebSocket event type for user channel messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PolymarketEventType {
    Placement,
    /// Emitted for a match.
    Update,
    Cancellation,
    Trade,
}

/// Order status on the Polymarket CLOB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PolymarketOrderStatus {
    Invalid,
    Live,
    /// Marketable but subject to matching delay.
    Delayed,
    Matched,
    /// Marketable but failure delaying, placement not successful.
    Unmatched,
    Canceled,
    CanceledMarketResolved,
}

/// Trade settlement status on the Polymarket exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, EnumString, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PolymarketTradeStatus {
    /// Sent to the executor service for on-chain submission.
    Matched,
    /// Mined on-chain, no finality threshold yet.
    Mined,
    /// Strong probabilistic finality achieved.
    Confirmed,
    /// Transaction failed, being retried by the operator.
    Retrying,
    /// Permanently failed, no more retries.
    Failed,
}

impl PolymarketTradeStatus {
    /// Returns `true` if this status represents a finalized trade.
    #[must_use]
    pub const fn is_finalized(&self) -> bool {
        matches!(self, Self::Mined | Self::Confirmed)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_signature_type_serializes_as_u8() {
        assert_eq!(serde_json::to_string(&SignatureType::Eoa).unwrap(), "0");
        assert_eq!(
            serde_json::to_string(&SignatureType::PolyProxy).unwrap(),
            "1"
        );
        assert_eq!(
            serde_json::to_string(&SignatureType::PolyGnosisSafe).unwrap(),
            "2"
        );
    }

    #[rstest]
    fn test_signature_type_deserializes_from_u8() {
        assert_eq!(
            serde_json::from_str::<SignatureType>("0").unwrap(),
            SignatureType::Eoa
        );
        assert_eq!(
            serde_json::from_str::<SignatureType>("1").unwrap(),
            SignatureType::PolyProxy
        );
        assert_eq!(
            serde_json::from_str::<SignatureType>("2").unwrap(),
            SignatureType::PolyGnosisSafe
        );
    }

    #[rstest]
    fn test_order_side_serde_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&PolymarketOrderSide::Buy).unwrap(),
            "\"BUY\""
        );
        assert_eq!(
            serde_json::from_str::<PolymarketOrderSide>("\"SELL\"").unwrap(),
            PolymarketOrderSide::Sell
        );
    }

    #[rstest]
    fn test_event_type_serde_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&PolymarketEventType::Placement).unwrap(),
            "\"PLACEMENT\""
        );
        assert_eq!(
            serde_json::from_str::<PolymarketEventType>("\"TRADE\"").unwrap(),
            PolymarketEventType::Trade
        );
    }

    #[rstest]
    fn test_order_status_serde_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&PolymarketOrderStatus::Live).unwrap(),
            "\"LIVE\""
        );
        assert_eq!(
            serde_json::from_str::<PolymarketOrderStatus>("\"CANCELED_MARKET_RESOLVED\"").unwrap(),
            PolymarketOrderStatus::CanceledMarketResolved
        );
    }

    #[rstest]
    fn test_trade_status_serde_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&PolymarketTradeStatus::Confirmed).unwrap(),
            "\"CONFIRMED\""
        );
        assert_eq!(
            serde_json::from_str::<PolymarketTradeStatus>("\"RETRYING\"").unwrap(),
            PolymarketTradeStatus::Retrying
        );
    }
}
