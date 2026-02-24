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

//! Common enumerations for the Betfair adapter.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Betfair order side.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetfairSide {
    /// Betting on the selection to win.
    Back,
    /// Betting against the selection to win.
    Lay,
}

/// Betfair order type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetfairOrderType {
    /// A normal exchange limit order for immediate execution.
    Limit,
    /// Limit order for the auction (SP).
    LimitOnClose,
    /// Market order for the auction (SP).
    MarketOnClose,
    /// Legacy name for `MarketOnClose` (appears in older settled orders).
    MarketAtTheClose,
}

/// Betfair order status.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetfairOrderStatus {
    /// Order is pending.
    Pending,
    /// Order has been fully matched/cancelled/lapsed.
    ExecutionComplete,
    /// Order has remaining unmatched volume.
    Executable,
    /// Order has expired.
    Expired,
}

/// Controls which data fields are returned with market catalogues.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketProjection {
    Competition,
    Event,
    EventType,
    MarketStartTime,
    MarketDescription,
    RunnerDescription,
    RunnerMetadata,
}

/// Market status.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketStatus {
    Inactive,
    Open,
    Suspended,
    Closed,
}

/// Sorting options for market listings.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketSort {
    MinimumTraded,
    MaximumTraded,
    MinimumAvailable,
    MaximumAvailable,
    FirstToStart,
    LastToStart,
}

/// Market betting type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketBettingType {
    Odds,
    Line,
    Range,
    AsianHandicapDoubleLine,
    AsianHandicapSingleLine,
    FixedOdds,
}

/// Exchange price data options.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceData {
    SpAvailable,
    SpTraded,
    ExBestOffers,
    ExAllOffers,
    ExTraded,
}

/// Matched amount rollup projection.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MatchProjection {
    NoRollup,
    RolledUpByPrice,
    RolledUpByAvgPrice,
}

/// Price ladder type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceLadderType {
    Classic,
    Finest,
    LineRange,
}

/// Order filter projection.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderProjection {
    All,
    Executable,
    ExecutionComplete,
}

/// Order sort field.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderBy {
    ByBet,
    ByMarket,
    ByMatchTime,
    ByPlaceTime,
    BySettledTime,
    ByVoidTime,
}

/// Sort direction for order listings.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum SortDir {
    EarliestToLatest,
    LatestToEarliest,
}

/// Betfair time-in-force (only FILL_OR_KILL supported).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetfairTimeInForce {
    FillOrKill,
}

/// How unmatched bets are handled at market turn in-play.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PersistenceType {
    /// Bet is lapsed (cancelled) when market turns in-play.
    Lapse,
    /// Bet persists when market turns in-play.
    Persist,
    /// Bet is placed as a Market On Close order.
    MarketOnClose,
}

/// Execution report status for batch order operations.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionReportStatus {
    Success,
    Failure,
    ProcessedWithErrors,
    Timeout,
}

/// Error codes for execution report failures.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionReportErrorCode {
    ErrorInMatcher,
    ProcessedWithErrors,
    BetActionError,
    InvalidAccountState,
    InvalidWalletStatus,
    InsufficientFunds,
    LossLimitExceeded,
    MarketSuspended,
    MarketNotOpenForBetting,
    DuplicateTransaction,
    InvalidOrder,
    InvalidMarketId,
    PermissionDenied,
    DuplicateBetids,
    NoActionRequired,
    ServiceUnavailable,
    RejectedByRegulator,
    NoChasing,
    RegulatorIsNotAvailable,
    TooManyInstructions,
    InvalidMarketVersion,
    InvalidProfitRatio,
    EventExposureLimitExceeded,
    EventMatchedExposureLimitExceeded,
    EventBlocked,
}

/// Instruction report status for individual order instructions.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum InstructionReportStatus {
    Success,
    Failure,
    Timeout,
}

