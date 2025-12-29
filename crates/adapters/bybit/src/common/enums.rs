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

//! Enumerations that model Bybit string/int enums across HTTP and WebSocket payloads.

use std::fmt::{Display, Formatter};

use chrono::{DateTime, Datelike, TimeZone, Utc};
use nautilus_model::enums::{AggressorSide, OrderSide, TriggerType};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::{AsRefStr, EnumIter, EnumString};

/// Unified margin account status values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
pub enum BybitUnifiedMarginStatus {
    /// Classic account.
    ClassicAccount = 1,
    /// Unified trading account 1.0.
    UnifiedTradingAccount10 = 3,
    /// Unified trading account 1.0 pro.
    UnifiedTradingAccount10Pro = 4,
    /// Unified trading account 2.0.
    UnifiedTradingAccount20 = 5,
    /// Unified trading account 2.0 pro.
    UnifiedTradingAccount20Pro = 6,
}

/// Margin mode used by Bybit when switching risk profiles.
#[derive(
    Clone,
    Copy,
    Debug,
    strum::Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub enum BybitMarginMode {
    IsolatedMargin,
    RegularMargin,
    PortfolioMargin,
}

/// Position mode as returned by the v5 API.
#[derive(
    Clone,
    Copy,
    Debug,
    strum::Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize_repr,
    Deserialize_repr,
)]
#[repr(i32)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub enum BybitPositionMode {
    /// Merged single position mode.
    MergedSingle = 0,
    /// Dual-side hedged position mode.
    BothSides = 3,
}

/// Position index values used for hedge mode payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
pub enum BybitPositionIdx {
    /// One-way mode position identifier.
    OneWay = 0,
    /// Buy side of a hedge-mode position.
    BuyHedge = 1,
    /// Sell side of a hedge-mode position.
    SellHedge = 2,
}

/// Account type enumeration.
#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
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
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub enum BybitAccountType {
    Unified,
}

/// Environments supported by the Bybit API stack.
#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub enum BybitEnvironment {
    /// Live trading environment.
    Mainnet,
    /// Demo (paper trading) environment.
    Demo,
    /// Testnet environment for spot/derivatives.
    Testnet,
}

/// Product categories supported by the v5 API.
#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
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
#[serde(rename_all = "lowercase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub enum BybitProductType {
    #[default]
    Spot,
    Linear,
    Inverse,
    Option,
}

/// Spot margin trading enablement states.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitMarginTrading {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "utaOnly")]
    UtaOnly,
    #[serde(rename = "both")]
    Both,
    #[serde(other)]
    Other,
}

/// Innovation market flag for spot instruments.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitInnovationFlag {
    #[serde(rename = "0")]
    Standard,
    #[serde(rename = "1")]
    Innovation,
    #[serde(other)]
    Other,
}

/// Instrument lifecycle status values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BybitInstrumentStatus {
    Trading,
    Settled,
    Delivering,
    ListedOnly,
    PendingListing,
    PreTrading,
    Closed,
    Suspended,
    #[serde(other)]
    Other,
}

impl BybitProductType {
    /// Returns the canonical lowercase identifier used for REST/WS routes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::Linear => "linear",
            Self::Inverse => "inverse",
            Self::Option => "option",
        }
    }

    /// Returns the uppercase suffix used in instrument identifiers (e.g. `-LINEAR`).
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Spot => "-SPOT",
            Self::Linear => "-LINEAR",
            Self::Inverse => "-INVERSE",
            Self::Option => "-OPTION",
        }
    }

    /// Returns `true` if the product is a spot instrument.
    #[must_use]
    pub fn is_spot(self) -> bool {
        matches!(self, Self::Spot)
    }

    /// Returns `true` if the product is a linear contract.
    #[must_use]
    pub fn is_linear(self) -> bool {
        matches!(self, Self::Linear)
    }

    /// Returns `true` if the product is an inverse contract.
    #[must_use]
    pub fn is_inverse(self) -> bool {
        matches!(self, Self::Inverse)
    }

    /// Returns `true` if the product is an option contract.
    #[must_use]
    pub fn is_option(self) -> bool {
        matches!(self, Self::Option)
    }
}

/// Contract type enumeration for linear and inverse derivatives.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BybitContractType {
    LinearPerpetual,
    LinearFutures,
    InversePerpetual,
    InverseFutures,
}

/// Option flavour values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BybitOptionType {
    Call,
    Put,
}

/// Position side as represented in REST/WebSocket payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitPositionSide {
    #[serde(rename = "")]
    Flat,
    #[serde(rename = "Buy")]
    Buy,
    #[serde(rename = "Sell")]
    Sell,
}

