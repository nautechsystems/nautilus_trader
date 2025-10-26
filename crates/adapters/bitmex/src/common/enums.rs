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

//! BitMEX-specific enumerations shared by HTTP and WebSocket components.

use nautilus_model::enums::{
    ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
};
use serde::{Deserialize, Deserializer, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use crate::error::{BitmexError, BitmexNonRetryableError};

/// Represents the status of a BitMEX symbol.
#[derive(
    Copy,
    Clone,
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
    Copy,
    Clone,
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
pub enum BitmexSide {
    /// Buy side of a trade or order.
    #[serde(rename = "Buy", alias = "BUY", alias = "buy")]
    Buy,
    /// Sell side of a trade or order.
    #[serde(rename = "Sell", alias = "SELL", alias = "sell")]
    Sell,
}

impl TryFrom<OrderSide> for BitmexSide {
    type Error = BitmexError;

    fn try_from(value: OrderSide) -> Result<Self, Self::Error> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            _ => Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation {
                    field: "order_side".to_string(),
                    message: format!("Invalid order side: {value:?}"),
                },
            }),
        }
    }
}

impl BitmexSide {
    /// Try to convert from Nautilus OrderSide.
    ///
    /// # Errors
    ///
    /// Returns an error if the order side is not Buy or Sell.
    pub fn try_from_order_side(value: OrderSide) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl From<BitmexSide> for OrderSide {
    fn from(side: BitmexSide) -> Self {
        match side {
            BitmexSide::Buy => Self::Buy,
            BitmexSide::Sell => Self::Sell,
        }
    }
}

/// Represents the position side for BitMEX positions.
#[derive(
    Copy,
    Clone,
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitmex", eq, eq_int)
)]
pub enum BitmexPositionSide {
    /// Long position.
    #[serde(rename = "LONG", alias = "Long", alias = "long")]
    Long,
    /// Short position.
    #[serde(rename = "SHORT", alias = "Short", alias = "short")]
    Short,
    /// No position.
    #[serde(rename = "FLAT", alias = "Flat", alias = "flat")]
    Flat,
}

impl From<BitmexPositionSide> for PositionSide {
    fn from(side: BitmexPositionSide) -> Self {
        match side {
            BitmexPositionSide::Long => Self::Long,
            BitmexPositionSide::Short => Self::Short,
            BitmexPositionSide::Flat => Self::Flat,
        }
    }
}

impl From<PositionSide> for BitmexPositionSide {
    fn from(side: PositionSide) -> Self {
        match side {
            PositionSide::Long => Self::Long,
            PositionSide::Short => Self::Short,
            PositionSide::Flat | PositionSide::NoPositionSide => Self::Flat,
        }
    }
}

/// Represents the available order types on BitMEX.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexOrderType {
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

impl TryFrom<OrderType> for BitmexOrderType {
    type Error = BitmexError;

    fn try_from(value: OrderType) -> Result<Self, Self::Error> {
        match value {
            OrderType::Market => Ok(Self::Market),
            OrderType::Limit => Ok(Self::Limit),
            OrderType::StopMarket => Ok(Self::Stop),
            OrderType::StopLimit => Ok(Self::StopLimit),
            OrderType::MarketIfTouched => Ok(Self::MarketIfTouched),
            OrderType::LimitIfTouched => Ok(Self::LimitIfTouched),
            OrderType::TrailingStopMarket => Ok(Self::Pegged),
            OrderType::TrailingStopLimit => Ok(Self::Pegged),
            OrderType::MarketToLimit => Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation {
                    field: "order_type".to_string(),
                    message: "MarketToLimit order type is not supported by BitMEX".to_string(),
                },
            }),
        }
    }
}

