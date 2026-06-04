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

//! Lighter venue enums mirrored from REST and WebSocket payloads.

use std::fmt::Display;

use nautilus_model::{
    data::{BarSpecification, BarType},
    enums::{AggregationSource, BarAggregation, OrderSide, OrderType, PriceType},
};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Lighter API environment.
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
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.lighter",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.lighter")
)]
pub enum LighterEnvironment {
    /// Mainnet trading environment.
    #[default]
    Mainnet,
    /// Testnet environment.
    Testnet,
}

/// Lighter product type. Markets on the venue are either perpetual futures or spot.
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
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum LighterProductType {
    /// Perpetual futures.
    Perp,
    /// Spot markets.
    Spot,
}

/// Lighter historical candle resolution.
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
#[strum(ascii_case_insensitive)]
pub enum LighterCandleResolution {
    /// One-minute candles.
    #[serde(rename = "1m")]
    #[strum(serialize = "1m")]
    OneMinute,
    /// Five-minute candles.
    #[serde(rename = "5m")]
    #[strum(serialize = "5m")]
    FiveMinute,
    /// Fifteen-minute candles.
    #[serde(rename = "15m")]
    #[strum(serialize = "15m")]
    FifteenMinute,
    /// Thirty-minute candles.
    #[serde(rename = "30m")]
    #[strum(serialize = "30m")]
    ThirtyMinute,
    /// One-hour candles.
    #[serde(rename = "1h")]
    #[strum(serialize = "1h")]
    OneHour,
    /// Four-hour candles.
    #[serde(rename = "4h")]
    #[strum(serialize = "4h")]
    FourHour,
    /// Twelve-hour candles.
    #[serde(rename = "12h")]
    #[strum(serialize = "12h")]
    TwelveHour,
    /// One-day candles.
    #[serde(rename = "1d")]
    #[strum(serialize = "1d")]
    OneDay,
    /// One-week candles.
    #[serde(rename = "1w")]
    #[strum(serialize = "1w")]
    OneWeek,
}

impl LighterCandleResolution {
    /// Returns the REST `resolution` string accepted by Lighter.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::FiveMinute => "5m",
            Self::FifteenMinute => "15m",
            Self::ThirtyMinute => "30m",
            Self::OneHour => "1h",
            Self::FourHour => "4h",
            Self::TwelveHour => "12h",
            Self::OneDay => "1d",
            Self::OneWeek => "1w",
        }
    }

    /// Returns the candle interval in milliseconds.
    #[must_use]
    pub const fn interval_millis(self) -> i64 {
        match self {
            Self::OneMinute => 60_000,
            Self::FiveMinute => 5 * 60_000,
            Self::FifteenMinute => 15 * 60_000,
            Self::ThirtyMinute => 30 * 60_000,
            Self::OneHour => 60 * 60_000,
            Self::FourHour => 4 * 60 * 60_000,
            Self::TwelveHour => 12 * 60 * 60_000,
            Self::OneDay => 24 * 60 * 60_000,
            Self::OneWeek => 7 * 24 * 60 * 60_000,
        }
    }

    /// Returns the Nautilus [`BarSpecification`] for this candle resolution (`Last` / `External`).
    #[must_use]
    pub fn to_bar_spec(self) -> BarSpecification {
        let (step, aggregation) = match self {
            Self::OneMinute => (1, BarAggregation::Minute),
            Self::FiveMinute => (5, BarAggregation::Minute),
            Self::FifteenMinute => (15, BarAggregation::Minute),
            Self::ThirtyMinute => (30, BarAggregation::Minute),
            Self::OneHour => (1, BarAggregation::Hour),
            Self::FourHour => (4, BarAggregation::Hour),
            Self::TwelveHour => (12, BarAggregation::Hour),
            Self::OneDay => (1, BarAggregation::Day),
            Self::OneWeek => (1, BarAggregation::Week),
        };
        BarSpecification::new(step, aggregation, PriceType::Last)
    }

    /// Returns `true` when this resolution is offered on the candle WebSocket stream.
    ///
    /// `1w` is REST-only; the streaming channel only carries `1m`..=`1d`.
    #[must_use]
    pub const fn is_ws_streamable(self) -> bool {
        !matches!(self, Self::OneWeek)
    }
}

impl TryFrom<&BarType> for LighterCandleResolution {
    type Error = anyhow::Error;