/// WebSocket order request operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitWsOrderRequestOp {
    #[serde(rename = "order.create")]
    Create,
    #[serde(rename = "order.amend")]
    Amend,
    #[serde(rename = "order.cancel")]
    Cancel,
    #[serde(rename = "order.create-batch")]
    CreateBatch,
    #[serde(rename = "order.amend-batch")]
    AmendBatch,
    #[serde(rename = "order.cancel-batch")]
    CancelBatch,
}

/// Available kline intervals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitKlineInterval {
    #[serde(rename = "1")]
    Minute1,
    #[serde(rename = "3")]
    Minute3,
    #[serde(rename = "5")]
    Minute5,
    #[serde(rename = "15")]
    Minute15,
    #[serde(rename = "30")]
    Minute30,
    #[serde(rename = "60")]
    Hour1,
    #[serde(rename = "120")]
    Hour2,
    #[serde(rename = "240")]
    Hour4,
    #[serde(rename = "360")]
    Hour6,
    #[serde(rename = "720")]
    Hour12,
    #[serde(rename = "D")]
    Day1,
    #[serde(rename = "W")]
    Week1,
    #[serde(rename = "M")]
    Month1,
}

impl BybitKlineInterval {
    /// Returns the end time in milliseconds for a bar that starts at `start_ms`.
    ///
    /// For most intervals this is simply `start_ms + duration`. For monthly bars,
    /// this calculates the actual first millisecond of the next month to handle
    /// variable month lengths (28-31 days).
    #[must_use]
    pub fn bar_end_time_ms(&self, start_ms: i64) -> i64 {
        match self {
            Self::Month1 => {
                let start_dt = DateTime::from_timestamp_millis(start_ms)
                    .unwrap_or_else(|| Utc.timestamp_millis_opt(0).unwrap());
                let (year, month) = if start_dt.month() == 12 {
                    (start_dt.year() + 1, 1)
                } else {
                    (start_dt.year(), start_dt.month() + 1)
                };
                Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
                    .single()
                    .map_or(start_ms + 2_678_400_000, |dt| dt.timestamp_millis())
            }
            _ => start_ms + self.duration_ms(),
        }
    }

    /// Returns the fixed duration of this interval in milliseconds.
    ///
    /// Note: For monthly bars, use [`Self::bar_end_time_ms`] instead as months have
    /// variable lengths (28-31 days).
    #[must_use]
    pub const fn duration_ms(&self) -> i64 {
        match self {
            Self::Minute1 => 60_000,
            Self::Minute3 => 180_000,
            Self::Minute5 => 300_000,
            Self::Minute15 => 900_000,
            Self::Minute30 => 1_800_000,
            Self::Hour1 => 3_600_000,
            Self::Hour2 => 7_200_000,
            Self::Hour4 => 14_400_000,
            Self::Hour6 => 21_600_000,
            Self::Hour12 => 43_200_000,
            Self::Day1 => 86_400_000,
            Self::Week1 => 604_800_000,
            Self::Month1 => 2_678_400_000, // 31 days - use bar_end_time_ms() for accurate calculation
        }
    }
}

impl Display for BybitKlineInterval {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Minute1 => "1",
            Self::Minute3 => "3",
            Self::Minute5 => "5",
            Self::Minute15 => "15",
            Self::Minute30 => "30",
            Self::Hour1 => "60",
            Self::Hour2 => "120",
            Self::Hour4 => "240",
            Self::Hour6 => "360",
            Self::Hour12 => "720",
            Self::Day1 => "D",
            Self::Week1 => "W",
            Self::Month1 => "M",
        };
        write!(f, "{s}")
    }
}

/// Order status values returned by Bybit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitOrderStatus {
    #[serde(rename = "Created")]
    Created,
    #[serde(rename = "New")]
    New,
    #[serde(rename = "Rejected")]
    Rejected,
    #[serde(rename = "PartiallyFilled")]
    PartiallyFilled,
    #[serde(rename = "PartiallyFilledCanceled")]
    PartiallyFilledCanceled,
    #[serde(rename = "Filled")]
    Filled,
    #[serde(rename = "Cancelled")]
    Canceled,
    #[serde(rename = "Untriggered")]
    Untriggered,
    #[serde(rename = "Triggered")]
    Triggered,
    #[serde(rename = "Deactivated")]
    Deactivated,
}

/// Order side enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitOrderSide {
    #[serde(rename = "")]
    Unknown,
    #[serde(rename = "Buy")]
    Buy,
    #[serde(rename = "Sell")]
    Sell,
}

