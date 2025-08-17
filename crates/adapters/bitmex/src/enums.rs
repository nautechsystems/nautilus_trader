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

use serde::{Deserialize, Deserializer, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Represents the status of a BitMEX symbol.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    PartialEq,
    Eq,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitmex", eq, eq_int)
)]
pub enum BitmexSymbolStatus {
    /// Symbol is open for trading.
    Open,
    /// Symbol is closed for trading.
    Closed,
    /// Symbol is unlisted.
    Unlisted,
}

/// Represents the side of an order or trade (Buy/Sell).
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum Side {
    /// Buy side of a trade or order.
    Buy,
    /// Sell side of a trade or order.
    Sell,
}

impl Side {
    /// Converts a Nautilus order side to a BitMEX side.
    ///
    /// # Panics
    ///
    /// Panics if the order side is not Buy or Sell.
    pub fn from_nautilus_order_side(value: nautilus_model::enums::OrderSide) -> Self {
        match value {
            nautilus_model::enums::OrderSide::Buy => Side::Buy,
            nautilus_model::enums::OrderSide::Sell => Side::Sell,
            _ => panic!("Invalid order side: {value:?}"),
        }
    }
}

/// Represents the available order types on `BitMEX`.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum OrderType {
    /// Market order, executed immediately at current market price.
    Market,
    /// Limit order, executed only at specified price or better.
    Limit,
    /// Stop Market order, triggers a market order when price reaches stop price.
    Stop,
    /// Stop Limit order, triggers a limit order when price reaches stop price.
    StopLimit,
    /// Market if touched order, triggers a market order when price reaches touch price.
    MarketIfTouched,
    /// Limit if touched order, triggers a limit order when price reaches touch price.
    LimitIfTouched,
    /// Pegged order, price automatically tracks market.
    Pegged,
}

impl OrderType {
    pub fn from_nautilus(value: nautilus_model::enums::OrderType) -> Self {
        match value {
            nautilus_model::enums::OrderType::Market => Self::Market,
            nautilus_model::enums::OrderType::Limit => Self::LimitIfTouched,
            nautilus_model::enums::OrderType::StopMarket => Self::Stop,
            nautilus_model::enums::OrderType::StopLimit => Self::StopLimit,
            nautilus_model::enums::OrderType::MarketIfTouched => Self::MarketIfTouched,
            nautilus_model::enums::OrderType::LimitIfTouched => Self::LimitIfTouched,
            nautilus_model::enums::OrderType::TrailingStopMarket => Self::Pegged,
            nautilus_model::enums::OrderType::TrailingStopLimit => Self::Pegged,
            nautilus_model::enums::OrderType::MarketToLimit => Self::Market, // TODO: Not
                                                                             // supported
        }
    }
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum OrderStatus {
    /// Order has been placed but not yet processed.
    New,
    /// Order has been partially filled.
    PartiallyFilled,
    /// Order has been completely filled.
    Filled,
    /// Order has been canceled by user or system.
    Canceled,
    /// Order was rejected by the system.
    Rejected,
    /// Order has expired according to its time in force.
    Expired,
}

/// Specifies how long an order should remain active.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum TimeInForce {
    Day,
    GoodTillCancel,
    AtTheOpening,
    ImmediateOrCancel,
    FillOrKill,
    GoodTillCrossing,
    GoodTillDate,
    AtTheClose,
    GoodThroughCrossing,
    AtCrossing,
}

/// Represents the available contingency types on `BitMEX`.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum ContingencyType {
    OneCancelsTheOther,
    OneTriggersTheOther,
    OneUpdatesTheOtherAbsolute,
    OneUpdatesTheOtherProportional,
    #[serde(rename = "")]
    Unknown, // Can be empty
}

/// Represents the available peg price types on `BitMEX`.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum PegPriceType {
    LastPeg,
    OpeningPeg,
    MidPricePeg,
    MarketPeg,
    PrimaryPeg,
    PegToVWAP,
    TrailingStopPeg,
    PegToLimitPrice,
    ShortSaleMinPricePeg,
    #[serde(rename = "")]
    Unknown, // Can be empty
}