    fn try_from(value: &BarType) -> Result<Self, Self::Error> {
        anyhow::ensure!(
            value.aggregation_source() == AggregationSource::External,
            "Lighter candles only support EXTERNAL aggregation",
        );

        let spec = value.spec();
        anyhow::ensure!(
            spec.price_type == PriceType::Last,
            "Lighter candles only support LAST price type",
        );

        let step = spec.step.get();
        match spec.aggregation {
            BarAggregation::Minute => match step {
                1 => Ok(Self::OneMinute),
                5 => Ok(Self::FiveMinute),
                15 => Ok(Self::FifteenMinute),
                30 => Ok(Self::ThirtyMinute),
                _ => anyhow::bail!("unsupported Lighter candle minute step: {step}"),
            },
            BarAggregation::Hour => match step {
                1 => Ok(Self::OneHour),
                4 => Ok(Self::FourHour),
                12 => Ok(Self::TwelveHour),
                _ => anyhow::bail!("unsupported Lighter candle hour step: {step}"),
            },
            BarAggregation::Day => match step {
                1 => Ok(Self::OneDay),
                _ => anyhow::bail!("unsupported Lighter candle day step: {step}"),
            },
            BarAggregation::Week => match step {
                1 => Ok(Self::OneWeek),
                _ => anyhow::bail!("unsupported Lighter candle week step: {step}"),
            },
            other => anyhow::bail!("unsupported Lighter candle aggregation: {other}"),
        }
    }
}

/// Lighter historical funding resolution.
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
#[strum(ascii_case_insensitive)]
pub enum LighterFundingResolution {
    /// One-hour funding history.
    #[default]
    #[serde(rename = "1h")]
    #[strum(serialize = "1h")]
    OneHour,
    /// One-day funding history.
    #[serde(rename = "1d")]
    #[strum(serialize = "1d")]
    OneDay,
}

impl LighterFundingResolution {
    /// Returns the funding interval in minutes.
    #[must_use]
    pub const fn interval_minutes(self) -> u16 {
        match self {
            Self::OneHour => 60,
            Self::OneDay => 24 * 60,
        }
    }

    /// Returns the funding interval in milliseconds.
    #[must_use]
    pub const fn interval_millis(self) -> i64 {
        match self {
            Self::OneHour => 60 * 60_000,
            Self::OneDay => 24 * 60 * 60_000,
        }
    }
}

/// Filter accepted by Lighter market metadata endpoints.
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
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum LighterOrderBookFilter {
    /// Return all markets.
    #[default]
    All,
    /// Return perpetual markets only.
    Perp,
    /// Return spot markets only.
    Spot,
}

/// Status for Lighter market metadata.
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
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum LighterMarketStatus {
    /// Market is not available for trading.
    Inactive,
    /// Market is available for trading.
    Active,
}

/// String order type used by REST and WebSocket order payloads.
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
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum LighterOrderKind {
    Limit,
    Market,
    StopLoss,
    StopLossLimit,
    TakeProfit,
    TakeProfitLimit,
    Twap,
    TwapSub,
    Liquidation,
}

/// String time-in-force used by REST and WebSocket order payloads.
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
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum LighterOrderTimeInForce {
    GoodTillTime,
    ImmediateOrCancel,
    PostOnly,
    #[serde(alias = "unknown")]
    #[serde(rename = "Unknown")]
    #[strum(serialize = "Unknown")]
    Unknown,
}

/// String order status used by REST and WebSocket order payloads.
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
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum LighterOrderStatus {
    InProgress,
    Pending,
    Open,
    Filled,
    Canceled,
    CanceledPostOnly,
    CanceledReduceOnly,
    CanceledPositionNotAllowed,
    CanceledMarginNotAllowed,
    CanceledTooMuchSlippage,
    CanceledNotEnoughLiquidity,
    CanceledSelfTrade,
    CanceledExpired,
    CanceledOco,
    CanceledChild,
    CanceledLiquidation,
    CanceledInvalidBalance,
}