impl From<BybitOrderSide> for AggressorSide {
    fn from(value: BybitOrderSide) -> Self {
        match value {
            BybitOrderSide::Buy => Self::Buyer,
            BybitOrderSide::Sell => Self::Seller,
            BybitOrderSide::Unknown => Self::NoAggressor,
        }
    }
}

impl From<BybitOrderSide> for OrderSide {
    fn from(value: BybitOrderSide) -> Self {
        match value {
            BybitOrderSide::Buy => Self::Buy,
            BybitOrderSide::Sell => Self::Sell,
            BybitOrderSide::Unknown => Self::NoOrderSide,
        }
    }
}

impl From<BybitTriggerType> for TriggerType {
    fn from(value: BybitTriggerType) -> Self {
        match value {
            BybitTriggerType::None => Self::Default,
            BybitTriggerType::LastPrice => Self::LastPrice,
            BybitTriggerType::IndexPrice => Self::IndexPrice,
            BybitTriggerType::MarkPrice => Self::MarkPrice,
        }
    }
}

/// Order cancel reason values as returned by Bybit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitCancelType {
    CancelByUser,
    CancelByReduceOnly,
    CancelByPrepareLackOfMargin,
    CancelByPrepareOrderFilter,
    CancelByPrepareOrderMarginCheckFailed,
    CancelByPrepareOrderCommission,
    CancelByPrepareOrderRms,
    CancelByPrepareOrderOther,
    CancelByRiskLimit,
    CancelOnDisconnect,
    CancelByStopOrdersExceeded,
    CancelByPzMarketClose,
    CancelByMarginCheckFailed,
    CancelByPzTakeover,
    CancelByAdmin,
    CancelByTpSlTsClear,
    CancelByAmendNotModified,
    CancelByPzCancel,
    CancelByCrossSelfMatch,
    CancelBySelfMatchPrevention,
    #[serde(other)]
    Other,
}

/// Order creation origin values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BybitCreateType {
    CreateByUser,
    CreateByClosing,
    CreateByTakeProfit,
    CreateByStopLoss,
    CreateByTrailingStop,
    CreateByStopOrder,
    CreateByPartialTakeProfit,
    CreateByPartialStopLoss,
    CreateByAdl,
    CreateByLiquidate,
    CreateByTakeover,
    CreateByTpsl,
    #[serde(other)]
    Other,
}

/// Venue order type enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitOrderType {
    #[serde(rename = "Market")]
    Market,
    #[serde(rename = "Limit")]
    Limit,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

/// Stop order type classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitStopOrderType {
    #[serde(rename = "")]
    None,
    #[serde(rename = "UNKNOWN")]
    Unknown,
    #[serde(rename = "TakeProfit")]
    TakeProfit,
    #[serde(rename = "StopLoss")]
    StopLoss,
    #[serde(rename = "TrailingStop")]
    TrailingStop,
    #[serde(rename = "Stop")]
    Stop,
    #[serde(rename = "PartialTakeProfit")]
    PartialTakeProfit,
    #[serde(rename = "PartialStopLoss")]
    PartialStopLoss,
    #[serde(rename = "tpslOrder")]
    TpslOrder,
    #[serde(rename = "OcoOrder")]
    OcoOrder,
    #[serde(rename = "MmRateClose")]
    MmRateClose,
    #[serde(rename = "BidirectionalTpslOrder")]
    BidirectionalTpslOrder,
}

/// Trigger type configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitTriggerType {
    #[serde(rename = "")]
    None,
    #[serde(rename = "LastPrice")]
    LastPrice,
    #[serde(rename = "IndexPrice")]
    IndexPrice,
    #[serde(rename = "MarkPrice")]
    MarkPrice,
}

/// Trigger direction integers used by the API.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitTriggerDirection {
    None = 0,
    RisesTo = 1,
    FallsTo = 2,
}

/// Take-profit/stop-loss mode for derivatives orders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitTpSlMode {
    Full,
    Partial,
    #[serde(other)]
    Unknown,
}

/// Time-in-force enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitTimeInForce {
    #[serde(rename = "GTC")]
    Gtc,
    #[serde(rename = "IOC")]
    Ioc,
    #[serde(rename = "FOK")]
    Fok,
    #[serde(rename = "PostOnly")]
    PostOnly,
}