/// Represents the available execution instruments on `BitMEX`.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum ExecInstruction {
    ParticipateDoNotInitiate,
    AllOrNone,
    MarkPrice,
    IndexPrice,
    LastPrice,
    Close,
    ReduceOnly,
    Fixed,
    #[serde(rename = "")]
    Unknown, // Can be empty
}

impl ExecInstruction {
    pub fn join(instructions: &[ExecInstruction]) -> String {
        instructions
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Represents the type of execution that generated a trade.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum ExecType {
    /// Normal trade execution.
    Trade,
    /// Settlement execution.
    Settlement,
    /// Funding rate execution.
    Funding,
    /// Liquidation execution.
    Liquidation,
    /// Bankruptcy execution.
    Bankruptcy,
}

/// Indicates whether the execution was maker or taker.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum LiquidityIndicator {
    /// Provided liquidity to the order book (maker).
    /// BitMEX returns "Added" in REST API responses and "AddedLiquidity" in WebSocket messages.
    #[serde(rename = "Added")]
    #[serde(alias = "AddedLiquidity")]
    Maker,
    /// Took liquidity from the order book (taker).
    /// BitMEX returns "Removed" in REST API responses and "RemovedLiquidity" in WebSocket messages.
    #[serde(rename = "Removed")]
    #[serde(alias = "RemovedLiquidity")]
    Taker,
}

/// Represents `BitMEX` instrument types.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
pub enum InstrumentType {
    #[serde(rename = "FXXXS")]
    Unknown1, // TODO: Determine name (option)

    #[serde(rename = "FMXXS")]
    Unknown2, // TODO: Determine name (option)

    #[serde(rename = "FFICSX")]
    Unknown3, // TODO: Determine name (option)

    /// Perpetual Contracts.
    #[serde(rename = "FFWCSX")]
    PerpetualContract,

    /// Perpetual Contracts (FX underliers).
    #[serde(rename = "FFWCSF")]
    PerpetualContractFx,

    /// Spot.
    #[serde(rename = "IFXXXP")]
    Spot,

    /// Futures.
    #[serde(rename = "FFCCSX")]
    Futures,

    /// `BitMEX` Basket Index.
    #[serde(rename = "MRBXXX")]
    BasketIndex,

    /// `BitMEX` Crypto Index.
    #[serde(rename = "MRCXXX")]
    CryptoIndex,

    /// `BitMEX` FX Index.
    #[serde(rename = "MRFXXX")]
    FxIndex,

    /// `BitMEX` Lending/Premium Index.
    #[serde(rename = "MRRXXX")]
    LendingIndex,

    /// `BitMEX` Volatility Index.
    #[serde(rename = "MRIXXX")]
    VolatilityIndex,
}

/// Represents the different types of instrument subscriptions available on `BitMEX`.
#[derive(Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize)]
pub enum BitmexProductType {
    /// All instruments AND indices.
    #[serde(rename = "instrument")]
    All,

    /// All instruments, but no indices.
    #[serde(rename = "CONTRACTS")]
    Contracts,

    /// All indices, but no tradeable instruments.
    #[serde(rename = "INDICES")]
    Indices,

    /// Only derivative instruments, and no indices.
    #[serde(rename = "DERIVATIVES")]
    Derivatives,

    /// Only spot instruments, and no indices.
    #[serde(rename = "SPOT")]
    Spot,