impl BitmexOrderType {
    /// Try to convert from Nautilus OrderType with anyhow::Result.
    ///
    /// # Errors
    ///
    /// Returns an error if the order type is MarketToLimit (not supported by BitMEX).
    pub fn try_from_order_type(value: OrderType) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl From<BitmexOrderType> for OrderType {
    fn from(value: BitmexOrderType) -> Self {
        match value {
            BitmexOrderType::Market => Self::Market,
            BitmexOrderType::Limit => Self::Limit,
            BitmexOrderType::Stop => Self::StopMarket,
            BitmexOrderType::StopLimit => Self::StopLimit,
            BitmexOrderType::MarketIfTouched => Self::MarketIfTouched,
            BitmexOrderType::LimitIfTouched => Self::LimitIfTouched,
            BitmexOrderType::Pegged => Self::Limit,
        }
    }
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexOrderStatus {
    /// Order has been placed but not yet processed.
    New,
    /// Order has been partially filled.
    PartiallyFilled,
    /// Order has been completely filled.
    Filled,
    /// Order cancellation is pending.
    PendingCancel,
    /// Order has been canceled by user or system.
    Canceled,
    /// Order was rejected by the system.
    Rejected,
    /// Order has expired according to its time in force.
    Expired,
}

impl From<BitmexOrderStatus> for OrderStatus {
    fn from(value: BitmexOrderStatus) -> Self {
        match value {
            BitmexOrderStatus::New => Self::Accepted,
            BitmexOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            BitmexOrderStatus::Filled => Self::Filled,
            BitmexOrderStatus::PendingCancel => Self::PendingCancel,
            BitmexOrderStatus::Canceled => Self::Canceled,
            BitmexOrderStatus::Rejected => Self::Rejected,
            BitmexOrderStatus::Expired => Self::Expired,
        }
    }
}

/// Specifies how long an order should remain active.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexTimeInForce {
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

impl TryFrom<BitmexTimeInForce> for TimeInForce {
    type Error = BitmexError;

    fn try_from(value: BitmexTimeInForce) -> Result<Self, Self::Error> {
        match value {
            BitmexTimeInForce::Day => Ok(Self::Day),
            BitmexTimeInForce::GoodTillCancel => Ok(Self::Gtc),
            BitmexTimeInForce::GoodTillDate => Ok(Self::Gtd),
            BitmexTimeInForce::ImmediateOrCancel => Ok(Self::Ioc),
            BitmexTimeInForce::FillOrKill => Ok(Self::Fok),
            BitmexTimeInForce::AtTheOpening => Ok(Self::AtTheOpen),
            BitmexTimeInForce::AtTheClose => Ok(Self::AtTheClose),
            _ => Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation {
                    field: "time_in_force".to_string(),
                    message: format!("Unsupported BitmexTimeInForce: {value}"),
                },
            }),
        }
    }
}

impl TryFrom<TimeInForce> for BitmexTimeInForce {
    type Error = crate::error::BitmexError;

    fn try_from(value: TimeInForce) -> Result<Self, Self::Error> {
        match value {
            TimeInForce::Day => Ok(Self::Day),
            TimeInForce::Gtc => Ok(Self::GoodTillCancel),
            TimeInForce::Gtd => Ok(Self::GoodTillDate),
            TimeInForce::Ioc => Ok(Self::ImmediateOrCancel),
            TimeInForce::Fok => Ok(Self::FillOrKill),
            TimeInForce::AtTheOpen => Ok(Self::AtTheOpening),
            TimeInForce::AtTheClose => Ok(Self::AtTheClose),
        }
    }
}

impl BitmexTimeInForce {
    /// Try to convert from Nautilus TimeInForce with anyhow::Result.
    ///
    /// # Errors
    ///
    /// Returns an error if the time in force is not supported by BitMEX.
    pub fn try_from_time_in_force(value: TimeInForce) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Represents the available contingency types on BitMEX.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexContingencyType {
    OneCancelsTheOther,
    OneTriggersTheOther,
    OneUpdatesTheOtherAbsolute,
    OneUpdatesTheOtherProportional,
    #[serde(rename = "")]
    Unknown, // Can be empty
}

impl From<BitmexContingencyType> for ContingencyType {
    fn from(value: BitmexContingencyType) -> Self {
        match value {
            BitmexContingencyType::OneCancelsTheOther => Self::Oco,
            BitmexContingencyType::OneTriggersTheOther => Self::Oto,
            BitmexContingencyType::OneUpdatesTheOtherProportional => Self::Ouo,
            BitmexContingencyType::OneUpdatesTheOtherAbsolute => Self::Ouo,
            BitmexContingencyType::Unknown => Self::NoContingency,
        }
    }
}

impl TryFrom<ContingencyType> for BitmexContingencyType {
    type Error = BitmexError;

