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

use nautilus_model::enums::{AggressorSide, OrderSide, OrderStatus, TimeInForce};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{Display, EnumString};

/// EIP-712 signature type for order signing.
///
/// Serialized as a numeric value (0/1/2) on the wire.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object,
    )
)]
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

impl From<PolymarketOrderSide> for OrderSide {
    fn from(value: PolymarketOrderSide) -> Self {
        match value {
            PolymarketOrderSide::Buy => Self::Buy,
            PolymarketOrderSide::Sell => Self::Sell,
        }
    }
}

impl TryFrom<OrderSide> for PolymarketOrderSide {
    type Error = anyhow::Error;

    fn try_from(value: OrderSide) -> anyhow::Result<Self> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            _ => anyhow::bail!("Invalid `OrderSide` for Polymarket: {value:?}"),
        }
    }
}

impl From<PolymarketOrderSide> for AggressorSide {
    fn from(value: PolymarketOrderSide) -> Self {
        match value {
            PolymarketOrderSide::Buy => Self::Buyer,
            PolymarketOrderSide::Sell => Self::Seller,
        }
    }
}

impl From<PolymarketOrderType> for TimeInForce {
    fn from(value: PolymarketOrderType) -> Self {
        match value {
            PolymarketOrderType::GTC => Self::Gtc,
            PolymarketOrderType::GTD => Self::Gtd,
            PolymarketOrderType::FOK => Self::Fok,
            // Fill-And-Kill is equivalent to Immediate-Or-Cancel
            PolymarketOrderType::FAK => Self::Ioc,
        }
    }
}

impl TryFrom<TimeInForce> for PolymarketOrderType {
    type Error = anyhow::Error;

    fn try_from(value: TimeInForce) -> anyhow::Result<Self> {
        match value {
            TimeInForce::Gtc => Ok(Self::GTC),
            TimeInForce::Gtd => Ok(Self::GTD),
            TimeInForce::Fok => Ok(Self::FOK),
            TimeInForce::Ioc => Ok(Self::FAK),
            _ => anyhow::bail!("Unsupported `TimeInForce` for Polymarket: {value:?}"),
        }
    }
}

impl From<PolymarketOrderStatus> for OrderStatus {
    fn from(value: PolymarketOrderStatus) -> Self {
        match value {
            PolymarketOrderStatus::Invalid => Self::Rejected,
            PolymarketOrderStatus::Live => Self::Accepted,
            PolymarketOrderStatus::Delayed => Self::Accepted,
            PolymarketOrderStatus::Matched => Self::Filled,
            // Placement failure (never became live) — treat as rejected
            PolymarketOrderStatus::Unmatched => Self::Rejected,
            PolymarketOrderStatus::Canceled => Self::Canceled,
            // Market resolved = order expired due to market settlement
            PolymarketOrderStatus::CanceledMarketResolved => Self::Expired,
        }
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

    #[rstest]
    #[case(PolymarketOrderSide::Buy, OrderSide::Buy)]
    #[case(PolymarketOrderSide::Sell, OrderSide::Sell)]
    fn test_order_side_to_nautilus(#[case] poly: PolymarketOrderSide, #[case] expected: OrderSide) {
        assert_eq!(OrderSide::from(poly), expected);
    }

    #[rstest]
    #[case(OrderSide::Buy, PolymarketOrderSide::Buy)]
    #[case(OrderSide::Sell, PolymarketOrderSide::Sell)]
    fn test_nautilus_order_side_to_poly(
        #[case] nautilus: OrderSide,
        #[case] expected: PolymarketOrderSide,
    ) {
        assert_eq!(PolymarketOrderSide::try_from(nautilus).unwrap(), expected);
    }

    #[rstest]
    #[case(PolymarketOrderSide::Buy, AggressorSide::Buyer)]
    #[case(PolymarketOrderSide::Sell, AggressorSide::Seller)]
    fn test_order_side_to_aggressor(
        #[case] poly: PolymarketOrderSide,
        #[case] expected: AggressorSide,
    ) {
        assert_eq!(AggressorSide::from(poly), expected);
    }

    #[rstest]
    #[case(PolymarketOrderType::GTC, TimeInForce::Gtc)]
    #[case(PolymarketOrderType::GTD, TimeInForce::Gtd)]
    #[case(PolymarketOrderType::FOK, TimeInForce::Fok)]
    #[case(PolymarketOrderType::FAK, TimeInForce::Ioc)]
    fn test_order_type_to_time_in_force(
        #[case] poly: PolymarketOrderType,
        #[case] expected: TimeInForce,
    ) {
        assert_eq!(TimeInForce::from(poly), expected);
    }

    #[rstest]
    #[case(TimeInForce::Gtc, PolymarketOrderType::GTC)]
    #[case(TimeInForce::Gtd, PolymarketOrderType::GTD)]
    #[case(TimeInForce::Fok, PolymarketOrderType::FOK)]
    #[case(TimeInForce::Ioc, PolymarketOrderType::FAK)]
    fn test_time_in_force_to_order_type(
        #[case] tif: TimeInForce,
        #[case] expected: PolymarketOrderType,
    ) {
        assert_eq!(PolymarketOrderType::try_from(tif).unwrap(), expected);
    }

    #[rstest]
    #[case(PolymarketOrderStatus::Invalid, OrderStatus::Rejected)]
    #[case(PolymarketOrderStatus::Live, OrderStatus::Accepted)]
    #[case(PolymarketOrderStatus::Delayed, OrderStatus::Accepted)]
    #[case(PolymarketOrderStatus::Matched, OrderStatus::Filled)]
    #[case(PolymarketOrderStatus::Unmatched, OrderStatus::Rejected)]
    #[case(PolymarketOrderStatus::Canceled, OrderStatus::Canceled)]
    #[case(PolymarketOrderStatus::CanceledMarketResolved, OrderStatus::Expired)]
    fn test_order_status_to_nautilus(
        #[case] poly: PolymarketOrderStatus,
        #[case] expected: OrderStatus,
    ) {
        assert_eq!(OrderStatus::from(poly), expected);
    }

    #[rstest]
    fn test_trade_status_is_finalized() {
        assert!(PolymarketTradeStatus::Mined.is_finalized());
        assert!(PolymarketTradeStatus::Confirmed.is_finalized());
        assert!(!PolymarketTradeStatus::Matched.is_finalized());
        assert!(!PolymarketTradeStatus::Retrying.is_finalized());
        assert!(!PolymarketTradeStatus::Failed.is_finalized());
    }
}