impl LighterOrderStatus {
    /// Short kebab-case label describing a cancellation cause, suitable for
    /// [`nautilus_model::reports::OrderStatusReport::with_cancel_reason`].
    ///
    /// Returns `None` for non-cancelled statuses and for the unqualified
    /// [`Canceled`](Self::Canceled) / [`CanceledExpired`](Self::CanceledExpired)
    /// variants, which carry their meaning via the Nautilus order status itself.
    #[must_use]
    pub fn as_cancel_reason(self) -> Option<&'static str> {
        match self {
            Self::CanceledPostOnly => Some("post-only"),
            Self::CanceledReduceOnly => Some("reduce-only"),
            Self::CanceledPositionNotAllowed => Some("position-not-allowed"),
            Self::CanceledMarginNotAllowed => Some("margin-not-allowed"),
            Self::CanceledTooMuchSlippage => Some("too-much-slippage"),
            Self::CanceledNotEnoughLiquidity => Some("not-enough-liquidity"),
            Self::CanceledSelfTrade => Some("self-trade"),
            Self::CanceledOco => Some("oco"),
            Self::CanceledChild => Some("child"),
            Self::CanceledLiquidation => Some("liquidation"),
            Self::CanceledInvalidBalance => Some("invalid-balance"),
            _ => None,
        }
    }
}

/// Side string used by REST and WebSocket order payloads.
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
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum LighterOrderSide {
    Buy,
    Sell,
}

/// Trigger status used by conditional order payloads.
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
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum LighterTriggerStatus {
    Na,
    Ready,
    MarkPrice,
    Twap,
    ParentOrder,
}

/// Trade type used by REST and WebSocket trade payloads.
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
#[serde(rename_all = "kebab-case")]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum LighterTradeType {
    Trade,
    Liquidation,
    Deleverage,
    MarketSettlement,
}

/// Lighter order type as encoded in the venue's binary order payload.
///
/// Numeric values are part of the wire format and must match the venue spec.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterOrderType {
    Limit = 0,
    Market = 1,
    StopLoss = 2,
    StopLossLimit = 3,
    TakeProfit = 4,
    TakeProfitLimit = 5,
    Twap = 6,
    TwapSub = 7,
    Liquidation = 8,
}

impl LighterOrderType {
    /// Returns the Nautilus [`OrderType`] that this Lighter order type maps to.
    ///
    /// `StopLoss` / `StopLossLimit` map to Nautilus stop-on-the-loss-side
    /// triggers (`StopMarket` / `StopLimit`), while `TakeProfit` /
    /// `TakeProfitLimit` map to "if-touched" triggers
    /// (`MarketIfTouched` / `LimitIfTouched`) which fire when price reaches
    /// a target rather than crosses a stop.
    ///
    /// # Errors
    ///
    /// Returns an error for venue-internal or algorithmic order types which
    /// do not map to a single Nautilus order type.
    pub fn as_nautilus(self) -> anyhow::Result<OrderType> {
        match self {
            Self::Limit => Ok(OrderType::Limit),
            Self::Market => Ok(OrderType::Market),
            Self::StopLoss => Ok(OrderType::StopMarket),
            Self::StopLossLimit => Ok(OrderType::StopLimit),
            Self::TakeProfit => Ok(OrderType::MarketIfTouched),
            Self::TakeProfitLimit => Ok(OrderType::LimitIfTouched),
            Self::Twap | Self::TwapSub | Self::Liquidation => Err(anyhow::anyhow!(
                "Lighter `{self:?}` has no Nautilus order-type equivalent",
            )),
        }
    }
}

impl TryFrom<OrderType> for LighterOrderType {
    type Error = anyhow::Error;

    fn try_from(value: OrderType) -> Result<Self, Self::Error> {
        match value {
            OrderType::Limit => Ok(Self::Limit),
            OrderType::Market => Ok(Self::Market),
            OrderType::StopMarket => Ok(Self::StopLoss),
            OrderType::StopLimit => Ok(Self::StopLossLimit),
            OrderType::MarketIfTouched => Ok(Self::TakeProfit),
            OrderType::LimitIfTouched => Ok(Self::TakeProfitLimit),
            other => Err(anyhow::anyhow!(
                "Nautilus `{other:?}` has no Lighter order-type equivalent",
            )),
        }
    }
}

/// Lighter time-in-force as encoded in the venue's binary order payload.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterTimeInForce {
    /// Immediate-or-cancel.
    ImmediateOrCancel = 0,
    /// Good-till-time.
    GoodTillTime = 1,
    /// Post-only.
    PostOnly = 2,
}

/// Lighter grouped-order relationship type.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterGroupingType {
    None = 0,
    OneTriggersTheOther = 1,
    OneCancelsTheOther = 2,
    OneTriggersOneCancelsTheOther = 3,
}

