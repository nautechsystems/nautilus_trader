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

use nautilus_model::enums::{
    MarketStatus as NautilusMarketStatus, OrderSide, OrderStatus, OrderType, TimeInForce,
};
use rust_decimal::Decimal;
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

// Betfair side mapping is INVERTED from financial convention:
// Back (betting on selection to win) = SELL
// Lay (betting against selection) = BUY

impl From<BetfairSide> for OrderSide {
    fn from(value: BetfairSide) -> Self {
        match value {
            BetfairSide::Back => Self::Sell,
            BetfairSide::Lay => Self::Buy,
        }
    }
}

impl From<OrderSide> for BetfairSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Buy => Self::Lay,
            OrderSide::Sell => Self::Back,
            _ => panic!("Invalid `OrderSide` for Betfair: {value}"),
        }
    }
}

impl From<StreamingSide> for OrderSide {
    fn from(value: StreamingSide) -> Self {
        match value {
            StreamingSide::Back => Self::Sell,
            StreamingSide::Lay => Self::Buy,
        }
    }
}

impl From<BetfairOrderType> for OrderType {
    fn from(value: BetfairOrderType) -> Self {
        match value {
            BetfairOrderType::Limit => Self::Limit,
            BetfairOrderType::LimitOnClose => Self::Limit,
            BetfairOrderType::MarketOnClose => Self::Market,
            BetfairOrderType::MarketAtTheClose => Self::Market,
        }
    }
}

impl From<StreamingOrderType> for OrderType {
    fn from(value: StreamingOrderType) -> Self {
        match value {
            StreamingOrderType::Limit => Self::Limit,
            StreamingOrderType::LimitOnClose => Self::Limit,
            StreamingOrderType::MarketOnClose => Self::Market,
        }
    }
}

/// Resolves the Nautilus `OrderStatus` for a Betfair order.
///
/// `ExecutionComplete` is a terminal state covering fills, cancels, and
/// lapses — the correct status depends on matched vs canceled quantities.
#[must_use]
pub fn resolve_order_status(
    status: BetfairOrderStatus,
    size_matched: Decimal,
    size_cancelled: Decimal,
) -> OrderStatus {
    match status {
        BetfairOrderStatus::Pending => OrderStatus::Submitted,
        BetfairOrderStatus::Executable if size_matched > Decimal::ZERO => {
            OrderStatus::PartiallyFilled
        }
        BetfairOrderStatus::Executable => OrderStatus::Accepted,
        BetfairOrderStatus::Expired => OrderStatus::Expired,
        BetfairOrderStatus::ExecutionComplete => {
            resolve_terminal_status(size_matched, size_cancelled)
        }
    }
}

/// Resolves the Nautilus `OrderStatus` for a streaming order update.
///
/// Same logic as [`resolve_order_status`] for the streaming enum.
#[must_use]
pub fn resolve_streaming_order_status(
    status: StreamingOrderStatus,
    size_matched: Decimal,
    size_cancelled: Decimal,
) -> OrderStatus {
    match status {
        StreamingOrderStatus::Executable if size_matched > Decimal::ZERO => {
            OrderStatus::PartiallyFilled
        }
        StreamingOrderStatus::Executable => OrderStatus::Accepted,
        StreamingOrderStatus::ExecutionComplete => {
            resolve_terminal_status(size_matched, size_cancelled)
        }
    }
}

fn resolve_terminal_status(size_matched: Decimal, size_cancelled: Decimal) -> OrderStatus {
    if size_matched > Decimal::ZERO && size_cancelled <= Decimal::ZERO {
        OrderStatus::Filled
    } else {
        // Any terminal order with cancelled quantity is closed, even if
        // partially matched. PartiallyFilled is an open status in Nautilus
        // and must not be used for ExecutionComplete orders.
        OrderStatus::Canceled
    }
}

impl From<MarketStatus> for NautilusMarketStatus {
    fn from(value: MarketStatus) -> Self {
        match value {
            MarketStatus::Open => Self::Open,
            MarketStatus::Closed => Self::Closed,
            MarketStatus::Suspended => Self::Suspended,
            MarketStatus::Inactive => Self::NotAvailable,
        }
    }
}