    fn try_from(value: ContingencyType) -> Result<Self, Self::Error> {
        match value {
            ContingencyType::NoContingency => Ok(Self::Unknown),
            ContingencyType::Oco => Ok(Self::OneCancelsTheOther),
            ContingencyType::Oto => Ok(Self::OneTriggersTheOther),
            ContingencyType::Ouo => Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation {
                    field: "contingency_type".to_string(),
                    message: "OUO contingency type not supported by BitMEX".to_string(),
                },
            }),
        }
    }
}

/// Represents the available peg price types on BitMEX.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexPegPriceType {
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

/// Represents the available execution instruments on BitMEX.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexExecInstruction {
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

impl BitmexExecInstruction {
    /// Joins execution instructions into the comma-separated string expected by BitMEX.
    pub fn join(instructions: &[Self]) -> String {
        instructions
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Represents the type of execution that generated a trade.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexExecType {
    /// New order placed.
    New,
    /// Normal trade execution.
    Trade,
    /// Order canceled.
    Canceled,
    /// Cancel request rejected.
    CancelReject,
    /// Order replaced.
    Replaced,
    /// Order rejected.
    Rejected,
    /// Order amendment rejected.
    AmendReject,
    /// Funding rate execution.
    Funding,
    /// Settlement execution.
    Settlement,
    /// Order suspended.
    Suspended,
    /// Order released.
    Released,
    /// Insurance payment.
    Insurance,
    /// Rebalance.
    Rebalance,
    /// Liquidation execution.
    Liquidation,
    /// Bankruptcy execution.
    Bankruptcy,
    /// Trial fill (testnet only).
    TrialFill,
    /// Stop/trigger order activated by system.
    TriggeredOrActivatedBySystem,
}

/// Indicates whether the execution was maker or taker.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexLiquidityIndicator {
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

impl From<BitmexLiquidityIndicator> for LiquiditySide {
    fn from(value: BitmexLiquidityIndicator) -> Self {
        match value {
            BitmexLiquidityIndicator::Maker => Self::Maker,
            BitmexLiquidityIndicator::Taker => Self::Taker,
        }
    }
}

/// Represents BitMEX instrument types.
#[derive(
    Copy,
    Clone,
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
#[serde(rename_all = "UPPERCASE")]
pub enum BitmexInstrumentType {
    #[serde(rename = "FXXXS")]
    Unknown1, // TODO: Determine name (option)

    #[serde(rename = "FMXXS")]
    Unknown2, // TODO: Determine name (option)

    /// Prediction Markets (non-standardized financial future on index, cash settled).
    /// CFI code FFICSX - traders predict outcomes of events.
    #[serde(rename = "FFICSX")]
    PredictionMarket,

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

    /// BitMEX Basket Index.
    #[serde(rename = "MRBXXX")]
    BasketIndex,

    /// BitMEX Crypto Index.
    #[serde(rename = "MRCXXX")]
    CryptoIndex,

    /// BitMEX FX Index.
    #[serde(rename = "MRFXXX")]
    FxIndex,

    /// BitMEX Lending/Premium Index.
    #[serde(rename = "MRRXXX")]
    LendingIndex,

    /// BitMEX Volatility Index.
    #[serde(rename = "MRIXXX")]
    VolatilityIndex,
}

/// Represents the different types of instrument subscriptions available on BitMEX.
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

/// Represents the tick direction of the last trade.
#[derive(
    Copy,
    Clone,
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
pub enum BitmexTickDirection {
    /// Price increased on last trade.
    PlusTick,
    /// Price decreased on last trade.
    MinusTick,
    /// Price unchanged, but previous tick was plus.
    ZeroPlusTick,
    /// Price unchanged, but previous tick was minus.
    ZeroMinusTick,
}

/// Represents the state of an instrument.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum BitmexInstrumentState {
    /// Instrument is open for trading.
    Open,
    /// Instrument is closed for trading.
    Closed,
    /// Instrument is unlisted.
    Unlisted,
    /// Instrument is settled.
    Settled,
}

/// Represents the fair price calculation method.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum BitmexFairMethod {
    /// Funding rate based.
    FundingRate,
    /// Impact mid price.
    ImpactMidPrice,
    /// Last price.
    LastPrice,
}