/// Error codes for individual instruction report failures.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum InstructionReportErrorCode {
    InvalidBetSize,
    InvalidRunner,
    BetTakenOrLapsed,
    BetInProgress,
    RunnerRemoved,
    MarketNotOpenForBetting,
    LossLimitExceeded,
    MarketNotOpenForBspBetting,
    InvalidPriceEdit,
    InvalidOdds,
    InsufficientFunds,
    InvalidPersistenceType,
    ErrorInMatcher,
    InvalidBackLayCombination,
    ErrorInOrder,
    InvalidBidType,
    InvalidBetId,
    CancelledNotPlaced,
    RelatedActionFailed,
    NoActionRequired,
    TimeInForceConflict,
    UnexpectedPersistenceType,
    InvalidOrderType,
    UnexpectedMinFillSize,
    InvalidCustomerOrderRef,
    InvalidMinFillSize,
    BetLapsedPriceImprovementTooLarge,
    InvalidCustomerStrategyRef,
    InvalidProfitRatio,
}

/// Runner status.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum RunnerStatus {
    Active,
    Winner,
    Loser,
    Placed,
    RemovedVacant,
    Removed,
    Hidden,
}

/// Bet settlement status.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetStatus {
    Settled,
    Voided,
    Lapsed,
    Cancelled,
}

/// Grouping level for cleared order reports.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum GroupBy {
    EventType,
    Event,
    Market,
    Side,
    Bet,
    Runner,
    Strategy,
}

/// Time aggregation granularity.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeGranularity {
    Days,
    Hours,
    Minutes,
}

/// Bet target type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetTargetType {
    BackersProfit,
    Payout,
}

/// Bet delay model for in-play markets.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BetDelayModel {
    Passive,
    Dynamic,
}

/// Volume rollup strategy.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum RollupModel {
    Stake,
    Payout,
    ManagedLiability,
    None,
}

/// Streaming order side (shorthand: B=Back, L=Lay).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum StreamingSide {
    #[serde(rename = "B")]
    #[strum(serialize = "B")]
    Back,
    #[serde(rename = "L")]
    #[strum(serialize = "L")]
    Lay,
}

/// Streaming order status (shorthand: E=Executable, EC=ExecutionComplete).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum StreamingOrderStatus {
    #[serde(rename = "E")]
    #[strum(serialize = "E")]
    Executable,
    #[serde(rename = "EC")]
    #[strum(serialize = "EC")]
    ExecutionComplete,
}

/// Streaming persistence type (shorthand: L=Lapse, P=Persist, MOC=MarketOnClose).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum StreamingPersistenceType {
    #[serde(rename = "L")]
    #[strum(serialize = "L")]
    Lapse,
    #[serde(rename = "P")]
    #[strum(serialize = "P")]
    Persist,
    #[serde(rename = "MOC")]
    #[strum(serialize = "MOC")]
    MarketOnClose,
}

/// Streaming order type (shorthand: L=Limit, LOC=LimitOnClose, MOC=MarketOnClose).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum StreamingOrderType {
    #[serde(rename = "L")]
    #[strum(serialize = "L")]
    Limit,
    #[serde(rename = "LOC")]
    #[strum(serialize = "LOC")]
    LimitOnClose,
    #[serde(rename = "MOC")]
    #[strum(serialize = "MOC")]
    MarketOnClose,
}

/// Streaming status error code.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum StatusErrorCode {
    InvalidInput,
    Timeout,
    NoAppKey,
    InvalidAppKey,
    NoSession,
    InvalidSessionInformation,
    NotAuthorized,
    MaxConnectionLimitExceeded,
    TooManyRequests,
    SubscriptionLimitExceeded,
    InvalidClock,
    UnexpectedError,
    ConnectionFailed,
    InvalidRequest,
}

/// Streaming change type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeType {
    Heartbeat,
    SubImage,
    ResubDelta,
}

/// Streaming segment type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum SegmentType {
    SegStart,
    Seg,
    SegEnd,
}

/// Reason code for bet lapse events on the streaming API.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum LapseStatusReasonCode {
    MktUnknown,
    MktInvalid,
    RnrUnknown,
    TimeElapsed,
    CurrencyUnknown,
    PriceInvalid,
    MktSuspended,
    MktVersion,
    LineTarget,
    LineSp,
    SpInPlay,
    SmallStake,
    PriceImpTooLarge,
}

/// Market data filter fields for streaming subscriptions.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketDataFilterField {
    ExBestOffersDisp,
    ExBestOffers,
    ExAllOffers,
    ExTraded,
    ExTradedVol,
    ExLtp,
    ExMarketDef,
    SpTraded,
    SpProjected,
}
