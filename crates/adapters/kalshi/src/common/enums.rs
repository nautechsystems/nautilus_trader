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

//! Kalshi venue enums mirrored from REST and WebSocket payloads.
//!
//! Enums with an `Other` variant retain unrecognized wire labels for admission checks and persistence.
//! Closed enums, including request filters, reject undocumented wire values.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

/// Kalshi API environment.
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
        module = "nautilus_trader._libnautilus.kalshi",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.kalshi")
)]
pub enum KalshiEnvironment {
    /// Production trading environment.
    #[default]
    Production,
    /// Demo trading environment.
    Demo,
}

impl KalshiEnvironment {
    /// Returns `true` when this is the demo environment.
    #[must_use]
    pub const fn is_demo(self) -> bool {
        matches!(self, Self::Demo)
    }
}

/// Event-contract exit trigger kind.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ExitTriggerKind {
    Bracket,
}

/// Candlestick duration in minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, FromRepr, Serialize, Deserialize)]
#[repr(u32)]
#[serde(try_from = "u32", into = "u32")]
pub enum CandlestickInterval {
    Minute = 1,
    Hour = 60,
    Day = 1440,
}

impl From<CandlestickInterval> for u32 {
    fn from(value: CandlestickInterval) -> Self {
        value as Self
    }
}

impl TryFrom<u32> for CandlestickInterval {
    type Error = strum::ParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::from_repr(value).ok_or(strum::ParseError::VariantNotFound)
    }
}

/// Event discovery status filter.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EventStatusFilter {
    Unopened,
    Open,
    Closed,
    Settled,
}

/// Forecast history interval; zero selects five-second samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, FromRepr, Serialize, Deserialize)]
#[repr(u32)]
#[serde(try_from = "u32", into = "u32")]
pub enum ForecastInterval {
    FiveSeconds = 0,
    Minute = 1,
    Hour = 60,
    Day = 1440,
}

impl From<ForecastInterval> for u32 {
    fn from(value: ForecastInterval) -> Self {
        value as Self
    }
}

impl TryFrom<u32> for ForecastInterval {
    type Error = strum::ParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::from_repr(value).ok_or(strum::ParseError::VariantNotFound)
    }
}

/// Multivariate event collection status filter.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MultivariateCollectionStatus {
    Unopened,
    Open,
    Closed,
}

/// Incentive program status filter.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum IncentiveStatusFilter {
    All,
    Active,
    Upcoming,
    Closed,
    PaidOut,
}

/// Incentive program type filter.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum IncentiveTypeFilter {
    All,
    Liquidity,
    Volume,
    MarginMakerVolume,
    MarginTakerVolume,
}

/// Settlement state selected by position queries.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PositionSettlementStatus {
    Unsettled,
    Settled,
    All,
}

/// Market discovery status filter, distinct from response lifecycle states.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MarketStatusFilter {
    Unopened,
    Open,
    Paused,
    Closed,
    Settled,
}

/// Multivariate market inclusion in live discovery.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MveFilter {
    Only,
    Exclude,
}

/// Multivariate market exclusion in archived discovery.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MveHistoricalFilter {
    Exclude,
}

/// Unit of an event-contract exit slippage limit.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ExitSlippageUnit {
    Bps,
    Cents,
}

/// Event-contract exit trigger state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ExitTriggerStatus {
    PendingOnEntry,
    Active,
    Filled,
    Canceled,
    Failed,
    Unknown,
}

/// Exit bracket leg that triggers execution.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ExitTriggerLeg {
    StopLoss,
    TakeProfit,
}