impl From<BetfairTimeInForce> for TimeInForce {
    fn from(value: BetfairTimeInForce) -> Self {
        match value {
            BetfairTimeInForce::FillOrKill => Self::Fok,
        }
    }
}

impl From<PersistenceType> for TimeInForce {
    fn from(value: PersistenceType) -> Self {
        match value {
            PersistenceType::Lapse => Self::Day,
            PersistenceType::Persist => Self::Gtc,
            PersistenceType::MarketOnClose => Self::AtTheClose,
        }
    }
}

impl From<StreamingPersistenceType> for TimeInForce {
    fn from(value: StreamingPersistenceType) -> Self {
        match value {
            StreamingPersistenceType::Lapse => Self::Day,
            StreamingPersistenceType::Persist => Self::Gtc,
            StreamingPersistenceType::MarketOnClose => Self::AtTheClose,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(BetfairSide::Back, OrderSide::Sell)]
    #[case(BetfairSide::Lay, OrderSide::Buy)]
    fn test_betfair_side_to_order_side(#[case] input: BetfairSide, #[case] expected: OrderSide) {
        assert_eq!(OrderSide::from(input), expected);
    }

    #[rstest]
    #[case(OrderSide::Buy, BetfairSide::Lay)]
    #[case(OrderSide::Sell, BetfairSide::Back)]
    fn test_order_side_to_betfair_side(#[case] input: OrderSide, #[case] expected: BetfairSide) {
        assert_eq!(BetfairSide::from(input), expected);
    }

    #[rstest]
    #[should_panic(expected = "Invalid `OrderSide`")]
    fn test_order_side_no_order_side_panics() {
        let _ = BetfairSide::from(OrderSide::NoOrderSide);
    }

    #[rstest]
    #[case(StreamingSide::Back, OrderSide::Sell)]
    #[case(StreamingSide::Lay, OrderSide::Buy)]
    fn test_streaming_side_to_order_side(
        #[case] input: StreamingSide,
        #[case] expected: OrderSide,
    ) {
        assert_eq!(OrderSide::from(input), expected);
    }

    #[rstest]
    #[case(BetfairOrderType::Limit, OrderType::Limit)]
    #[case(BetfairOrderType::LimitOnClose, OrderType::Limit)]
    #[case(BetfairOrderType::MarketOnClose, OrderType::Market)]
    #[case(BetfairOrderType::MarketAtTheClose, OrderType::Market)]
    fn test_betfair_order_type(#[case] input: BetfairOrderType, #[case] expected: OrderType) {
        assert_eq!(OrderType::from(input), expected);
    }

    #[rstest]
    #[case(StreamingOrderType::Limit, OrderType::Limit)]
    #[case(StreamingOrderType::LimitOnClose, OrderType::Limit)]
    #[case(StreamingOrderType::MarketOnClose, OrderType::Market)]
    fn test_streaming_order_type(#[case] input: StreamingOrderType, #[case] expected: OrderType) {
        assert_eq!(OrderType::from(input), expected);
    }

    #[rstest]
    fn test_resolve_order_status_non_terminal() {
        assert_eq!(
            resolve_order_status(BetfairOrderStatus::Pending, Decimal::ZERO, Decimal::ZERO),
            OrderStatus::Submitted,
        );
        assert_eq!(
            resolve_order_status(BetfairOrderStatus::Executable, Decimal::ZERO, Decimal::ZERO),
            OrderStatus::Accepted,
        );
        assert_eq!(
            resolve_order_status(BetfairOrderStatus::Expired, Decimal::ZERO, Decimal::ZERO),
            OrderStatus::Expired,
        );
    }

    #[rstest]
    fn test_resolve_order_status_executable_partially_matched() {
        assert_eq!(
            resolve_order_status(
                BetfairOrderStatus::Executable,
                Decimal::new(5, 0),
                Decimal::ZERO
            ),
            OrderStatus::PartiallyFilled,
        );
    }

    #[rstest]
    #[case(Decimal::TEN, Decimal::ZERO, OrderStatus::Filled)]
    #[case(Decimal::new(5, 0), Decimal::new(5, 0), OrderStatus::Canceled)]
    #[case(Decimal::ZERO, Decimal::TEN, OrderStatus::Canceled)]
    fn test_resolve_order_status_execution_complete(
        #[case] size_matched: Decimal,
        #[case] size_cancelled: Decimal,
        #[case] expected: OrderStatus,
    ) {
        assert_eq!(
            resolve_order_status(
                BetfairOrderStatus::ExecutionComplete,
                size_matched,
                size_cancelled,
            ),
            expected,
        );
    }

    #[rstest]
    fn test_resolve_streaming_order_status_executable() {
        assert_eq!(
            resolve_streaming_order_status(
                StreamingOrderStatus::Executable,
                Decimal::ZERO,
                Decimal::ZERO,
            ),
            OrderStatus::Accepted,
        );
    }

    #[rstest]
    fn test_resolve_streaming_order_status_executable_partially_matched() {
        assert_eq!(
            resolve_streaming_order_status(
                StreamingOrderStatus::Executable,
                Decimal::new(5, 0),
                Decimal::ZERO,
            ),
            OrderStatus::PartiallyFilled,
        );
    }

    #[rstest]
    #[case(Decimal::TEN, Decimal::ZERO, OrderStatus::Filled)]
    #[case(Decimal::new(5, 0), Decimal::new(5, 0), OrderStatus::Canceled)]
    #[case(Decimal::ZERO, Decimal::TEN, OrderStatus::Canceled)]
    fn test_resolve_streaming_order_status_execution_complete(
        #[case] size_matched: Decimal,
        #[case] size_cancelled: Decimal,
        #[case] expected: OrderStatus,
    ) {
        assert_eq!(
            resolve_streaming_order_status(
                StreamingOrderStatus::ExecutionComplete,
                size_matched,
                size_cancelled,
            ),
            expected,
        );
    }

    #[rstest]
    #[case(MarketStatus::Open, NautilusMarketStatus::Open)]
    #[case(MarketStatus::Closed, NautilusMarketStatus::Closed)]
    #[case(MarketStatus::Suspended, NautilusMarketStatus::Suspended)]
    #[case(MarketStatus::Inactive, NautilusMarketStatus::NotAvailable)]
    fn test_market_status(#[case] input: MarketStatus, #[case] expected: NautilusMarketStatus) {
        assert_eq!(NautilusMarketStatus::from(input), expected);
    }

    #[rstest]
    fn test_betfair_time_in_force() {
        assert_eq!(
            TimeInForce::from(BetfairTimeInForce::FillOrKill),
            TimeInForce::Fok
        );
    }

    #[rstest]
    #[case(PersistenceType::Lapse, TimeInForce::Day)]
    #[case(PersistenceType::Persist, TimeInForce::Gtc)]
    #[case(PersistenceType::MarketOnClose, TimeInForce::AtTheClose)]
    fn test_persistence_type_to_time_in_force(
        #[case] input: PersistenceType,
        #[case] expected: TimeInForce,
    ) {
        assert_eq!(TimeInForce::from(input), expected);
    }

    #[rstest]
    #[case(StreamingPersistenceType::Lapse, TimeInForce::Day)]
    #[case(StreamingPersistenceType::Persist, TimeInForce::Gtc)]
    #[case(StreamingPersistenceType::MarketOnClose, TimeInForce::AtTheClose)]
    fn test_streaming_persistence_type_to_time_in_force(
        #[case] input: StreamingPersistenceType,
        #[case] expected: TimeInForce,
    ) {
        assert_eq!(TimeInForce::from(input), expected);
    }

    #[rstest]
    fn test_resolve_streaming_lapsed_and_voided_count_as_closed() {
        // size_closed includes lapsed + voided, so these should resolve to Canceled
        // even if size_cancelled itself is zero (the caller aggregates them)
        assert_eq!(
            resolve_streaming_order_status(
                StreamingOrderStatus::ExecutionComplete,
                Decimal::ZERO,
                Decimal::new(5, 0), // aggregated lapsed/voided/cancelled
            ),
            OrderStatus::Canceled,
        );
    }

    #[rstest]
    fn test_resolve_streaming_partial_match_then_cancel() {
        // Partially matched then remainder cancelled
        assert_eq!(
            resolve_streaming_order_status(
                StreamingOrderStatus::ExecutionComplete,
                Decimal::new(3, 0), // matched
                Decimal::new(7, 0), // cancelled remainder
            ),
            OrderStatus::Canceled,
        );
    }
}