/// Lighter cancel-all-orders time-in-force.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterCancelAllTimeInForce {
    Immediate = 0,
    Scheduled = 1,
    AbortScheduled = 2,
}

/// Lighter asset margin mode.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterAssetMarginMode {
    Disabled = 0,
    Enabled = 1,
}

/// Lighter asset route type.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterAssetRouteType {
    Perps = 0,
    Spot = 1,
}

/// Lighter position margin mode.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterPositionMarginMode {
    Cross = 0,
    Isolated = 1,
}

/// Lighter isolated-margin update direction.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterMarginUpdateDirection {
    RemoveFromIsolated = 0,
    AddToIsolated = 1,
}

/// Lighter account tier, classified from the venue `account_type` code.
///
/// The code `0` is confirmed to be the standard tier. Codes for the higher
/// tiers are not published in the venue schema, so they are mapped on a
/// best-effort basis and any unrecognized code is preserved as
/// [`Self::Unknown`] rather than silently misclassified. This type is a
/// classification of the raw `account_type` byte, not a wire representation, so
/// it is not serialized.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LighterAccountTier {
    Standard,
    Premium,
    Plus,
    Builder,
    Unknown(u8),
}

impl LighterAccountTier {
    /// Classifies a venue `account_type` code into a tier.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Standard,
            1 => Self::Premium,
            2 => Self::Plus,
            3 => Self::Builder,
            other => Self::Unknown(other),
        }
    }

    /// Returns the documented REST weighted limit (requests per minute) for the
    /// tier, or `None` when the tier is unrecognized.
    ///
    /// This drives log hints only. The adapter never sets the active quota from
    /// this value, because the higher limits require registering the caller IP
    /// with the venue and so are not guaranteed by the tier alone.
    #[must_use]
    pub const fn documented_rest_quota_per_min(self) -> Option<u32> {
        match self {
            Self::Standard => Some(60),
            Self::Premium => Some(24_000),
            Self::Plus => Some(120_000),
            Self::Builder => Some(240_000),
            Self::Unknown(_) => None,
        }
    }
}

impl Display for LighterAccountTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
            Self::Plus => f.write_str("Plus"),
            Self::Builder => f.write_str("Builder"),
            Self::Unknown(code) => write!(f, "Unknown({code})"),
        }
    }
}

// Conversions between `LighterTimeInForce` and Nautilus `TimeInForce` are
// intentionally not provided here. `GoodTillTime` is ambiguous in isolation:
// the venue uses it for both true GTD (paired with a positive `order_expiry`
// timestamp) and venue-default lifetime (paired with `order_expiry = -1`).
// The mapping must happen at the parse / order-build call site where the
// `order_expiry` value is also in scope.

/// Converts a Nautilus [`OrderSide`] to the venue's `is_ask` boolean.
///
/// Lighter's tx body encodes the side as `is_ask`: `false` for a bid (buy)
/// and `true` for an ask (sell).
///
/// # Errors
///
/// Returns an error if `side` is [`OrderSide::NoOrderSide`].
pub fn is_ask_from_order_side(side: OrderSide) -> anyhow::Result<bool> {
    match side {
        OrderSide::Buy => Ok(false),
        OrderSide::Sell => Ok(true),
        OrderSide::NoOrderSide => Err(anyhow::anyhow!("Lighter requires a specified order side")),
    }
}

/// Converts the venue's `is_ask` boolean to a Nautilus [`OrderSide`].
#[must_use]
pub fn order_side_from_is_ask(is_ask: bool) -> OrderSide {
    if is_ask {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    }
}