/// Represents the mark price calculation method.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
pub enum BitmexMarkMethod {
    /// Fair price.
    FairPrice,
    /// Last price.
    LastPrice,
    /// Composite index.
    CompositeIndex,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_bitmex_side_deserialization() {
        // Test all case variations
        assert_eq!(
            serde_json::from_str::<BitmexSide>(r#""Buy""#).unwrap(),
            BitmexSide::Buy
        );
        assert_eq!(
            serde_json::from_str::<BitmexSide>(r#""BUY""#).unwrap(),
            BitmexSide::Buy
        );
        assert_eq!(
            serde_json::from_str::<BitmexSide>(r#""buy""#).unwrap(),
            BitmexSide::Buy
        );
        assert_eq!(
            serde_json::from_str::<BitmexSide>(r#""Sell""#).unwrap(),
            BitmexSide::Sell
        );
        assert_eq!(
            serde_json::from_str::<BitmexSide>(r#""SELL""#).unwrap(),
            BitmexSide::Sell
        );
        assert_eq!(
            serde_json::from_str::<BitmexSide>(r#""sell""#).unwrap(),
            BitmexSide::Sell
        );
    }

    #[rstest]
    fn test_bitmex_order_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""Market""#).unwrap(),
            BitmexOrderType::Market
        );
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""Limit""#).unwrap(),
            BitmexOrderType::Limit
        );
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""Stop""#).unwrap(),
            BitmexOrderType::Stop
        );
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""StopLimit""#).unwrap(),
            BitmexOrderType::StopLimit
        );
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""MarketIfTouched""#).unwrap(),
            BitmexOrderType::MarketIfTouched
        );
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""LimitIfTouched""#).unwrap(),
            BitmexOrderType::LimitIfTouched
        );
        assert_eq!(
            serde_json::from_str::<BitmexOrderType>(r#""Pegged""#).unwrap(),
            BitmexOrderType::Pegged
        );
    }