/// Series fee model or event override; unrecognized labels remain available for qualification.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FeeType {
    Quadratic,
    QuadraticWithMakerFees,
    QuadraticWithComboMakerFees,
    Flat,
    MarginMarketMakerProgramFees,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Exchange lane attached to an API usage grant.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ExchangeInstance {
    EventContract,
    Margined,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Communications creator filter; an empty value selects all users.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum UserFilter {
    #[serde(rename = "")]
    #[strum(serialize = "")]
    All,
    #[serde(rename = "self")]
    #[strum(serialize = "self")]
    SelfUser,
}

/// Scope granted to an API key.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ApiKeyScope {
    Read,
    Write,
    #[serde(rename = "read::block_trade_accept")]
    #[strum(serialize = "read::block_trade_accept")]
    ReadBlockTradeAccept,
    #[serde(rename = "read::portfolio_balance")]
    #[strum(serialize = "read::portfolio_balance")]
    ReadPortfolioBalance,
    #[serde(rename = "write::trade")]
    #[strum(serialize = "write::trade")]
    WriteTrade,
    #[serde(rename = "write::transfer")]
    #[strum(serialize = "write::transfer")]
    WriteTransfer,
    #[serde(rename = "write::fcm_risk")]
    #[strum(serialize = "write::fcm_risk")]
    WriteFcmRisk,
    #[serde(rename = "write::block_trade_accept")]
    #[strum(serialize = "write::block_trade_accept")]
    WriteBlockTradeAccept,
}

/// API request signature algorithm.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ApiKeyType {
    Rsa,
    Ed25519,
}

/// Market index session and freshness state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MarketIndexStatus {
    Open,
    Closed,
    Stale,
}

/// Credit processing state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CreditStatus {
    Unknown,
    Pending,
    Failed,
    Applied,
}

/// Market settlement outcome; an empty value means no result.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MarketResult {
    Yes,
    No,
    Scalar,
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Unsettled,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Deposit or withdrawal processing state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Applied,
    Failed,
    Returned,
}

/// Deposit or withdrawal payment method.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PaymentType {
    Ach,
    Wire,
    Crypto,
    Debit,
    Apm,
}

/// Legacy REST and WebSocket order action; an empty value means unknown.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OrderAction {
    Buy,
    Sell,
    SellPartial,
    SellToClose,
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Unknown,
}

/// Venue order type retained in order reports.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OrderType {
    Limit,
    Market,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Incentive program reward type.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum IncentiveType {
    Liquidity,
    Volume,
    MarginMakerVolume,
    MarginTakerVolume,
}

/// Collateral reservation for resting orders during automatic rebalancing.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RestingMarginReservation {
    None,
    Max,
    Sum,
}

/// Intra-exchange-instance transfer state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TransferStatus {
    Pending,
    Complete,
}

/// Request-for-quote state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RfqStatus {
    Open,
    Closed,
}

/// Request-for-quote quote state; cancellation uses the venue spelling.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum QuoteStatus {
    Open,
    Accepted,
    Confirmed,
    Executed,
    Cancelled,
}

/// Request-for-quote batch state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RfqBatchStatus {
    Open,
    Accepted,
    Closed,
}

/// Request-for-quote batch quote state.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RfqBatchQuoteStatus {
    Open,
    Accepted,
    Confirmed,
    Executed,
    PartiallyExecuted,
    Cancelled,
}

/// Venue market type; unsupported families remain available for admission checks.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MarketType {
    Binary,
    Scalar,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Market lifecycle state returned in metadata.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MarketStatus {
    Initialized,
    Inactive,
    Active,
    Closed,
    Determined,
    Disputed,
    Amended,
    Finalized,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Settlement bounds applied to a market.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettlementBoundsType {
    Default,
    Floor,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Market strike evaluation rule.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StrikeType {
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Between,
    Functional,
    Custom,
    Structured,
}

/// Reason for the most recent private order update.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "PascalCase")]
#[strum(serialize_all = "PascalCase")]
pub enum OrderUpdateReason {
    Decrease,
    Amend,
    MarginCancel,
    SelfTradeCancel,
    ExpiryCancel,
    CloseCancel,
    HaltCancel,
    Trade,
    PostOnlyCrossCancel,
    ReduceOnlyCancel,
    SettlementBoundsCancel,
}

/// Order-book conflation interval in milliseconds; zero disables conflation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, FromRepr, Serialize, Deserialize)]
#[repr(u32)]
#[serde(try_from = "u32", into = "u32")]
pub enum OrderbookUpdateInterval {
    Immediate = 0,
    Millis100 = 100,
    Millis1000 = 1000,
}

impl From<OrderbookUpdateInterval> for u32 {
    fn from(value: OrderbookUpdateInterval) -> Self {
        value as Self
    }
}

impl TryFrom<u32> for OrderbookUpdateInterval {
    type Error = strum::ParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::from_repr(value).ok_or(strum::ParseError::VariantNotFound)
    }
}

/// CF Benchmarks subscription update action.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CfBenchmarksUpdateAction {
    SubscribeIndices,
    UnsubscribeIndices,
    Indexlist,
}