/// Lighter L2 transaction-type discriminants.
///
/// These are the `tx_type` values the venue accepts on `sendTx` / `sendTxBatch`.
/// Numeric values are wire-format and must match the venue spec.
#[derive(
    Copy, Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum LighterTxType {
    Empty = 0,
    L1Deposit = 1,
    L1ChangePubKey = 2,
    L1CreateMarket = 3,
    L1UpdateMarket = 4,
    L1CancelAllOrders = 5,
    L1Withdraw = 6,
    L1CreateOrder = 7,
    ChangePubKey = 8,
    CreateSubAccount = 9,
    CreatePublicPool = 10,
    UpdatePublicPool = 11,
    Transfer = 12,
    Withdraw = 13,
    CreateOrder = 14,
    CancelOrder = 15,
    CancelAllOrders = 16,
    ModifyOrder = 17,
    MintShares = 18,
    BurnShares = 19,
    UpdateLeverage = 20,
    InternalClaimOrder = 21,
    InternalCancelOrder = 22,
    InternalDeleverage = 23,
    InternalExitPosition = 24,
    InternalCancelAllOrders = 25,
    InternalLiquidatePosition = 26,
    InternalCreateOrder = 27,
    CreateGroupedOrders = 28,
    UpdateMargin = 29,
    L1BurnShares = 30,
    L1RegisterAsset = 31,
    L1UpdateAsset = 32,
    CreateStakingPool = 33,
    StakeAssets = 35,
    UnstakeAssets = 36,
    L1UnstakeAssets = 37,
    L1SetSystemConfig = 38,
    ForceBurnShares = 40,
    UpdateAccountConfig = 41,
    StrategyTransfer = 43,
    UpdateMarketConfig = 44,
    ApproveIntegrator = 45,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        data::{BarSpecification, BarType},
        identifiers::InstrumentId,
    };
    use rstest::rstest;
    use serde_json;

    use super::*;

    #[rstest]
    fn test_environment_default_is_mainnet() {
        assert_eq!(LighterEnvironment::default(), LighterEnvironment::Mainnet);
    }

    #[rstest]
    fn test_product_type_serde_uses_wire_values() {
        assert_eq!(
            serde_json::to_string(&LighterProductType::Perp).unwrap(),
            r#""perp""#,
        );
        assert_eq!(
            serde_json::from_str::<LighterProductType>(r#""spot""#).unwrap(),
            LighterProductType::Spot,
        );
    }

    #[rstest]
    fn test_market_status_serde() {
        assert_eq!(
            serde_json::from_str::<LighterMarketStatus>(r#""active""#).unwrap(),
            LighterMarketStatus::Active,
        );
        assert_eq!(
            serde_json::to_string(&LighterMarketStatus::Inactive).unwrap(),
            r#""inactive""#,
        );
    }

    #[rstest]
    fn test_string_order_enums_serde() {
        assert_eq!(
            serde_json::from_str::<LighterOrderKind>(r#""take-profit-limit""#).unwrap(),
            LighterOrderKind::TakeProfitLimit,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderKind>(r#""twap-sub""#).unwrap(),
            LighterOrderKind::TwapSub,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderKind>(r#""liquidation""#).unwrap(),
            LighterOrderKind::Liquidation,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderTimeInForce>(r#""good-till-time""#).unwrap(),
            LighterOrderTimeInForce::GoodTillTime,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderTimeInForce>(r#""Unknown""#).unwrap(),
            LighterOrderTimeInForce::Unknown,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderTimeInForce>(r#""unknown""#).unwrap(),
            LighterOrderTimeInForce::Unknown,
        );
        assert_eq!(
            serde_json::to_string(&LighterOrderTimeInForce::Unknown).unwrap(),
            r#""Unknown""#,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderStatus>(r#""canceled-not-enough-liquidity""#)
                .unwrap(),
            LighterOrderStatus::CanceledNotEnoughLiquidity,
        );
        assert_eq!(
            serde_json::from_str::<LighterTriggerStatus>(r#""parent-order""#).unwrap(),
            LighterTriggerStatus::ParentOrder,
        );
        assert_eq!(
            serde_json::from_str::<LighterOrderSide>(r#""sell""#).unwrap(),
            LighterOrderSide::Sell,
        );
    }

    #[rstest]
    fn test_trade_type_serde() {
        assert_eq!(
            serde_json::from_str::<LighterTradeType>(r#""market-settlement""#).unwrap(),
            LighterTradeType::MarketSettlement,
        );
    }

    #[rstest]
    fn test_order_type_repr_serde() {
        assert_eq!(
            serde_json::to_string(&LighterOrderType::Limit).unwrap(),
            "0",
        );
        assert_eq!(serde_json::to_string(&LighterOrderType::Twap).unwrap(), "6",);
        assert_eq!(
            serde_json::to_string(&LighterOrderType::TwapSub).unwrap(),
            "7",
        );
        assert_eq!(
            serde_json::to_string(&LighterOrderType::Liquidation).unwrap(),
            "8",
        );
        let parsed: LighterOrderType = serde_json::from_str("3").unwrap();
        assert_eq!(parsed, LighterOrderType::StopLossLimit);
    }

    #[rstest]
    fn test_numeric_constant_enums_repr_serde() {
        assert_eq!(
            serde_json::to_string(&LighterGroupingType::OneTriggersOneCancelsTheOther).unwrap(),
            "3",
        );
        assert_eq!(
            serde_json::to_string(&LighterCancelAllTimeInForce::AbortScheduled).unwrap(),
            "2",
        );
        assert_eq!(
            serde_json::to_string(&LighterPositionMarginMode::Isolated).unwrap(),
            "1",
        );
        assert_eq!(
            serde_json::to_string(&LighterMarginUpdateDirection::AddToIsolated).unwrap(),
            "1",
        );
    }

    #[rstest]
    fn test_time_in_force_repr_serde() {
        assert_eq!(
            serde_json::to_string(&LighterTimeInForce::ImmediateOrCancel).unwrap(),
            "0",
        );
        assert_eq!(
            serde_json::to_string(&LighterTimeInForce::PostOnly).unwrap(),
            "2",
        );
    }

    #[rstest]
    #[case::one_minute(LighterCandleResolution::OneMinute, "1m")]
    #[case::five_minute(LighterCandleResolution::FiveMinute, "5m")]
    #[case::fifteen_minute(LighterCandleResolution::FifteenMinute, "15m")]
    #[case::thirty_minute(LighterCandleResolution::ThirtyMinute, "30m")]
    #[case::one_hour(LighterCandleResolution::OneHour, "1h")]
    #[case::four_hour(LighterCandleResolution::FourHour, "4h")]
    #[case::twelve_hour(LighterCandleResolution::TwelveHour, "12h")]
    #[case::one_day(LighterCandleResolution::OneDay, "1d")]
    #[case::one_week(LighterCandleResolution::OneWeek, "1w")]
    fn test_candle_resolution_string_round_trip(
        #[case] resolution: LighterCandleResolution,
        #[case] expected: &str,
    ) {
        assert_eq!(resolution.as_str(), expected);
        assert_eq!(resolution.to_string(), expected);
        assert_eq!(
            serde_json::to_string(&resolution).unwrap(),
            format!("\"{expected}\""),
        );
        assert_eq!(
            serde_json::from_str::<LighterCandleResolution>(&format!("\"{expected}\"")).unwrap(),
            resolution,
        );
        assert_eq!(
            LighterCandleResolution::from_str(expected).unwrap(),
            resolution
        );
        assert!(resolution.interval_millis() > 0);
    }

    #[rstest]
    #[case::one_minute(1, BarAggregation::Minute, LighterCandleResolution::OneMinute)]
    #[case::five_minute(5, BarAggregation::Minute, LighterCandleResolution::FiveMinute)]
    #[case::fifteen_minute(15, BarAggregation::Minute, LighterCandleResolution::FifteenMinute)]
    #[case::thirty_minute(30, BarAggregation::Minute, LighterCandleResolution::ThirtyMinute)]
    #[case::one_hour(1, BarAggregation::Hour, LighterCandleResolution::OneHour)]
    #[case::four_hour(4, BarAggregation::Hour, LighterCandleResolution::FourHour)]
    #[case::twelve_hour(12, BarAggregation::Hour, LighterCandleResolution::TwelveHour)]
    #[case::one_day(1, BarAggregation::Day, LighterCandleResolution::OneDay)]
    #[case::one_week(1, BarAggregation::Week, LighterCandleResolution::OneWeek)]
    fn test_candle_resolution_from_bar_type(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: LighterCandleResolution,
    ) {
        let bar_type = lighter_bar_type(
            step,
            aggregation,
            PriceType::Last,
            AggregationSource::External,
        );

        assert_eq!(
            LighterCandleResolution::try_from(&bar_type).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case::three_minute(3, BarAggregation::Minute, "minute step")]
    #[case::two_hour(2, BarAggregation::Hour, "hour step")]
    #[case::two_day(2, BarAggregation::Day, "day step")]
    #[case::two_week(2, BarAggregation::Week, "week step")]
    #[case::one_second(1, BarAggregation::Second, "aggregation")]
    fn test_candle_resolution_rejects_unsupported_bars(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: &str,
    ) {
        let bar_type = lighter_bar_type(
            step,
            aggregation,
            PriceType::Last,
            AggregationSource::External,
        );

        let err = LighterCandleResolution::try_from(&bar_type).unwrap_err();
        assert!(err.to_string().contains(expected));
    }

    #[rstest]
    fn test_candle_resolution_rejects_internal_bars() {
        let bar_type = lighter_bar_type(
            1,
            BarAggregation::Minute,
            PriceType::Last,
            AggregationSource::Internal,
        );

        let err = LighterCandleResolution::try_from(&bar_type).unwrap_err();
        assert!(err.to_string().contains("EXTERNAL aggregation"));
    }

    #[rstest]
    #[case::one_minute(LighterCandleResolution::OneMinute, 1, BarAggregation::Minute)]
    #[case::five_minute(LighterCandleResolution::FiveMinute, 5, BarAggregation::Minute)]
    #[case::fifteen_minute(LighterCandleResolution::FifteenMinute, 15, BarAggregation::Minute)]
    #[case::thirty_minute(LighterCandleResolution::ThirtyMinute, 30, BarAggregation::Minute)]
    #[case::one_hour(LighterCandleResolution::OneHour, 1, BarAggregation::Hour)]
    #[case::four_hour(LighterCandleResolution::FourHour, 4, BarAggregation::Hour)]
    #[case::twelve_hour(LighterCandleResolution::TwelveHour, 12, BarAggregation::Hour)]
    #[case::one_day(LighterCandleResolution::OneDay, 1, BarAggregation::Day)]
    #[case::one_week(LighterCandleResolution::OneWeek, 1, BarAggregation::Week)]
    fn test_candle_resolution_to_bar_spec(
        #[case] resolution: LighterCandleResolution,
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
    ) {
        let spec = resolution.to_bar_spec();
        assert_eq!(spec.step.get(), step);
        assert_eq!(spec.aggregation, aggregation);
        assert_eq!(spec.price_type, PriceType::Last);
    }

    #[rstest]
    #[case::one_minute(LighterCandleResolution::OneMinute, true)]
    #[case::five_minute(LighterCandleResolution::FiveMinute, true)]
    #[case::fifteen_minute(LighterCandleResolution::FifteenMinute, true)]
    #[case::thirty_minute(LighterCandleResolution::ThirtyMinute, true)]
    #[case::one_hour(LighterCandleResolution::OneHour, true)]
    #[case::four_hour(LighterCandleResolution::FourHour, true)]
    #[case::twelve_hour(LighterCandleResolution::TwelveHour, true)]
    #[case::one_day(LighterCandleResolution::OneDay, true)]
    #[case::one_week(LighterCandleResolution::OneWeek, false)]
    fn test_candle_resolution_is_ws_streamable(
        #[case] resolution: LighterCandleResolution,
        #[case] expected: bool,
    ) {
        assert_eq!(resolution.is_ws_streamable(), expected);
    }

    #[rstest]
    fn test_candle_resolution_rejects_non_last_price_type() {
        let bar_type = lighter_bar_type(
            1,
            BarAggregation::Minute,
            PriceType::Mark,
            AggregationSource::External,
        );

        let err = LighterCandleResolution::try_from(&bar_type).unwrap_err();
        assert!(err.to_string().contains("LAST price type"));
    }

    #[rstest]
    #[case::limit(LighterOrderType::Limit, OrderType::Limit)]
    #[case::market(LighterOrderType::Market, OrderType::Market)]
    #[case::stop_loss(LighterOrderType::StopLoss, OrderType::StopMarket)]
    #[case::stop_loss_limit(LighterOrderType::StopLossLimit, OrderType::StopLimit)]
    #[case::take_profit(LighterOrderType::TakeProfit, OrderType::MarketIfTouched)]
    #[case::take_profit_limit(LighterOrderType::TakeProfitLimit, OrderType::LimitIfTouched)]
    fn test_order_type_round_trip(#[case] lighter: LighterOrderType, #[case] nautilus: OrderType) {
        assert_eq!(lighter.as_nautilus().unwrap(), nautilus);
        assert_eq!(LighterOrderType::try_from(nautilus).unwrap(), lighter);
    }

    #[rstest]
    #[case::twap(LighterOrderType::Twap)]
    #[case::twap_sub(LighterOrderType::TwapSub)]
    #[case::liquidation(LighterOrderType::Liquidation)]
    fn test_order_type_internal_variants_have_no_nautilus_mapping(
        #[case] order_type: LighterOrderType,
    ) {
        let err = order_type.as_nautilus().unwrap_err();
        assert!(
            err.to_string()
                .contains("no Nautilus order-type equivalent")
        );
    }

    #[rstest]
    #[case(OrderType::TrailingStopMarket)]
    #[case(OrderType::TrailingStopLimit)]
    #[case(OrderType::MarketToLimit)]
    fn test_order_type_unsupported_nautilus_variants_error(#[case] nautilus: OrderType) {
        let err = LighterOrderType::try_from(nautilus).unwrap_err();
        assert!(err.to_string().contains("no Lighter order-type equivalent"));
    }

    #[rstest]
    fn test_is_ask_round_trip() {
        assert!(!is_ask_from_order_side(OrderSide::Buy).unwrap());
        assert!(is_ask_from_order_side(OrderSide::Sell).unwrap());
        assert_eq!(order_side_from_is_ask(false), OrderSide::Buy);
        assert_eq!(order_side_from_is_ask(true), OrderSide::Sell);
    }

    #[rstest]
    fn test_is_ask_rejects_unspecified_side() {
        let err = is_ask_from_order_side(OrderSide::NoOrderSide).unwrap_err();
        assert!(err.to_string().contains("specified order side"));
    }

    #[rstest]
    fn test_tx_type_repr_serde() {
        assert_eq!(
            serde_json::to_string(&LighterTxType::CreateOrder).unwrap(),
            "14"
        );
        assert_eq!(
            serde_json::to_string(&LighterTxType::CancelAllOrders).unwrap(),
            "16",
        );
        assert_eq!(
            serde_json::to_string(&LighterTxType::ApproveIntegrator).unwrap(),
            "45",
        );
        assert_eq!(
            serde_json::to_string(&LighterTxType::CreateGroupedOrders).unwrap(),
            "28",
        );
        assert_eq!(serde_json::to_string(&LighterTxType::Empty).unwrap(), "0");
        assert_eq!(
            serde_json::to_string(&LighterTxType::L1RegisterAsset).unwrap(),
            "31",
        );
        assert_eq!(
            serde_json::to_string(&LighterTxType::ForceBurnShares).unwrap(),
            "40",
        );
        assert_eq!(
            serde_json::to_string(&LighterTxType::UpdateMarketConfig).unwrap(),
            "44",
        );
    }

    fn lighter_bar_type(
        step: usize,
        aggregation: BarAggregation,
        price_type: PriceType,
        aggregation_source: AggregationSource,
    ) -> BarType {
        BarType::new(
            InstrumentId::from("BTC-PERP.LIGHTER"),
            BarSpecification::new(step, aggregation, price_type),
            aggregation_source,
        )
    }

    #[rstest]
    #[case(0, LighterAccountTier::Standard)]
    #[case(1, LighterAccountTier::Premium)]
    #[case(2, LighterAccountTier::Plus)]
    #[case(3, LighterAccountTier::Builder)]
    #[case(4, LighterAccountTier::Unknown(4))]
    #[case(255, LighterAccountTier::Unknown(255))]
    fn test_account_tier_from_code(#[case] code: u8, #[case] expected: LighterAccountTier) {
        assert_eq!(LighterAccountTier::from_code(code), expected);
    }

    #[rstest]
    #[case(LighterAccountTier::Standard, Some(60))]
    #[case(LighterAccountTier::Premium, Some(24_000))]
    #[case(LighterAccountTier::Plus, Some(120_000))]
    #[case(LighterAccountTier::Builder, Some(240_000))]
    #[case(LighterAccountTier::Unknown(9), None)]
    fn test_account_tier_documented_rest_quota(
        #[case] tier: LighterAccountTier,
        #[case] expected: Option<u32>,
    ) {
        assert_eq!(tier.documented_rest_quota_per_min(), expected);
    }

    #[rstest]
    #[case(LighterAccountTier::Standard, "Standard")]
    #[case(LighterAccountTier::Premium, "Premium")]
    #[case(LighterAccountTier::Plus, "Plus")]
    #[case(LighterAccountTier::Builder, "Builder")]
    #[case(LighterAccountTier::Unknown(7), "Unknown(7)")]
    fn test_account_tier_display(#[case] tier: LighterAccountTier, #[case] expected: &str) {
        assert_eq!(tier.to_string(), expected);
    }
}