    #[rstest]
    fn test_instrument_type_serialization() {
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::PerpetualContract).unwrap(),
            r#""FFWCSX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::PerpetualContractFx).unwrap(),
            r#""FFWCSF""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::Spot).unwrap(),
            r#""IFXXXP""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::Futures).unwrap(),
            r#""FFCCSX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::BasketIndex).unwrap(),
            r#""MRBXXX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::CryptoIndex).unwrap(),
            r#""MRCXXX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::FxIndex).unwrap(),
            r#""MRFXXX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::LendingIndex).unwrap(),
            r#""MRRXXX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::VolatilityIndex).unwrap(),
            r#""MRIXXX""#
        );
        assert_eq!(
            serde_json::to_string(&BitmexInstrumentType::PredictionMarket).unwrap(),
            r#""FFICSX""#
        );
    }

    #[rstest]
    fn test_instrument_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""FFWCSX""#).unwrap(),
            BitmexInstrumentType::PerpetualContract
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""FFWCSF""#).unwrap(),
            BitmexInstrumentType::PerpetualContractFx
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""IFXXXP""#).unwrap(),
            BitmexInstrumentType::Spot
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""FFCCSX""#).unwrap(),
            BitmexInstrumentType::Futures
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""MRBXXX""#).unwrap(),
            BitmexInstrumentType::BasketIndex
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""MRCXXX""#).unwrap(),
            BitmexInstrumentType::CryptoIndex
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""MRFXXX""#).unwrap(),
            BitmexInstrumentType::FxIndex
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""MRRXXX""#).unwrap(),
            BitmexInstrumentType::LendingIndex
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""MRIXXX""#).unwrap(),
            BitmexInstrumentType::VolatilityIndex
        );
        assert_eq!(
            serde_json::from_str::<BitmexInstrumentType>(r#""FFICSX""#).unwrap(),
            BitmexInstrumentType::PredictionMarket
        );

        // Error case
        assert!(serde_json::from_str::<BitmexInstrumentType>(r#""INVALID""#).is_err());
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

    #[rstest]
    fn test_order_side_try_from() {
        // Valid conversions
        assert_eq!(
            BitmexSide::try_from(OrderSide::Buy).unwrap(),
            BitmexSide::Buy
        );
        assert_eq!(
            BitmexSide::try_from(OrderSide::Sell).unwrap(),
            BitmexSide::Sell
        );

        // Invalid conversions
        let result = BitmexSide::try_from(OrderSide::NoOrderSide);
        assert!(result.is_err());
        match result {
            Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation { field, .. },
                ..
            }) => {
                assert_eq!(field, "order_side");
            }
            _ => panic!("Expected validation error"),
        }
    }

    #[rstest]
    fn test_order_type_try_from() {
        // Valid conversions
        assert_eq!(
            BitmexOrderType::try_from(OrderType::Market).unwrap(),
            BitmexOrderType::Market
        );
        assert_eq!(
            BitmexOrderType::try_from(OrderType::Limit).unwrap(),
            BitmexOrderType::Limit
        );

        // MarketToLimit should fail
        let result = BitmexOrderType::try_from(OrderType::MarketToLimit);
        assert!(result.is_err());
        match result {
            Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation { message, .. },
                ..
            }) => {
                assert!(message.contains("not supported"));
            }
            _ => panic!("Expected validation error"),
        }
    }

    #[rstest]
    fn test_time_in_force_conversions() {
        // BitMEX to Nautilus (all supported variants)
        assert_eq!(
            TimeInForce::try_from(BitmexTimeInForce::Day).unwrap(),
            TimeInForce::Day
        );
        assert_eq!(
            TimeInForce::try_from(BitmexTimeInForce::GoodTillCancel).unwrap(),
            TimeInForce::Gtc
        );
        assert_eq!(
            TimeInForce::try_from(BitmexTimeInForce::ImmediateOrCancel).unwrap(),
            TimeInForce::Ioc
        );

        // Unsupported BitMEX variants should fail
        let result = TimeInForce::try_from(BitmexTimeInForce::GoodTillCrossing);
        assert!(result.is_err());
        match result {
            Err(BitmexError::NonRetryable {
                source: BitmexNonRetryableError::Validation { field, message },
                ..
            }) => {
                assert_eq!(field, "time_in_force");
                assert!(message.contains("Unsupported"));
            }
            _ => panic!("Expected validation error"),
        }

        // Nautilus to BitMEX (all supported variants)
        assert_eq!(
            BitmexTimeInForce::try_from(TimeInForce::Day).unwrap(),
            BitmexTimeInForce::Day
        );
        assert_eq!(
            BitmexTimeInForce::try_from(TimeInForce::Gtc).unwrap(),
            BitmexTimeInForce::GoodTillCancel
        );
        assert_eq!(
            BitmexTimeInForce::try_from(TimeInForce::Fok).unwrap(),
            BitmexTimeInForce::FillOrKill
        );
    }

    #[rstest]
    fn test_helper_methods() {
        // Test try_from_order_side helper
        let result = BitmexSide::try_from_order_side(OrderSide::Buy);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BitmexSide::Buy);

        let result = BitmexSide::try_from_order_side(OrderSide::NoOrderSide);
        assert!(result.is_err());

        // Test try_from_order_type helper
        let result = BitmexOrderType::try_from_order_type(OrderType::Limit);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BitmexOrderType::Limit);

        let result = BitmexOrderType::try_from_order_type(OrderType::MarketToLimit);
        assert!(result.is_err());

        // Test try_from_time_in_force helper
        let result = BitmexTimeInForce::try_from_time_in_force(TimeInForce::Ioc);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), BitmexTimeInForce::ImmediateOrCancel);
    }
}