/// Pyth subscription update action.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PythUpdateAction {
    SubscribeUnderlyings,
    UnsubscribeUnderlyings,
    UnderlyingList,
}

/// Documented WebSocket error code; retired codes remain reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, FromRepr, Serialize, Deserialize)]
#[repr(u32)]
#[serde(try_from = "u32", into = "u32")]
pub enum KalshiWsErrorCode {
    UnableToProcessMessage = 1,
    ParamsRequired = 2,
    ChannelsRequired = 3,
    SubscriptionIdsRequired = 4,
    UnknownCommand = 5,
    UnknownSubscriptionId = 7,
    UnknownChannelName = 8,
    AuthenticationRequired = 9,
    ChannelError = 10,
    InvalidParameter = 11,
    ExactlyOneSubscriptionIdRequired = 12,
    UnsupportedAction = 13,
    MarketTickerRequired = 14,
    ActionRequired = 15,
    CommandTimeout = 18,
    InvalidShardFactor = 19,
    ShardFactorRequired = 20,
    InvalidShardKey = 21,
    ShardFactorTooLarge = 22,
    MatchIdsRequired = 23,
    IndexIdsRequired = 24,
    SubscriptionBufferOverflow = 25,
    SubscriptionMarketLimitExceeded = 26,
    TooManyRequests = 27,
    UnderlyingTickersRequired = 28,
    InvalidUpdateInterval = 29,
}

impl From<KalshiWsErrorCode> for u32 {
    fn from(value: KalshiWsErrorCode) -> Self {
        value as Self
    }
}

impl TryFrom<u32> for KalshiWsErrorCode {
    type Error = strum::ParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::from_repr(value).ok_or(strum::ParseError::VariantNotFound)
    }
}

/// Market lifecycle notification kind.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MarketLifecycleEvent {
    Created,
    Deactivated,
    Activated,
    CloseDateUpdated,
    Determined,
    Settled,
    PriceLevelStructureUpdated,
    MetadataUpdated,
}

/// Multivariate market lifecycle notification kind.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MultivariateMarketLifecycleEvent {
    Created,
    Deactivated,
    Activated,
    CloseDateUpdated,
    Determined,
    Settled,
}

/// Venue price-grid label; price ranges define the actual valid ticks.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PriceLevelStructure {
    BandedCentiCent,
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Unknown,
    LinearCent,
    DeciCent,
    TaperedDeciCent,
    CenterWholeEdgeHalfCent,
    CenterWholeEdgeQuintCent,
    CenterHalfEdgeHalfCent,
    CenterHalfEdgeQuintCent,
    CenterHalfEdgeDeciCent,
    CenterQuintEdgeQuintCent,
    CenterQuintEdgeDeciCent,
    CenterCentiEdgeCentiCent,
    CenterDeciEdgeCentiCent,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Event collateral netting scheme; an empty value disables netting.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CollateralReturnType {
    #[serde(rename = "MECNET")]
    #[strum(serialize = "MECNET")]
    Mecnet,
    #[serde(rename = "DIRECNET")]
    #[strum(serialize = "DIRECNET")]
    Direcnet,
    #[serde(rename = "")]
    #[strum(serialize = "")]
    None,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Order-group lifecycle notification kind.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OrderGroupEvent {
    Created,
    Triggered,
    Reset,
    Deleted,
    LimitUpdated,
}

/// Published API usage tier; unrecognized tiers retain conservative quota handling.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ApiUsageTier {
    Basic,
    Advanced,
    Expert,
    Premier,
    Paragon,
    Prime,
    Prestige,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Origin of an API usage-level grant.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum UsageGrantSource {
    Volume,
    Manual,
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(String),
}

/// Position field required to have a nonzero value.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PositionCountFilter {
    Position,
    TotalTraded,
}

/// Order report status filter.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OrderStatusFilter {
    Resting,
    Canceled,
    Executed,
}

/// WebSocket client command name.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum KalshiCommandKind {
    Subscribe,
    Unsubscribe,
    UpdateSubscription,
    ListSubscriptions,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_environment_default_is_production() {
        assert_eq!(KalshiEnvironment::default(), KalshiEnvironment::Production);
    }
}