    /// Specific instrument subscription (e.g., "instrument:XBTUSD").
    #[serde(rename = "instrument")]
    #[serde(untagged)]
    Specific(String),
}

impl BitmexProductType {
    /// Converts the product type to its websocket subscription string
    #[must_use]
    pub fn to_subscription(&self) -> String {
        match self {
            Self::All => "instrument".to_string(),
            Self::Specific(symbol) => format!("instrument:{symbol}"),
            Self::Contracts => "CONTRACTS".to_string(),
            Self::Indices => "INDICES".to_string(),
            Self::Derivatives => "DERIVATIVES".to_string(),
            Self::Spot => "SPOT".to_string(),
        }
    }
}

impl<'de> Deserialize<'de> for BitmexProductType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        match s.as_str() {
            "instrument" => Ok(Self::All),
            "CONTRACTS" => Ok(Self::Contracts),
            "INDICES" => Ok(Self::Indices),
            "DERIVATIVES" => Ok(Self::Derivatives),
            "SPOT" => Ok(Self::Spot),
            s if s.starts_with("instrument:") => {
                let symbol = s.strip_prefix("instrument:").unwrap();
                Ok(Self::Specific(symbol.to_string()))
            }
            _ => Err(serde::de::Error::custom(format!(
                "Invalid product type: {s}"
            ))),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_instrument_type_serialization() {
        assert_eq!(
            serde_json::to_string(&InstrumentType::PerpetualContract).unwrap(),
            r#""FFWCSX""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::PerpetualContractFx).unwrap(),
            r#""FFWCSF""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::Spot).unwrap(),
            r#""IFXXXP""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::Futures).unwrap(),
            r#""FFCCSX""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::BasketIndex).unwrap(),
            r#""MRBXXX""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::CryptoIndex).unwrap(),
            r#""MRCXXX""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::FxIndex).unwrap(),
            r#""MRFXXX""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::LendingIndex).unwrap(),
            r#""MRRXXX""#
        );
        assert_eq!(
            serde_json::to_string(&InstrumentType::VolatilityIndex).unwrap(),
            r#""MRIXXX""#
        );
    }

    #[rstest]
    fn test_instrument_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""FFWCSX""#).unwrap(),
            InstrumentType::PerpetualContract
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""FFWCSF""#).unwrap(),
            InstrumentType::PerpetualContractFx
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""IFXXXP""#).unwrap(),
            InstrumentType::Spot
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""FFCCSX""#).unwrap(),
            InstrumentType::Futures
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""MRBXXX""#).unwrap(),
            InstrumentType::BasketIndex
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""MRCXXX""#).unwrap(),
            InstrumentType::CryptoIndex
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""MRFXXX""#).unwrap(),
            InstrumentType::FxIndex
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""MRRXXX""#).unwrap(),
            InstrumentType::LendingIndex
        );
        assert_eq!(
            serde_json::from_str::<InstrumentType>(r#""MRIXXX""#).unwrap(),
            InstrumentType::VolatilityIndex
        );

        // Error case
        assert!(serde_json::from_str::<InstrumentType>(r#""INVALID""#).is_err());
    }

    #[rstest]
    fn test_subscription_strings() {
        assert_eq!(BitmexProductType::All.to_subscription(), "instrument");
        assert_eq!(
            BitmexProductType::Specific("XBTUSD".to_string()).to_subscription(),
            "instrument:XBTUSD"
        );
        assert_eq!(BitmexProductType::Contracts.to_subscription(), "CONTRACTS");
        assert_eq!(BitmexProductType::Indices.to_subscription(), "INDICES");
        assert_eq!(
            BitmexProductType::Derivatives.to_subscription(),
            "DERIVATIVES"
        );
        assert_eq!(BitmexProductType::Spot.to_subscription(), "SPOT");
    }

    #[rstest]
    fn test_serialization() {
        // Test serialization
        assert_eq!(
            serde_json::to_string(&BitmexProductType::All).unwrap(),
            r#""instrument""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexProductType::Specific("XBTUSD".to_string())).unwrap(),
            r#""XBTUSD""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexProductType::Contracts).unwrap(),
            r#""CONTRACTS""#
        );
    }

    #[rstest]
    fn test_deserialization() {
        assert_eq!(
            serde_json::from_str::<BitmexProductType>(r#""instrument""#).unwrap(),
            BitmexProductType::All
        );
        assert_eq!(
            serde_json::from_str::<BitmexProductType>(r#""instrument:XBTUSD""#).unwrap(),
            BitmexProductType::Specific("XBTUSD".to_string())
        );
        assert_eq!(
            serde_json::from_str::<BitmexProductType>(r#""CONTRACTS""#).unwrap(),
            BitmexProductType::Contracts
        );
    }

    #[rstest]
    fn test_error_cases() {
        assert!(serde_json::from_str::<BitmexProductType>(r#""invalid_type""#).is_err());
        assert!(serde_json::from_str::<BitmexProductType>(r"123").is_err());
        assert!(serde_json::from_str::<BitmexProductType>(r"{}").is_err());
    }
}