/// Execution type values used in execution reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitExecType {
    #[serde(rename = "Trade")]
    Trade,
    #[serde(rename = "AdlTrade")]
    AdlTrade,
    #[serde(rename = "Funding")]
    Funding,
    #[serde(rename = "BustTrade")]
    BustTrade,
    #[serde(rename = "Delivery")]
    Delivery,
    #[serde(rename = "Settle")]
    Settle,
    #[serde(rename = "BlockTrade")]
    BlockTrade,
    #[serde(rename = "MovePosition")]
    MovePosition,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

/// Transaction types for wallet funding records.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BybitTransactionType {
    #[serde(rename = "TRANSFER_IN")]
    TransferIn,
    #[serde(rename = "TRANSFER_OUT")]
    TransferOut,
    #[serde(rename = "TRADE")]
    Trade,
    #[serde(rename = "SETTLEMENT")]
    Settlement,
    #[serde(rename = "DELIVERY")]
    Delivery,
    #[serde(rename = "LIQUIDATION")]
    Liquidation,
    #[serde(rename = "AIRDRP")]
    Airdrop,
}

/// Endpoint classifications used by the Bybit API.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BybitEndpointType {
    None,
    Asset,
    Market,
    Account,
    Trade,
    Position,
    User,
}

/// Filter for open orders query.
///
/// Used with `GET /v5/order/realtime` to filter order status.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitOpenOnly {
    /// Query open status orders only (New, PartiallyFilled).
    #[default]
    OpenOnly = 0,
    /// Query up to 500 recent closed orders (cancelled, rejected, filled).
    ClosedRecent = 1,
}

/// Order filter for querying specific order types.
///
/// Used with `GET /v5/order/realtime` to filter by order category.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", eq, eq_int)
)]
pub enum BybitOrderFilter {
    /// Active orders (default).
    #[default]
    Order,
    /// Conditional orders (futures and spot).
    StopOrder,
    /// Spot take-profit/stop-loss orders.
    #[serde(rename = "tpslOrder")]
    TpslOrder,
    /// Spot one-cancels-other orders.
    OcoOrder,
    /// Spot bidirectional TP/SL orders.
    BidirectionalTpslOrder,
}

/// Margin actions for spot margin trading operations.
#[derive(
    Clone,
    Copy,
    Debug,
    strum::Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        hash,
        frozen,
        module = "nautilus_trader.core.nautilus_pyo3.bybit"
    )
)]
pub enum BybitMarginAction {
    /// Borrow funds for margin trading.
    Borrow,
    /// Repay borrowed funds.
    Repay,
    /// Query current borrowed amount.
    GetBorrowAmount,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::minute1(BybitKlineInterval::Minute1, 60_000)]
    #[case::minute3(BybitKlineInterval::Minute3, 180_000)]
    #[case::minute5(BybitKlineInterval::Minute5, 300_000)]
    #[case::minute15(BybitKlineInterval::Minute15, 900_000)]
    #[case::minute30(BybitKlineInterval::Minute30, 1_800_000)]
    #[case::hour1(BybitKlineInterval::Hour1, 3_600_000)]
    #[case::hour2(BybitKlineInterval::Hour2, 7_200_000)]
    #[case::hour4(BybitKlineInterval::Hour4, 14_400_000)]
    #[case::hour6(BybitKlineInterval::Hour6, 21_600_000)]
    #[case::hour12(BybitKlineInterval::Hour12, 43_200_000)]
    #[case::day1(BybitKlineInterval::Day1, 86_400_000)]
    #[case::week1(BybitKlineInterval::Week1, 604_800_000)]
    #[case::month1(BybitKlineInterval::Month1, 2_678_400_000)]
    fn test_kline_interval_duration_ms(
        #[case] interval: BybitKlineInterval,
        #[case] expected_ms: i64,
    ) {
        assert_eq!(interval.duration_ms(), expected_ms);
    }

    #[rstest]
    fn test_bar_end_time_ms_non_monthly_adds_duration() {
        let interval = BybitKlineInterval::Minute1;
        let start_ms = 1704067200000i64;
        assert_eq!(interval.bar_end_time_ms(start_ms), start_ms + 60_000);
    }

    #[rstest]
    #[case::jan_31_days(1704067200000i64, 1706745600000i64)]
    #[case::feb_leap_year_29_days(1706745600000i64, 1709251200000i64)]
    #[case::apr_30_days(1711929600000i64, 1714521600000i64)]
    #[case::dec_to_next_year(1733011200000i64, 1735689600000i64)]
    fn test_bar_end_time_ms_monthly_variable_lengths(
        #[case] start_ms: i64,
        #[case] expected_end_ms: i64,
    ) {
        let interval = BybitKlineInterval::Month1;
        assert_eq!(interval.bar_end_time_ms(start_ms), expected_end_ms);
    }
}
